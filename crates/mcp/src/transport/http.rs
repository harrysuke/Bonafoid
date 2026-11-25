//! HTTP-facing helpers. The Axum route itself lives in `nocode-ide-server`
//! so it can reuse `AuthUser`; this module owns JSON-RPC framing and scope
//! classification only.

use serde_json::{json, Value};

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::session::SessionKey;
use crate::McpServer;

/// Identity of the caller. Owner scoping prevents cross-user session access.
#[derive(Debug, Clone)]
pub struct CallerContext {
    pub owner: String,
    pub user_id: String,
    pub tenant_id: String,
    pub scopes: Vec<String>,
}

impl CallerContext {
    /// The stdio binary runs as the local user with full local permissions.
    pub fn local() -> Self {
        Self {
            owner: "local".into(),
            user_id: "local".into(),
            tenant_id: "local".into(),
            scopes: vec![
                "workflows:read".into(),
                "workflows:write".into(),
                "project:read".into(),
                "project:write".into(),
            ],
        }
    }

    /// Remote callers are scoped by their signed tenant and user claims.
    pub fn authenticated(user_id: &str, tenant_id: &str, scopes: Vec<String>) -> Self {
        Self {
            owner: format!("{tenant_id}:{user_id}"),
            user_id: user_id.to_string(),
            tenant_id: tenant_id.to_string(),
            scopes,
        }
    }

    pub fn session_key(&self, session_id: &str) -> SessionKey {
        SessionKey::new(&self.owner, session_id)
    }

    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|value| value == scope)
    }
}

/// Scope required by a tool.
///
/// The BPMN tools keep their Milestone 4 scopes so existing tokens keep
/// working; the Milestone 5 tools use the project scopes from plan §9.
pub fn required_scope(tool_name: &str) -> &'static str {
    match tool_name {
        "get_workflow" | "validate_workflow" | "simulate_workflow" | "export_bpmn_xml" => {
            "workflows:read"
        }
        "project_open" | "project_get" => "project:read",
        name if is_project_write(name) => "project:write",
        _ => "workflows:write",
    }
}

/// Whether a tool mutates project state (and therefore needs `project:write`).
pub fn is_project_write(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "project_save"
            | "project_close"
            | "entity_create"
            | "entity_add_field"
            | "entity_add_relation"
            | "form_create"
            | "form_add_field"
            | "page_create"
            | "page_add_component"
            | "theme_update"
            | "workflow_create"
            | "workflow_scaffold_crud"
            | "workflow_open"
            | "workflow_commit"
            | "workflow_set_start_trigger"
            | "workflow_bind_service_task"
    )
}

/// The scope a whole request needs, not just its tool.
///
/// `resources/read` of a `project://` URI exposes tenant data, so it must be
/// gated even though it is not a tool call. Everything unknown falls back to
/// the read scope, which is the weakest grant.
pub fn required_scope_for_request(method: &str, params: Option<&Value>) -> &'static str {
    match method {
        "tools/call" => params
            .and_then(|params| params.get("name"))
            .and_then(Value::as_str)
            .map(required_scope)
            .unwrap_or("workflows:read"),
        "resources/read" => params
            .and_then(|params| params.get("uri"))
            .and_then(Value::as_str)
            .filter(|uri| uri.starts_with("project://"))
            .map(|_| "project:read")
            .unwrap_or("workflows:read"),
        _ => "workflows:read",
    }
}

/// A framed HTTP response. `status` is the HTTP status; `body` is the
/// JSON-RPC response object.
#[derive(Debug, Clone)]
pub struct HttpMcpResponse {
    pub status: u16,
    pub body: Value,
}

/// Handle a Streamable HTTP POST body synchronously.
///
/// Kept for the stdio-parity tests; the server route uses
/// [`handle_json_async`] so project tools can reach storage.
pub fn handle_json(server: &McpServer, caller: &CallerContext, body: &[u8]) -> HttpMcpResponse {
    let request: JsonRpcRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(error) => {
            return HttpMcpResponse {
                status: 400,
                body: serde_json::to_value(JsonRpcResponse::parse_error(
                    Value::Null,
                    &error.to_string(),
                ))
                .expect("error serialises"),
            }
        }
    };
    let notification = request.id.is_none();
    match server.dispatch(caller, &request) {
        Some(response) => HttpMcpResponse {
            status: 200,
            body: serde_json::to_value(response).expect("response serialises"),
        },
        None => HttpMcpResponse {
            status: if notification { 202 } else { 200 },
            body: json!({}),
        },
    }
}

/// Handle a Streamable HTTP POST body, awaiting project storage.
pub async fn handle_json_async(
    server: &McpServer,
    caller: &CallerContext,
    body: &[u8],
) -> HttpMcpResponse {
    let request: JsonRpcRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(error) => {
            return HttpMcpResponse {
                status: 400,
                body: serde_json::to_value(JsonRpcResponse::parse_error(
                    Value::Null,
                    &error.to_string(),
                ))
                .expect("error serialises"),
            }
        }
    };
    let notification = request.id.is_none();
    match server.dispatch_async(caller, &request).await {
        Some(response) => HttpMcpResponse {
            status: 200,
            body: serde_json::to_value(response).expect("response serialises"),
        },
        None => HttpMcpResponse {
            status: if notification { 202 } else { 200 },
            body: json!({}),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[test]
    fn callers_are_owner_scoped() {
        let local = CallerContext::local();
        let remote = CallerContext::authenticated("u1", "t1", vec![]);
        assert_ne!(local.session_key("s1"), remote.session_key("s1"));
    }

    #[test]
    fn scopes_split_read_from_write() {
        assert_eq!(required_scope("get_workflow"), "workflows:read");
        assert_eq!(required_scope("add_node"), "workflows:write");
        assert_eq!(required_scope("save_workflow"), "workflows:write");
    }

    #[test]
    fn project_tools_use_project_scopes() {
        assert_eq!(required_scope("project_get"), "project:read");
        assert_eq!(required_scope("project_open"), "project:read");
        assert_eq!(required_scope("project_save"), "project:write");
        assert_eq!(required_scope("entity_create"), "project:write");
        assert_eq!(
            required_scope("workflow_bind_service_task"),
            "project:write"
        );
    }

    #[test]
    fn project_resources_need_the_project_read_scope() {
        assert_eq!(
            required_scope_for_request(
                "resources/read",
                Some(&json!({ "uri": "project://abc/catalog" }))
            ),
            "project:read"
        );
        assert_eq!(
            required_scope_for_request(
                "resources/read",
                Some(&json!({ "uri": "bpmn://guide/authoring" }))
            ),
            "workflows:read"
        );
    }

    #[test]
    fn malformed_http_body_gets_a_parse_error() {
        let server = McpServer::new(Config::from_env());
        let response = handle_json(&server, &CallerContext::local(), b"{oops");
        assert_eq!(response.status, 400);
        assert_eq!(response.body["error"]["code"], -32700);
    }

    #[test]
    fn http_dispatch_reaches_tools() {
        let server = McpServer::new(Config::from_env());
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_workflow","arguments":{"name":"HTTP"}}}"#;
        let response = handle_json(&server, &CallerContext::local(), body);
        assert_eq!(response.status, 200);
        assert!(response.body["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("session_id"));
    }

    #[tokio::test]
    async fn async_http_dispatch_answers_notifications_with_202() {
        let server = McpServer::new(Config::from_env());
        let body = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let response = handle_json_async(&server, &CallerContext::local(), body).await;
        assert_eq!(response.status, 202);
    }
}
