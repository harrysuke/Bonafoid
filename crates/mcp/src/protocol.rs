//! Minimal MCP JSON-RPC 2.0 dispatcher.
//!
//! This is the documented fallback from plan §4.3: a small, dependency-free
//! transport boundary implementing `initialize`, `tools/list`,
//! `tools/call`, `resources/list`, `resources/read` and prompts. All SDK
//! knowledge is confined to this module and `transport/*`, so replacing it
//! with `rmcp` later does not touch any tool handler.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write;

use crate::error;
use crate::resources;
use crate::tools;
use crate::{CallerContext, McpServer};

const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// A JSON-RPC 2.0 request. Notifications have `id: null`.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

/// JSON-RPC error object.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// A JSON-RPC 2.0 response (either `result` or `error` is populated).
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    pub fn parse_error(id: Value, detail: &str) -> Self {
        Self::failure(id, -32700, format!("parse error: {detail}"))
    }
}

/// Dispatch one request. Notifications return `None`.
pub fn dispatch(
    server: &McpServer,
    caller: &CallerContext,
    request: &JsonRpcRequest,
) -> Option<JsonRpcResponse> {
    let id = request.id.clone().unwrap_or(Value::Null);
    let is_notification = request.id.is_none();

    if request.jsonrpc != "2.0" {
        return Some(JsonRpcResponse::failure(
            id,
            -32600,
            "jsonrpc must be \"2.0\"",
        ));
    }

    let outcome: Result<Value, (i64, String)> = match request.method.as_str() {
        "initialize" => Ok(initialize(request.params.as_ref())),
        "notifications/initialized" => Ok(json!({})),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::definitions() })),
        "tools/call" => call_tool(server, caller, request.params.as_ref()),
        "resources/list" => Ok(json!({ "resources": resources::list_json() })),
        "resources/read" => read_resource(request.params.as_ref()),
        "prompts/list" => Ok(json!({
            "prompts": [{
                "name": "build_workflow",
                "description": "Build a Bonafoid BPMN workflow from natural-language requirements",
                "arguments": [{
                    "name": "requirements",
                    "description": "What the workflow should do",
                    "required": true,
                }],
            }],
        })),
        "prompts/get" => get_prompt(request.params.as_ref()),
        other => Err((-32601, format!("method not found: {other}"))),
    };

    if is_notification {
        return None;
    }
    Some(match outcome {
        Ok(value) => JsonRpcResponse::success(id, value),
        Err((code, message)) => JsonRpcResponse::failure(id, code, message),
    })
}

fn initialize(params: Option<&Value>) -> Value {
    let requested = params
        .and_then(|value| value.get("protocolVersion"))
        .and_then(Value::as_str);
    let version = requested
        .filter(|value| PROTOCOL_VERSIONS.contains(value))
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": {
            "tools": { "listChanged": false },
            "resources": { "subscribe": false, "listChanged": false },
            "prompts": { "listChanged": false },
        },
        "serverInfo": {
            "name": "bonafoid-mcp",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": "Use bpmn://guide/authoring before authoring. Never generate raw BPMN XML.",
    })
}

fn call_tool(
    server: &McpServer,
    caller: &CallerContext,
    params: Option<&Value>,
) -> Result<Value, (i64, String)> {
    let params = params.ok_or_else(|| (-32602, "tools/call requires params".to_string()))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| (-32602, "tools/call requires params.name".to_string()))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match tools::dispatch(server, caller, name, arguments.clone()) {
        Ok(value) => {
            audit(server, caller, name, &arguments, "ok");
            Ok(value)
        }
        Err(error) => {
            let status = if error.is_client_error() {
                "client_error"
            } else {
                "server_error"
            };
            audit(server, caller, name, &arguments, status);
            Ok(error::tool_error(&error))
        }
    }
}

fn read_resource(params: Option<&Value>) -> Result<Value, (i64, String)> {
    let uri = params
        .and_then(|value| value.get("uri"))
        .and_then(Value::as_str)
        .ok_or_else(|| (-32602, "resources/read requires params.uri".to_string()))?;
    let resource = resources::read(uri).map_err(|error| (error.code(), error.to_string()))?;
    Ok(json!({
        "contents": [{
            "uri": resource.meta.uri,
            "mimeType": resource.meta.mime_type,
            "text": resource.text,
        }],
    }))
}

