//! Core document tools: create/get/load/save/discard a workflow session.

use nocode_bpmn::xml::parse_bpmn;
use nocode_model::Workflow;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::McpError;
use crate::persistence;
use crate::tools::{parse_args, schema_for, ToolDefinition};
use crate::{CallerContext, McpServer};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateParams {
    name: String,
    description: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum WorkflowView {
    Summary,
    Full,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GetParams {
    session_id: String,
    view: Option<WorkflowView>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LoadParams {
    workflow_json: Option<String>,
    xml: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum SaveTarget {
    File,
    Project,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SaveParams {
    session_id: String,
    target: SaveTarget,
    path: Option<String>,
    project_id: Option<String>,
    expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DiscardParams {
    session_id: String,
}

/// Compact node/type summary used by `get_workflow(view="summary")`.
pub fn workflow_summary(workflow: &Workflow) -> Value {
    let nodes = workflow
        .nodes
        .iter()
        .map(|node| {
            json!({
                "id": node.id.to_string(),
                "name": node.name,
                "node_type": crate::tools::graph::node_type_tag(&node.node_type),
            })
        })
        .collect::<Vec<_>>();
    let edges = workflow
        .edges
        .iter()
        .map(|edge| {
            json!({
                "id": edge.id.to_string(),
                "source": edge.source.to_string(),
                "target": edge.target.to_string(),
                "label": edge.label,
                "condition": edge.condition,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "id": workflow.id.to_string(),
        "name": workflow.name,
        "description": workflow.description,
        "version": workflow.version,
        "node_count": workflow.nodes.len(),
        "edge_count": workflow.edges.len(),
        "nodes": nodes,
        "edges": edges,
    })
}

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "create_workflow".into(),
            description: "Create a new, empty workflow session and return its session_id. Use the granular add_node/connect_nodes tools next; valid node types are in bpmn://catalog/node-types.".into(),
            input_schema: schema_for::<CreateParams>(),
        },
        ToolDefinition {
            name: "get_workflow".into(),
            description: "Read a workflow session. view=summary (default) returns counts and node/edge ids; view=full returns the complete Workflow JSON. See bpmn://schema/workflow.".into(),
            input_schema: schema_for::<GetParams>(),
        },
        ToolDefinition {
            name: "load_workflow".into(),
            description: "Create a session from native workflow_json OR BPMN XML (xml plus optional name). Import is an interop entry point; prefer create_workflow for authoring. See bpmn://examples/json/minimal and bpmn://examples/xml/minimal.".into(),
            input_schema: schema_for::<LoadParams>(),
        },
        ToolDefinition {
            name: "save_workflow".into(),
            description: "Persist a session to a JSON file (target=file, path relative to MCP_FILE_ROOT) or into a Bonafoid project (target=project, project_id) through the project API. Call auto_layout and validate_workflow first.".into(),
            input_schema: schema_for::<SaveParams>(),
        },
        ToolDefinition {
            name: "discard_workflow".into(),
            description: "Remove a session and its undo history from memory. The workflow itself is not deleted from any project; use save_workflow for durability.".into(),
            input_schema: schema_for::<DiscardParams>(),
        },
    ]
}

pub fn dispatch(
    server: &McpServer,
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
        "create_workflow" => Some(call(arguments, |p| create(server, caller, p))),
        "get_workflow" => Some(call(arguments, |p| get(server, caller, p))),
        "load_workflow" => Some(call(arguments, |p| load(server, caller, p))),
        "save_workflow" => Some(call(arguments, |p| save(server, caller, p))),
        "discard_workflow" => Some(call(arguments, |p| discard(server, caller, p))),
        _ => None,
    }
}

fn create(
    server: &McpServer,
    caller: &CallerContext,
    params: CreateParams,
) -> Result<Value, McpError> {
    let mut workflow = Workflow::new(params.name);
    workflow.description = params.description;
    let workflow_id = workflow.id.to_string();
    let (session_id, revision) = server.sessions.create(&caller.owner, workflow)?;
    Ok(json!({ "session_id": session_id, "workflow_id": workflow_id, "revision": revision }))
}

fn get(server: &McpServer, caller: &CallerContext, params: GetParams) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    server.sessions.with_read(&key, |session| {
        let workflow = match params.view.unwrap_or(WorkflowView::Summary) {
            WorkflowView::Full => serde_json::to_value(&session.workflow)?,
            WorkflowView::Summary => workflow_summary(&session.workflow),
        };
        Ok(json!({
            "session_id": params.session_id,
            "revision": session.revision,
            "workflow": workflow,
        }))
    })
}

fn load(server: &McpServer, caller: &CallerContext, params: LoadParams) -> Result<Value, McpError> {
    let workflow = match (params.workflow_json, params.xml) {
        (Some(json), None) => serde_json::from_str::<Workflow>(&json)?,
        (None, Some(xml)) => parse_bpmn(&xml, params.name.as_deref().unwrap_or("Imported Process"))
            .map_err(|error| McpError::InvalidXml(error.to_string()))?,
        (Some(_), Some(_)) => {
            return Err(McpError::InvalidParam {
                name: "workflow_json",
                detail: "provide workflow_json OR xml, not both".into(),
            })
        }
        (None, None) => {
            return Err(McpError::InvalidParam {
                name: "workflow_json",
                detail: "provide workflow_json or xml".into(),
            })
        }
    };
    if workflow.nodes.len() > server.config.max_nodes {
        return Err(McpError::NodeLimit {
            limit: server.config.max_nodes,
        });
    }
    let workflow_id = workflow.id.to_string();
    let (session_id, revision) = server.sessions.create(&caller.owner, workflow.clone())?;
    Ok(json!({
        "session_id": session_id,
        "workflow_id": workflow_id,
        "revision": revision,
        "node_count": workflow.nodes.len(),
        "edge_count": workflow.edges.len(),
    }))
}

fn save(server: &McpServer, caller: &CallerContext, params: SaveParams) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let workflow = server
        .sessions
        .with_read(&key, |session| Ok(session.workflow.clone()))?;

    match params.target {
        SaveTarget::File => {
            let path = params.path.ok_or_else(|| McpError::InvalidParam {
                name: "path",
                detail: "required when target=file".into(),
            })?;
            let written = persistence::save_to_file(&server.config.file_root, &path, &workflow)?;
            Ok(json!({
                "saved": "file",
                "path": written.to_string_lossy(),
                "workflow_id": workflow.id.to_string(),
            }))
        }
        SaveTarget::Project => {
            let project_id = params.project_id.ok_or_else(|| McpError::InvalidParam {
                name: "project_id",
                detail: "required when target=project".into(),
            })?;
            // Honour optimistic concurrency for the session as well as the
            // project-level revision checked inside save_to_project.
            if let Some(expected) = params.expected_revision {
                server.sessions.with_read(&key, |session| {
                    if session.revision != expected {
                        Err(McpError::StaleRevision {
                            expected,
                            current: session.revision,
                        })
                    } else {
                        Ok(())
                    }
                })?;
            }
            persistence::save_to_project(&server.config, &project_id, &workflow)
        }
    }
}

fn discard(
    server: &McpServer,
    caller: &CallerContext,
    params: DiscardParams,
) -> Result<Value, McpError> {
    server
        .sessions
        .discard(&caller.session_key(&params.session_id))?;
    Ok(json!({ "discarded": params.session_id }))
}
