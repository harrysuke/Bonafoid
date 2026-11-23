//! Node-type catalogue: the single source used by both the
//! `bpmn://catalog/node-types` resource and `add_node` defaults/validation.
//!
//! Labels mirror `NODE_PALETTE` in
//! `packages/ide-core/src/utils/workflowModel.ts`; defaults mirror its
//! `buildNodeType()` so agent-authored nodes look identical to IDE-authored
//! ones.

use indexmap::IndexMap;
use nocode_model::retry::RetryConfig;
use nocode_model::{
    EndEventType, HttpMethod, ServiceTaskImplementation, StartEventType, TaskType, WorkflowId,
    WorkflowNodeType,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::McpError;

/// Public description of one node type.
#[derive(Debug, Clone)]
pub struct NodeTypeInfo {
    pub node_type: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub config_keys: &'static [&'static str],
    pub defaults: Value,
}

/// All 16 node types, in palette order.
pub const NODE_TYPE_NAMES: [&str; 16] = [
    "StartEvent",
    "EndEvent",
    "UserTask",
    "ServiceTask",
    "ScriptTask",
    "BusinessRuleTask",
    "SendTask",
    "ReceiveTask",
    "ManualTask",
    "Task",
    "ExclusiveGateway",
    "InclusiveGateway",
    "ParallelGateway",
    "EventBasedGateway",
    "SubProcess",
    "CallActivity",
];

/// Build the default payload for a node type tag.
pub fn default_node_type(tag: &str) -> Option<WorkflowNodeType> {
    let node_type = match tag {
        "StartEvent" => WorkflowNodeType::StartEvent {
            event_type: StartEventType::None,
        },
        "EndEvent" => WorkflowNodeType::EndEvent {
            event_type: EndEventType::None,
        },
        "UserTask" => WorkflowNodeType::UserTask {
            assignee: None,
            candidate_groups: Vec::new(),
            form_id: None,
            due_date: None,
        },
        "ServiceTask" => WorkflowNodeType::ServiceTask {
            implementation: ServiceTaskImplementation::Http {
                url: String::new(),
                method: HttpMethod::Post,
                headers: IndexMap::new(),
                body_template: None,
                timeout_ms: 30_000,
                retry: RetryConfig::default(),
            },
            r#async: false,
            exclusive: false,
        },
        "ScriptTask" => WorkflowNodeType::ScriptTask {
            script: String::new(),
            script_format: "javascript".into(),
        },
        "BusinessRuleTask" => WorkflowNodeType::BusinessRuleTask {
            decision_ref: String::new(),
            decision_table: None,
        },
        "SendTask" => WorkflowNodeType::SendTask {
            message_ref: String::new(),
        },
        "ReceiveTask" => WorkflowNodeType::ReceiveTask {
            message_ref: String::new(),
        },
        "ManualTask" => WorkflowNodeType::ManualTask,
        "Task" => WorkflowNodeType::Task {
            task_type: TaskType::User,
        },
        "ExclusiveGateway" => WorkflowNodeType::ExclusiveGateway { default_flow: None },
        "InclusiveGateway" => WorkflowNodeType::InclusiveGateway { default_flow: None },
        "ParallelGateway" => WorkflowNodeType::ParallelGateway,
        "EventBasedGateway" => WorkflowNodeType::EventBasedGateway,
        "SubProcess" => WorkflowNodeType::SubProcess {
            is_event_subprocess: false,
            is_transaction: false,
            workflow_id: None,
        },
        "CallActivity" => WorkflowNodeType::CallActivity {
            // Deterministic placeholder; the agent must set a real id via config.
            called_workflow_id: WorkflowId(Uuid::nil()),
            input_mapping: IndexMap::new(),
            output_mapping: IndexMap::new(),
        },
        _ => return None,
    };
    Some(node_type)
}

