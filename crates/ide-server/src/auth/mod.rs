//! Authentication: password hashing, RS256 tokens, cookie sessions and routes.
//!
//! Sessions are cookie-based: the access token lives in the `nocode_access`
//! cookie (or an `Authorization: Bearer` header), the refresh token in the
//! `nocode_refresh` cookie, and a double-submit CSRF token in the `nocode_csrf`
//! cookie that must be echoed in the `x-csrf-token` header for state-changing
//! requests.

pub mod handlers;
pub mod jwt;
pub mod password;
pub mod store;

pub use jwt::JwtKeys;

use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use std::collections::HashMap;

use crate::server::AppState;

/// Cookie carrying the short-lived access token (HttpOnly).
pub const COOKIE_ACCESS: &str = "nocode_access";
/// Cookie carrying the long-lived refresh token (HttpOnly).
pub const COOKIE_REFRESH: &str = "nocode_refresh";
/// Cookie carrying the non-HttpOnly double-submit CSRF token.
pub const COOKIE_CSRF: &str = "nocode_csrf";
/// Header that must echo the CSRF cookie on state-changing requests.
pub const CSRF_HEADER: &str = "x-csrf-token";

/// Errors returned by authentication handlers.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// 400: the request was malformed (e.g. invalid email or weak password).
    #[error("bad request: {0}")]
    BadRequest(&'static str),
    /// 401: missing or invalid credentials.
    #[error("unauthorized")]
    Unauthorized,
    /// 403: authenticated but not permitted.
    #[error("forbidden: {0}")]
    Forbidden(&'static str),
    /// 409: the request conflicts with existing state.
    #[error("conflict: {0}")]
    Conflict(&'static str),
    /// 500: an unexpected internal failure.
    #[error("internal error")]
    Internal,
}

impl AuthError {
    /// Map the error to an HTTP status and a caller-facing message.
    fn parts(&self) -> (StatusCode, &'static str) {
        match self {
            AuthError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            AuthError::Unauthorized => (StatusCode::UNAUTHORIZED, "authentication required"),
            AuthError::Forbidden(message) => (StatusCode::FORBIDDEN, message),
            AuthError::Conflict(message) => (StatusCode::CONFLICT, message),
            AuthError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal server error"),
        }
    }
}

impl From<anyhow::Error> for AuthError {
    fn from(error: anyhow::Error) -> Self {
        tracing::error!(error = %error, "auth internal error");
        AuthError::Internal
    }
}

/// Tenant lookups during token issuance use the shared store, which reports
/// `ApiError`. The wire-facing auth handlers speak `AuthError`. The store's
/// string variants are only produced by request validation, never by a
/// database detail, so a stable static message is enough here; no heap
/// value is leaked into a `'static` lifetime.
impl From<crate::error::ApiError> for AuthError {
    fn from(error: crate::error::ApiError) -> Self {
        match error {
            crate::error::ApiError::BadRequest(_) => AuthError::BadRequest("invalid request"),
            crate::error::ApiError::Forbidden(_) => AuthError::Forbidden("forbidden"),
            crate::error::ApiError::Conflict(_) => AuthError::Conflict("conflict"),
            crate::error::ApiError::Unauthorized => AuthError::Unauthorized,
            _ => {
                tracing::error!(error = %error, "auth store error");
                AuthError::Internal
            }
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = self.parts();
        let code = match status {
            StatusCode::BAD_REQUEST => "bad_request",
            StatusCode::UNAUTHORIZED => "unauthorized",
            StatusCode::FORBIDDEN => "forbidden",
            StatusCode::CONFLICT => "conflict",
            _ => "internal_error",
        };
        let body = Json(serde_json::json!({ "error": code, "message": message }));
        (status, body).into_response()
    }
}

/// An authenticated user extracted from a request.
///
/// `tenant_id` and `roles` come from the SIGNED token claim, never from a
/// query parameter or header the caller controls. Tokens are short-lived
/// (15 minutes), so a role change takes effect at the next refresh.
#[derive(Debug, Clone)]
pub struct AuthUser {
    /// User id (the JWT subject).
    pub id: String,
    /// User email (convenience only; not used for authorization).
    pub email: String,
    /// Active tenant, signed into the token by the auth handler.
    pub tenant_id: String,
    /// Tenant roles (`owner`/`admin`/`member`) from the signed claim.
    pub roles: Vec<String>,
    /// Platform-level role. `Some("super_admin")` is NOT a tenant role.
    pub platform_role: Option<String>,
}

impl AuthUser {
    /// Whether the user holds a tenant role.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|value| value == role)
    }

    /// Whether the user may perform tenant-admin actions.
    pub fn is_tenant_admin(&self) -> bool {
        self.has_role("owner") || self.has_role("admin")
    }

    /// Whether the user is a platform super administrator.
    pub fn is_super_admin(&self) -> bool {
        self.platform_role.as_deref() == Some("super_admin")
    }

    fn from_claims(claims: jwt::Claims) -> Self {
        Self {
            id: claims.sub,
            email: claims.email,
            tenant_id: claims.tenant_id,
            roles: claims.roles,
            platform_role: claims.platform_role,
        }
    }
}

#[async_trait::async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers)
            .or_else(|| cookie_value(COOKIE_ACCESS, &parts.headers))
            .ok_or(AuthError::Unauthorized)?;
        let claims = state
            .keys
            .decode_access(&token)
            .map_err(|_| AuthError::Unauthorized)?;
        Ok(Self::from_claims(claims))
    }
}

