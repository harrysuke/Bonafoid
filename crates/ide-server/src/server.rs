use crate::auth::{self, JwtKeys};
use crate::plugins::PluginRegistry;
use crate::preview::preview_server_router;
use crate::preview::runs::RunConfig;
use crate::preview::{BuildManager, PreviewConfig, RunManager};
use anyhow::{Context, Result};
use axum::{extract::State, routing::get, Json, Router};
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
}

impl AppState {
    pub fn new(db: DatabaseConnection, keys: Arc<JwtKeys>) -> Self {
        Self {
            db,
            keys,
            preview: Arc::new(BuildManager::new(PreviewConfig::from_env())),
            runs: Arc::new(RunManager::new(RunConfig::from_env())),
            plugins: Arc::new(PluginRegistry::default()),
        }
    }

    /// Construct the state with explicit preview/run configurations (tests).
    pub fn with_preview(db: DatabaseConnection, keys: Arc<JwtKeys>, config: PreviewConfig) -> Self {
        Self {
            db,
            keys,
            preview: Arc::new(BuildManager::new(config)),
            runs: Arc::new(RunManager::new(RunConfig::from_env())),
            plugins: Arc::new(PluginRegistry::default()),
        }
    }
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
        .merge(crate::projects::projects_router())
        .merge(crate::generation::generation_router())
        .merge(crate::bpmn::bpmn_router())
        .merge(crate::plugins::plugins_router())
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

    let result = axum::serve(listener, router).await;

    reaper.abort();
    state.runs.stop_all().await;
    result.context("ide server error")
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
