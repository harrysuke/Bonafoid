//! GitHub / Docker Hub integrations and deployment history.
//!
//! This module is the HTTP surface over `crates/deploy`. Both the web server
//! and the desktop host use that crate, so there is exactly one
//! implementation of credential encryption, git pushing and CI status.
//!
//! # Status honesty
//! A successful `git push` is reported as `pushed`, NOT as "deployed". The
//! image build happens later on a GitHub runner, and only a workflow run
//! whose `head_sha` matches this deployment's commit may promote it to
//! `ci_succeeded`. `deployed` is derived from that status rather than being
//! stored, so a caller cannot disagree with the CI truth.

pub mod handlers;
pub mod store;

use axum::extract::Request;
use axum::http::Method;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Serialize;
use std::sync::Arc;

use crate::auth::{self, COOKIE_ACCESS};
use crate::error::{require_csrf, ApiError};
use crate::server::AppState;
use nocode_deploy::{DeployService, DeploymentStatus};

/// The message shown when the operator has not configured the master key.
/// It is guidance, never a diagnostic: no key material is ever included.
pub const SETUP_MESSAGE: &str = "deployments are not configured: set IDE_SECRET_KEY to a \
     64-character hex (32-byte) master key and restart the server";

/// Feature switch. A missing key disables only deployment routes; the rest of
/// the IDE remains usable. Per the user's decision this is a 503, not a
/// silently generated key that would make stored credentials undecryptable
/// after a restart.
pub fn require_deploy(state: &AppState) -> Result<Arc<DeployService>, ApiError> {
    state
        .deploy
        .clone()
        .ok_or_else(|| ApiError::ServiceUnavailable(SETUP_MESSAGE.to_string()))
}

/// Reject cookie-authenticated mutations that lack a matching CSRF token
/// *before* any handler extractor runs.
///
/// Axum extracts `Json<T>` before it calls a handler, so a handler-level
/// `require_csrf` alone lets a malformed body answer 422 first and lets an
/// unauthenticated cross-origin request reach the JSON parser. Deciding
/// authorisation in a layer keeps "no CSRF => nothing is parsed or mutated"
/// true regardless of the body. Handlers still call `require_csrf` as
/// defence in depth.
///
/// Requests with no session cookie are left to `AuthUser` (401). Bearer
/// requests are exempt from CSRF by design, as enforced by `require_csrf`.
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

pub fn integrations_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/integrations/github",
            get(handlers::get_github).post(handlers::set_github),
        )
        .route(
            "/api/v1/integrations/dockerhub",
            get(handlers::get_dockerhub).post(handlers::set_dockerhub),
        )
        .route(
            "/api/v1/projects/:id/deploy",
            post(handlers::deploy_project),
        )
        .route(
            "/api/v1/projects/:id/deployments",
            get(handlers::list_deployments),
        )
        .layer(middleware::from_fn(csrf_before_body))
}

/// What the API says about one deployment. `deployed` and `ci_state` come
/// from the deploy crate's own record, not from a separate flag.
#[derive(Debug, Serialize)]
pub struct DeploymentResponse {
    pub id: String,
    pub project_id: String,
    pub commit_sha: String,
    pub status: DeploymentStatus,
    pub deployed: bool,
    pub ci_state: &'static str,
    pub github_run_id: Option<u64>,
    pub github_run_url: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl DeploymentResponse {
    pub fn from_row(row: &store::DeploymentRow) -> Self {
        let deployment = row.to_deployment();
        Self {
            id: row.id.clone(),
            project_id: row.project_id.clone(),
            commit_sha: row.commit_sha.clone(),
            status: deployment.status,
            deployed: deployment.deployed(),
            ci_state: deployment.ci_state(),
            github_run_id: deployment.github_run_id,
            github_run_url: deployment.github_run_url.clone(),
            error_message: deployment.error_message.clone(),
            created_at: row.created_at.clone(),
            updated_at: row.updated_at.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
