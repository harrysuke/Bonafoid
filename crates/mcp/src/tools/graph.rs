//! Graph-editing tools — the primary authoring path.
//!
//! The server owns all UUIDs and, by default, all coordinates. Tools return
//! only affected ids/objects so agent context stays small.

use nocode_model::{
    EdgeId, EdgeStyle, NodeConfig, NodeId, NodePosition, WorkflowEdge, WorkflowNode,
    WorkflowNodeType,
};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::McpError;
use crate::layout::{self, LayoutDirection, LayoutOptions};
use crate::resources::catalogue::{
    self, default_node_type, merge_json_objects, parse_node_type_strict, validate_config_patch,
};
use crate::tools::{parse_args, schema_for, ToolDefinition};
use crate::{CallerContext, McpServer};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddNodeParams {
    session_id: String,
    node_type: String,
    name: Option<String>,
    config: Option<Value>,
    position: Option<NodePosition>,
    documentation: Option<String>,
    expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateNodeParams {
    session_id: String,
    node_id: String,
    name: Option<String>,
    config: Option<Value>,
    position: Option<NodePosition>,
    documentation: Option<Option<String>>,
    expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RemoveNodeParams {
    session_id: String,
    node_id: String,
    expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ConnectParams {
    session_id: String,
    source: String,
    target: String,
    label: Option<String>,
    condition: Option<String>,
    source_handle: Option<String>,
    target_handle: Option<String>,
    expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateEdgeParams {
    session_id: String,
    edge_id: String,
    label: Option<String>,
    condition: Option<Option<String>>,
    style: Option<Value>,
    expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RemoveEdgeParams {
    session_id: String,
    edge_id: String,
    expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LayoutParams {
    session_id: String,
    direction: Option<LayoutDirection>,
    column_width: Option<f64>,
    row_spacing: Option<f64>,
    margin: Option<f64>,
    expected_revision: Option<u64>,
}

/// The serialised `type` tag of a node's typed payload.
pub fn node_type_tag(node_type: &WorkflowNodeType) -> String {
    serde_json::to_value(node_type)
        .ok()
        .and_then(|value| value.get("type")?.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn node_index(workflow: &nocode_model::Workflow, id: &str) -> Result<usize, McpError> {
    workflow
        .nodes
        .iter()
        .position(|node| node.id.to_string() == id)
        .ok_or_else(|| McpError::NodeNotFound(id.to_string()))
}

fn edge_index(workflow: &nocode_model::Workflow, id: &str) -> Result<usize, McpError> {
    workflow
        .edges
        .iter()
        .position(|edge| edge.id.to_string() == id)
        .ok_or_else(|| McpError::EdgeNotFound(id.to_string()))
}

fn clear_default_flows(workflow: &mut nocode_model::Workflow, removed: &[EdgeId]) {
    for node in &mut workflow.nodes {
        if let WorkflowNodeType::ExclusiveGateway { default_flow }
        | WorkflowNodeType::InclusiveGateway { default_flow } = &mut node.node_type
        {
            if let Some(flow) = default_flow.as_ref() {
                if removed.contains(flow) {
                    *default_flow = None;
                }
            }
        }
    }
}

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "add_node".into(),
            description: "Add one node to the current workflow. node_type must be one of the 16 tags in bpmn://catalog/node-types; config is a partial typed payload and defaults are applied. Omit position to let the server place it. Returns node_id and applied defaults.".into(),
            input_schema: schema_for::<AddNodeParams>(),
        },
        ToolDefinition {
            name: "update_node".into(),
            description: "Update a node's name, documentation, position or config. config is deep-merged into the existing typed payload, never replaced wholesale. Use get_workflow(view=full) to see the current shape.".into(),
            input_schema: schema_for::<UpdateNodeParams>(),
        },
        ToolDefinition {
            name: "remove_node".into(),
            description: "Remove a node and every incident edge; returns the removed edge ids. Gateway default_flow references to those edges are cleared automatically.".into(),
            input_schema: schema_for::<RemoveNodeParams>(),
        },
        ToolDefinition {
            name: "connect_nodes".into(),
            description: "Connect two existing nodes and return the generated edge_id. Put gateway conditions on the outgoing edge via condition (for example approved == true), not on the node. Both endpoints must exist; see bpmn://guide/authoring.".into(),
            input_schema: schema_for::<ConnectParams>(),
        },
        ToolDefinition {
            name: "update_edge".into(),
            description: "Update a sequence flow's label, condition or style patch. Gateway branch expressions live in condition; conditions use the syntax described in bpmn://guide/authoring.".into(),
            input_schema: schema_for::<UpdateEdgeParams>(),
        },
        ToolDefinition {
            name: "remove_edge".into(),
            description: "Remove one sequence flow by edge_id and clear any gateway default_flow that pointed at it.".into(),
            input_schema: schema_for::<RemoveEdgeParams>(),
        },
        ToolDefinition {
            name: "auto_layout".into(),
            description: "Recompute every node position from graph topology with deterministic layered placement (LR by default). Call this after a batch of edits and before export; it is the single owner of coordinates.".into(),
            input_schema: schema_for::<LayoutParams>(),
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
        "add_node" => Some(call(arguments, |p| add_node(server, caller, p))),
        "update_node" => Some(call(arguments, |p| update_node(server, caller, p))),
        "remove_node" => Some(call(arguments, |p| remove_node(server, caller, p))),
        "connect_nodes" => Some(call(arguments, |p| connect_nodes(server, caller, p))),
        "update_edge" => Some(call(arguments, |p| update_edge(server, caller, p))),
        "remove_edge" => Some(call(arguments, |p| remove_edge(server, caller, p))),
        "auto_layout" => Some(call(arguments, |p| auto_layout(server, caller, p))),
        _ => None,
    }
}

fn add_node(
    server: &McpServer,
    caller: &CallerContext,
    params: AddNodeParams,
) -> Result<Value, McpError> {
    let entry = catalogue::catalogue()
        .into_iter()
        .find(|entry| entry.node_type == params.node_type)
        .ok_or_else(|| McpError::UnknownNodeType(params.node_type.clone()))?;
    let default = default_node_type(&params.node_type)
        .ok_or_else(|| McpError::UnknownNodeType(params.node_type.clone()))?;
    let mut payload = serde_json::to_value(default).map_err(McpError::from)?;
    let supplied_keys: Vec<String> = params
        .config
        .as_ref()
        .and_then(Value::as_object)
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();

    if let Some(patch) = params.config.as_ref() {
        validate_config_patch(&params.node_type, patch)?;
        if let Some(config) = payload.get_mut("config") {
            merge_json_objects(config, patch);
        } else if !supplied_keys.is_empty() {
            return Err(McpError::UnknownConfigKey {
                node_type: params.node_type.clone(),
                key: supplied_keys[0].clone(),
            });
        }
    }
    let typed: WorkflowNodeType = parse_node_type_strict(&payload, &params.node_type)?;
    let defaults_applied: Vec<&str> = entry
        .config_keys
        .iter()
        .copied()
        .filter(|key| {
            !supplied_keys
                .iter()
                .any(|supplied| supplied.as_str() == *key)
        })
        .collect();
    let node_id = NodeId::new();
    let name = params.name.unwrap_or_else(|| entry.label.to_string());
    let position = params.position.unwrap_or_default();
    let documentation = params.documentation;
    let key = caller.session_key(&params.session_id);

    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            if workflow.nodes.len() >= server.config.max_nodes {
                return Err(McpError::NodeLimit {
                    limit: server.config.max_nodes,
                });
            }
            workflow.nodes.push(WorkflowNode {
                id: node_id.clone(),
                name: name.clone(),
                node_type: typed.clone(),
                position: position.clone(),
                config: NodeConfig::default(),
                documentation: documentation.clone(),
            });
            Ok(json!({
                "node_id": node_id.to_string(),
                "name": name,
                "node_type": params.node_type,
                "position": position,
                "config_defaults_applied": defaults_applied,
            }))
        },
    )?;
    Ok(result)
}

fn update_node(
    server: &McpServer,
    caller: &CallerContext,
    params: UpdateNodeParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let node_id = params.node_id;
    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            let index = node_index(workflow, &node_id)?;
            let node = &mut workflow.nodes[index];
            let mut updated = Vec::new();

            if let Some(name) = params.name {
                node.name = name;
                updated.push("name");
            }
            if let Some(documentation) = params.documentation {
                node.documentation = documentation;
                updated.push("documentation");
            }
            if let Some(position) = params.position {
                node.position = position;
                updated.push("position");
            }
            if let Some(patch) = params.config.as_ref() {
                let tag = node_type_tag(&node.node_type);
                validate_config_patch(&tag, patch)?;
                let mut payload = serde_json::to_value(&node.node_type).map_err(McpError::from)?;
                if let Some(config) = payload.get_mut("config") {
                    merge_json_objects(config, patch);
                } else if patch.as_object().is_some_and(|map| !map.is_empty()) {
                    return Err(McpError::UnknownConfigKey {
                        node_type: tag,
                        key: patch
                            .as_object()
                            .and_then(|map| map.keys().next().cloned())
                            .unwrap_or_default(),
                    });
                }
                node.node_type = parse_node_type_strict(&payload, &tag)?;
                updated.push("config");
            }
            Ok(json!({ "node_id": node_id, "updated": updated }))
        },
    )?;
    Ok(result)
}

fn remove_node(
    server: &McpServer,
    caller: &CallerContext,
    params: RemoveNodeParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let node_id = params.node_id;
    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            let index = node_index(workflow, &node_id)?;
            let id = workflow.nodes[index].id.clone();
            let removed: Vec<EdgeId> = workflow
                .edges
                .iter()
                .filter(|edge| edge.source == id || edge.target == id)
                .map(|edge| edge.id.clone())
                .collect();
            let removed_strings: Vec<String> = removed.iter().map(EdgeId::to_string).collect();
            workflow
                .edges
                .retain(|edge| edge.source != id && edge.target != id);
            clear_default_flows(workflow, &removed);
            workflow.nodes.remove(index);
            Ok(json!({
                "node_id": node_id,
                "removed_edge_ids": removed_strings,
            }))
        },
    )?;
    Ok(result)
}

