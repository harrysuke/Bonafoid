//! Definition tools: variables, signals, timers and workflow settings.

use chrono::{DateTime, Utc};
use nocode_model::{
    HistoryLevel, NodeId, SerializationFormat, SignalDefinition, SignalId, SignalScope,
    TimerDefinition, TimerId, TimerType, VariableId, VariableScope, VariableType, WorkflowVariable,
};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::McpError;
use crate::executor::McpToolExecutor;
use crate::tools::{parse_args, schema_for, ToolDefinition};
use crate::CallerContext;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddVariableParams {
    session_id: String,
    name: String,
    /// Either a simple tag ("String", "Boolean", ...) or the full typed payload.
    variable_type: Value,
    default_value: Option<Value>,
    scope: Option<VariableScope>,
    description: Option<String>,
    expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateVariableParams {
    session_id: String,
    name: String,
    new_name: Option<String>,
    variable_type: Option<Value>,
    default_value: Option<Option<Value>>,
    scope: Option<VariableScope>,
    description: Option<Option<String>>,
    expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RemoveVariableParams {
    session_id: String,
    name: String,
    expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddSignalParams {
    session_id: String,
    name: String,
    scope: Option<SignalScope>,
    payload_type: Option<Value>,
    expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddTimerParams {
    session_id: String,
    name: String,
    timer_type: TimerType,
    time_duration: Option<String>,
    time_cycle: Option<String>,
    time_date: Option<DateTime<Utc>>,
    attached_to: Option<String>,
    expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SettingsParams {
    session_id: String,
    is_executable: Option<bool>,
    history_level: Option<HistoryLevel>,
    async_continuations: Option<bool>,
    exclusive_gateway_default: Option<bool>,
    timeout: Option<Option<String>>,
    variables_serialization: Option<SerializationFormat>,
    expected_revision: Option<u64>,
}

fn parse_variable_type(value: &Value) -> Result<VariableType, McpError> {
    let payload = match value {
        Value::String(name) => json!({ "type": name }),
        other => other.clone(),
    };
    serde_json::from_value(payload).map_err(|error| McpError::InvalidParam {
        name: "variable_type",
        detail: error.to_string(),
    })
}

fn parse_node_id(value: &str) -> Result<NodeId, McpError> {
    Uuid::parse_str(value)
        .map(NodeId)
        .map_err(|error| McpError::InvalidParam {
            name: "attached_to",
            detail: error.to_string(),
        })
}

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "add_variable".into(),
            description: "Declare a workflow variable. variable_type accepts a simple tag such as String/Integer/Boolean/Object/Array or the full typed payload. See bpmn://schema/workflow.".into(),
            input_schema: schema_for::<AddVariableParams>(),
        },
        ToolDefinition {
            name: "update_variable".into(),
            description: "Update a variable by name. Fields are optional patches; pass null for default_value or description to clear them.".into(),
            input_schema: schema_for::<UpdateVariableParams>(),
        },
        ToolDefinition {
            name: "remove_variable".into(),
            description: "Remove a workflow variable by name. Returns the removed variable name.".into(),
            input_schema: schema_for::<RemoveVariableParams>(),
        },
        ToolDefinition {
            name: "add_signal".into(),
            description: "Declare a signal definition with an optional payload type. Signal events reference signals by name.".into(),
            input_schema: schema_for::<AddSignalParams>(),
        },
        ToolDefinition {
            name: "add_timer".into(),
            description: "Declare a timer definition (duration, cycle or date), optionally attached to a node id. Use it for timer start events and escalation branches.".into(),
            input_schema: schema_for::<AddTimerParams>(),
        },
        ToolDefinition {
            name: "set_workflow_settings".into(),
            description: "Patch workflow settings such as is_executable, history_level, timeout and serialization format. Only supplied fields change.".into(),
            input_schema: schema_for::<SettingsParams>(),
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
        "add_variable" => Some(call(arguments, |p| add_variable(server, caller, p))),
        "update_variable" => Some(call(arguments, |p| update_variable(server, caller, p))),
        "remove_variable" => Some(call(arguments, |p| remove_variable(server, caller, p))),
        "add_signal" => Some(call(arguments, |p| add_signal(server, caller, p))),
        "add_timer" => Some(call(arguments, |p| add_timer(server, caller, p))),
        "set_workflow_settings" => Some(call(arguments, |p| set_settings(server, caller, p))),
        _ => None,
    }
}

fn add_variable(
    server: &McpToolExecutor,
    caller: &CallerContext,
    params: AddVariableParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let name = params.name;
    let variable_type = parse_variable_type(&params.variable_type)?;
    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            if workflow.variables.iter().any(|v| v.name == name) {
                return Err(McpError::InvalidParam {
                    name: "name",
                    detail: format!("variable `{name}` already exists"),
                });
            }
            let id = VariableId::new();
            workflow.variables.push(WorkflowVariable {
                id: id.clone(),
                name: name.clone(),
                variable_type,
                default_value: params.default_value,
                scope: params.scope.unwrap_or(VariableScope::Process),
                description: params.description,
            });
            Ok(json!({ "id": id.to_string(), "name": name }))
        },
    )?;
    Ok(result)
}

fn update_variable(
    server: &McpToolExecutor,
    caller: &CallerContext,
    params: UpdateVariableParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let name = params.name;
    let variable_type = params
        .variable_type
        .as_ref()
        .map(parse_variable_type)
        .transpose()?;
    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            let index = workflow
                .variables
                .iter()
                .position(|v| v.name == name)
                .ok_or_else(|| McpError::InvalidParam {
                    name: "name",
                    detail: format!("variable `{name}` not found"),
                })?;
            let variable = &mut workflow.variables[index];
            let mut updated = Vec::new();
            if let Some(new_name) = params.new_name {
                variable.name = new_name;
                updated.push("name");
            }
            if let Some(value) = variable_type {
                variable.variable_type = value;
                updated.push("variable_type");
            }
            if let Some(default_value) = params.default_value {
                variable.default_value = default_value;
                updated.push("default_value");
            }
            if let Some(scope) = params.scope {
                variable.scope = scope;
                updated.push("scope");
            }
            if let Some(description) = params.description {
                variable.description = description;
                updated.push("description");
            }
            Ok(json!({ "name": name, "updated": updated }))
        },
    )?;
    Ok(result)
}

