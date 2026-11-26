//! Errors raised by the agent runtime.

use thiserror::Error;

/// Everything that can stop an agent run.
///
/// Provider failures are deliberately separate from tool failures: a tool error
/// is fed back to the model as text (it can self-correct), while a provider
/// error ends the run.
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("no LLM provider is configured; set AGENT_PROVIDER and AGENT_API_KEY")]
    NotConfigured,

    #[error("unknown provider `{0}`; see the provider catalog for the accepted platforms")]
    UnknownProvider(String),

    #[error("provider `{0}` needs a base URL; set AGENT_BASE_URL or a base_url on its key row")]
    MissingBaseUrl(String),

    #[error("provider `{0}` needs an API key")]
    MissingKey(String),

    #[error("the credential for a provider is malformed: {0}")]
    BadCredential(String),

    #[error("{provider} request failed: {message}")]
    Provider {
        provider: &'static str,
        message: String,
    },

    #[error("{provider} returned an unusable response: {message}")]
    BadResponse {
        provider: &'static str,
        message: String,
    },

    #[error("provider request timed out after {0} seconds")]
    Timeout(u64),

    #[error("the run hit its {0} limit")]
    Limit(&'static str),

    #[error("the client disconnected")]
    Cancelled,

    #[error("tool `{tool}` failed: {message}")]
    Tool { tool: String, message: String },

    #[error("session error: {0}")]
    Session(String),

    #[error("validation failed: {0}")]
    Validation(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl AgentError {
    /// Whether a caller could sensibly retry the same request.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            AgentError::Timeout(_) | AgentError::Provider { .. } | AgentError::Cancelled
        )
    }

    /// A short, stable label for the `error` SSE event.
    pub fn kind(&self) -> &'static str {
        match self {
            AgentError::NotConfigured => "not_configured",
            AgentError::UnknownProvider(_) => "unknown_provider",
            AgentError::MissingBaseUrl(_) => "missing_base_url",
            AgentError::MissingKey(_) => "missing_key",
            AgentError::BadCredential(_) => "bad_credential",
            AgentError::Provider { .. } => "provider_error",
            AgentError::BadResponse { .. } => "bad_response",
            AgentError::Timeout(_) => "timeout",
            AgentError::Limit(_) => "limit",
            AgentError::Cancelled => "cancelled",
            AgentError::Tool { .. } => "tool_error",
            AgentError::Session(_) => "session_error",
            AgentError::Validation(_) => "validation",
            AgentError::Internal(_) => "internal",
        }
    }
}

impl From<nocode_mcp::error::McpError> for AgentError {
    fn from(error: nocode_mcp::error::McpError) -> Self {
        AgentError::Tool {
            tool: "unknown".into(),
            message: error.to_string(),
        }
    }
}

impl From<serde_json::Error> for AgentError {
    fn from(error: serde_json::Error) -> Self {
        AgentError::Internal(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_failures_are_recoverable_and_tool_failures_are_not() {
        assert!(AgentError::Timeout(30).is_recoverable());
        assert!(!AgentError::NotConfigured.is_recoverable());
        assert_eq!(AgentError::Limit("max_turns").kind(), "limit");
    }
}
