//! BPMN 2.0 helpers shared by every IDE front-end (desktop + web).
//!
//! The heavy lifting lives in [`nocode_bpmn`]: a token-based process engine,
//! an expression evaluator and the BPMN 2.0 XML importer/exporter. This module
//! only adapts those calls to the plain `String`-in / `String`-out shape the
//! JavaScript shells use, so no host needs direct access to the crate types.

use nocode_bpmn::engine::{
    BpmnEngine, InstanceStatus, Severity, TaskStatus, TokenStatus, ValidationIssue,
};
use nocode_bpmn::xml::{export_bpmn, parse_bpmn};
use nocode_model::Workflow;
use indexmap::IndexMap;
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

/// How a workflow run should behave.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunOptions {
    /// Complete every user task automatically so the instance runs to the
    /// end. Off by default: the IDE's "Run" is a simulation, and the user
    /// should know the tasks were auto-completed rather than really done.
    #[serde(default)]
    pub auto_complete_tasks: bool,
    /// Safety net for loops created by auto-completion (default 50).
    #[serde(default)]
    pub max_rounds: Option<u32>,
}

/// One node the instance visited, in execution order.
#[derive(Debug, Clone, Serialize)]
pub struct RunStep {
    pub node_id: String,
    pub node_name: String,
    pub node_type: String,
    /// `passed` (flowed through), `waiting` (token parked here), or
    /// `open` (a user task is pending).
    pub outcome: String,
}

/// A user task the instance created and has not completed.
#[derive(Debug, Clone, Serialize)]
pub struct OpenTask {
    pub id: String,
    pub node_name: String,
    pub status: String,
}

/// One engine history entry, flattened for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct RunEvent {
    pub event_type: String,
    pub node_name: Option<String>,
    pub detail: Option<String>,
}

/// The result of running a workflow.
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub instance_id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    /// `running`, `completed`, `failed` or `terminated`.
    pub status: String,
    pub completed: bool,
    /// True when tasks were auto-completed by this run.
    pub auto_completed_tasks: bool,
    pub steps: Vec<RunStep>,
    pub open_tasks: Vec<OpenTask>,
    pub events: Vec<RunEvent>,
    pub tokens_waiting: Vec<String>,
    pub variables: serde_json::Map<String, serde_json::Value>,
}

/// Execute a workflow (given as JSON) and summarize what happened.
///
/// The instance is validated first (the engine refuses to start an invalid
/// workflow), then auto-completed up to `max_rounds` when requested.
pub fn run_workflow(
    workflow_json: &str,
    variables: serde_json::Value,
    options: RunOptions,
) -> Result<RunSummary, String> {
    let workflow = parse_workflow(workflow_json)?;
    let mut supplied: IndexMap<String, serde_json::Value> = match variables {
        serde_json::Value::Object(map) => map.into_iter().collect(),
        serde_json::Value::Null => Default::default(),
        other => return Err(format!("variables must be an object, got {other}")),
    };

    let engine = BpmnEngine::new();
    let mut instance = engine
        .start(&workflow, supplied.clone())
        .map_err(|error| error.to_string())?;

    let max_rounds = options.max_rounds.unwrap_or(50).max(1);
    if options.auto_complete_tasks {
        let mut rounds = 0u32;
        while instance.is_running() && rounds < max_rounds {
            let open: Vec<String> = instance
                .open_tasks()
                .iter()
                .map(|task| task.id.clone())
                .collect();
            if open.is_empty() {
                break;
            }
            for task_id in open {
                engine
                    .complete_task(&workflow, &mut instance, &task_id, Default::default())
                    .map_err(|error| error.to_string())?;
            }
            rounds += 1;
        }
    }

    supplied.extend(instance.variables.iter().map(|(k, v)| (k.clone(), v.clone())));
    Ok(summarize(&workflow, &instance, options.auto_complete_tasks, supplied))
}