/// Role-guarded extractor. `RequireRole::admin` requires an owner/admin role
/// in the signed tenant; `RequireRole::super_admin` requires the separate
/// platform role and fails closed for a tenant admin.
#[derive(Debug, Clone)]
pub struct RequireRole(pub AuthUser);

impl RequireRole {
    const ADMIN: &'static str = "tenant admin or owner";
    const SUPER_ADMIN: &'static str = "platform super admin";

    /// Require a tenant owner/admin role.
    pub async fn admin(parts: &mut Parts, state: &AppState) -> Result<Self, AuthError> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if !user.is_tenant_admin() {
            return Err(AuthError::Forbidden(Self::ADMIN));
        }
        Ok(Self(user))
    }

    /// Require the platform super-admin role. Tenant roles are not enough.
    pub async fn super_admin(parts: &mut Parts, state: &AppState) -> Result<Self, AuthError> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if !user.is_super_admin() {
            return Err(AuthError::Forbidden(Self::SUPER_ADMIN));
        }
        Ok(Self(user))
    }
}

impl std::ops::Deref for RequireRole {
    type Target = AuthUser;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[async_trait::async_trait]
impl FromRequestParts<AppState> for RequireRole {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if !user.has_role("member") && !user.is_tenant_admin() {
            return Err(AuthError::Forbidden("tenant membership"));
        }
        Ok(Self(user))
    }
}

/// Parse a `Bearer` token from the `Authorization` header.
pub fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() {
        Some(token.to_string())
    } else {
        None
    }
}

/// Parse the `Cookie` header into a name/value map.
pub fn cookies(headers: &HeaderMap) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(value) = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
    else {
        return map;
    };
    for pair in value.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        if let Some((name, value)) = pair.split_once('=') {
            map.insert(name.trim().to_string(), value.trim().to_string());
        }
    }
    map
}

/// Read a single cookie value from the request headers.
pub fn cookie_value(name: &str, headers: &HeaderMap) -> Option<String> {
    cookies(headers).remove(name)
}

/// Build a `Set-Cookie` value. `Secure` is intentionally omitted so local HTTP
/// development works; add it when serving over TLS in production.
pub fn cookie_header(name: &str, value: &str, max_age: i64, http_only: bool) -> String {
    let mut cookie = format!("{name}={value}; Path=/; SameSite=Lax; Max-Age={max_age}");
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    cookie
}

/// Build a `Set-Cookie` value that deletes a cookie.
pub fn clear_cookie(name: &str) -> String {
    cookie_header(name, "", 0, true)
}

/// Compare two strings without early exit on the first differing byte.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Verify the double-submit CSRF token on a state-changing request.
pub fn verify_csrf(headers: &HeaderMap) -> Result<(), AuthError> {
    let cookie =
        cookie_value(COOKIE_CSRF, headers).ok_or(AuthError::Forbidden("missing csrf cookie"))?;
    let header_value = headers
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or(AuthError::Forbidden("missing csrf header"))?;
    if constant_time_eq(&cookie, header_value) {
        Ok(())
    } else {
        Err(AuthError::Forbidden("csrf token mismatch"))
    }
}

/// The authentication sub-router, merged into the main router.
pub fn auth_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/auth/register", post(handlers::register))
        .route("/api/v1/auth/login", post(handlers::login))
        .route("/api/v1/auth/refresh", post(handlers::refresh))
        .route("/api/v1/auth/logout", post(handlers::logout))
        .route("/api/v1/auth/me", get(handlers::me))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_header_marks_http_only() {
        let cookie = cookie_header(COOKIE_ACCESS, "abc", 900, true);
        assert!(cookie.starts_with("nocode_access=abc;"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Max-Age=900"));
    }

    #[test]
    fn clear_cookie_expires() {
        let cookie = clear_cookie(COOKIE_REFRESH);
        assert!(cookie.starts_with("nocode_refresh=;"));
        assert!(cookie.contains("Max-Age=0"));
    }

    #[test]
    fn constant_time_eq_matches_semantics() {
        assert!(constant_time_eq("token", "token"));
        assert!(!constant_time_eq("token", "tokeN"));
        assert!(!constant_time_eq("token", "token "));
    }

    #[test]
    fn bearer_token_parses_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer abc.def.ghi".parse().unwrap());
        assert_eq!(bearer_token(&headers).as_deref(), Some("abc.def.ghi"));
    }

    #[test]
    fn cookies_split_on_semicolon() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "a=1; b=2".parse().unwrap());
        let map = cookies(&headers);
        assert_eq!(map.get("a").map(String::as_str), Some("1"));
        assert_eq!(map.get("b").map(String::as_str), Some("2"));
    }
}
