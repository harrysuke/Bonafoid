//! Cross-domain referential integrity for a whole project.
//!
//! The BPMN engine validates one workflow in isolation. Nothing validated the
//! *project*: a relation could point at a deleted entity, a form at a missing
//! entity, a page at a form that no longer exists, and the IDE would happily
//! save it. This module closes that gap.
//!
//! It deliberately does **not** run `BpmnEngine::validate` — `nocode-bpmn`
//! depends on this crate, so the dependency cannot point the other way. The
//! per-workflow BPMN pass lives in `nocode_mcp::integrity`, which composes
//! both halves. Callers that only have the model (codegen, for instance) get
//! the cross-domain half, which is the part they can act on.

use std::collections::{BTreeMap, BTreeSet};

use crate::page::ComponentType;
use crate::project::Project;
use crate::workflow::{ServiceTaskImplementation, WorkflowNodeType};

/// How serious an integrity finding is.
///
/// Mirrors `nocode_bpmn::engine::Severity` (same wire spelling) but is defined
/// here so the model crate keeps no dependency on the BPMN engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The project cannot be generated or run until this is fixed.
    Error,
    /// Suspicious but runnable — an unknown connector, an empty page.
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

impl serde::Serialize for Severity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// One finding, addressed to the domain and object that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityIssue {
    pub severity: Severity,
    /// `entities` | `forms` | `pages` | `workflows` | `themes` | `project`
    pub domain: &'static str,
    /// Id of the offending object, when there is one.
    pub object_id: Option<String>,
    pub message: String,
}

impl IntegrityIssue {
    fn error(domain: &'static str, object_id: Option<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            domain,
            object_id,
            message: message.into(),
        }
    }

    fn warning(
        domain: &'static str,
        object_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: Severity::Warning,
            domain,
            object_id,
            message: message.into(),
        }
    }
}

impl serde::Serialize for IntegrityIssue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("IntegrityIssue", 4)?;
        state.serialize_field("severity", &self.severity)?;
        state.serialize_field("domain", self.domain)?;
        state.serialize_field("object_id", &self.object_id)?;
        state.serialize_field("message", &self.message)?;
        state.end()
    }
}

/// Validate every cross-domain reference in `project`.
///
/// The result is deterministic: issues are emitted domain by domain, in
/// `IndexMap` insertion order within each domain, so a golden test can compare
/// the whole vector.
pub fn validate_integrity(project: &Project) -> Vec<IntegrityIssue> {
    let mut issues = Vec::new();
    check_entities(project, &mut issues);
    check_forms(project, &mut issues);
    check_pages(project, &mut issues);
    check_workflows(project, &mut issues);
    check_themes(project, &mut issues);
    issues
}

/// Only the errors, as borrowed slices — the shape the save gate needs.
pub fn errors(issues: &[IntegrityIssue]) -> Vec<&IntegrityIssue> {
    issues
        .iter()
        .filter(|issue| issue.severity == Severity::Error)
        .collect()
}

fn check_entities(project: &Project, issues: &mut Vec<IntegrityIssue>) {
    for (entity_id, entity) in &project.entities {
        let mut field_names: BTreeSet<&str> = BTreeSet::new();
        for field in &entity.fields {
            if !field_names.insert(field.name.as_str()) {
                issues.push(IntegrityIssue::error(
                    "entities",
                    Some(entity_id.to_string()),
                    format!(
                        "entity `{}` has two fields named `{}`",
                        entity.name, field.name
                    ),
                ));
            }
        }

        for relation in &entity.relations {
            if !project.entities.contains_key(&relation.to_entity) {
                issues.push(IntegrityIssue::error(
                    "entities",
                    Some(entity_id.to_string()),
                    format!(
                        "relation `{}` targets entity {} which does not exist",
                        relation.name, relation.to_entity
                    ),
                ));
                continue;
            }
            // `to_field` belongs to the *target* entity, `from_field` to this
            // one. Checking the wrong side is the classic mistake, so both are
            // checked explicitly against the right entity.
            let target = &project.entities[&relation.to_entity];
            if !entity.fields.iter().any(|f| f.id == relation.from_field) {
                issues.push(IntegrityIssue::error(
                    "entities",
                    Some(entity_id.to_string()),
                    format!(
                        "relation `{}` uses from_field {} which is not a field of `{}`",
                        relation.name, relation.from_field.0, entity.name
                    ),
                ));
            }
            if !target.fields.iter().any(|f| f.id == relation.to_field) {
                issues.push(IntegrityIssue::error(
                    "entities",
                    Some(entity_id.to_string()),
                    format!(
                        "relation `{}` uses to_field {} which is not a field of `{}`",
                        relation.name, relation.to_field.0, target.name
                    ),
                ));
            }
        }
    }
}

