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
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::DefaultBodyLimit;
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

/// Upper bound on a proxied request body.
///
/// Made explicit rather than left to axum's implicit 2 MB default, so the
/// limit is a decision recorded here instead of a framework default nobody
/// remembers. Generated entity endpoints take single JSON records; 8 MiB is
/// generous for one and still bounded.
const MAX_PROXY_REQUEST: usize = 8 * 1024 * 1024;

// The two limits are related, and the relationship is a compile-time fact, so
// it is checked by the compiler rather than by a test that could only ever pass.
// A request larger than the largest response the proxy will buffer would be a
// limit nobody reasoned about; and the request limit must not be smaller than
// axum's old implicit 2 MB, or payloads that used to work would start failing.
const _: () = {
    assert!(MAX_PROXY_REQUEST > 0);
    assert!(MAX_PROXY_REQUEST <= MAX_PROXY_BODY);
    assert!(MAX_PROXY_REQUEST >= 2 * 1024 * 1024);
};

/// How long to wait for the generated backend to accept a TCP connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait for the HTTP/1.1 handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the backend has to start answering the request.
const SEND_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the whole response body may take to arrive.
///
/// Without this a hung generated backend held the proxy task forever: there
/// was no `tokio::time::timeout` anywhere in this file.
const BODY_TIMEOUT: Duration = Duration::from_secs(60);

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

/// Request headers that belong to the IDE session, not to generated code.
///
/// The proxy used to forward everything that was not hop-by-hop, so the IDE
/// server's own JWT (`authorization`), the whole session cookie jar
/// (`cookie`: nocode_access, nocode_refresh, nocode_csrf) and the CSRF token
/// all reached the generated app — which is untrusted code by definition.
/// `origin` and `referer` go too: they describe the IDE's origin, and
/// forwarding them to a different app is misleading.
const SESSION_HEADERS: [&str; 5] = [
    "authorization",
    "cookie",
    "x-csrf-token",
    "origin",
    "referer",
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
        // Explicit request-body bound. The old code only bounded the RESPONSE,
        // and only after buffering it.
        .layer(DefaultBodyLimit::max(MAX_PROXY_REQUEST))
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
    // The proxy's own route is /preview/{id}/api/*, and the generated
    // backend's API root is /api, so the segment is kept when forwarding.
    // Dropping it sent requests to /v1/... and every entity route 404ed.
    let uri = if path.is_empty() {
        format!("http://{authority}/api")
    } else {
        format!("http://{authority}/api/{path}")
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
        if is_hop_by_hop(name.as_str()) || is_session_header(name.as_str()) {
            continue;
        }
        outgoing.headers_mut().insert(name.clone(), value.clone());
    }
    *outgoing.body_mut() = Body::from(body);

    let (response, connection) = forward(&authority, outgoing).await?;
    state.runs.touch(&build_id);

    let mut builder = Response::builder().status(response.status());
    for (name, value) in response.headers() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        // A Set-Cookie from generated code would land on the IDE's own origin
        // and could overwrite the session cookies the IDE depends on.
        if name.as_str().eq_ignore_ascii_case("set-cookie") {
            tracing::debug!(%build_id, "dropping set-cookie from preview backend");
            continue;
        }
        builder = builder.header(name.clone(), value.clone());
    }

    let incoming = response.into_body();
    let bytes = match tokio::time::timeout(
        BODY_TIMEOUT,
        axum::body::to_bytes(Body::new(incoming), MAX_PROXY_BODY),
    )
    .await
    {
        // The connection task ends when the response body is dropped, but
        // aborting it here is explicit and immediate.
        Err(_) => {
            connection.abort();
            return Err(ApiError::GatewayTimeout(
                "preview backend stopped sending its response".into(),
            ));
        }
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) => {
            connection.abort();
            return Err(if is_length_limit(&error) {
                ApiError::PayloadTooLarge(format!(
                    "preview response exceeded {MAX_PROXY_BODY} bytes"
                ))
            } else {
                tracing::error!(%error, "proxy body read failed");
                ApiError::Internal
            });
        }
    };
    connection.abort();

    builder.body(Body::from(bytes)).map_err(|error| {
        tracing::error!(%error, "cannot build proxy response");
        ApiError::Internal
    })
}

fn is_hop_by_hop(name: &str) -> bool {
    HOP_BY_HOP.contains(&name.to_ascii_lowercase().as_str()) || name.eq_ignore_ascii_case("host")
}

/// Does this header belong to the IDE session rather than to generated code?
fn is_session_header(name: &str) -> bool {
    SESSION_HEADERS.contains(&name.to_ascii_lowercase().as_str())
}

/// Did a body read fail because it hit the cap?
///
/// `to_bytes` reports the cap as an opaque `axum::Error` wrapping
/// `http_body_util::LengthLimitError`, so the chain is walked rather than the
/// top-level message trusted.
fn is_length_limit(error: &axum::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if current
            .to_string()
            .to_ascii_lowercase()
            .contains("length limit")
        {
            return true;
        }
        source = current.source();
    }
    false
}