fn connect_nodes(
    server: &McpServer,
    caller: &CallerContext,
    params: ConnectParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            if params.source == params.target {
                return Err(McpError::InvalidParam {
                    name: "target",
                    detail: "source and target must be different nodes".into(),
                });
            }
            let source = workflow.nodes[node_index(workflow, &params.source)?]
                .id
                .clone();
            let target = workflow.nodes[node_index(workflow, &params.target)?]
                .id
                .clone();
            let edge = WorkflowEdge {
                id: EdgeId::new(),
                source: source.clone(),
                target: target.clone(),
                source_handle: params.source_handle,
                target_handle: params.target_handle,
                condition: params.condition,
                label: params.label,
                style: EdgeStyle::default(),
            };
            let edge_id = edge.id.to_string();
            workflow.edges.push(edge);
            Ok(json!({
                "edge_id": edge_id,
                "source": source.to_string(),
                "target": target.to_string(),
            }))
        },
    )?;
    Ok(result)
}

fn update_edge(
    server: &McpServer,
    caller: &CallerContext,
    params: UpdateEdgeParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let edge_id = params.edge_id;
    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            let index = edge_index(workflow, &edge_id)?;
            let edge = &mut workflow.edges[index];
            let mut updated = Vec::new();
            if let Some(label) = params.label {
                edge.label = Some(label);
                updated.push("label");
            }
            if let Some(condition) = params.condition {
                edge.condition = condition;
                updated.push("condition");
            }
            if let Some(patch) = params.style.as_ref() {
                let mut style = serde_json::to_value(&edge.style).map_err(McpError::from)?;
                merge_json_objects(&mut style, patch);
                edge.style =
                    serde_json::from_value(style).map_err(|error| McpError::InvalidParam {
                        name: "style",
                        detail: error.to_string(),
                    })?;
                updated.push("style");
            }
            Ok(json!({ "edge_id": edge_id, "updated": updated }))
        },
    )?;
    Ok(result)
}

fn remove_edge(
    server: &McpServer,
    caller: &CallerContext,
    params: RemoveEdgeParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    let edge_id = params.edge_id;
    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            let index = edge_index(workflow, &edge_id)?;
            let id = workflow.edges.remove(index).id;
            clear_default_flows(workflow, &[id]);
            Ok(json!({ "edge_id": edge_id }))
        },
    )?;
    Ok(result)
}

fn auto_layout(
    server: &McpServer,
    caller: &CallerContext,
    params: LayoutParams,
) -> Result<Value, McpError> {
    let options = LayoutOptions {
        direction: params.direction.unwrap_or_default(),
        column_width: params
            .column_width
            .unwrap_or_else(crate::layout::default_column_width),
        row_spacing: params
            .row_spacing
            .unwrap_or_else(crate::layout::default_row_spacing),
        margin: params.margin.unwrap_or_else(crate::layout::default_margin),
    };
    let key = caller.session_key(&params.session_id);
    let (result, _) = server.sessions.mutate(
        &key,
        params.expected_revision,
        |workflow| -> Result<Value, McpError> {
            let positions = layout::apply_layout(workflow, &options);
            Ok(json!({
                "count": positions.len(),
                "positions": positions,
            }))
        },
    )?;
    Ok(result)
}
