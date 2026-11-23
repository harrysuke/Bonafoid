//! Deterministic agent-evaluation proxy (Phase 21 gate).
//!
//! `evals/prompts.json` holds the five natural-language prompts. The scripted
//! flows below exercise the same tool sequences an agent should discover,
//! scoring `valid == true`, tool-call count and no raw-XML authoring calls.
//! A live-LLM harness can reuse the same scoring function.

use nocode_mcp::tools;
use nocode_mcp::{CallerContext, Config, McpServer};
use serde_json::{json, Value};

struct EvalResult {
    name: &'static str,
    valid: bool,
    tool_calls: usize,
    raw_xml_calls: usize,
}

fn call(
    server: &McpServer,
    caller: &CallerContext,
    result: &mut EvalResult,
    name: &str,
    args: Value,
) -> Value {
    result.tool_calls += 1;
    if name == "load_workflow" && args.get("xml").is_some() {
        result.raw_xml_calls += 1;
    }
    let response = tools::dispatch(server, caller, name, args).expect("tool exists");
    match response {
        Ok(value) => {
            let text = value["content"][0]["text"].as_str().unwrap().to_string();
            serde_json::from_str(&text).unwrap_or_else(|_| json!({ "text": text }))
        }
        Err(error) => panic!("{}: tool {name} failed: {error}", result.name),
    }
}

fn new_eval(name: &'static str) -> EvalResult {
    EvalResult {
        name,
        valid: false,
        tool_calls: 0,
        raw_xml_calls: 0,
    }
}

fn node_id(value: &Value) -> String {
    value["node_id"].as_str().unwrap().to_string()
}

fn finish(server: &McpServer, caller: &CallerContext, result: &mut EvalResult, session: &str) {
    call(
        server,
        caller,
        result,
        "auto_layout",
        json!({ "session_id": session }),
    );
    let validation = call(
        server,
        caller,
        result,
        "validate_workflow",
        json!({ "session_id": session }),
    );
    result.valid = validation["valid"] == json!(true);
    assert!(result.valid, "{}: {:?}", result.name, validation["issues"]);
}

fn linear_approval(server: &McpServer, caller: &CallerContext) -> EvalResult {
    let mut result = new_eval("linear_approval");
    let created = call(
        server,
        caller,
        &mut result,
        "create_workflow",
        json!({ "name": "Invoice approval" }),
    );
    let session = created["session_id"].as_str().unwrap();
    let start = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "StartEvent" }),
    ));
    let review = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "UserTask" }),
    ));
    let end = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "EndEvent" }),
    ));
    for (source, target) in [(&start, &review), (&review, &end)] {
        call(
            server,
            caller,
            &mut result,
            "connect_nodes",
            json!({ "session_id": session, "source": source, "target": target }),
        );
    }
    finish(server, caller, &mut result, session);
    result
}

fn parallel_signing(server: &McpServer, caller: &CallerContext) -> EvalResult {
    let mut result = new_eval("parallel_signing");
    let created = call(
        server,
        caller,
        &mut result,
        "create_workflow",
        json!({ "name": "Parallel signing" }),
    );
    let session = created["session_id"].as_str().unwrap();
    let start = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "StartEvent" }),
    ));
    let fork = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "ParallelGateway" }),
    ));
    let a = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "UserTask", "name": "Legal" }),
    ));
    let b = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "UserTask", "name": "Finance" }),
    ));
    let e1 = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "EndEvent" }),
    ));
    let e2 = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "EndEvent" }),
    ));
    for (source, target) in [
        (&start, &fork),
        (&fork, &a),
        (&fork, &b),
        (&a, &e1),
        (&b, &e2),
    ] {
        call(
            server,
            caller,
            &mut result,
            "connect_nodes",
            json!({ "session_id": session, "source": source, "target": target }),
        );
    }
    finish(server, caller, &mut result, session);
    result
}

fn timer_escalation(server: &McpServer, caller: &CallerContext) -> EvalResult {
    let mut result = new_eval("timer_escalation");
    let created = call(
        server,
        caller,
        &mut result,
        "create_workflow",
        json!({ "name": "Ticket escalation" }),
    );
    let session = created["session_id"].as_str().unwrap();
    let start = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({
            "session_id": session,
            "node_type": "StartEvent",
            "config": { "event_type": { "timer": { "time_duration": "PT1H" } } },
        }),
    ));
    let agent = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "UserTask", "name": "Agent" }),
    ));
    let end = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "EndEvent" }),
    ));
    for (source, target) in [(&start, &agent), (&agent, &end)] {
        call(
            server,
            caller,
            &mut result,
            "connect_nodes",
            json!({ "session_id": session, "source": source, "target": target }),
        );
    }
    call(
        server,
        caller,
        &mut result,
        "add_timer",
        json!({
            "session_id": session,
            "name": "escalate",
            "timer_type": "duration",
            "time_duration": "PT1H",
        }),
    );
    finish(server, caller, &mut result, session);
    result
}

