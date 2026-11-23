//! Domain errors and their JSON-RPC / HTTP mapping.
//!
//! Tool handlers use typed errors; the transport layer decides whether an
//! error becomes a JSON-RPC error object or an MCP tool result with
//! `isError: true` (domain failures use the latter so agents can self-correct).

use thiserror::Error;

/// Errors raised by MCP tools and the session store.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("unknown tool `{0}`")]
    UnknownTool(String),

    #[error("unknown node type `{0}`; see bpmn://catalog/node-types")]
    UnknownNodeType(String),

    #[error("unknown config key `{key}` for node type `{node_type}`")]
    UnknownConfigKey { node_type: String, key: String },

    #[error("node not found: {0}")]
    NodeNotFound(String),

    #[error("edge not found: {0}")]
    EdgeNotFound(String),

    #[error("stale revision: expected {expected}, current {current}")]
    StaleRevision { expected: u64, current: u64 },

    #[error("workflow would exceed the node limit of {limit}")]
    NodeLimit { limit: usize },

    #[error("invalid JSON: {0}")]
    InvalidJson(String),

    #[error("invalid parameter `{name}`: {detail}")]
    InvalidParam { name: &'static str, detail: String },

    #[error("invalid BPMN XML: {0}")]
    InvalidXml(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("project save failed: {0}")]
    ProjectSave(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl McpError {
    /// JSON-RPC 2.0 error code. -32602 is "invalid params" and is used for
    /// all recoverable domain failures, including stale revisions.
    pub fn code(&self) -> i64 {
        match self {
            Self::InvalidJson(_) | Self::InvalidParam { .. } | Self::InvalidXml(_) => -32602,
            Self::UnknownTool(_) => -32601,
            Self::Forbidden(_) => -32003,
            Self::Internal(_) | Self::Io(_) | Self::ProjectSave(_) => -32603,
            _ => -32602,
        }
    }

    /// Whether the failure is caused by the caller rather than the server.
    pub fn is_client_error(&self) -> bool {
        !matches!(self, Self::Internal(_) | Self::Io(_) | Self::ProjectSave(_))
    }
}

impl From<serde_json::Error> for McpError {
    fn from(error: serde_json::Error) -> Self {
        Self::InvalidJson(error.to_string())
    }
}

impl From<std::io::Error> for McpError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Convert a result into a JSON value suitable for `tools/call` responses.
pub fn into_tool_result<T: serde::Serialize>(result: Result<T, McpError>) -> serde_json::Value {
    match result {
        Ok(value) => match serde_json::to_value(value) {
            Ok(json) => serde_json::json!({
                "content": [{ "type": "text", "text": serde_json::to_string(&json).unwrap_or_else(|_| json.to_string()) }],
                "isError": false,
            }),
            Err(error) => tool_error(&McpError::Internal(error.to_string())),
        },
        Err(error) => tool_error(&error),
    }
}

/// Error shape for `tools/call`: an MCP result with `isError: true`, not a
/// protocol-level error, so the agent receives the message as tool feedback.
pub fn tool_error(error: &McpError) -> serde_json::Value {
    serde_json::json!({
        "content": [{ "type": "text", "text": error.to_string() }],
        "isError": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_errors_are_flagged() {
        let value = tool_error(&McpError::NodeNotFound("n1".into()));
        assert_eq!(value["isError"], serde_json::json!(true));
        assert!(value["content"][0]["text"].as_str().unwrap().contains("n1"));
    }

    #[test]
    fn success_results_are_compact_text() {
        let value = into_tool_result(Ok(serde_json::json!({ "node_id": "n1" })));
        assert_eq!(value["isError"], serde_json::json!(false));
        assert_eq!(value["content"][0]["type"], serde_json::json!("text"));
    }
}
