//! BPMN 2.0 helpers shared by every IDE front-end (desktop + web).
//!
//! The heavy lifting lives in [`nocode_bpmn`]: a token-based process engine,
//! an expression evaluator and the BPMN 2.0 XML importer/exporter. This module
//! only adapts those calls to the plain `String`-in / `String`-out shape the
//! JavaScript shells use, so no host needs direct access to the crate types.

use nocode_bpmn::engine::{BpmnEngine, Severity, ValidationIssue};
use nocode_bpmn::xml::{export_bpmn, parse_bpmn};
use nocode_model::Workflow;
use serde::{Deserialize, Serialize};

/// Result of a workflow validation pass, in the shape the IDE frontend reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowValidation {
    pub valid: bool,
    pub issues: Vec<ValidationIssueDto>,
}

/// One validation finding. Mirrors [`ValidationIssue`] with a string severity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssueDto {
    pub severity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub message: String,
}

impl From<ValidationIssue> for ValidationIssueDto {
    fn from(issue: ValidationIssue) -> Self {
        let severity = match issue.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        Self {
            severity: severity.to_string(),
            node_id: issue.node_id,
            message: issue.message,
        }
    }
}

fn parse_workflow(workflow_json: &str) -> Result<Workflow, String> {
    serde_json::from_str(workflow_json).map_err(|error| format!("invalid workflow JSON: {error}"))
}

/// Parse BPMN 2.0 XML into a `Workflow` and return its canonical JSON.
pub fn import_bpmn(xml: &str, name: &str) -> Result<String, String> {
    let workflow = parse_bpmn(xml, name).map_err(|error| error.to_string())?;
    serde_json::to_string(&workflow).map_err(|error| format!("failed to serialize workflow: {error}"))
}

/// Serialize a `Workflow` (given as JSON) into BPMN 2.0 XML.
pub fn export_bpmn_xml(workflow_json: &str) -> Result<String, String> {
    let workflow = parse_workflow(workflow_json)?;
    export_bpmn(&workflow).map_err(|error| error.to_string())
}

/// Structurally validate a `Workflow` (given as JSON).
pub fn validate_workflow(workflow_json: &str) -> Result<WorkflowValidation, String> {
    let workflow = parse_workflow(workflow_json)?;
    let result = BpmnEngine::new().validate(&workflow);
    Ok(WorkflowValidation {
        valid: result.is_valid(),
        issues: result.issues.into_iter().map(ValidationIssueDto::from).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_WORKFLOW: &str = r#"{
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
                    "boundary_events": [],
                    "execution_listeners": [],
                    "inputs": {},
                    "outputs": {},
                    "properties": {}
                }
            },
            {
                "id": "33333333-3333-4333-8333-333333333333",
                "name": "End",
                "node_type": { "type": "EndEvent", "config": { "event_type": "none" } },
                "position": { "x": 200.0, "y": 0.0 },
                "config": {
                    "boundary_events": [],
                    "execution_listeners": [],
                    "inputs": {},
                    "outputs": {},
                    "properties": {}
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

    #[test]
    fn validates_a_minimal_workflow() {
        let report = validate_workflow(MINIMAL_WORKFLOW).expect("validation runs");
        assert!(report.valid, "expected valid workflow: {:?}", report.issues);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(validate_workflow("{not json").is_err());
    }

    #[test]
    fn exports_and_reimports_bpmn_xml() {
        let xml = export_bpmn_xml(MINIMAL_WORKFLOW).expect("export runs");
        assert!(xml.contains("<definitions"));
        let json = import_bpmn(&xml, "Round Trip").expect("import runs");
        let report = validate_workflow(&json).expect("round-trip workflow validates");
        assert!(report.valid, "round-trip issues: {:?}", report.issues);
    }
}