//! Validation, simulation, BPMN export and history tools.

use nocode_bpmn::engine::BpmnEngine;
use nocode_bpmn::xml::export_bpmn;
use nocode_ide::bpmn::{run_workflow, RunOptions};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::McpError;
use crate::executor::McpToolExecutor;
use crate::tools::{parse_args, schema_for, ToolDefinition};
use crate::CallerContext;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SessionParams {
    session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SimulateParams {
    session_id: String,
    variables: Option<Value>,
    auto_complete_tasks: Option<bool>,
    max_rounds: Option<u32>,
}

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "validate_workflow".into(),
            description: "Structurally validate the session with BpmnEngine::validate. Tools allow temporarily incomplete graphs, so call this after a batch of edits and fix every error before saving.".into(),
            input_schema: schema_for::<SessionParams>(),
        },
        ToolDefinition {
            name: "simulate_workflow".into(),
            description: "Run the workflow in the IDE's token engine and return steps, open user tasks, events and status. Invalid workflows are refused. Set auto_complete_tasks=true to finish human tasks; max_rounds guards loops.".into(),
            input_schema: schema_for::<SimulateParams>(),
        },
        ToolDefinition {
            name: "export_bpmn_xml".into(),
            description: "Export BPMN 2.0 XML with BPMNDI/dc:Bounds generated from server-owned positions. This is a final interop artifact, never an authoring surface. Call auto_layout and validate_workflow first.".into(),
            input_schema: schema_for::<SessionParams>(),
        },
        ToolDefinition {
            name: "undo".into(),
            description: "Restore the previous workflow snapshot (bounded history, default depth 20). Use get_workflow(view=summary) afterwards to confirm the change.".into(),
            input_schema: schema_for::<SessionParams>(),
        },
        ToolDefinition {
            name: "redo".into(),
            description: "Re-apply a snapshot undone by undo. Returns the new revision.".into(),
            input_schema: schema_for::<SessionParams>(),
        },
    ]
}

pub fn dispatch(
    server: &McpToolExecutor,
    caller: &CallerContext,
    name: &str,
    arguments: Value,
) -> Option<Result<Value, McpError>> {
    fn call<T, F>(arguments: Value, handler: F) -> Result<Value, McpError>
    where
        T: DeserializeOwned,
        F: FnOnce(T) -> Result<Value, McpError>,
    {
        handler(parse_args::<T>(arguments)?)
    }

    match name {
        "validate_workflow" => Some(call(arguments, |p| validate(server, caller, p))),
        "simulate_workflow" => Some(call(arguments, |p| simulate(server, caller, p))),
        "export_bpmn_xml" => Some(call(arguments, |p| export(server, caller, p))),
        "undo" => Some(call(arguments, |p| history(server, caller, p, true))),
        "redo" => Some(call(arguments, |p| history(server, caller, p, false))),
        _ => None,
    }
}

fn validate(
    server: &McpToolExecutor,
    caller: &CallerContext,
    params: SessionParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    server.sessions.with_read(&key, |session| {
        let result = BpmnEngine::new().validate(&session.workflow);
        Ok(json!({
            "valid": result.is_valid(),
            "issues": result.issues,
        }))
    })
}

fn simulate(
    server: &McpToolExecutor,
    caller: &CallerContext,
    params: SimulateParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let workflow_json = server.sessions.with_read(&key, |session| {
        serde_json::to_string(&session.workflow).map_err(McpError::from)
    })?;
    let summary = run_workflow(
        &workflow_json,
        params.variables.unwrap_or(Value::Null),
        RunOptions {
            auto_complete_tasks: params.auto_complete_tasks.unwrap_or(false),
            max_rounds: params.max_rounds,
        },
    )
    .map_err(|error| McpError::InvalidParam {
        name: "workflow",
        detail: error,
    })?;
    serde_json::to_value(summary).map_err(McpError::from)
}

fn export(
    server: &McpToolExecutor,
    caller: &CallerContext,
    params: SessionParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    server.sessions.with_read(&key, |session| {
        let xml = export_bpmn(&session.workflow)
            .map_err(|error| McpError::InvalidXml(error.to_string()))?;
        Ok(json!({ "xml": xml, "bytes": xml.len() }))
    })
}

fn history(
    server: &McpToolExecutor,
    caller: &CallerContext,
    params: SessionParams,
    undo: bool,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let revision = if undo {
        server.sessions.undo(&key)?
    } else {
        server.sessions.redo(&key)?
    };
    Ok(json!({ "session_id": params.session_id, "revision": revision }))
}
