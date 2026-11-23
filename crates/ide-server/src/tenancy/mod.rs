//! SaaS tenancy: tenants, memberships, roles and tenant isolation (Phase 7).
//!
//! The signed access token carries the active tenant and role set. All owned
//! queries include `tenant_id` in their WHERE clause; no handler accepts a
//! tenant id from a request body for authorization purposes.

pub mod handlers;
pub mod schema;
pub mod store;

use axum::{
    routing::{get, post},
    Router,
};

use crate::server::AppState;

/// The tenant API. All routes require an authenticated session.
pub fn tenancy_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/tenants",
            get(handlers::list).post(handlers::create),
        )
        .route(
            "/api/v1/tenants/:id",
            axum::routing::patch(handlers::update),
        )
        .route("/api/v1/tenants/:id/switch", post(handlers::switch))
        .route(
            "/api/v1/tenants/:id/members",
            get(handlers::members).post(handlers::add_member),
        )
        .route(
            "/api/v1/tenants/:id/members/:user_id",
            axum::routing::delete(handlers::remove_member),
        )
}

#[cfg(test)]
mod tests;
