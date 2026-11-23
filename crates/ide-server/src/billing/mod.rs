//! Billing and monetization (Phase 8).
//!
//! Provider-agnostic subscriptions, seats, entitlements and usage meters,
//! plus a Stripe adapter whose webhook verification works without any extra
//! network dependency. See [`stripe`] for the checkout transport limitation.

pub mod engine;
pub mod handlers;
pub mod schema;
pub mod store;
pub mod stripe;

#[cfg(test)]
mod tests;

use axum::{
    routing::{get, post},
    Router,
};

use crate::server::AppState;

/// The billing API. Mutating routes require an authenticated session; the
/// Stripe webhook authenticates itself with a signed body instead.
pub fn billing_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/billing/plans",
            get(handlers::list_plans).post(handlers::create_plan),
        )
        .route(
            "/api/v1/billing/subscription",
            get(handlers::current_subscription),
        )
        .route("/api/v1/billing/subscribe", post(handlers::subscribe))
        .route("/api/v1/billing/cancel", post(handlers::cancel))
        .route("/api/v1/billing/invoices", get(handlers::invoices))
        .route("/api/v1/billing/entitlements", get(handlers::entitlements))
        .route("/api/v1/billing/usage", get(handlers::usage))
        .route("/api/v1/billing/record-usage", post(handlers::record_usage))
        .route("/api/v1/billing/checkout", post(handlers::checkout))
        .route("/api/v1/billing/portal", post(handlers::portal))
        .route("/api/v1/billing/webhooks/stripe", post(handlers::webhook))
}