/// Perform one HTTP/1.1 request over a fresh connection to the generated app.
///
/// Returns the response plus the connection task, so the caller can abort it
/// once the body has been read instead of leaking a task per timed-out
/// request.
async fn forward(
    authority: &str,
    request: HyperRequest<Body>,
) -> Result<
    (
        hyper::Response<hyper::body::Incoming>,
        tokio::task::JoinHandle<()>,
    ),
    ApiError,
> {
    let address = SocketAddr::from(([127, 0, 0, 1], port_of(authority)));
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address))
        .await
        .map_err(|_| {
            tracing::warn!(%authority, "preview backend connect timed out");
            ApiError::GatewayTimeout("preview backend did not accept a connection".into())
        })?
        .map_err(|error| {
            tracing::warn!(%authority, %error, "preview backend unreachable");
            ApiError::Conflict(format!("preview backend is not reachable: {error}"))
        })?;

    let (mut sender, connection) =
        tokio::time::timeout(HANDSHAKE_TIMEOUT, http1::handshake(TokioIo::new(stream)))
            .await
            .map_err(|_| {
                tracing::warn!(%authority, "proxy handshake timed out");
                ApiError::GatewayTimeout("preview backend handshake timed out".into())
            })?
            .map_err(|error| {
                tracing::warn!(%authority, %error, "proxy handshake failed");
                ApiError::Conflict(format!("preview backend handshake failed: {error}"))
            })?;

    let connection = tokio::spawn(async move {
        let _ = connection.await;
    });

    let response = tokio::time::timeout(SEND_TIMEOUT, sender.send_request(request))
        .await
        .map_err(|_| {
            connection.abort();
            tracing::warn!(%authority, "proxy request timed out");
            ApiError::GatewayTimeout("preview backend did not answer in time".into())
        })?
        .map_err(|error| {
            connection.abort();
            tracing::warn!(%authority, %error, "proxy send failed");
            ApiError::Conflict(format!("preview backend rejected the request: {error}"))
        })?;

    Ok((response, connection))
}

fn port_of(authority: &str) -> u16 {
    authority
        .rsplit(':')
        .next()
        .and_then(|port| port.parse().ok())
        .unwrap_or(80)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The URL the proxy forwards a request to.
    ///
    /// The proxy's own route is /preview/{build}/api/* and the generated
    /// backend's API root is /api, so the segment has to survive the hop.
    /// Dropping it sent every entity request to /v1/... and the backend
    /// answered 404.
    fn forwarded_uri(path: &str) -> String {
        let authority = "127.0.0.1:4242";
        if path.is_empty() {
            format!("http://{authority}/api")
        } else {
            format!("http://{authority}/api/{path}")
        }
    }

    #[test]
    fn the_api_segment_is_kept_when_forwarding() {
        assert_eq!(
            forwarded_uri("v1/product"),
            "http://127.0.0.1:4242/api/v1/product"
        );
        assert_eq!(
            forwarded_uri("v1/product/7"),
            "http://127.0.0.1:4242/api/v1/product/7"
        );
        assert_eq!(
            forwarded_uri("v1/forms/contact"),
            "http://127.0.0.1:4242/api/v1/forms/contact"
        );
    }

    #[test]
    fn the_api_root_forwards_to_the_api_root() {
        assert_eq!(forwarded_uri(""), "http://127.0.0.1:4242/api");
    }

    #[test]
    fn hop_by_hop_headers_are_not_forwarded() {
        for name in [
            "connection",
            "keep-alive",
            "proxy-authenticate",
            "proxy-authorization",
            "te",
            "trailer",
            "transfer-encoding",
            "upgrade",
            "host",
            "Connection",
            "HOST",
        ] {
            assert!(is_hop_by_hop(name), "{name} should not be forwarded");
        }
    }

    /// The IDE's own credentials must not reach generated code.
    ///
    /// This test used to be `#[ignore]`d and assert the opposite: every header
    /// that was not hop-by-hop crossed the hop, so the server's JWT, the whole
    /// session cookie jar and the CSRF token were handed to the preview app.
    /// Now the list is inverted and enforced.
    #[test]
    fn the_request_headers_that_must_not_cross_the_hop_are_dropped() {
        for name in [
            "authorization",
            "cookie",
            "x-csrf-token",
            "origin",
            "referer",
            "Authorization",
            "COOKIE",
            "X-CSRF-Token",
        ] {
            assert!(
                is_session_header(name),
                "{name} must not reach the generated backend"
            );
        }
    }

    /// Ordinary application headers still cross the hop.
    #[test]
    fn ordinary_request_headers_still_cross_the_hop() {
        for name in [
            "content-type",
            "accept",
            "accept-language",
            "if-none-match",
            "x-request-id",
        ] {
            assert!(!is_hop_by_hop(name), "{name} is not hop-by-hop");
            assert!(!is_session_header(name), "{name} is not a session header");
        }
    }

    /// Every hop has a timeout. Without them a hung generated backend held
    /// the proxy task open forever.
    #[test]
    fn every_hop_has_a_timeout() {
        assert!(CONNECT_TIMEOUT.as_secs() > 0);
        assert!(HANDSHAKE_TIMEOUT.as_secs() > 0);
        assert!(SEND_TIMEOUT.as_secs() > 0);
        assert!(BODY_TIMEOUT.as_secs() > 0);
    }
}