/// Reject unknown top-level config keys before serialising into stored JSON.
pub fn validate_config_patch(node_type: &str, patch: &Value) -> Result<(), McpError> {
    let Some(value) = patch.as_object() else {
        return Err(McpError::InvalidParam {
            name: "config",
            detail: "must be a JSON object".into(),
        });
    };
    let entry = catalogue()
        .into_iter()
        .find(|entry| entry.node_type == node_type)
        .ok_or_else(|| McpError::UnknownNodeType(node_type.to_string()))?;
    for key in value.keys() {
        if !entry.config_keys.contains(&key.as_str()) {
            return Err(McpError::UnknownConfigKey {
                node_type: node_type.to_string(),
                key: key.clone(),
            });
        }
    }
    Ok(())
}

/// Compare an agent payload with its canonical serialization, treating keys
/// omitted from the payload as equivalent to explicit `null` in the model.
fn equivalent_to_canonical(payload: &Value, canonical: &Value) -> bool {
    match (payload, canonical) {
        (Value::Object(input), Value::Object(model)) => {
            input.keys().all(|key| model.contains_key(key))
                && model.iter().all(|(key, value)| match input.get(key) {
                    Some(input_value) => equivalent_to_canonical(input_value, value),
                    None => value.is_null(),
                })
        }
        (Value::Array(input), Value::Array(model)) => {
            input.len() == model.len()
                && input
                    .iter()
                    .zip(model)
                    .all(|(a, b)| equivalent_to_canonical(a, b))
        }
        _ => payload == canonical,
    }
}

/// Deserialise a node payload and reject unknown nested keys.
///
/// Serde ignores unknown fields by default; round-tripping the parsed value
/// and comparing it with the input closes that gap so agent-supplied typos
/// never silently disappear into stored JSON.
pub fn parse_node_type_strict(
    payload: &Value,
    node_type: &str,
) -> Result<WorkflowNodeType, McpError> {
    let typed: WorkflowNodeType =
        serde_json::from_value(payload.clone()).map_err(|error| McpError::InvalidParam {
            name: "config",
            detail: error.to_string(),
        })?;
    let canonical = serde_json::to_value(&typed).map_err(McpError::from)?;
    if !equivalent_to_canonical(payload, &canonical) {
        return Err(McpError::InvalidParam {
            name: "config",
            detail: format!("unsupported keys or non-canonical values in `{node_type}` config"),
        });
    }
    Ok(typed)
}

/// Deep-merge a partial config into the default config. Objects merge
/// recursively; arrays and scalars replace the previous value.
pub fn merge_json_objects(base: &mut Value, patch: &Value) {
    let Some(patch_map) = patch.as_object() else {
        return;
    };
    let Some(base_map) = base.as_object_mut() else {
        return;
    };
    for (key, value) in patch_map {
        if let (Some(previous), Some(incoming)) = (base_map.get_mut(key), value.as_object()) {
            if previous.is_object() {
                merge_json_objects(previous, incoming);
                continue;
            }
        }
        base_map.insert(key.clone(), value.clone());
    }
}

