//! Provider abstraction. The orchestrator only ever sees [`ChatResponse`].
//!
//! Three wire formats, one shape. All provider-specific code lives in this
//! module tree, so an API change is a one-file edit. Which platform needs which
//! format, endpoint and headers is not decided here — [`catalog`] owns that,
//! and both this module and the IDE server read it.

pub mod anthropic;
pub mod catalog;
pub mod http;
pub mod ollama;
pub mod openai;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{AgentConfig, ProviderKind};
use crate::error::AgentError;
use crate::events::Usage;

/// One normalized tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Already-parsed arguments. Providers send JSON, sometimes as a string.
    pub arguments: Value,
}

/// A tool advertised to the model, in MCP's own schema shape.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Conversation state. Provider-neutral.
///
/// `Serialize`/`Deserialize` exist for one consumer: the conversation store
/// (plan §15.3.1). It persists *this* shape, never a provider's raw payload,
/// because a raw payload differs per vendor and would pin the table to a
/// provider version. The wire form is externally tagged (`{"User":"…"}`,
/// `{"Assistant":{…}}`), which is stable and needs no `tag` attribute that a
/// newtype variant could not satisfy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatMessage {
    User(String),
    Assistant {
        text: Option<String>,
        tool_calls: Vec<ToolCall>,
    },
    ToolResult {
        call_id: String,
        tool: String,
        content: String,
    },
}

impl ChatMessage {
    /// `user`, `assistant` or `tool` — the column value the store writes.
    pub fn role(&self) -> &'static str {
        match self {
            ChatMessage::User(_) => "user",
            ChatMessage::Assistant { .. } => "assistant",
            ChatMessage::ToolResult { .. } => "tool",
        }
    }

    /// Rough token count, for the 8k context cap.
    ///
    /// Four characters per token is the usual English approximation and is
    /// deliberately pessimistic: under-estimating would blow the window, over-
    /// estimating only drops a turn the model did not strictly need.
    pub fn approx_tokens(&self) -> usize {
        let chars = match self {
            ChatMessage::User(text) => text.chars().count(),
            ChatMessage::Assistant { text, tool_calls } => {
                text.as_deref().map(|t| t.chars().count()).unwrap_or(0)
                    + tool_calls
                        .iter()
                        .map(|call| call.name.chars().count() + call.arguments.to_string().len())
                        .sum::<usize>()
            }
            ChatMessage::ToolResult { content, .. } => content.chars().count(),
        };
        chars / 4 + 1
    }
}

/// One completion request.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolSchema>,
    pub max_tokens: u32,
    pub temperature: f32,
}

/// Why the model stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Other,
}

/// One completion response.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Usage,
    pub stop_reason: StopReason,
}

/// Receives text as the model produces it.
///
/// Streaming is a *display* concern and never a control-flow concern: tool
/// calls still run only after their arguments are complete and parse, and a
/// delta never triggers work. That is what makes it safe to add to a
/// transactional pipeline.
pub trait DeltaSink: Send {
    /// A fragment of assistant text, in arrival order.
    fn text(&mut self, delta: &str);

    /// A fragment of extended thinking, where the provider exposes it.
    fn thinking(&mut self, _delta: &str) {}

    /// True once the client is gone. Providers should stop reading, because
    /// nobody will read the rest and the tokens are already paid for.
    fn closed(&self) -> bool {
        false
    }
}

/// A sink that discards everything.
///
/// This is what `complete` passes to `stream`, so each provider has exactly one
/// parse path: two parsers would drift, one parser plus a no-op sink cannot.
pub struct NullSink;

impl DeltaSink for NullSink {
    fn text(&mut self, _delta: &str) {}
}

/// A chat-completion backend.
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// Wire name, e.g. `anthropic`.
    fn name(&self) -> &'static str;

    /// Model id sent to the API.
    fn model(&self) -> &str;

    async fn complete(&self, request: ChatRequest) -> Result<ChatResponse, AgentError>;

    /// The same request, reporting text as it arrives.
    ///
    /// The default is the pre-streaming behaviour: one delta carrying the whole
    /// text. A provider that cannot stream, and every mock, needs nothing more
    /// than `complete`.
    async fn stream(
        &self,
        request: ChatRequest,
        sink: &mut (dyn DeltaSink + Send),
    ) -> Result<ChatResponse, AgentError> {
        let response = self.complete(request).await?;
        if let Some(text) = response.text.as_deref() {
            sink.text(text);
        }
        Ok(response)
    }
}

/// Build the provider named by the configuration.
pub fn build(config: &AgentConfig) -> Result<Arc<dyn LlmProvider>, AgentError> {
    let client = http::HttpClient::new();
    Ok(match config.kind {
        ProviderKind::Anthropic => Arc::new(anthropic::AnthropicProvider::new(config, client)),
        ProviderKind::OpenAi => Arc::new(openai::OpenAiProvider::new(config, client)),
        ProviderKind::Ollama => Arc::new(ollama::OllamaProvider::new(config, client)),
    })
}

/// Split a tool call's `arguments` that arrived as a JSON *string*.
///
/// Anthropic sends an object, OpenAI sends a string, Ollama sends an object.
/// Normalizing here means no provider has to be special-cased downstream.
pub(crate) fn parse_arguments(value: Value) -> Value {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Value::Object(Default::default())
            } else {
                serde_json::from_str(trimmed).unwrap_or(Value::Object(Default::default()))
            }
        }
        Value::Null => Value::Object(Default::default()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn string_arguments_are_parsed() {
        assert_eq!(
            parse_arguments(json!("{\"name\":\"Expense\"}")),
            json!({"name": "Expense"})
        );
    }

    #[test]
    fn object_arguments_pass_through() {
        assert_eq!(parse_arguments(json!({"a": 1})), json!({"a": 1}));
    }

    #[test]
    fn malformed_arguments_become_an_empty_object() {
        assert_eq!(parse_arguments(json!("not json")), json!({}));
    }
}
