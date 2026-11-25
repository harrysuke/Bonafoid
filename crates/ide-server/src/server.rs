use crate::auth::{self, JwtKeys};
use crate::plugins::PluginRegistry;
use crate::preview::preview_server_router;
use crate::preview::runs::RunConfig;
use crate::preview::{BuildManager, PreviewConfig, RunManager};
use anyhow::{Context, Result};
use axum::{extract::State, routing::get, Json, Router};
use nocode_deploy::{CredentialVault, DeployService};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::services::{ServeDir, ServeFile};

/// Listen configuration for the IDE server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// Directory containing the built web IDE (SPA). Overridable via `IDE_WEB_DIST`.
    pub web_dist: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8081,
            web_dist: std::env::var("IDE_WEB_DIST").unwrap_or_else(|_| "apps/ide-web/dist".into()),
        }
    }
}

impl ServerConfig {
    pub fn address(&self) -> Result<SocketAddr> {
        format!("{}:{}", self.host, self.port)
            .parse()
            .with_context(|| format!("invalid bind address {}:{}", self.host, self.port))
    }
}

/// Shared server state handed to every handler.
#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub keys: Arc<JwtKeys>,
    /// Registry of hosted preview builds (workspaces, state, logs).
    pub preview: Arc<BuildManager>,
    /// Running preview backends and their ports.
    pub runs: Arc<RunManager>,
    /// The per-user WebAssembly plugin hosts.
    pub plugins: Arc<PluginRegistry>,
    /// Deployment service. `None` means `IDE_SECRET_KEY` is not configured,
    /// in which case every integration/deploy route returns 503. There is
    /// deliberately no auto-generated key: a key that changes between
    /// restarts makes every stored credential undecryptable.
    pub deploy: Option<Arc<DeployService>>,
    /// Shared MCP tool/resource dispatcher used by the `/mcp` route.
    pub mcp: Arc<nocode_mcp::McpServer>,
    /// The assistant's provider and limits. Off unless the operator configured
    /// a model; the endpoint answers 503 and the drawer stays hidden.
    pub agent: Arc<crate::agent::AgentRuntime>,
    /// One writer per project. In memory today; a database-backed
    /// implementation only has to satisfy the same trait (plan §15.2.2).
    pub leases: Arc<dyn nocode_mcp::LeaseStore>,
}

/// Build the deploy service from the environment, or report why it is off.
///
/// This is a *feature* switch, not an error: the rest of the IDE stays fully
/// usable when deployments are not configured.
fn deploy_from_env() -> Option<Arc<DeployService>> {
    let vault = match CredentialVault::from_env() {
        Ok(vault) => vault,
        Err(error) => {
            // The error text never contains the key itself, but keep it
            // generic anyway: this is operator guidance, not diagnostics.
            tracing::warn!(
                reason = %error,
                "deploy service disabled; set IDE_SECRET_KEY to a 64-character hex (32-byte) key"
            );
            return None;
        }
    };
    let staging_root = std::env::var("IDE_DEPLOY_STAGING")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("nocode-ide-deploys"));
    Some(Arc::new(DeployService::new(vault, staging_root)))
}

impl AppState {
    pub fn new(db: DatabaseConnection, keys: Arc<JwtKeys>) -> Self {
        Self {
            mcp: mcp_with_provider(&db),
            agent: Arc::new(crate::agent::AgentRuntime::from_env()),
            leases: Arc::new(nocode_mcp::MemoryLeaseStore::new()),
            db,
            keys,
            preview: Arc::new(BuildManager::new(PreviewConfig::from_env())),
            runs: Arc::new(RunManager::new(RunConfig::from_env())),
            plugins: Arc::new(PluginRegistry::default()),
            deploy: deploy_from_env(),
        }
    }

    /// Replace the lease store (tests, or a multi-instance deployment).
    pub fn with_leases(mut self, leases: Arc<dyn nocode_mcp::LeaseStore>) -> Self {
        self.leases = leases;
        self
    }

    /// Replace the assistant runtime (tests, or a host that builds its own
    /// provider).
    pub fn with_agent(mut self, agent: Arc<crate::agent::AgentRuntime>) -> Self {
        self.agent = agent;
        self
    }

    /// Inject a deploy service (tests) or explicitly disable deployments.
    pub fn with_deploy(mut self, deploy: Option<Arc<DeployService>>) -> Self {
        self.deploy = deploy;
        self
    }

    /// Construct the state with explicit preview/run configurations (tests).
    pub fn with_preview(db: DatabaseConnection, keys: Arc<JwtKeys>, config: PreviewConfig) -> Self {
        Self::with_configs(db, keys, config, RunConfig::from_env())
    }

