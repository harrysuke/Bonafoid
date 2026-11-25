//! Streamable HTTP MCP endpoint mounted at `/mcp`.
//!
//! Authentication reuses `AuthUser` (Bearer JWT works without the cookie
//! CSRF flow). Scopes are derived from the signed tenant roles: all
//! authenticated users may read/simulate/export; tenant admins/owners,
//! platform super admins, or tokens explicitly carrying `workflows:write`
//! may mutate sessions and projects.

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use bytes::Bytes;
use nocode_mcp::protocol::JsonRpcRequest;
use nocode_mcp::transport::http::{self, CallerContext};
use serde_json::json;

use crate::auth::AuthUser;
use crate::server::AppState;

/// The MCP route fragment, merged into the main Axum router.
pub fn mcp_router() -> Router<AppState> {
    Router::new().route("/mcp", post(handle))
}

/// Map signed claims to the scopes this server honours.
///
/// Every authenticated user may read their own projects and drive the BPMN
/// tools. Writing needs `workflows:write`, which is granted to tenant
/// admins/owners, platform super admins, and tokens that carry the role
/// explicitly — the same rule as Milestone 4, extended to the project scopes
/// from plan §9. A member with no write role can still ask the assistant to
/// read, and is refused before the stream starts if it would have to write.
pub fn scopes_for(user: &AuthUser) -> Vec<String> {
    let mut scopes = vec!["workflows:read".to_string(), "project:read".to_string()];
    if user.is_tenant_admin()
        || user.is_super_admin()
        || user.roles.iter().any(|role| role == "workflows:write")
    {
        scopes.push("workflows:write".to_string());
    }
    if user.is_tenant_admin()
        || user.is_super_admin()
        || user.roles.iter().any(|role| role == "project:write")
    {
        scopes.push("project:write".to_string());
    }
    scopes
}

/// The caller identity every MCP entry point uses.
///
/// `owner` is `tenant:user`, so two users in the same tenant cannot see each
/// other's sessions even with a guessed session id.
pub fn caller_for(user: &AuthUser) -> CallerContext {
    CallerContext::authenticated(&user.id, &user.tenant_id, scopes_for(user))
}

async fn handle(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "bad_request", "message": error.to_string() })),
            )
                .into_response();
        }
    };

    // Scope on the whole request, not just the tool name: reading a
    // `project://` resource exposes tenant data and has no tool to gate on.
    let required = http::required_scope_for_request(&request.method, request.params.as_ref());
    let caller = caller_for(&user);
    if !caller.has_scope(required) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "forbidden",
                "message": format!("{required} scope required for this MCP request"),
            })),
        )
            .into_response();
    }

    let response = http::handle_json_async(&state.mcp, &caller, &body).await;
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut result = (status, Json(response.body)).into_response();
    result.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    // Echo a client-supplied MCP session id when present; this server does
    // not yet issue its own ids (single shared in-process session store).
    if let Some(id) = headers
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|id| header::HeaderValue::try_from(id).ok())
    {
        result.headers_mut().insert("mcp-session-id", id);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::jwt::{now_secs, Claims};
    use crate::auth::JwtKeys;
    use crate::db::connect_sqlite_memory;
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value;
    use std::sync::{Arc, OnceLock};
    use tower::ServiceExt;

    fn keys() -> Arc<JwtKeys> {
        static KEYS: OnceLock<Arc<JwtKeys>> = OnceLock::new();
        KEYS.get_or_init(|| Arc::new(JwtKeys::generate_ephemeral().expect("test keys")))
            .clone()
    }

    async fn router() -> axum::Router {
        let db = connect_sqlite_memory().await.unwrap();
        crate::build_router(AppState::new(db, keys()), "apps/ide-web/dist")
    }

    fn bearer(roles: Vec<String>) -> String {
        let claims = Claims::access_for(
            "user-1",
            "mcp@example.com",
            "default".into(),
            roles,
            None,
            "jti",
            now_secs(),
        );
        format!("Bearer {}", keys().encode(&claims).unwrap())
    }

    fn rpc(method: &str, params: Value) -> String {
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string()
    }

    async fn post(body: String, token: Option<String>) -> Response {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("authorization", token);
        }
        router()
            .await
            .oneshot(builder.body(Body::from(body)).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn mcp_requires_authentication() {
        let response = post(rpc("tools/list", json!({})), None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn read_scope_cannot_mutate() {
        let body = rpc(
            "tools/call",
            json!({ "name": "add_node", "arguments": { "session_id": "x", "node_type": "StartEvent" } }),
        );
        let response = post(body, Some(bearer(vec![]))).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn read_scope_can_discover_tools() {
        let response = post(rpc("tools/list", json!({})), Some(bearer(vec![]))).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert!(json["result"]["tools"].is_array());
    }

    #[tokio::test]
    async fn write_scope_can_create_a_session() {
        let body = rpc(
            "tools/call",
            json!({ "name": "create_workflow", "arguments": { "name": "HTTP workflow" } }),
        );
        let response = post(body, Some(bearer(vec!["admin".into()]))).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert!(json["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("session_id"));
    }
}
