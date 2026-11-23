//! Bonafoid MCP server: granular, stateful tools for agentic BPMN authoring.
//!
//! Agents never emit raw BPMN XML. They build a graph through small tools,
//! use the server-owned deterministic layout, and validate/simulate before
//! saving through the Bonafoid project API.

pub mod config;
pub mod error;
pub mod layout;
pub mod persistence;
pub mod protocol;
pub mod resources;
pub mod session;
pub mod tools;
pub mod transport;

use std::sync::Arc;

pub use config::Config;
pub use session::SessionStore;
pub use transport::http::CallerContext;

/// Shared server state. Both transports dispatch through the same instance.
#[derive(Clone)]
pub struct McpServer {
    pub config: Arc<Config>,
    pub sessions: Arc<SessionStore>,
}

impl McpServer {
    pub fn new(config: Config) -> Self {
        let undo_depth = config.undo_depth;
        Self {
            config: Arc::new(config),
            sessions: Arc::new(SessionStore::new(undo_depth)),
        }
    }

    pub fn from_parts(config: Arc<Config>, sessions: Arc<SessionStore>) -> Self {
        Self { config, sessions }
    }

    /// Dispatch one JSON-RPC request. Returns `None` for notifications.
    pub fn dispatch(
        &self,
        caller: &CallerContext,
        request: &protocol::JsonRpcRequest,
    ) -> Option<protocol::JsonRpcResponse> {
        protocol::dispatch(self, caller, request)
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