    /// Construct the state with BOTH preview and run configurations injected.
    ///
    /// The run limits used to come only from the environment, so a test could
    /// not exercise per-user `max_runs` without mutating process-wide state.
    pub fn with_configs(
        db: DatabaseConnection,
        keys: Arc<JwtKeys>,
        config: PreviewConfig,
        runs: RunConfig,
    ) -> Self {
        Self {
            mcp: mcp_with_provider(&db),
            agent: Arc::new(crate::agent::AgentRuntime::from_env()),
            leases: Arc::new(nocode_mcp::MemoryLeaseStore::new()),
            db,
            keys,
            preview: Arc::new(BuildManager::new(config)),
            runs: Arc::new(RunManager::new(runs)),
            plugins: Arc::new(PluginRegistry::default()),
            deploy: deploy_from_env(),
        }
    }
}

/// The MCP dispatcher, backed by this server's project tables.
///
/// The `/mcp` route and the agent endpoint share one executor, so a project
/// opened by an MCP client is the same session the agent edits. Without the
/// provider, every `project_*` tool would answer "requires an IDE server
/// connection" — which is the correct answer for the stdio binary, and the
/// wrong one here.
fn mcp_with_provider(db: &DatabaseConnection) -> Arc<nocode_mcp::McpServer> {
    let provider = Arc::new(crate::agent::SeaOrmProjectProvider::new(db.clone()));
    Arc::new(nocode_mcp::McpServer::with_provider(
        nocode_mcp::Config::from_env(),
        provider,
    ))
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

#[derive(Debug, Serialize)]
struct VersionResponse {
    name: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct ReadyResponse {
    status: &'static str,
    database: &'static str,
}

/// Build the IDE server router: API routes plus SPA static serving.
pub fn build_router(state: AppState, web_dist: &str) -> Router {
    let index = ServeFile::new(format!("{web_dist}/index.html"));
    let spa = ServeDir::new(web_dist).not_found_service(index);

    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/api/v1/version", get(version))
        .merge(auth::auth_router())
        .merge(crate::tenancy::tenancy_router())
        .merge(crate::billing::billing_router())
        .merge(crate::admin::admin_router())
        .merge(crate::connectors::connectors_router())
        .merge(crate::versioning::versioning_router())
        .merge(crate::projects::projects_router())
        .merge(crate::generation::generation_router())
        .merge(crate::bpmn::bpmn_router())
        .merge(crate::mcp::mcp_router())
        .merge(crate::agent::agent_router())
        .merge(crate::plugins::plugins_router())
        .merge(crate::integrations::integrations_router())
        .merge(crate::preview::preview_router())
        .merge(preview_server_router())
        .fallback_service(spa)
        .with_state(state)
}

/// Bind and serve until the process is stopped.
pub async fn serve(config: ServerConfig, state: AppState) -> Result<()> {
    let addr = config.address()?;
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!(%addr, web_dist = %config.web_dist, "ide server listening");

    let router = build_router(state.clone(), &config.web_dist);

    // Replay plugin installations from the database at startup.
    if let Err(error) = state.plugins.replay(&state.db).await {
        tracing::warn!(%error, "failed to replay plugins from database");
    }

    // Idle teardown for hosted previews so a long-lived server does not
    // accumulate generated backend processes.
    let idle = state.runs.clone();
    let reaper = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            ticker.tick().await;
            idle.reap_idle().await;
        }
    });

    // Drain in-flight requests on SIGTERM instead of being killed after the
    // container runtime's grace period. `docker stop`, `compose down` and
    // Kubernetes all send SIGTERM first and SIGKILL ten seconds later; without
    // this handler the process ignores SIGTERM, every stop takes the full
    // grace period, and a request mid-flight loses its response.
    let result = axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await;

    reaper.abort();
    state.runs.stop_all().await;
    result.context("ide server error")
}

/// Resolves when the process is asked to stop: Ctrl-C interactively, SIGTERM
/// from a process supervisor or container runtime.
///
/// If a handler cannot be installed the corresponding branch never resolves:
/// failing to listen for a signal must not shut the server down.
async fn shutdown_signal() {
    let ctrl_c = async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {}
            Err(error) => {
                tracing::warn!(%error, "failed to install Ctrl-C handler");
                std::future::pending::<()>().await
            }
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                let _ = signal.recv().await;
            }
            Err(error) => {
                tracing::warn!(%error, "failed to install SIGTERM handler");
                std::future::pending::<()>().await
            }
        }
    };

    // SIGTERM does not exist on Windows; Ctrl-C and the console close event do.
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received, draining in-flight requests");
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "nocode-ide-server",
    })
}

