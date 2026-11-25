//! The one callable surface shared by `POST /mcp` and the in-process agent.
//!
//! Before this existed the dispatcher was a free function over `McpServer`,
//! which meant anything that wanted to call a tool had to go through JSON-RPC.
//! The agent endpoint would have had to HTTP-POST to the server's own `/mcp`,
//! which is both slow and a security smell (a loopback request that still has
//! to be authenticated).
//!
//! `McpToolExecutor` is that function, made a value:
//!
//! - `call_sync` runs the 23 shipped BPMN tools (no I/O, no `await`);
//! - `call` additionally runs the project/domain/backend tools, which need the
//!   [`ProjectStateProvider`] and therefore `await`;
//! - `read_resource` resolves static and `project://` resources.

use std::sync::Arc;

use serde_json::Value;

use crate::config::Config;
use crate::error::McpError;
use crate::provider::ProjectStateProvider;
use crate::resources::{self, ResourceContent};
use crate::session::{ProjectSessionStore, SessionStore};
use crate::tools::{self, ToolDefinition};
use crate::transport::http::CallerContext;

/// Shared tool surface. Cheap to clone (everything behind an `Arc`).
pub struct McpToolExecutor {
    pub config: Arc<Config>,
    /// Workflow sessions for the shipped BPMN tools. Named `sessions` to keep
    /// the 23 shipped handlers (and their tests) reading as before.
    pub sessions: Arc<SessionStore>,
    /// Whole-project sessions for the domain tools.
    pub projects: Arc<ProjectSessionStore>,
    /// Storage for `project_*` tools. `None` in the stdio binary and in tests
    /// that do not touch projects.
    pub provider: Option<Arc<dyn ProjectStateProvider>>,
}

impl McpToolExecutor {
    /// An executor with no project storage: BPMN tools work, `project_*`
    /// returns a typed "requires an IDE server connection" error.
    pub fn new(config: Arc<Config>) -> Self {
        let undo_depth = config.undo_depth;
        Self {
            config,
            sessions: Arc::new(SessionStore::new(undo_depth)),
            projects: Arc::new(ProjectSessionStore::new(undo_depth)),
            provider: None,
        }
    }

    /// An executor backed by project storage (the IDE server).
    pub fn with_provider(config: Arc<Config>, provider: Arc<dyn ProjectStateProvider>) -> Self {
        let undo_depth = config.undo_depth;
        Self {
            config,
            sessions: Arc::new(SessionStore::new(undo_depth)),
            projects: Arc::new(ProjectSessionStore::new(undo_depth)),
            provider: Some(provider),
        }
    }

    /// The provider, or the typed error the plan asks for (§5.2).
    pub fn provider(&self) -> Result<&Arc<dyn ProjectStateProvider>, McpError> {
        self.provider
            .as_ref()
            .ok_or_else(crate::provider::provider_required)
    }

    pub fn list_tools(&self) -> Vec<ToolDefinition> {
        tools::definitions()
    }

    /// Tools restricted to an allowlist. Unknown names in `allow` are ignored,
    /// so a step that mentions a tool this build does not ship still runs.
    pub fn list_tools_filtered(&self, allow: &[&str]) -> Vec<ToolDefinition> {
        self.list_tools()
            .into_iter()
            .filter(|tool| allow.is_empty() || allow.contains(&tool.name.as_str()))
            .collect()
    }

    pub fn list_resources(&self) -> Value {
        resources::list_json()
    }

    pub fn list_resource_templates(&self) -> Value {
        resources::project::templates_json()
    }

    /// Dispatch a tool that needs no I/O.
    ///
    /// Returns the MCP tool-result envelope (`{ content, isError }`).
    pub fn call_sync(
        &self,
        caller: &CallerContext,
        name: &str,
        arguments: Value,
    ) -> Result<Value, McpError> {
        tools::dispatch(self, caller, name, arguments)
    }

    /// Dispatch any tool. Project tools await the provider; everything else
    /// delegates to [`Self::call_sync`].
    pub async fn call(
        &self,
        caller: &CallerContext,
        name: &str,
        arguments: Value,
    ) -> Result<Value, McpError> {
        if tools::project::handles(name) {
            return tools::project::dispatch(self, caller, name, arguments).await;
        }
        self.call_sync(caller, name, arguments)
    }

    /// Whether a tool name exists at all (used to reject hallucinated calls
    /// before they reach the handler and produce a confusing error).
    pub fn has_tool(&self, name: &str) -> bool {
        tools::definitions().iter().any(|tool| tool.name == name)
    }

    /// Resolve a resource URI, static or project-scoped.
    pub async fn read_resource(
        &self,
        caller: &CallerContext,
        uri: &str,
    ) -> Result<ResourceContent, McpError> {
        if resources::project::matches(uri) && !uri.starts_with("project://schema/") {
            return resources::project::read(self, caller, uri).await;
        }
        resources::read(uri).map(resources::Resource::into_content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn executor() -> McpToolExecutor {
        McpToolExecutor::new(Arc::new(Config::from_env()))
    }

    #[test]
    fn project_tools_need_a_provider() {
        let executor = executor();
        assert!(matches!(executor.provider(), Err(McpError::Forbidden(_))));
    }

    #[test]
    fn tools_can_be_filtered_to_a_step_allowlist() {
        let executor = executor();
        let filtered = executor.list_tools_filtered(&["entity_create", "project_get"]);
        let names: Vec<&str> = filtered.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"entity_create"));
    }

    #[test]
    fn an_empty_allowlist_means_everything() {
        let executor = executor();
        assert_eq!(
            executor.list_tools_filtered(&[]).len(),
            executor.list_tools().len()
        );
    }
}
