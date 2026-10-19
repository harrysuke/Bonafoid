//! Auth HTTP handlers and session-cookie plumbing.
//!
//! Tokens are delivered as HttpOnly cookies (`nocode_access` / `nocode_refresh`)
//! plus a readable `nocode_csrf` cookie for the double-submit CSRF check.

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use uuid::Uuid;

use crate::server::AppState;

use super::{
    clear_cookie, constant_time_eq, cookie_header, cookie_value, jwt, password, store, verify_csrf,
    AuthError, AuthUser, COOKIE_ACCESS, COOKIE_CSRF, COOKIE_REFRESH,
};

/// Credentials submitted to register or log in.
#[derive(Debug, Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

/// Public projection of a user.
#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub is_verified: bool,
    pub created_at: String,
}

impl From<&store::UserRow> for UserResponse {
    fn from(user: &store::UserRow) -> Self {
        Self {
            id: user.id.clone(),
            email: user.email.clone(),
            is_verified: user.is_verified,
            created_at: user.created_at.clone(),
        }
    }
}

/// Successful auth payload.
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub user: UserResponse,
    pub expires_in: i64,
    pub csrf_token: String,
}

struct IssuedTokens {
    access: String,
    refresh: String,
    csrf: String,
    expires_in: i64,
}

/// Register a new account and start a session.
pub async fn register(
    State(state): State<AppState>,
    Json(credentials): Json<Credentials>,
) -> Result<Response, AuthError> {
    let email = normalize_email(&credentials.email)?;
    validate_password(&credentials.password)?;

    if store::find_user_by_email(&state.db, &email)
        .await?
        .is_some()
    {
        return Err(AuthError::Conflict("email already registered"));
    }

    let password_hash = password::hash_password(&credentials.password)?;
    let user_id = Uuid::new_v4().to_string();
    store::insert_user(&state.db, &user_id, &email, &password_hash).await?;

    let user = store::find_user_by_id(&state.db, &user_id)
        .await?
        .ok_or(AuthError::Internal)?;
    let tokens = issue_tokens(&state, &user).await?;
    store::update_last_login(&state.db, &user.id).await?;
    Ok(auth_response(StatusCode::CREATED, &user, tokens))
}

/// Log in with an existing account and start a session.
pub async fn login(
    State(state): State<AppState>,
    Json(credentials): Json<Credentials>,
) -> Result<Response, AuthError> {
    let email = normalize_email(&credentials.email)?;

    let user = store::find_user_by_email(&state.db, &email)
        .await?
        .ok_or(AuthError::Unauthorized)?;

    if !password::verify_password(&credentials.password, &user.password_hash) {
        return Err(AuthError::Unauthorized);
    }
    if !user.is_active {
        return Err(AuthError::Forbidden("account is disabled"));
    }

    let tokens = issue_tokens(&state, &user).await?;
    store::update_last_login(&state.db, &user.id).await?;
    Ok(auth_response(StatusCode::OK, &user, tokens))
}

/// Exchange a refresh token for a fresh pair (with rotation).
pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AuthError> {
    let token = cookie_value(COOKIE_REFRESH, &headers).ok_or(AuthError::Unauthorized)?;
    let claims = state
        .keys
        .decode_refresh(&token)
        .map_err(|_| AuthError::Unauthorized)?;

    let stored = store::find_refresh_token_by_jti(&state.db, &claims.jti)
        .await?
        .ok_or(AuthError::Unauthorized)?;
    if stored.revoked {
        return Err(AuthError::Unauthorized);
    }
    if !constant_time_eq(&stored.token_hash, &hash_token(&token)) {
        return Err(AuthError::Unauthorized);
    }

    let user = store::find_user_by_id(&state.db, &stored.user_id)
        .await?
        .ok_or(AuthError::Unauthorized)?;
    if !user.is_active {
        return Err(AuthError::Forbidden("account is disabled"));
    }

    store::revoke_refresh_token(&state.db, &claims.jti).await?;
    let tokens = issue_tokens(&state, &user).await?;
    Ok(auth_response(StatusCode::OK, &user, tokens))
}

