//! End-to-end tool flow: build entirely through granular tools, then
//! validate, simulate, export and re-import (Phase 18/20 gate).

use nocode_bpmn::engine::BpmnEngine;
use nocode_bpmn::xml::parse_bpmn;
use nocode_mcp::error::McpError;
use nocode_mcp::tools;
use nocode_mcp::{CallerContext, Config, McpServer};
use serde_json::{json, Value};

fn tool(server: &McpServer, caller: &CallerContext, name: &str, args: Value) -> Value {
    let response = tools::dispatch(server, caller, name, args).expect("tool exists");
    let text = response["content"][0]["text"].as_str().unwrap().to_string();
    assert_eq!(response["isError"], json!(false), "{name} failed: {text}");
    serde_json::from_str(&text).unwrap_or_else(|_| json!({ "text": text }))
}

fn tool_error(server: &McpServer, caller: &CallerContext, name: &str, args: Value) -> McpError {
    tools::dispatch(server, caller, name, args).expect_err("tool should fail")
}

fn server() -> McpServer {
    McpServer::new(Config {
        file_root: std::env::temp_dir(),
        ..Config::from_env()
    })
}

/// Start -> review -> gateway -> two ends, built with tool calls only.
fn build_approval(server: &McpServer, caller: &CallerContext) -> Value {
    let created = tool(
        server,
        caller,
        "create_workflow",
        json!({ "name": "Invoice approval" }),
    );
    let session_id = created["session_id"].as_str().unwrap().to_string();

    let start = tool(
        server,
        caller,
        "add_node",
        json!({ "session_id": session_id, "node_type": "StartEvent" }),
    );
    let review = tool(
        server,
        caller,
        "add_node",
        json!({
            "session_id": session_id,
            "node_type": "UserTask",
            "name": "Review",
            "config": { "assignee": "manager", "candidate_groups": ["finance"] },
        }),
    );
    let gateway = tool(
        server,
        caller,
        "add_node",
        json!({ "session_id": session_id, "node_type": "ExclusiveGateway" }),
    );
    let approved = tool(
        server,
        caller,
        "add_node",
        json!({ "session_id": session_id, "node_type": "EndEvent", "name": "Approved" }),
    );
    let rejected = tool(
        server,
        caller,
        "add_node",
        json!({ "session_id": session_id, "node_type": "EndEvent", "name": "Rejected" }),
    );

    let id = |value: &Value| value["node_id"].as_str().unwrap().to_string();
    let (s, r, g, a, x) = (
        id(&start),
        id(&review),
        id(&gateway),
        id(&approved),
        id(&rejected),
    );
    assert_eq!(start["config_defaults_applied"], json!(["event_type"]));
    assert_eq!(review["node_type"], "UserTask");

    for (source, target) in [(&s, &r), (&r, &g)] {
        tool(
            server,
            caller,
            "connect_nodes",
            json!({ "session_id": session_id, "source": source, "target": target }),
        );
    }
    tool(
        server,
        caller,
        "connect_nodes",
        json!({
            "session_id": session_id,
            "source": g,
            "target": a,
            "condition": "approved == true",
            "label": "yes",
        }),
    );
    tool(
        server,
        caller,
        "connect_nodes",
        json!({
            "session_id": session_id,
            "source": g,
            "target": x,
            "label": "no",
        }),
    );
    json!({
        "session_id": session_id,
        "nodes": [s, r, g, a, x],
    })
}

#[test]
fn build_validate_simulate_export_reimport() {
    let server = server();
    let caller = CallerContext::local();
    let built = build_approval(&server, &caller);
    let session_id = built["session_id"].as_str().unwrap();

    let layout = tool(
        &server,
        &caller,
        "auto_layout",
        json!({ "session_id": session_id }),
    );
    assert_eq!(layout["count"], json!(5));

    let validation = tool(
        &server,
        &caller,
        "validate_workflow",
        json!({ "session_id": session_id }),
    );
    assert_eq!(validation["valid"], json!(true), "{validation}");

    let simulation = tool(
        &server,
        &caller,
        "simulate_workflow",
        json!({
            "session_id": session_id,
            "variables": { "approved": true },
            "auto_complete_tasks": true,
        }),
    );
    assert_eq!(simulation["status"], json!("completed"));
    assert_eq!(simulation["variables"]["approved"], json!(true));
    let visited: Vec<&str> = simulation["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|step| step["node_name"].as_str())
        .collect();
    assert!(visited.contains(&"Approved"));
    assert!(!visited.contains(&"Rejected"));

    let exported = tool(
        &server,
        &caller,
        "export_bpmn_xml",
        json!({ "session_id": session_id }),
    );
    let xml = exported["xml"].as_str().unwrap();
    assert!(xml.contains("bpmndi:BPMNDiagram"));
    assert!(xml.contains("approved == true"));
    assert!(xml.contains("dc:Bounds"));

    let parsed = parse_bpmn(xml, "fallback").expect("exported XML imports");
    assert!(BpmnEngine::new().validate(&parsed).is_valid());
    assert_eq!(parsed.nodes.len(), 5);
    assert_eq!(parsed.edges.len(), 4);
    let conditions: Vec<&str> = parsed
        .edges
        .iter()
        .filter_map(|edge| edge.condition.as_deref())
        .collect();
    assert!(conditions.contains(&"approved == true"));

    let full = tool(
        &server,
        &caller,
        "get_workflow",
        json!({ "session_id": session_id, "view": "full" }),
    );
    let before: Vec<(f64, f64)> = full["workflow"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            (
                node["position"]["x"].as_f64().unwrap(),
                node["position"]["y"].as_f64().unwrap(),
            )
        })
        .collect();
    let after: Vec<(f64, f64)> = parsed
        .nodes
        .iter()
        .map(|node| (node.position.x, node.position.y))
        .collect();
    // Element ids are regenerated by the BPMN importer, so compare the
    // coordinate sets rather than pairing them by position in the vector.
    let sorted = |mut points: Vec<(f64, f64)>| {
        points.sort_by(|a, b| a.partial_cmp(b).expect("finite coordinates"));
        points
    };
    assert_eq!(
        sorted(before),
        sorted(after),
        "auto_layout coordinates survive round-trip"
    );
}