fn check_forms(project: &Project, issues: &mut Vec<IntegrityIssue>) {
    for (form_id, form) in &project.forms {
        let Some(entity_id) = form.entity_id.as_ref() else {
            continue;
        };
        let Some(entity) = project.entities.get(entity_id) else {
            issues.push(IntegrityIssue::error(
                "forms",
                Some(form_id.to_string()),
                format!(
                    "form `{}` is bound to entity {} which does not exist",
                    form.name, entity_id
                ),
            ));
            continue;
        };
        // A bound form whose field names do not exist on the entity submits
        // nothing. That is a design error the user wants to hear about, but it
        // does not stop the app from building, so it is a warning.
        for field in &form.fields {
            if !entity.fields.iter().any(|f| f.name == field.name) {
                issues.push(IntegrityIssue::warning(
                    "forms",
                    Some(form_id.to_string()),
                    format!(
                        "form field `{}` has no matching field on entity `{}`",
                        field.name, entity.name
                    ),
                ));
            }
        }
    }
}

fn check_pages(project: &Project, issues: &mut Vec<IntegrityIssue>) {
    let mut paths: BTreeMap<&str, &str> = BTreeMap::new();

    for (page_id, page) in &project.pages {
        let page_ref = page_id.to_string();

        if page.path.trim().is_empty() {
            issues.push(IntegrityIssue::error(
                "pages",
                Some(page_ref.clone()),
                format!("page `{}` has an empty path", page.name),
            ));
        } else if !page.path.starts_with('/') {
            issues.push(IntegrityIssue::warning(
                "pages",
                Some(page_ref.clone()),
                format!(
                    "page `{}` path `{}` should start with `/`",
                    page.name, page.path
                ),
            ));
        }
        if let Some(previous) = paths.insert(page.path.as_str(), page.name.as_str()) {
            issues.push(IntegrityIssue::error(
                "pages",
                Some(page_ref.clone()),
                format!(
                    "pages `{previous}` and `{}` both use path `{}`",
                    page.name, page.path
                ),
            ));
        }

        let source_ids: BTreeSet<&str> = page
            .data_sources
            .iter()
            .map(|source| source.id.as_str())
            .collect();
        let component_ids: BTreeSet<String> =
            page.components.iter().map(|c| c.id.0.to_string()).collect();

        for component in &page.components {
            match &component.component {
                ComponentType::Form { form_id, .. } => {
                    if !project.forms.contains_key(form_id) {
                        issues.push(IntegrityIssue::error(
                            "pages",
                            Some(page_ref.clone()),
                            format!(
                                "page `{}` embeds form {} which does not exist",
                                page.name, form_id
                            ),
                        ));
                    }
                }
                ComponentType::Table { data_source, .. }
                | ComponentType::List { data_source, .. }
                | ComponentType::DetailView { data_source, .. }
                | ComponentType::Chart { data_source, .. } => {
                    if !data_source.is_empty() && !source_ids.contains(data_source.as_str()) {
                        issues.push(IntegrityIssue::error(
                            "pages",
                            Some(page_ref.clone()),
                            format!(
                                "component {} on page `{}` reads data source `{}` which is not declared",
                                component.id.0, page.name, data_source
                            ),
                        ));
                    }
                }
                _ => {}
            }

            // Child ids must resolve. A dangling child renders nothing, which
            // looks like a layout bug and is painful to find by eye.
            for (slot, children) in &component.slots {
                for child in children {
                    if !component_ids.contains(&child.0.to_string()) {
                        issues.push(IntegrityIssue::error(
                            "pages",
                            Some(page_ref.clone()),
                            format!(
                                "slot `{slot}` of component {} references missing component {}",
                                component.id.0, child.0
                            ),
                        ));
                    }
                }
            }
        }
    }
}

