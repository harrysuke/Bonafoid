//! Backend wiring tools: create workflows and bind them to triggers and service
//! tasks.
//!
//! These are the tools that turn a design into something the generated backend
//! can execute. They mutate the workflow **inside a project session**, which is
//! why they take `session_id` + `workflow_id` rather than a workflow-session id.
//!
//! For graphs richer than a straight line, `workflow_open` projects one
//! workflow into a normal BPMN session so the 23 shipped graph tools can edit
//! it, and `workflow_commit` writes it back. That keeps the shipped tool
//! contract untouched.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use nocode_model::{
    EndEventType, HttpMethod, NodeConfig, NodeId, NodePosition, RetryConfig,
    ServiceTaskImplementation, StartEventType, Workflow, WorkflowEdge, WorkflowId, WorkflowNode,
    WorkflowNodeType,
};

use crate::error::McpError;
use crate::executor::McpToolExecutor;
use crate::tools::{parse_args, schema_for, ToolDefinition};
use crate::transport::http::CallerContext;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "workflow_create".into(),
            description: "Create an empty workflow in the project session. Returns workflow_id. \
                          Use workflow_scaffold_crud for the common CRUD case, or workflow_open to \
                          edit the graph with the BPMN tools."
                .into(),
            input_schema: schema_for::<WorkflowCreateParams>(),
        },
        ToolDefinition {
            name: "workflow_scaffold_crud".into(),
            description: "Create a start -> service task -> end workflow for one entity in a \
                          single call. `operation` is create, read, update or delete. The service \
                          task points at the generated /api/v1/{plural} route."
                .into(),
            input_schema: schema_for::<ScaffoldParams>(),
        },
        ToolDefinition {
            name: "workflow_open".into(),
            description: "Copy a workflow from the project session into an editable BPMN session. \
                          Returns a workflow_session_id for add_node/connect_nodes/auto_layout. \
                          Finish with workflow_commit."
                .into(),
            input_schema: schema_for::<WorkflowOpenParams>(),
        },
        ToolDefinition {
            name: "workflow_commit".into(),
            description: "Write an edited BPMN session back into the project session. Validates \
                          that the workflow still exists before overwriting it."
                .into(),
            input_schema: schema_for::<WorkflowCommitParams>(),
        },
        ToolDefinition {
            name: "workflow_set_start_trigger".into(),
            description: "Set how a workflow starts: webhook, schedule (cron), signal, message or \
                          none. node_id must be a StartEvent. Records a trigger descriptor so the \
                          UI and the catalogue can show it."
                .into(),
            input_schema: schema_for::<StartTriggerParams>(),
        },
        ToolDefinition {
            name: "workflow_bind_service_task".into(),
            description: "Bind a ServiceTask to an implementation: entity_crud (generated REST \
                          route), http, function, plugin or external connector. node_id must be a \
                          ServiceTask."
                .into(),
            input_schema: schema_for::<BindServiceTaskParams>(),
        },
    ]
}

