//! `project://` resources: cheap ID catalogues and the full model.
//!
//! The single most important anti-hallucination device in the milestone is
//! that an agent never has to invent an id: `project://{id}/catalog` returns
//! every id and name across every domain in one small document, and the
//! per-domain summaries are cheaper still.
//!
//! Everything here is tenant-scoped through the caller: the provider is asked
//! for `(tenant_id, user_id, project_id)`, and a project from another tenant is
//! reported as "not found".

use serde_json::{json, Value};

use crate::error::McpError;
use crate::executor::McpToolExecutor;
use crate::provider::provider_required;
use crate::resources::ResourceContent;
use crate::transport::http::CallerContext;

/// One advertised `resources/templates/list` entry.
#[derive(Debug, Clone)]
pub struct ResourceTemplate {
    pub uri_template: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub mime_type: &'static str,
}

pub const TEMPLATES: &[ResourceTemplate] = &[
    ResourceTemplate {
        uri_template: "project://{project_id}/model",
        name: "Project model",
        description: "Full project JSON, including every domain",
        mime_type: "application/json",
    },
    ResourceTemplate {
        uri_template: "project://{project_id}/entities",
        name: "Entity catalogue",
        description: "Entities with their field ids, names and types",
        mime_type: "application/json",
    },
    ResourceTemplate {
        uri_template: "project://{project_id}/forms",
        name: "Form catalogue",
        description: "Forms with their bound entity and field count",
        mime_type: "application/json",
    },
    ResourceTemplate {
        uri_template: "project://{project_id}/workflows",
        name: "Workflow catalogue",
        description: "Workflows with node counts and their start trigger",
        mime_type: "application/json",
    },
    ResourceTemplate {
        uri_template: "project://{project_id}/pages",
        name: "Page catalogue",
        description: "Pages with their path and component count",
        mime_type: "application/json",
    },
    ResourceTemplate {
        uri_template: "project://{project_id}/themes",
        name: "Theme catalogue",
        description: "Themes, flagging the active one",
        mime_type: "application/json",
    },
    ResourceTemplate {
        uri_template: "project://{project_id}/catalog",
        name: "Whole-project id catalogue",
        description:
            "Every id and name across all domains — the cheapest way to resolve references",
        mime_type: "application/json",
    },
];

/// `resources/templates/list` payload.
pub fn templates_json() -> Value {
    json!(TEMPLATES
        .iter()
        .map(|template| json!({
            "uriTemplate": template.uri_template,
            "name": template.name,
            "description": template.description,
            "mimeType": template.mime_type,
        }))
        .collect::<Vec<_>>())
}

/// Whether `uri` is handled by this module.
pub fn matches(uri: &str) -> bool {
    uri.starts_with("project://")
}

/// Resolve a `project://{id}/{kind}` URI.
pub async fn read(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    uri: &str,
) -> Result<ResourceContent, McpError> {
    let (project_id, kind) = parse_uri(uri)?;
    let provider = executor.provider()?;
    let loaded = provider
        .load_project(&caller.tenant_id, &caller.user_id, project_id)
        .await?;
    let project = &loaded.project;

    let (text, mime_type) = match kind {
        "model" => (
            serde_json::to_string_pretty(project).map_err(McpError::from)?,
            "application/json",
        ),
        "entities" => (compact(&entities_summary(project))?, "application/json"),
        "forms" => (compact(&forms_summary(project))?, "application/json"),
        "workflows" => (compact(&workflows_summary(project))?, "application/json"),
        "pages" => (compact(&pages_summary(project))?, "application/json"),
        "themes" => (compact(&themes_summary(project))?, "application/json"),
        "catalog" => (compact(&catalog(project))?, "application/json"),
        other => {
            return Err(McpError::InvalidParam {
                name: "uri",
                detail: format!("unknown project resource `{other}`"),
            })
        }
    };
    Ok(ResourceContent {
        uri: uri.to_string(),
        mime_type,
        text,
    })
}

fn compact(value: &Value) -> Result<String, McpError> {
    serde_json::to_string(value).map_err(McpError::from)
}

/// Split `project://{project_id}/{kind}`.
fn parse_uri(uri: &str) -> Result<(&str, &str), McpError> {
    let rest = uri
        .strip_prefix("project://")
        .ok_or_else(|| McpError::InvalidParam {
            name: "uri",
            detail: format!("`{uri}` is not a project resource"),
        })?;
    let (project_id, kind) = rest.split_once('/').ok_or_else(|| McpError::InvalidParam {
        name: "uri",
        detail: "project resources look like project://{project_id}/{kind}".into(),
    })?;
    if project_id.is_empty() || kind.is_empty() {
        return Err(McpError::InvalidParam {
            name: "uri",
            detail: "project resources look like project://{project_id}/{kind}".into(),
        });
    }
    Ok((project_id, kind))
}

/// The `type` tag of an adjacently-tagged enum, e.g. `"String"` or `"Decimal"`.
pub fn type_tag<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".into())
}

/// Entities with the field ids an agent needs to build relations and forms.
pub fn entities_summary(project: &nocode_model::Project) -> Value {
    json!(project
        .entities
        .values()
        .map(|entity| json!({
            "id": entity.id.to_string(),
            "name": entity.name,
            "plural_name": entity.plural_name,
            "fields": entity
                .fields
                .iter()
                .map(|field| json!({
                    "id": field.id.0.to_string(),
                    "name": field.name,
                    "type": type_tag(&field.field_type),
                    "nullable": field.nullable,
                }))
                .collect::<Vec<_>>(),
            "relations": entity
                .relations
                .iter()
                .map(|relation| json!({
                    "name": relation.name,
                    "to_entity": relation.to_entity.to_string(),
                }))
                .collect::<Vec<_>>(),
        }))
        .collect::<Vec<_>>())
}

