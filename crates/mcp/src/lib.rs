//! Bonafoid MCP server: granular, stateful tools for agentic app authoring.
//!
//! Agents never emit raw BPMN XML or a whole project document. They build a
//! model through small tools, use the server-owned deterministic layout and
//! ids, and validate before saving.
//!
//! Milestone 4 shipped the BPMN half (23 tools, 7 resources). Milestone 5 adds
//! project sessions and the entity/form/page/theme/backend tools, and extracts
//! [`McpToolExecutor`] so the in-process agent can call the exact same handlers
//! without going through JSON-RPC.

pub mod config;
pub mod error;
pub mod executor;
pub mod integrity;
pub mod layout;
pub mod lease;
pub mod persistence;
pub mod protocol;
pub mod provider;
pub mod resources;
pub mod session;
pub mod tools;
pub mod transport;

use std::sync::Arc;

pub use config::Config;
pub use executor::McpToolExecutor;
pub use integrity::{Issue, Severity};
pub use lease::{LeaseError, LeaseGuard, LeaseStore, MemoryLeaseStore, ProjectLease};
pub use provider::{LoadedProject, ProjectStateProvider, ProjectSummaryLite};
pub use session::{ProjectSessionStore, SessionKey, SessionStore};
pub use transport::http::CallerContext;

/// Shared server state. Both transports dispatch through the same instance.
#[derive(Clone)]
pub struct McpServer {
    pub config: Arc<Config>,
    pub executor: Arc<McpToolExecutor>,
}

impl McpServer {
    /// An executor with no project storage: BPMN tools work, `project_*`
    /// returns a typed "requires an IDE server connection" error.
    pub fn new(config: Config) -> Self {
        let config = Arc::new(config);
        Self {
            config: config.clone(),
            executor: Arc::new(McpToolExecutor::new(config)),
        }
    }

    /// An executor backed by project storage (the IDE server).
    pub fn with_provider(config: Config, provider: Arc<dyn ProjectStateProvider>) -> Self {
        let config = Arc::new(config);
        Self {
            config: config.clone(),
            executor: Arc::new(McpToolExecutor::with_provider(config, provider)),
        }
    }

    /// Share an executor that was built elsewhere.
    pub fn from_executor(executor: Arc<McpToolExecutor>) -> Self {
        Self {
            config: executor.config.clone(),
            executor,
        }
    }

    /// Workflow sessions (the shipped BPMN tools' store).
    pub fn sessions(&self) -> &SessionStore {
        &self.executor.sessions
    }

    /// Project sessions (Milestone 5 domain tools).
    pub fn projects(&self) -> &ProjectSessionStore {
        &self.executor.projects
    }

    /// Dispatch one JSON-RPC request synchronously. Returns `None` for
    /// notifications.
    ///
    /// Used by the stdio transport. Requests that need project storage are
    /// rejected with a typed error rather than silently ignored.
    pub fn dispatch(
        &self,
        caller: &CallerContext,
        request: &protocol::JsonRpcRequest,
    ) -> Option<protocol::JsonRpcResponse> {
        protocol::dispatch(self, caller, request)
    }

    /// Dispatch one JSON-RPC request, awaiting project storage when a tool
    /// needs it. Used by `POST /mcp`.
    pub async fn dispatch_async(
        &self,
        caller: &CallerContext,
        request: &protocol::JsonRpcRequest,
    ) -> Option<protocol::JsonRpcResponse> {
        protocol::dispatch_async(self, caller, request).await
    }

    /// Handle one newline-delimited stdio message, returning a response line.
    pub fn handle_line(&self, caller: &CallerContext, line: &str) -> Option<String> {
        match serde_json::from_str::<protocol::JsonRpcRequest>(line) {
            Ok(request) => self
                .dispatch(caller, &request)
                .map(|response| serde_json::to_string(&response).expect("response serialises")),
            Err(error) => Some(
                serde_json::to_string(&protocol::JsonRpcResponse::parse_error(
                    serde_json::Value::Null,
                    &error.to_string(),
                ))
                .expect("error serialises"),
            ),
        }
    }

    /// Async sibling of [`Self::handle_line`], for hosts that have a provider.
    pub async fn handle_line_async(&self, caller: &CallerContext, line: &str) -> Option<String> {
        match serde_json::from_str::<protocol::JsonRpcRequest>(line) {
            Ok(request) => self
                .dispatch_async(caller, &request)
                .await
                .map(|response| serde_json::to_string(&response).expect("response serialises")),
            Err(error) => Some(
                serde_json::to_string(&protocol::JsonRpcResponse::parse_error(
                    serde_json::Value::Null,
                    &error.to_string(),
                ))
                .expect("error serialises"),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_succeeds() {
        let server = McpServer::new(Config {
            file_root: std::env::temp_dir(),
            ..Config::from_env()
        });
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let response = server.handle_line(&CallerContext::local(), line).unwrap();
        assert!(response.contains("serverInfo"));
        assert!(!response.contains("\"error\""));
    }
}