fn remove_variable(
    server: &McpToolExecutor,
    caller: &CallerContext,
    params: RemoveVariableParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let name = params.name;
    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            let index = workflow
                .variables
                .iter()
                .position(|v| v.name == name)
                .ok_or_else(|| McpError::InvalidParam {
                    name: "name",
                    detail: format!("variable `{name}` not found"),
                })?;
            workflow.variables.remove(index);
            Ok(json!({ "removed": name }))
        },
    )?;
    Ok(result)
}

fn add_signal(
    server: &McpToolExecutor,
    caller: &CallerContext,
    params: AddSignalParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let name = params.name;
    let payload_type = params
        .payload_type
        .as_ref()
        .map(parse_variable_type)
        .transpose()?;
    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            if workflow.signals.iter().any(|s| s.name == name) {
                return Err(McpError::InvalidParam {
                    name: "name",
                    detail: format!("signal `{name}` already exists"),
                });
            }
            let id = SignalId::new();
            workflow.signals.push(SignalDefinition {
                id: id.clone(),
                name: name.clone(),
                scope: params.scope.unwrap_or(SignalScope::Process),
                payload_type,
            });
            Ok(json!({ "id": id.to_string(), "name": name }))
        },
    )?;
    Ok(result)
}

fn add_timer(
    server: &McpToolExecutor,
    caller: &CallerContext,
    params: AddTimerParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let name = params.name;
    let attached_to = params
        .attached_to
        .as_deref()
        .map(parse_node_id)
        .transpose()?;
    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            if let Some(node_id) = &attached_to {
                if !workflow.nodes.iter().any(|node| &node.id == node_id) {
                    return Err(McpError::NodeNotFound(node_id.to_string()));
                }
            }
            let id = TimerId::new();
            workflow.timers.push(TimerDefinition {
                id: id.clone(),
                name: name.clone(),
                timer_type: params.timer_type,
                time_duration: params.time_duration,
                time_cycle: params.time_cycle,
                time_date: params.time_date,
                attached_to,
            });
            Ok(json!({ "id": id.to_string(), "name": name }))
        },
    )?;
    Ok(result)
}

fn set_settings(
    server: &McpToolExecutor,
    caller: &CallerContext,
    params: SettingsParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            let settings = &mut workflow.settings;
            let mut updated = Vec::new();
            if let Some(value) = params.is_executable {
                settings.is_executable = value;
                updated.push("is_executable");
            }
            if let Some(value) = params.history_level {
                settings.history_level = value;
                updated.push("history_level");
            }
            if let Some(value) = params.async_continuations {
                settings.async_continuations = value;
                updated.push("async_continuations");
            }
            if let Some(value) = params.exclusive_gateway_default {
                settings.exclusive_gateway_default = value;
                updated.push("exclusive_gateway_default");
            }
            if let Some(value) = params.timeout {
                settings.timeout = value;
                updated.push("timeout");
            }
            if let Some(value) = params.variables_serialization {
                settings.variables_serialization = value;
                updated.push("variables_serialization");
            }
            Ok(json!({ "updated": updated }))
        },
    )?;
    Ok(result)
}
