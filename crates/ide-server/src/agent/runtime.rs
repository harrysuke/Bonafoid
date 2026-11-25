//! Resolved agent settings for this process.
//!
//! Read once at startup rather than per request: the environment does not
//! change while the server runs, and a per-request read would let a test and a
//! request disagree about whether the feature is on.
//!
//! The runtime is also the seam that makes the endpoint testable. A test
//! injects a scripted [`LlmProvider`]; production reads the environment. The
//! handler cannot tell the difference, so the SSE path, the audit row and the
//! session lifecycle are all exercised by tests.

use std::sync::Arc;

use nocode_agent::{AgentConfig, ChatOptions, Limits, LlmProvider};

use crate::error::ApiError;

/// Whether the assistant can run, and with which settings.
pub struct AgentRuntime {
    provider: Option<Arc<dyn LlmProvider>>,
    chat: ChatOptions,
    limits: Limits,
    /// Why the assistant is off, for the 503 body. `None` when it is on.
    disabled_reason: Option<String>,
}

impl AgentRuntime {
    /// Read the environment. A missing provider is a normal state: the IDE
    /// works, the drawer is hidden, and the endpoint answers 503 with the fix.
    pub fn from_env() -> Self {
        let config = match AgentConfig::from_env() {
            Ok(config) => config,
            Err(error) => {
                tracing::info!(reason = %error, "assistant disabled");
                return Self::disabled(error.to_string());
            }
        };
        // `build_provider` can only fail on a programming error (an unknown
        // provider kind). Turning that into "disabled with a reason" keeps the
        // failure at startup rather than on the user's first request.
        let provider = match nocode_agent::build_provider(&config) {
            Ok(provider) => provider,
            Err(error) => {
                tracing::warn!(reason = %error, "assistant disabled: provider could not be built");
                return Self::disabled(error.to_string());
            }
        };
        Self {
            provider: Some(provider),
            chat: ChatOptions {
                max_tokens: config.max_tokens,
                temperature: config.temperature,
                stream_deltas: config.stream_deltas,
            },
            limits: config.limits,
            disabled_reason: None,
        }
    }

    /// An always-on runtime around a caller-supplied provider (tests).
    pub fn with_provider(provider: Arc<dyn LlmProvider>, limits: Limits) -> Self {
        Self {
            provider: Some(provider),
            chat: ChatOptions::default(),
            limits,
            disabled_reason: None,
        }
    }

    /// An explicitly off runtime. Used by tests and by a host that wants to
    /// disable the assistant even though the environment is configured.
    pub fn disabled(reason: String) -> Self {
        Self {
            provider: None,
            chat: ChatOptions::default(),
            limits: Limits::default(),
            disabled_reason: Some(reason),
        }
    }
    /// The provider, or a 503 that tells the operator what to set.
    pub fn provider(&self) -> Result<&Arc<dyn LlmProvider>, ApiError> {
        self.provider.as_ref().ok_or_else(|| {
            ApiError::ServiceUnavailable(format!(
                "the assistant is not configured: {}",
                self.disabled_reason
                    .as_deref()
                    .unwrap_or("set AGENT_PROVIDER and AGENT_API_KEY")
            ))
        })
    }

    pub fn chat(&self) -> ChatOptions {
        self.chat
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Whether the endpoint will stream or answer 503.
    pub fn is_enabled(&self) -> bool {
        self.provider.is_some()
    }
}
