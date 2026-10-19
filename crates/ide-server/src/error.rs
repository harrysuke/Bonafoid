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
