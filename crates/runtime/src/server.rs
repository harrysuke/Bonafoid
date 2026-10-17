use anyhow::{Context, Result};
use axum::{
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::net::SocketAddr;
use tokio::net::TcpListener;

/// Minimal serving runtime for a generated application.
///
/// Owns the HTTP listener configuration and exposes an Axum [`Router`] with a
/// health probe and a version endpoint. Model-driven CRUD routes can be layered
/// on top of [`RuntimeServer::router`].
#[derive(Debug, Clone)]
pub struct RuntimeServer {
    pub port: u16,
    pub host: String,
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

impl RuntimeServer {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            host: "0.0.0.0".to_string(),
        }
    }

    /// Build the runtime router.
    pub fn router(&self) -> Router {
        Router::new()
            .route("/health", get(health))
            .route("/api/v1/version", get(version))
            .route("/api/v1/echo", post(echo))
    }

    pub fn address(&self) -> Result<SocketAddr> {
        format!("{}:{}", self.host, self.port)
            .parse()
            .with_context(|| format!("invalid bind address {}:{}", self.host, self.port))
    }

    /// Bind and serve until the process is stopped.
    pub async fn serve(self) -> Result<()> {
        let addr = self.address()?;
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("failed to bind {addr}"))?;
        tracing::info!(%addr, "runtime server listening");
        axum::serve(listener, self.router())
            .await
            .context("runtime server error")?;
        Ok(())
    }
}

impl Default for RuntimeServer {
    fn default() -> Self {
        Self::new(8080)
    }
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "nocode-runtime",
    })
}

async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn echo(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
    Json(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let server = RuntimeServer::new(0);
        let response = server
            .router()
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
    async fn echo_round_trips_json() {
        let server = RuntimeServer::new(0);
        let response = server
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/echo")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"hello":"world"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