/// Revoke the refresh token and clear the session cookies.
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AuthError> {
    verify_csrf(&headers)?;

    if let Some(token) = cookie_value(COOKIE_REFRESH, &headers) {
        match state.keys.decode_refresh(&token) {
            Ok(claims) => {
                if let Err(error) = store::revoke_refresh_token(&state.db, &claims.jti).await {
                    tracing::warn!(error = %error, "failed to revoke refresh token on logout");
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "logout received an invalid refresh token");
            }
        }
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    append_clear_cookies(response.headers_mut());
    Ok(response)
}

/// Return the currently authenticated user.
pub async fn me(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<UserResponse>, AuthError> {
    let row = store::find_user_by_id(&state.db, &user.id)
        .await?
        .ok_or(AuthError::Unauthorized)?;
    Ok(Json(UserResponse::from(&row)))
}

async fn issue_tokens(state: &AppState, user: &store::UserRow) -> Result<IssuedTokens, AuthError> {
    let now = jwt::now_secs();

    let access_jti = Uuid::new_v4().to_string();
    let access_claims = jwt::Claims::access(&user.id, &user.email, &access_jti, now);
    let access = state.keys.encode(&access_claims)?;

    let refresh_jti = Uuid::new_v4().to_string();
    let refresh_claims = jwt::Claims::refresh(&user.id, &user.email, &refresh_jti, now);
    let refresh = state.keys.encode(&refresh_claims)?;

    let token_hash = hash_token(&refresh);
    let expires_at = (Utc::now() + Duration::seconds(jwt::REFRESH_TTL_SECS)).to_rfc3339();
    store::insert_refresh_token(
        &state.db,
        &Uuid::new_v4().to_string(),
        &user.id,
        &refresh_jti,
        &token_hash,
        &expires_at,
    )
    .await?;

    Ok(IssuedTokens {
        access,
        refresh,
        csrf: new_csrf_token(),
        expires_in: jwt::ACCESS_TTL_SECS,
    })
}

fn auth_response(status: StatusCode, user: &store::UserRow, tokens: IssuedTokens) -> Response {
    let body = AuthResponse {
        user: UserResponse::from(user),
        expires_in: tokens.expires_in,
        csrf_token: tokens.csrf.clone(),
    };
    let mut response = (status, Json(body)).into_response();
    append_auth_cookies(response.headers_mut(), &tokens);
    response
}

fn append_auth_cookies(headers: &mut HeaderMap, tokens: &IssuedTokens) {
    push_cookie(
        headers,
        cookie_header(COOKIE_ACCESS, &tokens.access, jwt::ACCESS_TTL_SECS, true),
    );
    push_cookie(
        headers,
        cookie_header(COOKIE_REFRESH, &tokens.refresh, jwt::REFRESH_TTL_SECS, true),
    );
    push_cookie(
        headers,
        cookie_header(COOKIE_CSRF, &tokens.csrf, jwt::REFRESH_TTL_SECS, false),
    );
}

fn append_clear_cookies(headers: &mut HeaderMap) {
    for name in [COOKIE_ACCESS, COOKIE_REFRESH, COOKIE_CSRF] {
        push_cookie(headers, clear_cookie(name));
    }
}

fn push_cookie(headers: &mut HeaderMap, cookie: String) {
    match header::HeaderValue::from_str(&cookie) {
        Ok(value) => {
            headers.append(header::SET_COOKIE, value);
        }
        Err(error) => tracing::warn!(error = %error, "failed to build Set-Cookie header"),
    }
}

fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn new_csrf_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn normalize_email(raw: &str) -> Result<String, AuthError> {
    let email = raw.trim().to_lowercase();
    if email.is_empty() || email.len() > 320 {
        return Err(AuthError::BadRequest("invalid email"));
    }
    let Some((local, domain)) = email.split_once('@') else {
        return Err(AuthError::BadRequest("invalid email"));
    };
    if local.is_empty()
        || domain.is_empty()
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        return Err(AuthError::BadRequest("invalid email"));
    }
    Ok(email)
}

fn validate_password(password: &str) -> Result<(), AuthError> {
    if password.chars().count() < 8 {
        return Err(AuthError::BadRequest(
            "password must be at least 8 characters",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{auth_router, JwtKeys};
    use crate::db::connect_sqlite_memory;
    use axum::body::Body;
    use axum::http::Request;
    use axum::Router;
    use std::sync::{Arc, OnceLock};
    use tower::ServiceExt;

    fn test_keys() -> Arc<JwtKeys> {
        static KEYS: OnceLock<Arc<JwtKeys>> = OnceLock::new();
        KEYS.get_or_init(|| Arc::new(JwtKeys::generate_ephemeral().expect("keygen")))
            .clone()
    }

    async fn test_app() -> Router {
        let db = connect_sqlite_memory().await.expect("in-memory db");
        let state = AppState::new(db, test_keys());
        Router::new().merge(auth_router()).with_state(state)
    }

    fn post_json(uri: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).expect("json")))
            .expect("request")
    }

    fn post_with_cookie(uri: &str, cookie: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .expect("request")
    }

    fn get_with_cookie(uri: &str, cookie: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .expect("request")
    }

    fn cookie_of(response: &Response, name: &str) -> Option<String> {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(|cookie| {
                let (key, rest) = cookie.split_once('=')?;
                (key == name).then(|| rest.split(';').next().unwrap_or("").to_string())
            })
    }

    async fn json_body(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    fn register_payload(email: &str, password: &str) -> serde_json::Value {
        serde_json::json!({ "email": email, "password": password })
    }

    #[tokio::test]
    async fn register_returns_user_and_sets_cookies() {
        let app = test_app().await;
        let response = app
            .oneshot(post_json(
                "/api/v1/auth/register",
                register_payload("New@Example.com", "supersecret"),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(cookie_of(&response, COOKIE_ACCESS).is_some());
        assert!(cookie_of(&response, COOKIE_REFRESH).is_some());
        assert!(cookie_of(&response, COOKIE_CSRF).is_some());

        let body = json_body(response).await;
        assert_eq!(body["user"]["email"], "new@example.com");
        assert!(body["csrf_token"].as_str().is_some_and(|t| t.len() == 64));
    }

    #[tokio::test]
    async fn duplicate_registration_conflicts() {
        let app = test_app().await;
        let first = app
            .clone()
            .oneshot(post_json(
                "/api/v1/auth/register",
                register_payload("dup@example.com", "supersecret"),
            ))
            .await
            .expect("response");
        assert_eq!(first.status(), StatusCode::CREATED);

        let second = app
            .oneshot(post_json(
                "/api/v1/auth/register",
                register_payload("dup@example.com", "supersecret"),
            ))
            .await
            .expect("response");
        assert_eq!(second.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn weak_password_is_rejected() {
        let app = test_app().await;
        let response = app
            .oneshot(post_json(
                "/api/v1/auth/register",
                register_payload("weak@example.com", "short"),
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn login_rejects_wrong_password() {
        let app = test_app().await;
        let registered = app
            .clone()
            .oneshot(post_json(
                "/api/v1/auth/register",
                register_payload("login@example.com", "supersecret"),
            ))
            .await
            .expect("response");
        assert_eq!(registered.status(), StatusCode::CREATED);

        let response = app
            .oneshot(post_json(
                "/api/v1/auth/login",
                register_payload("login@example.com", "wrongpassword"),
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn me_requires_authentication() {
        let app = test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/me")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn me_returns_user_with_access_cookie() {
        let app = test_app().await;
        let registered = app
            .clone()
            .oneshot(post_json(
                "/api/v1/auth/register",
                register_payload("me@example.com", "supersecret"),
            ))
            .await
            .expect("response");
        let access = cookie_of(&registered, COOKIE_ACCESS).expect("access cookie");

        let response = app
            .oneshot(get_with_cookie(
                "/api/v1/auth/me",
                &format!("{COOKIE_ACCESS}={access}"),
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let body = json_body(response).await;
        assert_eq!(body["email"], "me@example.com");
    }

    #[tokio::test]
    async fn refresh_rotates_and_rejects_reuse() {
        let app = test_app().await;
        let registered = app
            .clone()
            .oneshot(post_json(
                "/api/v1/auth/register",
                register_payload("rotate@example.com", "supersecret"),
            ))
            .await
            .expect("response");
        let refresh = cookie_of(&registered, COOKIE_REFRESH).expect("refresh cookie");
        let cookie = format!("{COOKIE_REFRESH}={refresh}");

        let rotated = app
            .clone()
            .oneshot(post_with_cookie("/api/v1/auth/refresh", &cookie))
            .await
            .expect("response");
        assert_eq!(rotated.status(), StatusCode::OK);
        assert!(cookie_of(&rotated, COOKIE_REFRESH).is_some());

        let reused = app
            .oneshot(post_with_cookie("/api/v1/auth/refresh", &cookie))
            .await
            .expect("response");
        assert_eq!(reused.status(), StatusCode::UNAUTHORIZED);
    }
}