async fn ready(State(state): State<AppState>) -> Json<ReadyResponse> {
    let database = match state.db.get_database_backend() {
        _ if state.db.ping().await.is_ok() => "ok",
        _ => "unavailable",
    };
    Json(ReadyResponse {
        status: "ready",
        database,
    })
}

async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_sqlite_memory;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::sync::OnceLock;
    use tower::ServiceExt;

    fn test_keys() -> Arc<JwtKeys> {
        static KEYS: OnceLock<Arc<JwtKeys>> = OnceLock::new();
        KEYS.get_or_init(|| Arc::new(JwtKeys::generate_ephemeral().expect("test keys")))
            .clone()
    }

    async fn test_router() -> Router {
        let db = connect_sqlite_memory().await.unwrap();
        build_router(AppState::new(db, test_keys()), "apps/ide-web/dist")
    }

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let response = test_router()
            .await
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn ready_endpoint_reports_database() {
        let response = test_router()
            .await
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// Start -> user task -> gateway -> end, used by the `/bpmn/run` test.
    const RUN_WORKFLOW: &str = r#"{
      "id": "55555555-5555-4555-8555-555555555555",
      "name": "Approval",
      "version": "1.0.0",
      "variables": [], "signals": [], "timers": [],
      "nodes": [
        {"id":"66666666-6666-4666-8666-666666666666","name":"Start","node_type":{"type":"StartEvent","config":{"event_type":"none"}},"position":{"x":0.0,"y":0.0},"config":{"boundary_events":[],"execution_listeners":[],"inputs":{},"outputs":{},"properties":{}}},
        {"id":"77777777-7777-4777-8777-777777777777","name":"Review","node_type":{"type":"UserTask","config":{"assignee":null,"candidate_groups":[],"due_date":null,"form_id":null}},"position":{"x":1.0,"y":0.0},"config":{"boundary_events":[],"execution_listeners":[],"inputs":{},"outputs":{},"properties":{}}},
        {"id":"88888888-8888-4888-8888-888888888888","name":"End","node_type":{"type":"EndEvent","config":{"event_type":"none"}},"position":{"x":2.0,"y":0.0},"config":{"boundary_events":[],"execution_listeners":[],"inputs":{},"outputs":{},"properties":{}}}
      ],
      "edges": [
        {"id":"e1000000-0000-4000-8000-000000000001","source":"66666666-6666-4666-8666-666666666666","target":"77777777-7777-4777-8777-777777777777","style":{"animated":false}},
        {"id":"e1000000-0000-4000-8000-000000000002","source":"77777777-7777-4777-8777-777777777777","target":"88888888-8888-4888-8888-888888888888","style":{"animated":false}}
      ],
      "settings": {"async_continuations":false,"exclusive_gateway_default":true,"history_level":"full","is_executable":true,"variables_serialization":"json"},
      "created_at":"2024-01-01T00:00:00Z","updated_at":"2024-01-01T00:00:00Z"
    }"#;

    fn bearer() -> String {
        let claims = crate::auth::jwt::Claims::access(
            "user-1",
            "run@example.com",
            "jti",
            crate::auth::jwt::now_secs(),
        );
        format!("Bearer {}", test_keys().encode(&claims).unwrap())
    }

    #[tokio::test]
    async fn bpmn_run_parks_on_the_first_user_task() {
        let body = serde_json::json!({
            "workflow": serde_json::from_str::<serde_json::Value>(RUN_WORKFLOW).unwrap(),
            "variables": { "amount": 10 },
        });
        let response = test_router()
            .await
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/bpmn/run")
                    .header("authorization", bearer())
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let summary: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(summary["completed"], serde_json::json!(false));
        assert_eq!(summary["status"], serde_json::json!("running"));
        assert_eq!(
            summary["open_tasks"][0]["node_name"],
            serde_json::json!("Review")
        );
        assert_eq!(summary["variables"]["amount"], serde_json::json!(10));
    }

    #[tokio::test]
    async fn bpmn_run_requires_authentication() {
        let response = test_router()
            .await
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/bpmn/run")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn bpmn_run_rejects_non_object_variables() {
        let body = serde_json::json!({
            "workflow": serde_json::from_str::<serde_json::Value>(RUN_WORKFLOW).unwrap(),
            "variables": "nope",
        });
        let response = test_router()
            .await
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/bpmn/run")
                    .header("authorization", bearer())
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
