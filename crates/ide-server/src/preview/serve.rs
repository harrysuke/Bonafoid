//! Serving and proxying a hosted preview.
//!
//! Two routes, both under `/preview/{build_id}`:
//!
//! - `/preview/{build_id}/*path` serves the built frontend (`dist/`) with SPA
//!   fallback to `index.html`.
//! - `/preview/{build_id}/api/*path` reverse-proxies to the running generated
//!   backend on its allocated port.
//!
//! Both check build ownership against the session, so a build id alone is never
//! enough to read another user's preview.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use hyper::client::conn::http1;
use hyper::Request as HyperRequest;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tower::ServiceExt;

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::preview::manager::BuildState;
use crate::server::AppState;

/// Upper bound on a proxied response body (generated APIs return JSON).
const MAX_PROXY_BODY: usize = 32 * 1024 * 1024;

/// Headers that must not be forwarded across a proxy hop.
const HOP_BY_HOP: [&str; 8] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// The preview routes (static + proxy).
///
/// The proxy accepts every method: the generated app exposes entity CRUD, not
/// just reads.
pub fn preview_server_router() -> Router<AppState> {
    Router::new()
        .route("/preview/:build_id", get(static_root))
        .route("/preview/:build_id/", get(static_root))
        .route("/preview/:build_id/*path", get(serve_static))
        .route("/preview/:build_id/api", any(proxy_root))
        .route("/preview/:build_id/api/*path", any(proxy_api))
}

/// Resolve a build directory, enforcing ownership and readiness.
async fn ready_dist(
    state: &AppState,
    build_id: &str,
    user: &AuthUser,
) -> Result<PathBuf, ApiError> {
    let record = state
        .preview
        .get_owned(build_id, &user.id)
        .ok_or(ApiError::NotFound)?;
    if record.state != BuildState::Ready {
        return Err(ApiError::Conflict(format!(
            "build is {} (not ready)",
            serde_json::to_value(record.state)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_else(|| "unavailable".into())
        )));
    }
    Ok(record.dist_dir)
}

/// `/preview/{build_id}` -> the SPA entry point.
async fn static_root(
    State(state): State<AppState>,
    user: AuthUser,
    Path(build_id): Path<String>,
) -> Result<Response, ApiError> {
    serve_static(State(state), user, Path((build_id, String::new()))).await
}

/// `/preview/{build_id}/*path` -> a file from `dist/`, SPA fallback for deep links.
async fn serve_static(
    State(state): State<AppState>,
    user: AuthUser,
    Path((build_id, path)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let dist = ready_dist(&state, &build_id, &user).await?;
    let index = dist.join("index.html");
    if !index.exists() {
        return Err(ApiError::NotFound);
    }

    let request = HyperRequest::builder()
        .uri(format!("/{path}"))
        .body(Body::empty())
        .map_err(|error| {
            tracing::error!(%error, "cannot build preview file request");
            ApiError::Internal
        })?;

    // A fresh `ServeDir` per request is cheap (no state) and lets the dist
    // directory be dynamic per build id.
    let service = tower_http::services::ServeDir::new(&dist)
        .not_found_service(tower_http::services::ServeFile::new(&index));

    let response = service.oneshot(request).await.map_err(|error| {
        tracing::error!(%error, "preview static service failed");
        ApiError::Internal
    })?;

    Ok(response.into_response())
}

/// `/preview/{build_id}/api` -> a running backend is required.
async fn proxy_root(
    State(state): State<AppState>,
    user: AuthUser,
    Path(build_id): Path<String>,
) -> Result<Response, ApiError> {
    proxy_api(
        State(state),
        user,
        Path((build_id, String::new())),
        Method::GET,
        HeaderMap::new(),
        Bytes::new(),
    )
    .await
}

/// `/preview/{build_id}/api/*path` -> reverse proxy to the generated backend.
async fn proxy_api(
    State(state): State<AppState>,
    user: AuthUser,
    Path((build_id, path)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    // Ensure the caller owns the build even though the run record already
    // scopes by user (keeps the 404 semantics identical for static + proxy).
    let _ = ready_dist(&state, &build_id, &user).await?;

    let run = state.runs.running_for(&build_id, &user.id).ok_or_else(|| {
        ApiError::Conflict("preview backend is not running (start it from the IDE)".into())
    })?;

    let authority = format!("127.0.0.1:{}", run.port);
    let uri = if path.is_empty() {
        format!("http://{authority}/")
    } else {
        format!("http://{authority}/{path}")
    };

    let mut outgoing = HyperRequest::builder()
        .method(method.clone())
        .uri(&uri)
        .body(Body::empty())
        .map_err(|error| {
            tracing::error!(%error, uri, "cannot build proxy request");
            ApiError::Internal
        })?;
    for (name, value) in headers.iter() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        outgoing.headers_mut().insert(name.clone(), value.clone());
    }
    *outgoing.body_mut() = Body::from(body);

    let response = forward(&authority, outgoing).await?;
    state.runs.touch(&build_id);

    let mut builder = Response::builder().status(response.status());
    for (name, value) in response.headers() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        builder = builder.header(name.clone(), value.clone());
    }
    let incoming = response.into_body();
    let bytes = axum::body::to_bytes(Body::new(incoming), MAX_PROXY_BODY)
        .await
        .map_err(|error| {
            tracing::error!(%error, "proxy body read failed");
            ApiError::Internal
        })?;
    builder.body(Body::from(bytes)).map_err(|error| {
        tracing::error!(%error, "cannot build proxy response");
        ApiError::Internal
    })
}

fn is_hop_by_hop(name: &str) -> bool {
    HOP_BY_HOP.contains(&name.to_ascii_lowercase().as_str()) || name.eq_ignore_ascii_case("host")
}

/// Perform one HTTP/1.1 request over a fresh connection to the generated app.
async fn forward(
    authority: &str,
    request: HyperRequest<Body>,
) -> Result<hyper::Response<hyper::body::Incoming>, ApiError> {
    let stream = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port_of(authority))))
        .await
        .map_err(|error| {
            tracing::warn!(%authority, %error, "preview backend unreachable");
            ApiError::Conflict(format!("preview backend is not reachable: {error}"))
        })?;

    let (mut sender, connection) =
        http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| {
                tracing::warn!(%authority, %error, "proxy handshake failed");
                ApiError::Conflict(format!("preview backend handshake failed: {error}"))
            })?;

    tokio::spawn(async move {
        let _ = connection.await;
    });

    sender.send_request(request).await.map_err(|error| {
        tracing::warn!(%authority, %error, "proxy send failed");
        ApiError::Conflict(format!("preview backend rejected the request: {error}"))
    })
}

fn port_of(authority: &str) -> u16 {
    authority
        .rsplit(':')
        .next()
        .and_then(|port| port.parse().ok())
        .unwrap_or(80)
}
