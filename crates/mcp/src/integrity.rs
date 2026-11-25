//! Whole-project validation: cross-domain references **and** BPMN structure.
//!
//! `nocode_model::validate_integrity` owns the cross-domain half (a relation
//! pointing at a deleted entity, a page path collision, a missing form). The
//! per-workflow structural pass needs `nocode_bpmn::BpmnEngine`, which the model
//! crate cannot depend on — so it is composed here, where both are available.
//!
//! This is what Step 4 of the agent pipeline runs, and what gates
//! `project_save` in strict mode.

use serde::Serialize;
use serde_json::{json, Value};

use nocode_model::Project;

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One finding, addressed to the domain and object that owns it.
#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub severity: Severity,
    pub domain: String,
    pub object_id: Option<String>,
    pub message: String,
}

/// Validate `project` across every domain.
///
/// Deterministic order: model issues first (entities, forms, pages, workflows,
/// themes), then BPMN issues in workflow insertion order.
pub fn report(project: &Project) -> Vec<Issue> {
    let mut issues: Vec<Issue> = nocode_model::validate_integrity(project)
        .into_iter()
        .map(|issue| Issue {
            severity: match issue.severity {
                nocode_model::Severity::Error => Severity::Error,
                nocode_model::Severity::Warning => Severity::Warning,
            },
            domain: issue.domain.to_string(),
            object_id: issue.object_id,
            message: issue.message,
        })
        .collect();

    let engine = nocode_bpmn::BpmnEngine::new();
    for (workflow_id, workflow) in &project.workflows {
        for issue in engine.validate(workflow).issues {
            issues.push(Issue {
                severity: match issue.severity {
                    nocode_bpmn::engine::Severity::Error => Severity::Error,
                    nocode_bpmn::engine::Severity::Warning => Severity::Warning,
                },
                domain: "workflows".to_string(),
                object_id: Some(workflow_id.to_string()),
                message: format!("{}: {}", workflow.name, issue.message),
            });
        }
    }
    issues
}

/// Whether the project can be generated and run.
pub fn is_valid(issues: &[Issue]) -> bool {
    !issues.iter().any(|issue| issue.severity == Severity::Error)
}

/// The `{ valid, issues }` shape the SSE `validation` event and the
/// `validate_project` tool both use.
pub fn report_json(project: &Project) -> Value {
    let issues = report(project);
    json!({
        "valid": is_valid(&issues),
        "issues": issues,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocode_model::{Entity, Workflow};

    #[test]
    fn an_empty_project_is_valid() {
        assert!(is_valid(&report(&Project::new("Empty"))));
    }

    #[test]
    fn a_workflow_without_a_start_event_is_invalid() {
        let mut project = Project::new("No start");
        let mut workflow = Workflow::new("Orphan");
        workflow.nodes.clear();
        project.workflows.insert(workflow.id.clone(), workflow);

        let issues = report(&project);
        assert!(!is_valid(&issues));
        assert!(issues
            .iter()
            .any(|issue| issue.domain == "workflows" && issue.message.contains("Orphan")));
    }

    #[test]
    fn entity_issues_are_reported_before_workflow_issues() {
        let mut project = Project::new("Ordered");
        let mut form = nocode_model::Form::new("Bound");
        form.entity_id = Some(nocode_model::EntityId::new());
        project.forms.insert(form.id.clone(), form);
        project
            .entities
            .insert(Entity::new("Expense").id, Entity::new("Expense"));

        let issues = report(&project);
        assert_eq!(issues[0].domain, "forms");
    }

    #[test]
    fn report_json_carries_the_valid_flag() {
        let value = report_json(&Project::new("Empty"));
        assert_eq!(value["valid"], json!(true));
        assert!(value["issues"].as_array().unwrap().is_empty());
    }
}
