//! NAPI-RS bridge exposing the BPMN engine to the JavaScript/TypeScript IDE
//! frontend (which renders diagrams with `bpmn-js`).
//!
//! Every function accepts or returns JSON strings so the bridge stays stable
//! as the underlying model evolves. When this crate is compiled as a Node
//! addon (`crate-type = ["cdylib"]`) these become native module exports; when
//! compiled as part of the Rust workspace they are ordinary functions that the
//! Tauri backend can call directly.

use indexmap::IndexMap;
use napi_derive::napi;
use serde_json::Value;

use nocode_model::Workflow;

use crate::engine::{BpmnEngine, ProcessInstance};
use crate::xml;

/// Parse a JSON workflow definition.
fn parse_workflow(json: &str) -> napi::Result<Workflow> {
    serde_json::from_str(json)
        .map_err(|e| napi::Error::from_reason(format!("invalid workflow: {e}")))
}

/// Serialize any value to a JSON string.
fn to_json<T: serde::Serialize>(value: &T) -> napi::Result<String> {
    serde_json::to_string(value)
        .map_err(|e| napi::Error::from_reason(format!("serialization error: {e}")))
}

/// Validate a workflow definition. Returns a JSON [`crate::engine::ValidationResult`].
#[napi]
pub fn validate_workflow(workflow_json: String) -> napi::Result<String> {
    let workflow = parse_workflow(&workflow_json)?;
    let engine = BpmnEngine::new();
    to_json(&engine.validate(&workflow))
}

/// Start a process instance. `variables_json` must be a JSON object (or empty).
#[napi]
pub fn start_process(workflow_json: String, variables_json: String) -> napi::Result<String> {
    let workflow = parse_workflow(&workflow_json)?;
    let variables = parse_variables(&variables_json)?;
    let engine = BpmnEngine::new();
    let instance = engine
        .start(&workflow, variables)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    to_json(&instance)
}

/// Complete a user task. `variables_json` provides variables to merge in.
#[napi]
pub fn complete_task(
    workflow_json: String,
    instance_json: String,
    task_id: String,
    variables_json: String,
) -> napi::Result<String> {
    let workflow = parse_workflow(&workflow_json)?;
    let mut instance: ProcessInstance = serde_json::from_str(&instance_json)
        .map_err(|e| napi::Error::from_reason(format!("invalid instance: {e}")))?;
    let variables = parse_variables(&variables_json)?;
    let engine = BpmnEngine::new();
    engine
        .complete_task(&workflow, &mut instance, &task_id, variables)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    to_json(&instance)
}

/// Claim a user task for an assignee.
#[napi]
pub fn claim_task(
    instance_json: String,
    task_id: String,
    assignee: String,
) -> napi::Result<String> {
    let mut instance: ProcessInstance = serde_json::from_str(&instance_json)
        .map_err(|e| napi::Error::from_reason(format!("invalid instance: {e}")))?;
    let engine = BpmnEngine::new();
    engine
        .claim_task(&mut instance, &task_id, &assignee)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    to_json(&instance)
}

/// Terminate a running instance.
#[napi]
pub fn terminate_process(instance_json: String) -> napi::Result<String> {
    let mut instance: ProcessInstance = serde_json::from_str(&instance_json)
        .map_err(|e| napi::Error::from_reason(format!("invalid instance: {e}")))?;
    let engine = BpmnEngine::new();
    engine.terminate(&mut instance);
    to_json(&instance)
}

/// Import a BPMN 2.0 XML document into a workflow JSON definition.
#[napi]
pub fn import_bpmn_xml(xml_str: String, name: String) -> napi::Result<String> {
    let workflow =
        xml::parse_bpmn(&xml_str, &name).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    to_json(&workflow)
}

/// Export a workflow JSON definition to BPMN 2.0 XML.
#[napi]
pub fn export_bpmn_xml(workflow_json: String) -> napi::Result<String> {
    let workflow = parse_workflow(&workflow_json)?;
    xml::export_bpmn(&workflow).map_err(|e| napi::Error::from_reason(e.to_string()))
}

/// Evaluate a boolean condition expression against a variable map.
#[napi]
pub fn evaluate_condition(expression: String, variables_json: String) -> napi::Result<bool> {
    let variables = parse_variables(&variables_json)?;
    crate::expr::evaluate(&expression, &variables)
        .map_err(|e| napi::Error::from_reason(e.to_string()))
}

fn parse_variables(json: &str) -> napi::Result<IndexMap<String, Value>> {
    let trimmed = json.trim();
    if trimmed.is_empty() {
        return Ok(IndexMap::new());
    }
    let value: Value =
        serde_json::from_str(trimmed).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    match value {
        Value::Object(map) => Ok(map.into_iter().collect()),
        Value::Null => Ok(IndexMap::new()),
        _ => Err(napi::Error::from_reason(
            "variables must be a JSON object".to_string(),
        )),
    }
}