pub fn forms_summary(project: &nocode_model::Project) -> Value {
    json!(project
        .forms
        .values()
        .map(|form| json!({
            "id": form.id.to_string(),
            "name": form.name,
            "entity_id": form.entity_id.as_ref().map(|id| id.to_string()),
            "field_count": form.fields.len(),
        }))
        .collect::<Vec<_>>())
}

pub fn workflows_summary(project: &nocode_model::Project) -> Value {
    json!(project
        .workflows
        .values()
        .map(|workflow| json!({
            "id": workflow.id.to_string(),
            "name": workflow.name,
            "node_count": workflow.nodes.len(),
            "trigger": trigger_descriptor(workflow),
        }))
        .collect::<Vec<_>>())
}

pub fn pages_summary(project: &nocode_model::Project) -> Value {
    json!(project
        .pages
        .values()
        .map(|page| json!({
            "id": page.id.to_string(),
            "name": page.name,
            "path": page.path,
            "component_count": page.components.len(),
        }))
        .collect::<Vec<_>>())
}

pub fn themes_summary(project: &nocode_model::Project) -> Value {
    let active = project.active_theme_id.as_ref();
    json!(project
        .themes
        .values()
        .map(|theme| json!({
            "id": theme.id.to_string(),
            "name": theme.name,
            "active": active == Some(&theme.id),
        }))
        .collect::<Vec<_>>())
}

/// Every id and name in one object. The agent's reference sheet.
pub fn catalog(project: &nocode_model::Project) -> Value {
    json!({
        "project": {
            "id": project.id.to_string(),
            "name": project.name,
            "slug": project.slug,
        },
        "entities": project
            .entities
            .values()
            .map(|entity| json!({
                "id": entity.id.to_string(),
                "name": entity.name,
                "fields": entity
                    .fields
                    .iter()
                    .map(|field| json!({
                        "id": field.id.0.to_string(),
                        "name": field.name,
                        "type": type_tag(&field.field_type),
                    }))
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
        "forms": forms_summary(project),
        "workflows": workflows_summary(project),
        "pages": pages_summary(project),
        "themes": themes_summary(project),
    })
}

/// A compact description of how a workflow starts.
///
/// Prefers the `trigger` descriptor written by `workflow_set_start_trigger`,
/// and falls back to the start event's own type so a workflow authored purely
/// with the BPMN tools still reports something true.
pub fn trigger_descriptor(workflow: &nocode_model::Workflow) -> Value {
    use nocode_model::WorkflowNodeType;

    let start = workflow.nodes.iter().find_map(|node| {
        if let WorkflowNodeType::StartEvent { event_type } = &node.node_type {
            Some((node, event_type))
        } else {
            None
        }
    });
    let Some((node, event_type)) = start else {
        return json!("none");
    };

    if let Some(trigger) = node.config.properties.get("trigger") {
        return trigger.clone();
    }

    let kind = match event_type {
        nocode_model::StartEventType::None => "none".to_string(),
        nocode_model::StartEventType::Message { message_ref } => format!("message:{message_ref}"),
        nocode_model::StartEventType::Timer { .. } => "timer".to_string(),
        nocode_model::StartEventType::Signal { signal_ref } => format!("signal:{signal_ref}"),
        nocode_model::StartEventType::Error { error_ref } => format!("error:{error_ref}"),
        nocode_model::StartEventType::Escalation { escalation_ref } => {
            format!("escalation:{escalation_ref}")
        }
        nocode_model::StartEventType::Compensation { activity_ref } => {
            format!("compensation:{activity_ref}")
        }
        nocode_model::StartEventType::Conditional { condition } => {
            format!("conditional:{condition}")
        }
    };
    json!(kind)
}

/// A compact project overview used by `project_open` / `project_get`.
pub fn summary(project: &nocode_model::Project) -> Value {
    json!({
        "id": project.id.to_string(),
        "name": project.name,
        "slug": project.slug,
        "entities": project.entities.len(),
        "forms": project.forms.len(),
        "workflows": project.workflows.len(),
        "pages": project.pages.len(),
        "themes": project.themes.len(),
    })
}

/// The guard every project tool uses when it needs a provider.
pub fn require_provider(executor: &McpToolExecutor) -> Result<(), McpError> {
    if executor.provider.is_some() {
        Ok(())
    } else {
        Err(provider_required())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocode_model::{Entity, Project};

    #[test]
    fn parses_project_uris() {
        assert_eq!(parse_uri("project://abc/model").unwrap(), ("abc", "model"));
        assert!(parse_uri("project://abc").is_err());
        assert!(parse_uri("project:///model").is_err());
        assert!(parse_uri("bpmn://schema/workflow").is_err());
    }

    #[test]
    fn catalog_lists_every_domain() {
        let mut project = Project::new("Expense App");
        let entity = Entity::new("Expense");
        project.entities.insert(entity.id.clone(), entity);
        let catalog = catalog(&project);
        for key in ["entities", "forms", "workflows", "pages", "themes"] {
            assert!(catalog.get(key).is_some(), "{key}");
        }
        assert_eq!(catalog["entities"][0]["name"], "Expense");
    }

    #[test]
    fn an_empty_project_has_an_empty_catalog() {
        let catalog = catalog(&Project::new("Empty"));
        assert_eq!(catalog["entities"].as_array().unwrap().len(), 0);
    }
}