/// Build the serialisable catalogue resource payload.
pub fn catalogue() -> Vec<NodeTypeInfo> {
    let specs: [(&str, &str, &str, &[&str]); 16] = [
        ("StartEvent", "Start", "events", &["event_type"]),
        ("EndEvent", "End", "events", &["event_type"]),
        (
            "UserTask",
            "User task",
            "tasks",
            &["assignee", "candidate_groups", "form_id", "due_date"],
        ),
        (
            "ServiceTask",
            "Service task",
            "tasks",
            &["implementation", "async", "exclusive"],
        ),
        (
            "ScriptTask",
            "Script task",
            "tasks",
            &["script", "script_format"],
        ),
        (
            "BusinessRuleTask",
            "Business rule",
            "tasks",
            &["decision_ref", "decision_table"],
        ),
        ("SendTask", "Send message", "tasks", &["message_ref"]),
        ("ReceiveTask", "Receive message", "tasks", &["message_ref"]),
        ("ManualTask", "Manual task", "tasks", &[]),
        ("Task", "Generic task", "tasks", &["task_type"]),
        (
            "ExclusiveGateway",
            "Exclusive gateway",
            "gateways",
            &["default_flow"],
        ),
        (
            "InclusiveGateway",
            "Inclusive gateway",
            "gateways",
            &["default_flow"],
        ),
        ("ParallelGateway", "Parallel gateway", "gateways", &[]),
        ("EventBasedGateway", "Event gateway", "gateways", &[]),
        (
            "SubProcess",
            "Sub-process",
            "composite",
            &["is_event_subprocess", "is_transaction", "workflow_id"],
        ),
        (
            "CallActivity",
            "Call activity",
            "composite",
            &["called_workflow_id", "input_mapping", "output_mapping"],
        ),
    ];

    specs
        .into_iter()
        .map(|(node_type, label, category, config_keys)| {
            let defaults = serde_json::to_value(default_node_type(node_type).unwrap())
                .expect("default node type serialises");
            NodeTypeInfo {
                node_type,
                label,
                category,
                config_keys,
                defaults,
            }
        })
        .collect()
}

/// Catalogue as a JSON resource payload.
pub fn catalogue_json() -> Value {
    let entries = catalogue()
        .into_iter()
        .map(|entry| {
            json!({
                "type": entry.node_type,
                "label": entry.label,
                "category": entry.category,
                "config_keys": entry.config_keys,
                "default_node_type": entry.defaults,
                "example": { "name": entry.label, "node_type": entry.defaults },
            })
        })
        .collect::<Vec<_>>();
    json!({
        "description": "Valid node_type tags and their default config payloads",
        "node_types": entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_all_sixteen_variants() {
        let entries = catalogue();
        assert_eq!(entries.len(), 16);
        for name in NODE_TYPE_NAMES {
            assert!(
                entries.iter().any(|entry| entry.node_type == name),
                "{name}"
            );
        }
    }

    #[test]
    fn defaults_deserialize_into_model_types() {
        for name in NODE_TYPE_NAMES {
            let value = serde_json::to_value(default_node_type(name).unwrap()).unwrap();
            let parsed: WorkflowNodeType = serde_json::from_value(value).unwrap();
            assert_eq!(
                std::mem::discriminant(&parsed),
                std::mem::discriminant(&default_node_type(name).unwrap())
            );
        }
    }

    #[test]
    fn rejects_unknown_config_keys() {
        let patch = json!({ "assignee": "alice", "bogus": 1 });
        assert!(validate_config_patch("UserTask", &patch).is_err());
        assert!(validate_config_patch("UserTask", &json!({ "assignee": "alice" })).is_ok());
        assert!(validate_config_patch("Nope", &json!({})).is_err());
    }

    #[test]
    fn strict_parse_allows_omitted_nulls_but_rejects_unknown_nested_keys() {
        let partial = json!({
            "type": "StartEvent",
            "config": { "event_type": { "timer": { "time_duration": "PT1H" } } }
        });
        assert!(parse_node_type_strict(&partial, "StartEvent").is_ok());

        let mut bad = serde_json::to_value(default_node_type("ServiceTask").unwrap()).unwrap();
        bad["config"]["implementation"]["config"]["nonsense"] = json!(1);
        assert!(parse_node_type_strict(&bad, "ServiceTask").is_err());
    }

    #[test]
    fn deep_merge_preserves_sibling_keys() {
        let mut base = json!({"implementation": {"type": "Http", "config": {"url": "", "timeout_ms": 30000}}, "async": false});
        merge_json_objects(
            &mut base,
            &json!({"implementation": {"config": {"url": "https://x"}}}),
        );
        assert_eq!(base["implementation"]["config"]["url"], "https://x");
        assert_eq!(base["implementation"]["config"]["timeout_ms"], 30000);
        assert_eq!(base["implementation"]["type"], "Http");
        assert_eq!(base["async"], false);
    }
}