fn service_notification(server: &McpServer, caller: &CallerContext) -> EvalResult {
    let mut result = new_eval("service_notification");
    let created = call(
        server,
        caller,
        &mut result,
        "create_workflow",
        json!({ "name": "Notify" }),
    );
    let session = created["session_id"].as_str().unwrap();
    let start = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "StartEvent" }),
    ));
    let service = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({
            "session_id": session,
            "node_type": "ServiceTask",
            "config": { "implementation": { "type": "Http", "config": { "url": "https://example.com/hook", "method": "POST" } } },
        }),
    ));
    let end = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "EndEvent" }),
    ));
    for (source, target) in [(&start, &service), (&service, &end)] {
        call(
            server,
            caller,
            &mut result,
            "connect_nodes",
            json!({ "session_id": session, "source": source, "target": target }),
        );
    }
    finish(server, caller, &mut result, session);
    result
}

fn gateway_loop(server: &McpServer, caller: &CallerContext) -> EvalResult {
    let mut result = new_eval("gateway_loop");
    let created = call(
        server,
        caller,
        &mut result,
        "create_workflow",
        json!({ "name": "Review loop" }),
    );
    let session = created["session_id"].as_str().unwrap();
    let start = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "StartEvent" }),
    ));
    let review = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "UserTask", "name": "Review" }),
    ));
    let decision = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "ExclusiveGateway" }),
    ));
    let end = node_id(&call(
        server,
        caller,
        &mut result,
        "add_node",
        json!({ "session_id": session, "node_type": "EndEvent" }),
    ));
    for (source, target) in [(&start, &review), (&review, &decision)] {
        call(
            server,
            caller,
            &mut result,
            "connect_nodes",
            json!({ "session_id": session, "source": source, "target": target }),
        );
    }
    call(
        server,
        caller,
        &mut result,
        "connect_nodes",
        json!({
            "session_id": session,
            "source": decision,
            "target": review,
            "condition": "approved == false",
            "label": "retry",
        }),
    );
    call(
        server,
        caller,
        &mut result,
        "connect_nodes",
        json!({
            "session_id": session,
            "source": decision,
            "target": end,
            "condition": "approved == true",
            "label": "done",
        }),
    );
    finish(server, caller, &mut result, session);
    result
}

#[test]
fn agent_evaluation_suite_scores_at_least_ninety_percent_valid() {
    let server = McpServer::new(Config {
        file_root: std::env::temp_dir(),
        ..Config::from_env()
    });
    let caller = CallerContext::local();
    let results = [
        linear_approval(&server, &caller),
        parallel_signing(&server, &caller),
        timer_escalation(&server, &caller),
        service_notification(&server, &caller),
        gateway_loop(&server, &caller),
    ];
    let valid = results.iter().filter(|result| result.valid).count();
    assert!(
        valid as f64 / results.len() as f64 >= 0.9,
        "valid {valid}/{}",
        results.len()
    );
    for result in &results {
        assert!(
            result.tool_calls <= 20,
            "{} used too many tools",
            result.name
        );
        assert_eq!(result.raw_xml_calls, 0, "{} used raw XML", result.name);
    }
}

#[test]
fn malformed_config_is_rejected_before_mutation() {
    let server = McpServer::new(Config::from_env());
    let caller = CallerContext::local();
    let created = tools::dispatch(
        &server,
        &caller,
        "create_workflow",
        json!({ "name": "Bad config" }),
    )
    .unwrap();
    let text = created["content"][0]["text"].as_str().unwrap();
    let session: Value = serde_json::from_str(text).unwrap();
    let session_id = session["session_id"].as_str().unwrap();
    let result = tools::dispatch(
        &server,
        &caller,
        "add_node",
        json!({
            "session_id": session_id,
            "node_type": "ServiceTask",
            "config": { "implementation": { "type": "Http", "config": { "nonsense": 1 } } },
        }),
    )
    .unwrap();
    assert_eq!(result["isError"], json!(true));
}
