//! Curated, known-good examples used by the grounding resources.

use nocode_model::Workflow;

/// Smallest valid workflow: start -> end. Mirrors the `MINIMAL_WORKFLOW`
/// fixture in `crates/ide/src/bpmn.rs`.
pub const MINIMAL_WORKFLOW_JSON: &str = r#"{
  "id": "11111111-1111-4111-8111-111111111111",
  "name": "Minimal",
  "version": "1.0.0",
  "nodes": [
    {
      "id": "22222222-2222-4222-8222-222222222222",
      "name": "Start",
      "node_type": { "type": "StartEvent", "config": { "event_type": "none" } },
      "position": { "x": 0.0, "y": 0.0 },
      "config": {
        "boundary_events": [], "execution_listeners": [],
        "inputs": {}, "outputs": {}, "properties": {}
      }
    },
    {
      "id": "33333333-3333-4333-8333-333333333333",
      "name": "End",
      "node_type": { "type": "EndEvent", "config": { "event_type": "none" } },
      "position": { "x": 200.0, "y": 0.0 },
      "config": {
        "boundary_events": [], "execution_listeners": [],
        "inputs": {}, "outputs": {}, "properties": {}
      }
    }
  ],
  "edges": [
    {
      "id": "44444444-4444-4444-8444-444444444444",
      "source": "22222222-2222-4222-8222-222222222222",
      "target": "33333333-3333-4333-8333-333333333333",
      "style": { "animated": false }
    }
  ],
  "variables": [],
  "signals": [],
  "timers": [],
  "settings": {
    "async_continuations": false,
    "exclusive_gateway_default": true,
    "history_level": "full",
    "is_executable": true,
    "variables_serialization": "json"
  },
  "created_at": "2024-01-01T00:00:00Z",
  "updated_at": "2024-01-01T00:00:00Z"
}"#;

/// Start -> user task -> exclusive gateway -> two ends with edge conditions.
/// Mirrors the `approval_workflow` fixture in `crates/ide/src/bpmn.rs`.
pub const GATEWAY_WORKFLOW_JSON: &str = r#"{
  "id": "55555555-5555-4555-8555-555555555555",
  "name": "Approval",
  "version": "1.0.0",
  "variables": [],
  "signals": [],
  "timers": [],
  "nodes": [
    {
      "id": "66666666-6666-4666-8666-666666666666", "name": "Start",
      "node_type": { "type": "StartEvent", "config": { "event_type": "none" } },
      "position": { "x": 0.0, "y": 0.0 },
      "config": { "boundary_events": [], "execution_listeners": [], "inputs": {}, "outputs": {}, "properties": {} }
    },
    {
      "id": "77777777-7777-4777-8777-777777777777", "name": "Review",
      "node_type": { "type": "UserTask", "config": { "assignee": null, "candidate_groups": [], "due_date": null, "form_id": null } },
      "position": { "x": 220.0, "y": 0.0 },
      "config": { "boundary_events": [], "execution_listeners": [], "inputs": {}, "outputs": {}, "properties": {} }
    },
    {
      "id": "88888888-8888-4888-8888-888888888888", "name": "Decision",
      "node_type": { "type": "ExclusiveGateway", "config": { "default_flow": null } },
      "position": { "x": 440.0, "y": 0.0 },
      "config": { "boundary_events": [], "execution_listeners": [], "inputs": {}, "outputs": {}, "properties": {} }
    },
    {
      "id": "99999999-9999-4999-8999-999999999999", "name": "Approved",
      "node_type": { "type": "ManualTask" },
      "position": { "x": 660.0, "y": -80.0 },
      "config": { "boundary_events": [], "execution_listeners": [], "inputs": {}, "outputs": {}, "properties": {} }
    },
    {
      "id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "name": "Rejected",
      "node_type": { "type": "ManualTask" },
      "position": { "x": 660.0, "y": 80.0 },
      "config": { "boundary_events": [], "execution_listeners": [], "inputs": {}, "outputs": {}, "properties": {} }
    },
    {
      "id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "name": "End",
      "node_type": { "type": "EndEvent", "config": { "event_type": "none" } },
      "position": { "x": 880.0, "y": 0.0 },
      "config": { "boundary_events": [], "execution_listeners": [], "inputs": {}, "outputs": {}, "properties": {} }
    }
  ],
  "edges": [
    { "id": "e1000000-0000-4000-8000-000000000001", "source": "66666666-6666-4666-8666-666666666666", "target": "77777777-7777-4777-8777-777777777777", "style": { "animated": false } },
    { "id": "e1000000-0000-4000-8000-000000000002", "source": "77777777-7777-4777-8777-777777777777", "target": "88888888-8888-4888-8888-888888888888", "style": { "animated": false } },
    { "id": "e1000000-0000-4000-8000-000000000003", "label": "approved", "condition": "approved == true", "source": "88888888-8888-4888-8888-888888888888", "target": "99999999-9999-4999-8999-999999999999", "style": { "animated": false } },
    { "id": "e1000000-0000-4000-8000-000000000004", "label": "rejected", "condition": "!( approved == true )", "source": "88888888-8888-4888-8888-888888888888", "target": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "style": { "animated": false } },
    { "id": "e1000000-0000-4000-8000-000000000005", "source": "99999999-9999-4999-8999-999999999999", "target": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "style": { "animated": false } },
    { "id": "e1000000-0000-4000-8000-000000000006", "source": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "target": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "style": { "animated": false } }
  ],
  "settings": {
    "async_continuations": false,
    "exclusive_gateway_default": true,
    "history_level": "full",
    "is_executable": true,
    "variables_serialization": "json"
  },
  "created_at": "2024-01-01T00:00:00Z",
  "updated_at": "2024-01-01T00:00:00Z"
}"#;

/// Minimal BPMN 2.0 XML, generated once with `export_bpmn`.
pub const MINIMAL_XML: &str = include_str!("../../resources/examples/minimal.bpmn.xml");

/// Hand-written authoring guide.
pub const AUTHORING_GUIDE: &str = include_str!("../../resources/guides/authoring.md");

/// Parse a curated JSON example into the native model.
pub fn workflow_from(example: &str) -> Result<Workflow, serde_json::Error> {
    serde_json::from_str(example)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocode_bpmn::engine::BpmnEngine;
    use nocode_bpmn::xml::{export_bpmn, parse_bpmn};

    #[test]
    fn json_examples_parse_and_validate() {
        for example in [MINIMAL_WORKFLOW_JSON, GATEWAY_WORKFLOW_JSON] {
            let workflow = workflow_from(example).expect("example parses");
            let result = BpmnEngine::new().validate(&workflow);
            assert!(result.is_valid(), "{:?}", result.issues);
        }
    }

    #[test]
    fn xml_example_round_trips_and_matches_exporter() {
        let parsed = parse_bpmn(MINIMAL_XML, "fallback").expect("XML parses");
        assert!(BpmnEngine::new().validate(&parsed).is_valid());
        assert_eq!(parsed.nodes.len(), 2);
        assert_eq!(parsed.edges.len(), 1);
        let regenerated = export_bpmn(&parsed).expect("export works");
        assert_eq!(regenerated, MINIMAL_XML);
    }

    #[test]
    fn guide_mentions_the_core_sequence() {
        assert!(AUTHORING_GUIDE.contains("create_workflow"));
        assert!(AUTHORING_GUIDE.contains("auto_layout"));
        assert!(AUTHORING_GUIDE.contains("save_workflow"));
    }
}
