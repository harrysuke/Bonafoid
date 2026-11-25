//! Agent configuration, read from the environment once per run.
//!
//! Keys live server-side only (plan §12 decision 5): they are never returned to
//! the client and never written to the audit log. `Debug` is implemented by
//! hand so a stray `tracing::debug!("{config:?}")` cannot print the key.

use std::time::Duration;

use crate::error::AgentError;

/// Which provider API to speak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    Ollama,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenAi => "openai",
            ProviderKind::Ollama => "ollama",
        }
    }

    pub fn parse(value: &str) -> Result<Self, AgentError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "anthropic" | "claude" => Ok(ProviderKind::Anthropic),
            "openai" | "gpt" => Ok(ProviderKind::OpenAi),
            "ollama" | "local" => Ok(ProviderKind::Ollama),
            other => Err(AgentError::UnknownProvider(other.to_string())),
        }
    }

    /// Default model for the provider, used when `AGENT_MODEL` is unset.
    pub fn default_model(&self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "claude-sonnet-4-5",
            ProviderKind::OpenAi => "gpt-4o-mini",
            ProviderKind::Ollama => "llama3.1",
        }
    }

    pub fn default_base_url(&self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "https://api.anthropic.com",
            ProviderKind::OpenAi => "https://api.openai.com",
            ProviderKind::Ollama => "http://127.0.0.1:11434",
        }
    }
}

/// Runtime limits. Bounded on purpose: an agent that cannot stop is a cost bug.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_turns: usize,
    pub max_tool_calls: usize,
    pub max_tokens: u64,
    pub max_wall_clock: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_turns: 12,
            max_tool_calls: 60,
            max_tokens: 200_000,
            max_wall_clock: Duration::from_secs(300),
        }
    }
}

impl Limits {
    pub fn from_env() -> Self {
        let default = Self::default();
        Self {
            max_turns: env_usize("AGENT_MAX_TURNS", default.max_turns),
            max_tool_calls: env_usize("AGENT_MAX_TOOL_CALLS", default.max_tool_calls),
            max_tokens: env_u64("AGENT_MAX_TOKENS", default.max_tokens),
            max_wall_clock: Duration::from_secs(env_u64(
                "AGENT_MAX_SECONDS",
                default.max_wall_clock.as_secs(),
            )),
        }
    }
}

/// Provider settings for one process.
#[derive(Clone)]
pub struct AgentConfig {
    pub kind: ProviderKind,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
    /// Whether runs stream `agent_delta` frames while the model writes.
    ///
    /// Default on. It exists as a switch because the event is additive: a
    /// deployment that suspects the delta frames (an aggressive proxy, a very
    /// old client) can turn them off without downgrading the server.
    pub stream_deltas: bool,
    pub request_timeout: Duration,
    pub limits: Limits,
}

impl std::fmt::Debug for AgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentConfig")
            .field("kind", &self.kind)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            // Never print the key, not even a prefix.
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("max_tokens", &self.max_tokens)
            .field("temperature", &self.temperature)
            .field("stream_deltas", &self.stream_deltas)
            .field("limits", &self.limits)
            .finish()
    }
}

impl AgentConfig {
    /// Read the environment. Returns `NotConfigured` when no provider is
    /// selected, which is the normal state of a deployment that has not set an
    /// LLM key — the IDE still works, the assistant drawer is hidden.
    pub fn from_env() -> Result<Self, AgentError> {
        let kind = match std::env::var("AGENT_PROVIDER") {
            Ok(value) if !value.trim().is_empty() => ProviderKind::parse(&value)?,
            _ => return Err(AgentError::NotConfigured),
        };
        let api_key = std::env::var("AGENT_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());
        // Ollama runs locally and needs no key; the hosted APIs do.
        if api_key.is_none() && kind != ProviderKind::Ollama {
            return Err(AgentError::NotConfigured);
        }
        let model = std::env::var("AGENT_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| kind.default_model().to_string());
        let base_url = std::env::var("AGENT_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| kind.default_base_url().to_string());

        Ok(Self {
            kind,
            model,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            max_tokens: env_u64("AGENT_MAX_OUTPUT_TOKENS", 4096) as u32,
            temperature: env_f32("AGENT_TEMPERATURE", 0.2),
            // On by default: streaming is the improvement, not the risk.
            stream_deltas: env_bool("AGENT_STREAM_DELTAS", true),
            request_timeout: Duration::from_secs(env_u64("AGENT_REQUEST_TIMEOUT", 120)),
            limits: Limits::from_env(),
        })
    }

    /// Whether the deployment can run an agent at all.
    pub fn is_configured() -> bool {
        Self::from_env().is_ok()
    }
}

fn env_usize(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

fn env_u64(name: &str, fallback: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

fn env_f32(name: &str, fallback: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

/// Read a boolean the way an operator expects: `0`, `false`, `no` and `off`
/// (any case) are false; anything else non-empty is true.
fn env_bool(name: &str, fallback: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "" => fallback,
            "0" | "false" | "no" | "off" => false,
            _ => true,
        },
        Err(_) => fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_names_are_forgiving() {
        assert_eq!(
            ProviderKind::parse("Claude").unwrap(),
            ProviderKind::Anthropic
        );
        assert_eq!(ProviderKind::parse("local").unwrap(), ProviderKind::Ollama);
        assert!(ProviderKind::parse("nope").is_err());
    }

    #[test]
    fn limits_have_safe_defaults() {
        let limits = Limits::default();
        assert!(limits.max_turns >= 1);
        assert!(limits.max_tool_calls >= 1);
        assert!(limits.max_wall_clock.as_secs() > 0);
    }

    #[test]
    fn debug_never_prints_the_key() {
        let config = AgentConfig {
            kind: ProviderKind::Anthropic,
            model: "m".into(),
            base_url: "https://example.test".into(),
            api_key: Some("sk-secret-value".into()),
            max_tokens: 10,
            temperature: 0.0,
            stream_deltas: true,
            request_timeout: Duration::from_secs(1),
            limits: Limits::default(),
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("sk-secret-value"));
        assert!(rendered.contains("<redacted>"));
    }
}