#[test]
fn rejects_unknown_node_types_and_missing_endpoints() {
    let server = server();
    let caller = CallerContext::local();
    let created = tool(&server, &caller, "create_workflow", json!({ "name": "X" }));
    let session_id = created["session_id"].as_str().unwrap();

    assert!(matches!(
        tool_error(
            &server,
            &caller,
            "add_node",
            json!({ "session_id": session_id, "node_type": "MagicTask" }),
        ),
        McpError::UnknownNodeType(_)
    ));
    assert!(matches!(
        tool_error(
            &server,
            &caller,
            "add_node",
            json!({ "session_id": session_id, "node_type": "Task", "config": { "nope": 1 } }),
        ),
        McpError::UnknownConfigKey { .. }
    ));
    assert!(matches!(
        tool_error(
            &server,
            &caller,
            "connect_nodes",
            json!({ "session_id": session_id, "source": "missing", "target": "missing2" }),
        ),
        McpError::NodeNotFound(_)
    ));
}

#[test]
fn revisions_undo_redo_and_session_isolation() {
    let server = server();
    let local = CallerContext::local();
    let remote = CallerContext::authenticated("u1", "t1", vec![]);
    let created = tool(&server, &local, "create_workflow", json!({ "name": "X" }));
    let session_id = created["session_id"].as_str().unwrap();

    tool(
        &server,
        &local,
        "add_node",
        json!({ "session_id": session_id, "node_type": "StartEvent", "expected_revision": 0 }),
    );
    assert!(matches!(
        tool_error(
            &server,
            &local,
            "add_node",
            json!({ "session_id": session_id, "node_type": "EndEvent", "expected_revision": 0 }),
        ),
        McpError::StaleRevision { .. }
    ));
    assert!(matches!(
        tool_error(
            &server,
            &remote,
            "get_workflow",
            json!({ "session_id": session_id }),
        ),
        McpError::SessionNotFound(_)
    ));

    tool(&server, &local, "undo", json!({ "session_id": session_id }));
    let summary = tool(
        &server,
        &local,
        "get_workflow",
        json!({ "session_id": session_id }),
    );
    assert_eq!(summary["workflow"]["node_count"], json!(0));
    tool(&server, &local, "redo", json!({ "session_id": session_id }));
    let summary = tool(
        &server,
        &local,
        "get_workflow",
        json!({ "session_id": session_id }),
    );
    assert_eq!(summary["workflow"]["node_count"], json!(1));
}

#[test]
fn loads_xml_and_saves_to_a_confined_file() {
    let root = tempfile::tempdir().unwrap();
    let server = McpServer::new(Config {
        file_root: root.path().to_path_buf(),
        ..Config::from_env()
    });
    let caller = CallerContext::local();
    let loaded = tool(
        &server,
        &caller,
        "load_workflow",
        json!({
            "xml": nocode_mcp::resources::examples::MINIMAL_XML,
            "name": "Imported",
        }),
    );
    assert_eq!(loaded["node_count"], json!(2));
    let session_id = loaded["session_id"].as_str().unwrap();
    let validation = tool(
        &server,
        &caller,
        "validate_workflow",
        json!({ "session_id": session_id }),
    );
    assert_eq!(validation["valid"], json!(true));

    let saved = tool(
        &server,
        &caller,
        "save_workflow",
        json!({ "session_id": session_id, "target": "file", "path": "out/workflow.json" }),
    );
    let path = saved["path"].as_str().unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    let workflow: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(workflow["name"], "Minimal");
}

#[test]
fn removing_a_node_removes_incident_edges() {
    let server = server();
    let caller = CallerContext::local();
    let built = build_approval(&server, &caller);
    let session_id = built["session_id"].as_str().unwrap();
    let gateway = &built["nodes"][2];

    let removed = tool(
        &server,
        &caller,
        "remove_node",
        json!({ "session_id": session_id, "node_id": gateway }),
    );
    assert_eq!(removed["removed_edge_ids"].as_array().unwrap().len(), 3);
    let summary = tool(
        &server,
        &caller,
        "get_workflow",
        json!({ "session_id": session_id }),
    );
    assert_eq!(summary["workflow"]["edge_count"], json!(1));
}