/* ------------------------------------------------------------------ *
 * Parameters
 * ------------------------------------------------------------------ */

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorkflowCreateParams {
    pub session_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScaffoldParams {
    pub session_id: String,
    pub entity_id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// `create` (default), `read`, `update` or `delete`.
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub trigger: Option<TriggerSpec>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorkflowOpenParams {
    pub session_id: String,
    pub workflow_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorkflowCommitParams {
    /// The BPMN session created by `workflow_open`.
    pub session_id: String,
    /// The project session the workflow belongs to.
    pub project_session_id: String,
    pub workflow_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartTriggerParams {
    pub session_id: String,
    pub workflow_id: String,
    pub node_id: String,
    pub trigger: TriggerSpec,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BindServiceTaskParams {
    pub session_id: String,
    pub workflow_id: String,
    pub node_id: String,
    pub target: ServiceTarget,
}

/// How a workflow starts.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TriggerSpec {
    /// `webhook`, `schedule`, `signal`, `message`, `conditional` or `none`.
    pub kind: String,
    /// Webhook path, e.g. `/hooks/expense`.
    #[serde(default)]
    pub path: Option<String>,
    /// Cron expression for `schedule`, e.g. `0 9 * * 1`.
    #[serde(default)]
    pub cron: Option<String>,
    #[serde(default)]
    pub signal_ref: Option<String>,
    #[serde(default)]
    pub message_ref: Option<String>,
    #[serde(default)]
    pub condition: Option<String>,
}

/// What a service task does.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ServiceTarget {
    /// `entity_crud`, `http`, `function`, `plugin` or `external`.
    pub kind: String,
    #[serde(default)]
    pub entity_id: Option<String>,
    /// `create`, `read`, `update` or `delete` (for `entity_crud`).
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub function_name: Option<String>,
    #[serde(default)]
    pub connector_id: Option<String>,
    #[serde(default)]
    pub input_mapping: Option<Value>,
    #[serde(default)]
    pub output_mapping: Option<Value>,
    #[serde(default)]
    pub config: Option<Value>,
}

pub fn handles(name: &str) -> bool {
    matches!(
        name,
        "workflow_create"
            | "workflow_scaffold_crud"
            | "workflow_open"
            | "workflow_commit"
            | "workflow_set_start_trigger"
            | "workflow_bind_service_task"
    )
}

pub fn dispatch(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    name: &str,
    arguments: Value,
) -> Option<Result<Value, McpError>> {
    fn call<T, F>(arguments: Value, handler: F) -> Result<Value, McpError>
    where
        T: serde::de::DeserializeOwned,
        F: FnOnce(T) -> Result<Value, McpError>,
    {
        handler(parse_args::<T>(arguments)?)
    }

    match name {
        "workflow_create" => Some(call(arguments, |p| workflow_create(executor, caller, p))),
        "workflow_scaffold_crud" => Some(call(arguments, |p| scaffold_crud(executor, caller, p))),
        "workflow_open" => Some(call(arguments, |p| workflow_open(executor, caller, p))),
        "workflow_commit" => Some(call(arguments, |p| workflow_commit(executor, caller, p))),
        "workflow_set_start_trigger" => {
            Some(call(arguments, |p| set_start_trigger(executor, caller, p)))
        }
        "workflow_bind_service_task" => {
            Some(call(arguments, |p| bind_service_task(executor, caller, p)))
        }
        _ => None,
    }
}

/* ------------------------------------------------------------------ *
 * Creation
 * ------------------------------------------------------------------ */

fn workflow_create(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    params: WorkflowCreateParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let (result, revision) = executor.projects.mutate(&key, None, |project| {
        let mut workflow = Workflow::new(params.name.clone());
        workflow.description = params.description.clone();
        let workflow_id = workflow.id.to_string();
        project.workflows.insert(workflow.id.clone(), workflow);
        Ok(json!({ "workflow_id": workflow_id }))
    })?;
    Ok(with_revision(result, revision))
}

fn scaffold_crud(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    params: ScaffoldParams,
) -> Result<Value, McpError> {
    let entity_id = uuid::Uuid::parse_str(&params.entity_id)
        .map(nocode_model::EntityId)
        .map_err(|_| McpError::InvalidParam {
            name: "entity_id",
            detail: format!("`{}` is not a UUID", params.entity_id),
        })?;
    let operation = params
        .operation
        .clone()
        .unwrap_or_else(|| "create".into())
        .to_ascii_lowercase();

    let key = caller.session_key(&params.session_id);
    let (result, revision) = executor.projects.mutate(&key, None, |project| {
        let entity = project
            .entities
            .get(&entity_id)
            .ok_or_else(|| McpError::NotFound(format!("entity {}", params.entity_id)))?;
        let entity_name = entity.name.clone();
        let plural = entity.plural_name.clone();

        let mut workflow = Workflow::new(
            params
                .name
                .clone()
                .unwrap_or_else(|| format!("{entity_name} {operation}")),
        );

        let event_type = match params.trigger.as_ref() {
            Some(trigger) => start_event_type(trigger)?,
            None => StartEventType::None,
        };
        let mut start = node(
            format!("{entity_name} {operation} received"),
            WorkflowNodeType::StartEvent { event_type },
            0.0,
            0.0,
        );
        if let Some(trigger) = params.trigger.as_ref() {
            start
                .config
                .properties
                .insert("trigger".into(), trigger_descriptor(trigger)?);
        }

        let service = node(
            format!("{operation} {entity_name}"),
            WorkflowNodeType::ServiceTask {
                implementation: entity_crud_implementation(&plural, &operation),
                r#async: false,
                exclusive: false,
            },
            260.0,
            0.0,
        );
        let end = node(
            format!("{entity_name} {operation} done"),
            WorkflowNodeType::EndEvent {
                event_type: EndEventType::None,
            },
            520.0,
            0.0,
        );

        let node_ids = vec![
            start.id.0.to_string(),
            service.id.0.to_string(),
            end.id.0.to_string(),
        ];
        workflow
            .edges
            .push(edge(start.id.clone(), service.id.clone()));
        workflow
            .edges
            .push(edge(service.id.clone(), end.id.clone()));
        workflow.nodes.extend([start, service, end]);

        let workflow_id = workflow.id.to_string();
        project.workflows.insert(workflow.id.clone(), workflow);
        Ok(json!({
            "workflow_id": workflow_id,
            "node_ids": {
                "start": node_ids[0],
                "service": node_ids[1],
                "end": node_ids[2],
            },
        }))
    })?;
    Ok(with_revision(result, revision))
}

fn node(name: String, node_type: WorkflowNodeType, x: f64, y: f64) -> WorkflowNode {
    WorkflowNode {
        id: NodeId::new(),
        name,
        node_type,
        position: NodePosition { x, y },
        config: NodeConfig::default(),
        documentation: None,
    }
}

fn edge(source: NodeId, target: NodeId) -> WorkflowEdge {
    WorkflowEdge {
        id: Default::default(),
        source,
        target,
        source_handle: None,
        target_handle: None,
        condition: None,
        label: None,
        style: Default::default(),
    }
}

/* ------------------------------------------------------------------ *
 * Session projection
 * ------------------------------------------------------------------ */

fn workflow_open(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    params: WorkflowOpenParams,
) -> Result<Value, McpError> {
    let workflow_id = parse_workflow_id(&params.workflow_id)?;
    let project_key = caller.session_key(&params.session_id);
    let workflow = executor.projects.with_read(&project_key, |session| {
        session
            .project
            .workflows
            .get(&workflow_id)
            .cloned()
            .ok_or_else(|| McpError::NotFound(format!("workflow {}", params.workflow_id)))
    })?;
    let (session_id, revision) = executor.sessions.create(&caller.owner, workflow)?;
    Ok(json!({
        "workflow_session_id": session_id,
        "revision": revision,
        "workflow_id": params.workflow_id,
    }))
}

fn workflow_commit(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    params: WorkflowCommitParams,
) -> Result<Value, McpError> {
    let workflow_id = parse_workflow_id(&params.workflow_id)?;
    let workflow_key = caller.session_key(&params.session_id);
    let workflow = executor
        .sessions
        .with_read(&workflow_key, |session| Ok(session.workflow.clone()))?;

    let project_key = caller.session_key(&params.project_session_id);
    let (result, revision) = executor.projects.mutate(&project_key, None, |project| {
        if !project.workflows.contains_key(&workflow_id) {
            return Err(McpError::NotFound(format!(
                "workflow {} in project session",
                params.workflow_id
            )));
        }
        project
            .workflows
            .insert(workflow_id.clone(), workflow.clone());
        Ok(json!({ "workflow_id": workflow_id.to_string() }))
    })?;
    Ok(with_revision(result, revision))
}

/* ------------------------------------------------------------------ *
 * Trigger and service-task binding
 * ------------------------------------------------------------------ */

fn set_start_trigger(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    params: StartTriggerParams,
) -> Result<Value, McpError> {
    let workflow_id = parse_workflow_id(&params.workflow_id)?;
    let node_id = parse_node_id(&params.node_id)?;
    let descriptor = trigger_descriptor(&params.trigger)?;
    let event_type = start_event_type(&params.trigger)?;

    let key = caller.session_key(&params.session_id);
    let (result, revision) = executor.projects.mutate(&key, None, |project| {
        let workflow = project
            .workflows
            .get_mut(&workflow_id)
            .ok_or_else(|| McpError::NotFound(format!("workflow {}", params.workflow_id)))?;
        let node = workflow
            .nodes
            .iter_mut()
            .find(|node| node.id == node_id)
            .ok_or_else(|| McpError::NotFound(format!("node {}", params.node_id)))?;

        match &mut node.node_type {
            WorkflowNodeType::StartEvent { event_type: slot } => {
                *slot = event_type;
            }
            _ => {
                return Err(McpError::InvalidParam {
                    name: "node_id",
                    detail: format!("`{}` is not a StartEvent", node.name),
                })
            }
        }
        node.config
            .properties
            .insert("trigger".into(), descriptor.clone());
        Ok(json!({ "trigger": descriptor }))
    })?;
    Ok(with_revision(result, revision))
}

fn bind_service_task(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    params: BindServiceTaskParams,
) -> Result<Value, McpError> {
    let workflow_id = parse_workflow_id(&params.workflow_id)?;
    let node_id = parse_node_id(&params.node_id)?;
    let target_kind = params.target.kind.to_ascii_lowercase();

    let key = caller.session_key(&params.session_id);
    let (result, revision) = executor.projects.mutate(&key, None, |project| {
        // `entity_crud` needs the entity, and the borrow of `project` must end
        // before the workflow is borrowed mutably.
        let implementation = match target_kind.as_str() {
            "entity_crud" => {
                let raw = params
                    .target
                    .entity_id
                    .as_deref()
                    .ok_or_else(|| McpError::InvalidParam {
                        name: "target.entity_id",
                        detail: "entity_crud needs an entity_id".into(),
                    })?;
                let entity_id = uuid::Uuid::parse_str(raw)
                    .map(nocode_model::EntityId)
                    .map_err(|_| McpError::InvalidParam {
                        name: "target.entity_id",
                        detail: format!("`{raw}` is not a UUID"),
                    })?;
                let entity = project
                    .entities
                    .get(&entity_id)
                    .ok_or_else(|| McpError::NotFound(format!("entity {raw}")))?;
                entity_crud_implementation(
                    &entity.plural_name,
                    params.target.operation.as_deref().unwrap_or("create"),
                )
            }
            "http" => ServiceTaskImplementation::Http {
                url: params
                    .target
                    .url
                    .clone()
                    .ok_or_else(|| McpError::InvalidParam {
                        name: "target.url",
                        detail: "http needs a url".into(),
                    })?,
                method: parse_method(params.target.method.as_deref())?,
                headers: Default::default(),
                body_template: None,
                timeout_ms: 5000,
                retry: RetryConfig::default(),
            },
            "function" => ServiceTaskImplementation::Function {
                function_name: params
                    .target
                    .function_name
                    .clone()
                    .ok_or_else(|| McpError::InvalidParam {
                        name: "target.function_name",
                        detail: "function needs a function_name".into(),
                    })?,
                input_mapping: mapping(&params.target.input_mapping)?,
                output_mapping: mapping(&params.target.output_mapping)?,
            },
            "plugin" => ServiceTaskImplementation::Plugin {
                plugin_id: params
                    .target
                    .plugin_id
                    .clone()
                    .ok_or_else(|| McpError::InvalidParam {
                        name: "target.plugin_id",
                        detail: "plugin needs a plugin_id".into(),
                    })?,
                config: params.target.config.clone().unwrap_or(Value::Null),
            },
            "external" => ServiceTaskImplementation::External {
                connector_id: params
                    .target
                    .connector_id
                    .clone()
                    .ok_or_else(|| McpError::InvalidParam {
                        name: "target.connector_id",
                        detail: "external needs a connector_id".into(),
                    })?,
                operation: params
                    .target
                    .operation
                    .clone()
                    .unwrap_or_else(|| "invoke".into()),
                config: params.target.config.clone().unwrap_or(Value::Null),
            },
            other => {
                return Err(McpError::InvalidParam {
                    name: "target.kind",
                    detail: format!(
                        "unknown target `{other}`; expected entity_crud, http, function, plugin or external"
                    ),
                })
            }
        };

        let workflow = project
            .workflows
            .get_mut(&workflow_id)
            .ok_or_else(|| McpError::NotFound(format!("workflow {}", params.workflow_id)))?;
        let node = workflow
            .nodes
            .iter_mut()
            .find(|node| node.id == node_id)
            .ok_or_else(|| McpError::NotFound(format!("node {}", params.node_id)))?;

        match &mut node.node_type {
            WorkflowNodeType::ServiceTask {
                implementation: slot,
                ..
            } => {
                *slot = implementation;
            }
            _ => {
                return Err(McpError::InvalidParam {
                    name: "node_id",
                    detail: format!("`{}` is not a ServiceTask", node.name),
                })
            }
        }
        node.config.properties.insert(
            "target".into(),
            json!({ "kind": target_kind, "operation": params.target.operation }),
        );
        Ok(json!({ "bound": true, "target": target_kind }))
    })?;
    Ok(with_revision(result, revision))
}

fn mapping(value: &Option<Value>) -> Result<indexmap::IndexMap<String, String>, McpError> {
    match value {
        Some(value) => {
            serde_json::from_value(value.clone()).map_err(|error| McpError::InvalidParam {
                name: "target.input_mapping",
                detail: error.to_string(),
            })
        }
        None => Ok(Default::default()),
    }
}

/// The generated REST route for an entity, mirroring `codegen`'s routes.
fn entity_crud_implementation(plural: &str, operation: &str) -> ServiceTaskImplementation {
    let path = format!("/api/v1/{}", slugify(plural));
    let method = match operation.to_ascii_lowercase().as_str() {
        "read" | "list" | "get" => HttpMethod::Get,
        "update" | "put" => HttpMethod::Put,
        "delete" | "remove" => HttpMethod::Delete,
        _ => HttpMethod::Post,
    };
    ServiceTaskImplementation::Http {
        url: path,
        method,
        headers: Default::default(),
        body_template: None,
        timeout_ms: 5000,
        retry: RetryConfig::default(),
    }
}

fn parse_method(value: Option<&str>) -> Result<HttpMethod, McpError> {
    match value.unwrap_or("POST").to_ascii_uppercase().as_str() {
        "GET" => Ok(HttpMethod::Get),
        "POST" => Ok(HttpMethod::Post),
        "PUT" => Ok(HttpMethod::Put),
        "PATCH" => Ok(HttpMethod::Patch),
        "DELETE" => Ok(HttpMethod::Delete),
        "HEAD" => Ok(HttpMethod::Head),
        "OPTIONS" => Ok(HttpMethod::Options),
        other => Err(McpError::InvalidParam {
            name: "target.method",
            detail: format!("unknown HTTP method `{other}`"),
        }),
    }
}

/// Map a trigger spec onto the model's start event type.
fn start_event_type(trigger: &TriggerSpec) -> Result<StartEventType, McpError> {
    match trigger.kind.to_ascii_lowercase().as_str() {
        "webhook" => Ok(StartEventType::Message {
            message_ref: trigger
                .path
                .clone()
                .or_else(|| trigger.message_ref.clone())
                .unwrap_or_else(|| "webhook".into()),
        }),
        "message" => Ok(StartEventType::Message {
            message_ref: trigger
                .message_ref
                .clone()
                .or_else(|| trigger.path.clone())
                .unwrap_or_else(|| "message".into()),
        }),
        "schedule" | "timer" => Ok(StartEventType::Timer {
            time_duration: if trigger.cron.is_some() {
                None
            } else {
                Some("PT1H".into())
            },
            time_cycle: trigger.cron.clone(),
        }),
        "signal" => Ok(StartEventType::Signal {
            signal_ref: trigger
                .signal_ref
                .clone()
                .unwrap_or_else(|| "signal".into()),
        }),
        "conditional" => Ok(StartEventType::Conditional {
            condition: trigger
                .condition
                .clone()
                .unwrap_or_else(|| "true".into()),
        }),
        "none" => Ok(StartEventType::None),
        other => Err(McpError::InvalidParam {
            name: "trigger.kind",
            detail: format!(
                "unknown trigger `{other}`; expected webhook, schedule, signal, message, conditional or none"
            ),
        }),
    }
}

/// The `trigger` property the UI and the catalogue read.
fn trigger_descriptor(trigger: &TriggerSpec) -> Result<Value, McpError> {
    let kind = trigger.kind.to_ascii_lowercase();
    match kind.as_str() {
        "webhook" => Ok(json!({
            "kind": "webhook",
            "path": trigger.path.clone().unwrap_or_else(|| "/hooks/workflow".into()),
        })),
        "schedule" | "timer" => Ok(json!({
            "kind": "schedule",
            "cron": trigger.cron.clone(),
        })),
        "signal" => Ok(json!({
            "kind": "signal",
            "signal_ref": trigger.signal_ref.clone(),
        })),
        "message" => Ok(json!({
            "kind": "message",
            "message_ref": trigger.message_ref.clone(),
        })),
        "conditional" => Ok(json!({
            "kind": "conditional",
            "condition": trigger.condition.clone(),
        })),
        "none" => Ok(json!({ "kind": "none" })),
        other => Err(McpError::InvalidParam {
            name: "trigger.kind",
            detail: format!("unknown trigger `{other}`"),
        }),
    }
}

fn parse_workflow_id(raw: &str) -> Result<WorkflowId, McpError> {
    uuid::Uuid::parse_str(raw)
        .map(WorkflowId)
        .map_err(|_| McpError::InvalidParam {
            name: "workflow_id",
            detail: format!("`{raw}` is not a UUID"),
        })
}

fn parse_node_id(raw: &str) -> Result<NodeId, McpError> {
    uuid::Uuid::parse_str(raw)
        .map(NodeId)
        .map_err(|_| McpError::InvalidParam {
            name: "node_id",
            detail: format!("`{raw}` is not a UUID"),
        })
}

fn with_revision(mut value: Value, revision: u64) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("revision".into(), json!(revision));
    }
    value
}

/// Reuse the page slug rules so route and path spellings cannot drift.
fn slugify(value: &str) -> String {
    crate::tools::domain::slugify(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_crud_maps_operations_to_methods() {
        match entity_crud_implementation("expenses", "read") {
            ServiceTaskImplementation::Http { method, url, .. } => {
                assert_eq!(method, HttpMethod::Get);
                assert_eq!(url, "/api/v1/expenses");
            }
            other => panic!("expected Http, got {other:?}"),
        }
        match entity_crud_implementation("expenses", "delete") {
            ServiceTaskImplementation::Http { method, .. } => {
                assert_eq!(method, HttpMethod::Delete)
            }
            other => panic!("expected Http, got {other:?}"),
        }
    }

    #[test]
    fn a_webhook_trigger_becomes_a_message_start() {
        let trigger = TriggerSpec {
            kind: "webhook".into(),
            path: Some("/hooks/expense".into()),
            cron: None,
            signal_ref: None,
            message_ref: None,
            condition: None,
        };
        assert_eq!(
            start_event_type(&trigger).unwrap(),
            StartEventType::Message {
                message_ref: "/hooks/expense".into()
            }
        );
        assert_eq!(trigger_descriptor(&trigger).unwrap()["kind"], "webhook");
    }

    #[test]
    fn a_cron_trigger_becomes_a_cycle_timer() {
        let trigger = TriggerSpec {
            kind: "schedule".into(),
            path: None,
            cron: Some("0 9 * * 1".into()),
            signal_ref: None,
            message_ref: None,
            condition: None,
        };
        match start_event_type(&trigger).unwrap() {
            StartEventType::Timer {
                time_cycle,
                time_duration,
            } => {
                assert_eq!(time_cycle.as_deref(), Some("0 9 * * 1"));
                assert!(time_duration.is_none());
            }
            other => panic!("expected Timer, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_trigger_is_rejected() {
        let trigger = TriggerSpec {
            kind: "cronish".into(),
            path: None,
            cron: None,
            signal_ref: None,
            message_ref: None,
            condition: None,
        };
        assert!(start_event_type(&trigger).is_err());
    }
}