fn summarize(
    workflow: &Workflow,
    instance: &nocode_bpmn::engine::ProcessInstance,
    auto_completed: bool,
    variables: indexmap::IndexMap<String, serde_json::Value>,
) -> RunSummary {
    // Walk the history to recover the visit order, then annotate each node
    // with where its token ended up.
    let mut steps: Vec<RunStep> = Vec::new();
    for event in &instance.history {
        let (Some(node_id), Some(node_name)) = (&event.node_id, &event.node_name) else {
            continue;
        };
        let node = workflow.nodes.iter().find(|node| node.id == *node_id);
        let node_type = node
            .map(|node| format!("{:?}", node.node_type))
            .unwrap_or_else(|| "Unknown".to_string());

        let waiting_here = instance
            .tokens
            .iter()
            .any(|token| &token.node_id == node_id && token.status == TokenStatus::Waiting);
        let task_here = instance
            .tasks
            .iter()
            .any(|task| &task.node_id == node_id && task.status == TaskStatus::Created);
        let outcome = if task_here {
            "open"
        } else if waiting_here {
            "waiting"
        } else {
            "passed"
        };

        if let Some(existing) = steps.iter_mut().find(|step| step.node_id == node_id.to_string()) {
            existing.outcome = outcome.to_string();
        } else {
            steps.push(RunStep {
                node_id: node_id.to_string(),
                node_name: node_name.clone(),
                node_type,
                outcome: outcome.to_string(),
            });
        }
    }

    let open_tasks = instance
        .tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Created | TaskStatus::Claimed))
        .map(|task| OpenTask {
            id: task.id.clone(),
            node_name: task.name.clone(),
            status: format!("{:?}", task.status).to_lowercase(),
        })
        .collect();

    let events = instance
        .history
        .iter()
        .map(|event| RunEvent {
            event_type: event.event_type.clone(),
            node_name: event.node_name.clone(),
            detail: event.detail.clone(),
        })
        .collect();

    let tokens_waiting = instance
        .tokens
        .iter()
        .filter(|token| token.status != TokenStatus::Consumed)
        .map(|token| token.node_id.to_string())
        .collect();

    RunSummary {
        instance_id: instance.id.clone(),
        workflow_id: instance.workflow_id.to_string(),
        workflow_name: instance.workflow_name.clone(),
        status: format!("{:?}", instance.status).to_lowercase(),
        completed: matches!(
            instance.status,
            InstanceStatus::Completed | InstanceStatus::Failed | InstanceStatus::Terminated
        ),
        auto_completed_tasks: auto_completed,
        steps,
        open_tasks,
        events,
        tokens_waiting,
        variables: variables.into_iter().collect(),
    }
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

    /// Start -> user task -> gateway (approved path) -> end.
    fn approval_workflow(gateway_expression: &str) -> String {
        format!(
            r#"{{
  "id": "55555555-5555-4555-8555-555555555555",
  "name": "Approval",
  "version": "1.0.0",
  "variables": [],
  "signals": [],
  "timers": [],
  "nodes": [
    {{
      "id": "66666666-6666-4666-8666-666666666666", "name": "Start",
      "node_type": {{ "type": "StartEvent", "config": {{ "event_type": "none" }} }},
      "position": {{ "x": 0.0, "y": 0.0 }},
      "config": {{ "boundary_events": [], "execution_listeners": [], "inputs": {{}}, "outputs": {{}}, "properties": {{}} }}
    }},
    {{
      "id": "77777777-7777-4777-8777-777777777777", "name": "Review",
      "node_type": {{ "type": "UserTask", "config": {{ "assignee": null, "candidate_groups": [], "due_date": null, "form_id": null }} }},
      "position": {{ "x": 100.0, "y": 0.0 }},
      "config": {{ "boundary_events": [], "execution_listeners": [], "inputs": {{}}, "outputs": {{}}, "properties": {{}} }}
    }},
    {{
      "id": "88888888-8888-4888-8888-888888888888", "name": "Decision",
      "node_type": {{ "type": "ExclusiveGateway", "config": {{ "default_flow": null }} }},
      "position": {{ "x": 200.0, "y": 0.0 }},
      "config": {{ "boundary_events": [], "execution_listeners": [], "inputs": {{}}, "outputs": {{}}, "properties": {{}} }}
    }},
    {{
      "id": "99999999-9999-4999-8999-999999999999", "name": "Approved",
      "node_type": {{ "type": "ManualTask" }},
      "position": {{ "x": 300.0, "y": -80.0 }},
      "config": {{ "boundary_events": [], "execution_listeners": [], "inputs": {{}}, "outputs": {{}}, "properties": {{}} }}
    }},
    {{
      "id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "name": "Rejected",
      "node_type": {{ "type": "ManualTask" }},
      "position": {{ "x": 300.0, "y": 80.0 }},
      "config": {{ "boundary_events": [], "execution_listeners": [], "inputs": {{}}, "outputs": {{}}, "properties": {{}} }}
    }},
    {{
      "id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "name": "End",
      "node_type": {{ "type": "EndEvent", "config": {{ "event_type": "none" }} }},
      "position": {{ "x": 400.0, "y": 0.0 }},
      "config": {{ "boundary_events": [], "execution_listeners": [], "inputs": {{}}, "outputs": {{}}, "properties": {{}} }}
    }}
  ],
  "edges": [
    {{ "id": "e1000000-0000-4000-8000-000000000001", "source": "66666666-6666-4666-8666-666666666666", "target": "77777777-7777-4777-8777-777777777777", "style": {{ "animated": false }} }},
    {{ "id": "e1000000-0000-4000-8000-000000000002", "source": "77777777-7777-4777-8777-777777777777", "target": "88888888-8888-4888-8888-888888888888", "style": {{ "animated": false }} }},
    {{ "id": "e1000000-0000-4000-8000-000000000003", "label": "approved", "condition": "{gateway_expression}", "source": "88888888-8888-4888-8888-888888888888", "target": "99999999-9999-4999-8999-999999999999", "style": {{ "animated": false }} }},
    {{ "id": "e1000000-0000-4000-8000-000000000004", "label": "rejected", "condition": "!( {gateway_expression} )", "source": "88888888-8888-4888-8888-888888888888", "target": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "style": {{ "animated": false }} }},
    {{ "id": "e1000000-0000-4000-8000-000000000005", "source": "99999999-9999-4999-8999-999999999999", "target": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "style": {{ "animated": false }} }},
    {{ "id": "e1000000-0000-4000-8000-000000000006", "source": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "target": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "style": {{ "animated": false }} }}
  ],
  "settings": {{
    "async_continuations": false,
    "exclusive_gateway_default": true,
    "history_level": "full",
    "is_executable": true,
    "variables_serialization": "json"
  }},
  "created_at": "2024-01-01T00:00:00Z",
  "updated_at": "2024-01-01T00:00:00Z"
}}"#
        )
    }

    #[test]
    fn run_starts_and_waits_on_the_user_task() {
        let summary = run_workflow(
            &approval_workflow("approved == true"),
            serde_json::json!({ "approved": true }),
            RunOptions::default(),
        )
        .expect("run succeeds");

        assert_eq!(summary.status, "running");
        assert!(!summary.completed);
        assert!(!summary.auto_completed_tasks);
        assert_eq!(summary.open_tasks.len(), 1, "review task should be open");
        assert_eq!(summary.open_tasks[0].node_name, "Review");
        // The run visited the start event and parked at the user task.
        let names: Vec<&str> = summary.steps.iter().map(|step| step.node_name.as_str()).collect();
        assert!(names.contains(&"Start"), "steps: {names:?}");
        assert!(names.contains(&"Review"), "steps: {names:?}");
    }

    #[test]
    fn run_with_auto_completion_takes_the_approved_path() {
        let summary = run_workflow(
            &approval_workflow("approved == true"),
            serde_json::json!({ "approved": true }),
            RunOptions {
                auto_complete_tasks: true,
                max_rounds: None,
            },
        )
        .expect("run succeeds");

        assert!(summary.auto_completed_tasks);
        assert!(summary.completed, "instance should finish: {}", summary.status);
        assert!(summary.open_tasks.is_empty());
        let names: Vec<&str> = summary.steps.iter().map(|step| step.node_name.as_str()).collect();
        assert!(names.contains(&"Approved"), "steps: {names:?}");
        assert!(!names.contains(&"Rejected"), "steps: {names:?}");
        assert_eq!(summary.variables.get("approved"), Some(&serde_json::json!(true)));
    }

    #[test]
    fn run_rejects_non_object_variables() {
        let error = run_workflow(
            MINIMAL_WORKFLOW,
            serde_json::json!("nope"),
            RunOptions::default(),
        )
        .expect_err("non-object variables must fail");
        assert!(error.contains("variables must be an object"), "got: {error}");
    }

    #[test]
    fn run_reports_invalid_workflows() {
        let error = run_workflow(
            r#"{"id":"66666666-6666-4666-8666-666666666666","name":"Empty","version":"1.0.0","nodes":[],"edges":[],"variables":[],"signals":[],"timers":[],"settings":{"async_continuations":false,"exclusive_gateway_default":true,"history_level":"full","is_executable":true,"variables_serialization":"json"},"created_at":"2024-01-01T00:00:00Z","updated_at":"2024-01-01T00:00:00Z"}"#,
            serde_json::json!({}),
            RunOptions::default(),
        )
        .expect_err("an empty workflow cannot run");
        assert!(error.contains("no nodes"), "got: {error}");
    }
}