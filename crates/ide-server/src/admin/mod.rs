//! Platform administration panel (Phase 9).
//!
//! All `/api/v1/admin/*` routes require the signed platform super-admin role;
//! the two `/api/v1/status*` routes are public and deliberately expose only
//! component health and an allowlisted incident projection.

pub mod analytics;
pub mod handlers;
pub mod schema;
pub mod store;

#[cfg(test)]
mod tests;

use axum::{
    routing::{delete, get, patch, post},
    Router,
};

use crate::server::AppState;

use handlers as h;

/// The complete administration and public status router.
pub fn admin_router() -> Router<AppState> {
    Router::new()
        // Static flag paths are registered before the `:key` catch-all.
        .route("/api/v1/admin/flags/evaluate", get(h::evaluate_flag))
        .route("/api/v1/admin/flags/overrides", post(h::set_override))
        .route("/api/v1/admin/tenants", get(h::list_tenants))
        .route(
            "/api/v1/admin/tenants/:id/status",
            patch(h::set_tenant_status),
        )
        .route("/api/v1/admin/users", get(h::list_users))
        .route("/api/v1/admin/users/:id", patch(h::update_user))
        .route("/api/v1/admin/users/:id/impersonate", post(h::impersonate))
        .route("/api/v1/admin/analytics", get(h::analytics))
        .route("/api/v1/admin/usage", get(h::usage))
        .route(
            "/api/v1/admin/flags",
            get(h::list_flags).post(h::create_flag),
        )
        .route(
            "/api/v1/admin/flags/:key",
            patch(h::update_flag).delete(h::delete_flag),
        )
        .route("/api/v1/admin/audit", get(h::audit))
        .route("/api/v1/admin/impersonations", get(h::impersonations))
        .route("/api/v1/status", get(h::status))
        .route("/api/v1/status/incidents", get(h::incidents))
}