fn get_prompt(params: Option<&Value>) -> Result<Value, (i64, String)> {
    let name = params
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .ok_or_else(|| (-32602, "prompts/get requires params.name".to_string()))?;
    if name != "build_workflow" {
        return Err((-32602, format!("unknown prompt: {name}")));
    }
    let requirements = params
        .and_then(|value| value.get("arguments"))
        .and_then(|value| value.get("requirements"))
        .and_then(Value::as_str)
        .unwrap_or("a workflow described by the user");
    let text = format!(
        "Requirements: {requirements}\n\n1. Read bpmn://guide/authoring and bpmn://catalog/node-types.\n2. create_workflow.\n3. add_node for each step; connect_nodes for each flow (conditions on outgoing edges).\n4. auto_layout, then validate_workflow and fix all issues.\n5. simulate_workflow with representative variables.\n6. save_workflow to a project and report validation results to the user.\nDo not generate or paste raw BPMN XML."
    );
    Ok(json!({
        "description": "Guided Bonafoid workflow authoring",
        "messages": [{ "role": "user", "content": { "type": "text", "text": text } }],
    }))
}

/// Append a JSON-lines audit entry. Arguments are hashed, never logged in
/// full, so tokens and workflow contents cannot leak into the audit file.
fn audit(server: &McpServer, caller: &CallerContext, tool: &str, arguments: &Value, status: &str) {
    let Some(path) = server.config.audit_log.as_ref() else {
        return;
    };
    let mut hasher = DefaultHasher::new();
    arguments.to_string().hash(&mut hasher);
    let digest = format!("{:016x}", hasher.finish());
    let entry = json!({
        "timestamp": Utc::now().to_rfc3339(),
        "owner": caller.owner,
        "tool": tool,
        "arguments_digest": digest,
        "result_status": status,
    })
    .to_string();
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{entry}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: method.into(),
            params,
        }
    }

    fn server() -> McpServer {
        McpServer::new(crate::Config {
            file_root: std::env::temp_dir(),
            ..crate::Config::from_env()
        })
    }

    #[test]
    fn initialize_negotiates_version() {
        let response = dispatch(
            &server(),
            &CallerContext::local(),
            &request(
                "initialize",
                Some(json!({ "protocolVersion": "2024-11-05" })),
            ),
        )
        .unwrap();
        assert_eq!(response.result.unwrap()["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn notifications_get_no_response() {
        let mut notification = request("notifications/initialized", None);
        notification.id = None;
        assert!(dispatch(&server(), &CallerContext::local(), &notification).is_none());
    }

    #[test]
    fn unknown_method_is_a_protocol_error() {
        let response =
            dispatch(&server(), &CallerContext::local(), &request("nope", None)).unwrap();
        assert_eq!(response.error.unwrap().code, -32601);
    }

    #[test]
    fn tools_and_resources_are_discoverable() {
        let caller = CallerContext::local();
        let server = server();
        let tools = dispatch(&server, &caller, &request("tools/list", None)).unwrap();
        assert!(tools.result.unwrap()["tools"].as_array().unwrap().len() >= 14);
        let resources = dispatch(&server, &caller, &request("resources/list", None)).unwrap();
        assert_eq!(
            resources.result.unwrap()["resources"]
                .as_array()
                .unwrap()
                .len(),
            7
        );
    }

    #[test]
    fn prompts_and_capabilities_are_exposed() {
        let caller = CallerContext::local();
        let server = server();
        let prompts = dispatch(&server, &caller, &request("prompts/list", None)).unwrap();
        assert_eq!(
            prompts.result.unwrap()["prompts"][0]["name"],
            "build_workflow"
        );
        let rendered = dispatch(
            &server,
            &caller,
            &request(
                "prompts/get",
                Some(json!({"name":"build_workflow","arguments":{"requirements":"approve invoices"}})),
            ),
        )
        .unwrap();
        assert!(rendered.result.unwrap()["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("approve invoices"));
    }
}
