//! Shared JSON API error type and CSRF helper for mutating endpoints.

use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use crate::auth;

/// A JSON API error mapped to an HTTP status and a stable error code.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    /// A feature is present but cannot be used because the operator has not
    /// configured it. Distinct from `Internal`: a 500 says "the server broke",
    /// while a missing `IDE_SECRET_KEY` is a setup step the operator owns.
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    /// A request or response body exceeded the limit for its route.
    ///
    /// Distinct from `Internal` on purpose: the preview proxy used to report a
    /// body that was too large as a 500, which reads as "the server is broken"
    /// when the truth is "the body was too big".
    #[error("payload too large: {0}")]
    PayloadTooLarge(String),
    /// The proxied preview backend did not answer in time.
    #[error("gateway timeout: {0}")]
    GatewayTimeout(String),
    #[error("internal error")]
    Internal,
}

impl ApiError {
    /// The HTTP status, stable error code and human-readable message.
    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            ApiError::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, "bad_request", message.clone())
            }
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "authentication required".to_string(),
            ),
            ApiError::Forbidden(message) => (StatusCode::FORBIDDEN, "forbidden", message.clone()),
            ApiError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "resource not found".to_string(),
            ),
            ApiError::Conflict(message) => (StatusCode::CONFLICT, "conflict", message.clone()),
            ApiError::ServiceUnavailable(message) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                message.clone(),
            ),
            ApiError::PayloadTooLarge(message) => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                message.clone(),
            ),
            ApiError::GatewayTimeout(message) => (
                StatusCode::GATEWAY_TIMEOUT,
                "gateway_timeout",
                message.clone(),
            ),
            ApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "internal server error".to_string(),
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        let body = Json(serde_json::json!({ "error": code, "message": message }));
        (status, body).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        tracing::error!(error = %error, "api internal error");
        ApiError::Internal
    }
}

impl From<auth::AuthError> for ApiError {
    fn from(error: auth::AuthError) -> Self {
        // `AuthError::parts` is private, so map the public variants directly.
        match error {
            auth::AuthError::BadRequest(message) => ApiError::BadRequest(message.to_string()),
            auth::AuthError::Unauthorized => ApiError::Unauthorized,
            auth::AuthError::Forbidden(message) => ApiError::Forbidden(message.to_string()),
            auth::AuthError::Conflict(message) => ApiError::Conflict(message.to_string()),
            auth::AuthError::Internal => ApiError::Internal,
        }
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        tracing::error!(error = %error, "json serialization error");
        ApiError::Internal
    }
}

/// Deployment errors must never carry a PAT into an HTTP response. Every
/// message that leaves this conversion goes through `nocode_deploy::safe_error`,
/// which masks `ghp_`/`github_pat_`/`Bearer` and inline URL credentials.
impl From<nocode_deploy::DeployError> for ApiError {
    fn from(error: nocode_deploy::DeployError) -> Self {
        let safe = nocode_deploy::safe_error(&error);
        match error {
            nocode_deploy::DeployError::Idempotent => ApiError::Conflict(
                "this generated commit is already deployed".to_string(),
            ),
            nocode_deploy::DeployError::BranchExists(branch) => ApiError::Conflict(format!(
                "branch `{branch}` already exists and was not created by a deployment of this project"
            )),
            nocode_deploy::DeployError::Config(_)
            | nocode_deploy::DeployError::Compose(_)
            | nocode_deploy::DeployError::GitHub(_) => ApiError::BadRequest(safe),
            _ => {
                // Vault failures in particular can indicate a rotated master
                // key: log the scrubbed reason, but do not expose internals.
                tracing::error!(error = %safe, "deployment failed");
                ApiError::Internal
            }
        }
    }
}

impl From<sea_orm::DbErr> for ApiError {
    fn from(error: sea_orm::DbErr) -> Self {
        tracing::error!(error = %error, "database error");
        ApiError::Internal
    }
}

/// Enforce the double-submit CSRF token for cookie-authenticated mutations.
///
/// Bearer-token requests are exempt: a cross-origin browser cannot read or
/// attach the token, so forging them is not possible.
pub fn require_csrf(headers: &HeaderMap) -> Result<(), ApiError> {
    if auth::bearer_token(headers).is_some() {
        return Ok(());
    }
    auth::verify_csrf(headers).map_err(ApiError::from)
}
