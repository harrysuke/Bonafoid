use crate::auth::{self, JwtKeys};
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
}

impl AppState {
    pub fn new(db: DatabaseConnection, keys: Arc<JwtKeys>) -> Self {
        Self { db, keys }
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
    axum::serve(listener, build_router(state, &config.web_dist))
        .await
        .context("ide server error")?;
    Ok(())
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
}
