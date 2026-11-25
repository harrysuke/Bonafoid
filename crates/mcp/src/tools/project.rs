//! Project session tools: open, inspect, save, close.
//!
//! These are the only tools that talk to storage. Everything else mutates the
//! in-memory session, which is what makes the agent's edits cheap and lets the
//! orchestrator diff before/after each call.
//!
//! Tenant isolation is the provider's job (it receives `tenant_id` and
//! `user_id`), and session isolation is the [`SessionKey`] owner's job. A
//! project belonging to another tenant is a "not found", never a "forbidden",
//! so the tools cannot be used to probe for existence.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::{self, McpError};
use crate::executor::McpToolExecutor;
use crate::resources::project as view;
use crate::tools::{parse_args, schema_for, ToolDefinition};
use crate::transport::http::CallerContext;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "project_open".into(),
            description: "Open a stored project into an editable session. Returns the session_id, \
                          the revision it was loaded at, and a summary. Every project tool takes \
                          that session_id."
                .into(),
            input_schema: schema_for::<OpenParams>(),
        },
        ToolDefinition {
            name: "project_get".into(),
            description:
                "Read the session. view=summary returns the id catalogue (cheapest way to \
                          resolve references); view=full returns the whole project JSON. Pass \
                          domain to get one domain only."
                    .into(),
            input_schema: schema_for::<GetParams>(),
        },
        ToolDefinition {
            name: "project_save".into(),
            description: "Persist the session to storage and append a revision. Fails if someone \
                          else wrote the project since it was opened. Prefer letting the agent \
                          pipeline save once at the end rather than after every edit."
                .into(),
            input_schema: schema_for::<SaveParams>(),
        },
        ToolDefinition {
            name: "project_close".into(),
            description: "Discard the session. Unsaved edits are lost; call project_save first."
                .into(),
            input_schema: schema_for::<CloseParams>(),
        },
    ]
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpenParams {
    /// Id of the project to open.
    pub project_id: String,
    /// `summary` (default) or `full`.
    #[serde(default)]
    pub view: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetParams {
    pub session_id: String,
    /// `summary` (default) or `full`.
    #[serde(default)]
    pub view: Option<String>,
    /// One of `entities`, `forms`, `workflows`, `pages`, `themes`, `catalog`.
    #[serde(default)]
    pub domain: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveParams {
    pub session_id: String,
    /// The session revision the caller last saw. Omit to save unconditionally
    /// from this session's point of view.
    #[serde(default)]
    pub expected_revision: Option<u64>,
    /// Commit message. The pipeline writes `AI: <prompt summary>`.
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloseParams {
    pub session_id: String,
}

/// Whether this module owns `name`.
pub fn handles(name: &str) -> bool {
    matches!(
        name,
        "project_open" | "project_get" | "project_save" | "project_close"
    )
}

fn ok(value: Value) -> Value {
    error::into_tool_result(Ok::<Value, McpError>(value))
}

pub async fn dispatch(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    name: &str,
    arguments: Value,
) -> Result<Value, McpError> {
    let value = match name {
        "project_open" => open(executor, caller, parse_args(arguments)?).await?,
        "project_get" => get(executor, caller, parse_args(arguments)?)?,
        "project_save" => save(executor, caller, parse_args(arguments)?).await?,
        "project_close" => close(executor, caller, parse_args(arguments)?)?,
        other => return Err(McpError::UnknownTool(other.to_string())),
    };
    Ok(ok(value))
}

async fn open(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    params: OpenParams,
) -> Result<Value, McpError> {
    let provider = executor.provider()?;
    let loaded = provider
        .load_project(&caller.tenant_id, &caller.user_id, &params.project_id)
        .await?;
    let revision = loaded.revision;
    let summary = view::summary(&loaded.project);
    let full = matches!(params.view.as_deref(), Some("full"));
    // Serialise before the project is moved into the session.
    let full_value = if full {
        Some(serde_json::to_value(&loaded.project).map_err(McpError::from)?)
    } else {
        None
    };

    let (session_id, revision) =
        executor
            .projects
            .create(&caller.owner, loaded.project, revision)?;

    let mut result = json!({
        "session_id": session_id,
        "revision": revision,
        "summary": summary,
    });
    if let Some(value) = full_value {
        // `view: "full"` is allowed here so an agent that already knows it needs
        // the whole model saves a round trip. `project_get` remains the normal
        // way to re-read.
        result["project"] = value;
    }
    Ok(result)
}

fn get(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    params: GetParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    executor.projects.with_read(&key, |session| {
        let project = &session.project;
        let full = matches!(params.view.as_deref(), Some("full"));
        let payload = match (full, params.domain.as_deref()) {
            (true, None) => serde_json::to_value(project).map_err(McpError::from)?,
            (true, Some(domain)) => domain_value(project, domain, true)?,
            (false, Some(domain)) => domain_value(project, domain, false)?,
            (false, None) => view::catalog(project),
        };
        Ok(json!({
            "revision": session.revision,
            "saved_revision": session.saved_revision,
            "view": if full { "full" } else { "summary" },
            "project": payload,
        }))
    })
}

fn domain_value(
    project: &nocode_model::Project,
    domain: &str,
    full: bool,
) -> Result<Value, McpError> {
    if full {
        let value = match domain {
            "entities" => json!(project.entities),
            "forms" => json!(project.forms),
            "workflows" => json!(project.workflows),
            "pages" => json!(project.pages),
            "themes" => json!(project.themes),
            "catalog" => return Ok(view::catalog(project)),
            other => return Err(unknown_domain(other)),
        };
        return Ok(value);
    }
    Ok(match domain {
        "entities" => view::entities_summary(project),
        "forms" => view::forms_summary(project),
        "workflows" => view::workflows_summary(project),
        "pages" => view::pages_summary(project),
        "themes" => view::themes_summary(project),
        "catalog" => view::catalog(project),
        other => return Err(unknown_domain(other)),
    })
}

fn unknown_domain(domain: &str) -> McpError {
    McpError::InvalidParam {
        name: "domain",
        detail: format!(
            "unknown domain `{domain}`; expected entities, forms, workflows, pages, themes or catalog"
        ),
    }
}

async fn save(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    params: SaveParams,
) -> Result<Value, McpError> {
    let provider = executor.provider()?;
    let key = caller.session_key(&params.session_id);

    // The guard is `saved_revision`, never `revision`: the latter counts edits
    // inside this session and no database row ever had that number. An explicit
    // `expected_revision` is an override for stricter callers, not a way to opt
    // out of concurrency control.
    let (project, expected) = executor.projects.with_read(&key, |session| {
        let expected = match params.expected_revision {
            Some(expected) => {
                let current = session.expected_revision();
                if expected != current {
                    return Err(McpError::StaleRevision { expected, current });
                }
                expected
            }
            None => session.expected_revision(),
        };
        Ok((session.project.clone(), expected))
    })?;

    let message = params.message.as_deref();
    let revision = provider
        .save_project(
            &caller.tenant_id,
            &caller.user_id,
            &project,
            expected,
            message,
        )
        .await?;
    executor.projects.mark_saved(&key, revision)?;

    Ok(json!({
        "saved": true,
        "revision": revision,
        "project_id": project.id.to_string(),
    }))
}

fn close(
    executor: &McpToolExecutor,
    caller: &CallerContext,
    params: CloseParams,
) -> Result<Value, McpError> {
    let key = caller.session_key(&params.session_id);
    executor.projects.discard(&key)?;
    Ok(json!({ "closed": true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_project_tools_are_handled() {
        assert!(handles("project_open"));
        assert!(!handles("entity_create"));
    }

    #[test]
    fn unknown_domains_are_rejected() {
        let project = nocode_model::Project::new("App");
        assert!(domain_value(&project, "invoices", false).is_err());
    }
}
