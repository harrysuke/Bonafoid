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
            scopes: vec!["workflows:read".into(), "workflows:write".into()],
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

/// Scope required by a tool. Read-only tools can run with `workflows:read`;
/// everything that mutates a session, file or project needs `workflows:write`.
pub fn required_scope(tool_name: &str) -> &'static str {
    match tool_name {
        "get_workflow" | "validate_workflow" | "simulate_workflow" | "export_bpmn_xml" => {
            "workflows:read"
        }
        _ => "workflows:write",
    }
}

/// A framed HTTP response. `status` is the HTTP status; `body` is the
/// JSON-RPC response object.
#[derive(Debug, Clone)]
pub struct HttpMcpResponse {
    pub status: u16,
    pub body: Value,
}

/// Handle a Streamable HTTP POST body. JSON responses are permitted by the
/// spec; SSE streaming is intentionally not implemented in this release.
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
}