fn check_workflows(project: &Project, issues: &mut Vec<IntegrityIssue>) {
    let plugin_ids: BTreeSet<&str> = project
        .plugins
        .iter()
        .map(|plugin| plugin.id.as_str())
        .collect();

    for (workflow_id, workflow) in &project.workflows {
        let workflow_ref = workflow_id.to_string();
        for node in &workflow.nodes {
            match &node.node_type {
                WorkflowNodeType::SubProcess {
                    workflow_id: Some(nested),
                    ..
                } => {
                    if !project.workflows.contains_key(nested) {
                        issues.push(IntegrityIssue::error(
                            "workflows",
                            Some(workflow_ref.clone()),
                            format!(
                                "node `{}` calls sub-process {} which does not exist",
                                node.name, nested
                            ),
                        ));
                    }
                }
                WorkflowNodeType::CallActivity {
                    called_workflow_id, ..
                } => {
                    if !project.workflows.contains_key(called_workflow_id) {
                        issues.push(IntegrityIssue::error(
                            "workflows",
                            Some(workflow_ref.clone()),
                            format!(
                                "node `{}` calls workflow {} which does not exist",
                                node.name, called_workflow_id
                            ),
                        ));
                    }
                }
                WorkflowNodeType::ServiceTask { implementation, .. } => match implementation {
                    ServiceTaskImplementation::Plugin { plugin_id, .. } => {
                        if !plugin_ids.contains(plugin_id.as_str()) {
                            // A plugin may be installed at deploy time, so an
                            // unknown id is a warning, not a build blocker.
                            issues.push(IntegrityIssue::warning(
                                "workflows",
                                Some(workflow_ref.clone()),
                                format!(
                                    "node `{}` uses plugin `{plugin_id}` which is not configured on the project",
                                    node.name
                                ),
                            ));
                        }
                    }
                    ServiceTaskImplementation::External { connector_id, .. } => {
                        issues.push(IntegrityIssue::warning(
                            "workflows",
                            Some(workflow_ref.clone()),
                            format!(
                                "node `{}` uses connector `{connector_id}`; connectors are resolved at run time",
                                node.name
                            ),
                        ));
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

fn check_themes(project: &Project, issues: &mut Vec<IntegrityIssue>) {
    if let Some(active) = project.active_theme_id.as_ref() {
        if !project.themes.contains_key(active) {
            issues.push(IntegrityIssue::error(
                "themes",
                Some(active.to_string()),
                "active_theme_id points at a theme that does not exist",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{
        Entity, EntityField, EntityFieldType, EntityRelation, OnDelete, OnUpdate, RelationType,
    };
    use crate::page::Page;
    use crate::workflow::Workflow;

    fn entity_with(name: &str, field: &str) -> Entity {
        let mut entity = Entity::new(name);
        entity.fields.push(EntityField {
            id: Default::default(),
            name: field.to_string(),
            field_type: EntityFieldType::String { max_length: None },
            nullable: false,
            unique: false,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 0,
        });
        entity
    }

    #[test]
    fn a_clean_project_has_no_issues() {
        let mut project = Project::new("Clean");
        let entity = entity_with("Expense", "amount");
        project.entities.insert(entity.id.clone(), entity);
        assert!(validate_integrity(&project).is_empty());
    }

    #[test]
    fn a_dangling_relation_is_an_error() {
        let mut project = Project::new("Dangling");
        let mut entity = entity_with("Expense", "amount");
        let ghost = crate::entity::EntityId::new();
        entity.relations.push(EntityRelation {
            id: Default::default(),
            name: "ghost".into(),
            relation_type: RelationType::BelongsTo {
                entity_id: ghost.clone(),
                field: "ghost_id".into(),
                on_delete: OnDelete::Cascade,
                on_update: OnUpdate::Cascade,
            },
            from_field: entity.fields[0].id.clone(),
            to_entity: ghost,
            to_field: Default::default(),
            on_delete: OnDelete::Cascade,
            on_update: OnUpdate::Cascade,
        });
        project.entities.insert(entity.id.clone(), entity);

        let issues = validate_integrity(&project);
        assert!(issues
            .iter()
            .any(|issue| issue.message.contains("does not exist")));
    }

    #[test]
    fn duplicate_page_paths_are_reported_once() {
        let mut project = Project::new("Pages");
        let mut first = Page::new("One");
        first.path = "/expenses".into();
        let mut second = Page::new("Two");
        second.path = "/expenses".into();
        project.pages.insert(first.id.clone(), first);
        project.pages.insert(second.id.clone(), second);

        let issues = validate_integrity(&project);
        assert_eq!(
            issues
                .iter()
                .filter(|issue| issue.message.contains("both use path"))
                .count(),
            1
        );
    }

    #[test]
    fn a_form_bound_to_a_missing_entity_is_an_error() {
        let mut project = Project::new("Forms");
        let mut form = crate::form::Form::new("Expense Form");
        form.entity_id = Some(crate::entity::EntityId::new());
        project.forms.insert(form.id.clone(), form);

        let issues = validate_integrity(&project);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].domain, "forms");
        assert_eq!(issues[0].severity, Severity::Error);
    }

    #[test]
    fn a_call_activity_to_a_missing_workflow_is_an_error() {
        let mut project = Project::new("Workflows");
        let mut workflow = Workflow::new("Main");
        workflow.nodes.push(crate::workflow::WorkflowNode {
            id: Default::default(),
            name: "Call".into(),
            node_type: WorkflowNodeType::CallActivity {
                called_workflow_id: crate::workflow::WorkflowId::new(),
                input_mapping: Default::default(),
                output_mapping: Default::default(),
            },
            position: crate::workflow::NodePosition { x: 0.0, y: 0.0 },
            config: Default::default(),
            documentation: None,
        });
        project.workflows.insert(workflow.id.clone(), workflow);

        let issues = validate_integrity(&project);
        assert!(issues.iter().any(|issue| issue.domain == "workflows"));
    }
}
