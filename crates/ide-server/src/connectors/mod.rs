//! Phase 10: third-party and API connectors.
//!
//! Tenant-scoped connector instances are stored with redacted config and
//! vault-sealed secrets. `http_rest` and `webhook` instances can make real
//! plain-HTTP calls; the remaining kinds validate their configuration and
//! report honestly that no transport is implemented.

pub mod handlers;
pub mod runtime;
pub mod schema;
pub mod store;

#[cfg(test)]
mod tests;

use axum::{
    extract::Request,
    http::Method,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Router,
};

use crate::{
    auth::{self, COOKIE_ACCESS},
    error::require_csrf,
    server::AppState,
};

/// Reject cookie-authenticated mutations before the JSON extractor runs, so
/// "no CSRF token => nothing parsed or mutated" holds even for a malformed
/// body. Bearer requests are exempt by design.
async fn csrf_before_body(request: Request, next: Next) -> Response {
    let read_only = matches!(
        request.method(),
        &Method::GET | &Method::HEAD | &Method::OPTIONS
    );
    let cookie_session = auth::cookie_value(COOKIE_ACCESS, request.headers()).is_some();
    if !read_only && cookie_session {
        if let Err(error) = require_csrf(request.headers()) {
            return error.into_response();
        }
    }
    next.run(request).await
}

/// Exactly seven API routes: catalog, list/create, patch/delete, test, invoke.
pub fn connectors_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/connectors/catalog", get(handlers::catalog))
        .route(
            "/api/v1/connectors",
            get(handlers::list).post(handlers::create),
        )
        .route(
            "/api/v1/connectors/:id",
            patch(handlers::patch).delete(handlers::delete),
        )
        .route("/api/v1/connectors/:id/test", post(handlers::test))
        .route("/api/v1/connectors/:id/invoke", post(handlers::invoke))
        .layer(middleware::from_fn(csrf_before_body))
}
