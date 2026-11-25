//! Protocol-level acceptance tests: discovery, schemas, resources and errors.

use nocode_mcp::protocol::{self, JsonRpcRequest, JsonRpcResponse};
use nocode_mcp::{CallerContext, Config, McpServer};
use serde_json::{json, Value};

fn server() -> McpServer {
    McpServer::new(Config {
        file_root: std::env::temp_dir(),
        ..Config::from_env()
    })
}

fn call(server: &McpServer, method: &str, params: Option<Value>) -> JsonRpcResponse {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(json!(7)),
        method: method.into(),
        params,
    };
    protocol::dispatch(server, &CallerContext::local(), &request)
        .expect("requests receive responses")
}

#[test]
fn every_tool_advertises_a_schema_with_required_fields() {
    let response = call(&server(), "tools/list", None);
    let tools = response.result.unwrap()["tools"]
        .as_array()
        .unwrap()
        .clone();
    assert!(tools.len() >= 14);
    for tool in &tools {
        let schema = &tool["inputSchema"];
        assert!(schema.get("type").is_some(), "{}", tool["name"]);
        assert!(schema.get("properties").is_some(), "{}", tool["name"]);
        if tool["name"] != "create_workflow" && tool["name"] != "load_workflow" {
            let required = schema["required"].as_array().expect("required fields");
            // Session tools are addressed by `session_id`; project tools by
            // `project_id`. What the rule protects is that no tool can be
            // called without naming the object it acts on — an id the model
            // cannot invent and the caller must already hold.
            let identifies = required
                .iter()
                .any(|value| value == "session_id" || value == "project_id");
            assert!(
                identifies,
                "{} requires neither session_id nor project_id",
                tool["name"]
            );
        }
    }
}

#[test]
fn every_resource_can_be_read_with_mime_type() {
    let server = server();
    let list = call(&server, "resources/list", None);
    let resources = list.result.unwrap()["resources"]
        .as_array()
        .unwrap()
        .clone();
    // 7 BPMN resources plus 4 project schemas. Exact rather than a lower bound,
    // so a resource that silently stops being advertised is a failure.
    assert_eq!(resources.len(), 11);
    for resource in resources {
        let uri = resource["uri"].as_str().unwrap();
        let response = call(&server, "resources/read", Some(json!({ "uri": uri })));
        let content = &response.result.unwrap()["contents"][0];
        assert_eq!(content["uri"], json!(uri));
        assert_eq!(content["mimeType"], resource["mimeType"]);
        assert!(content["text"].as_str().unwrap().len() > 10);
    }
}

#[test]
fn error_shapes_are_stable() {
    let server = server();
    let unknown_method = call(&server, "does/not/exist", None);
    assert_eq!(unknown_method.error.unwrap().code, -32601);
    assert!(unknown_method.result.is_none());

    let bad_version = JsonRpcRequest {
        jsonrpc: "1.0".into(),
        id: Some(json!(1)),
        method: "ping".into(),
        params: None,
    };
    let response = protocol::dispatch(&server, &CallerContext::local(), &bad_version).unwrap();
    assert_eq!(response.error.unwrap().code, -32600);

    let missing_uri = call(&server, "resources/read", Some(json!({})));
    assert_eq!(missing_uri.error.unwrap().code, -32602);
}

#[test]
fn tool_failures_are_results_with_is_error() {
    let server = server();
    let response = call(
        &server,
        "tools/call",
        json!({
            "name": "get_workflow",
            "arguments": { "session_id": "unknown" },
        })
        .into(),
    );
    assert!(response.error.is_none());
    assert_eq!(response.result.unwrap()["isError"], json!(true));
}

#[test]
fn stdio_line_framing_round_trips() {
    let server = server();
    let line = r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}}"#;
    let response = server.handle_line(&CallerContext::local(), line).unwrap();
    let parsed: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(parsed["id"], json!(3));
    assert!(parsed["result"]["tools"].is_array());
}
