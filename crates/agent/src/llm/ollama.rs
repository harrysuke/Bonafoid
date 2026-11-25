//! Ollama (`POST /api/chat`), for a local model with no API key.
//!
//! The wire shape is close to OpenAI's but not identical: tool calls carry
//! arguments as an object, and the response is a single `message` rather than a
//! `choices` array. Streaming is NDJSON — one complete JSON object per line —
//! rather than SSE, so there is no fragment assembly to do.

use serde_json::{json, Value};

use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::events::Usage;
use crate::llm::http::HttpClient;
use crate::llm::{
    parse_arguments, ChatMessage, ChatRequest, ChatResponse, DeltaSink, LlmProvider, NullSink,
    StopReason, ToolCall,
};

const PROVIDER: &str = "ollama";

pub struct OllamaProvider {
    client: HttpClient,
    model: String,
    base_url: String,
    timeout: std::time::Duration,
}

impl OllamaProvider {
    pub fn new(config: &AgentConfig, client: HttpClient) -> Self {
        Self {
            client,
            model: config.model.clone(),
            base_url: config.base_url.clone(),
            timeout: config.request_timeout,
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/api/chat", self.base_url)
    }
}

#[async_trait::async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> &'static str {
        PROVIDER
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, request: ChatRequest) -> Result<ChatResponse, AgentError> {
        // One parse path; see the note in `anthropic.rs`.
        self.stream(request, &mut NullSink).await
    }

    async fn stream(
        &self,
        request: ChatRequest,
        sink: &mut (dyn DeltaSink + Send),
    ) -> Result<ChatResponse, AgentError> {
        let mut messages = vec![json!({ "role": "system", "content": request.system })];
        messages.extend(request.messages.iter().map(message_json));

        let body = json!({
            "model": self.model,
            "stream": true,
            "messages": messages,
            "tools": request.tools.iter().map(|tool| json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                }
            })).collect::<Vec<_>>(),
            "options": {
                "temperature": request.temperature,
                "num_predict": request.max_tokens,
            },
        });

        let mut reader = self
            .client
            .post_stream(PROVIDER, &self.endpoint(), &[], &body, self.timeout)
            .await?;

        let mut state = StreamState::default();
        while let Some(line) = reader.next_line().await? {
            if sink.closed() {
                // See `anthropic.rs`: a half-read response must not be executed.
                return Err(AgentError::Cancelled);
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line).map_err(|_| AgentError::BadResponse {
                provider: PROVIDER,
                message: "malformed NDJSON line".into(),
            })?;
            if state.absorb(&value, sink)? {
                // `done: true` is the last object in the stream.
                break;
            }
        }
        state.finish()
    }
}

#[derive(Default)]
struct StreamState {
    text: String,
    tool_calls: Vec<ToolCall>,
    tokens_in: u64,
    tokens_out: u64,
    done_reason: Option<StopReason>,
}

impl StreamState {
    /// Fold one NDJSON object in. Returns true when the stream is finished.
    fn absorb(
        &mut self,
        value: &Value,
        sink: &mut (dyn DeltaSink + Send),
    ) -> Result<bool, AgentError> {
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(AgentError::Provider {
                provider: PROVIDER,
                message: error.chars().take(300).collect(),
            });
        }

        if let Some(message) = value.get("message") {
            if let Some(text) = message.get("content").and_then(Value::as_str) {
                if !text.is_empty() {
                    self.text.push_str(text);
                    sink.text(text);
                }
            }
            // Ollama sends a tool call whole, never in fragments, so there is
            // nothing to assemble — only an id to synthesize.
            if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    let Some(function) = call.get("function") else {
                        continue;
                    };
                    let Some(name) = function.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    self.tool_calls.push(ToolCall {
                        id: call
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("call_{}", self.tool_calls.len())),
                        name: name.to_string(),
                        arguments: parse_arguments(
                            function.get("arguments").cloned().unwrap_or(Value::Null),
                        ),
                    });
                }
            }
        }

        if value.get("done").and_then(Value::as_bool) != Some(true) {
            return Ok(false);
        }
        self.tokens_in = number(value, "prompt_eval_count");
        self.tokens_out = number(value, "eval_count");
        self.done_reason = value
            .get("done_reason")
            .and_then(Value::as_str)
            .map(|reason| match reason {
                "length" => StopReason::MaxTokens,
                _ => StopReason::EndTurn,
            });
        Ok(true)
    }

    fn finish(self) -> Result<ChatResponse, AgentError> {
        // A turn that ended on a tool call is `ToolUse`, whatever the provider's
        // `done_reason` said.
        let stop_reason = if self.tool_calls.is_empty() {
            self.done_reason.unwrap_or(StopReason::EndTurn)
        } else {
            StopReason::ToolUse
        };

        Ok(ChatResponse {
            text: if self.text.trim().is_empty() {
                None
            } else {
                Some(self.text)
            },
            tool_calls: self.tool_calls,
            usage: Usage {
                tokens_in: self.tokens_in,
                tokens_out: self.tokens_out,
                ..Usage::default()
            },
            stop_reason,
        })
    }
}

fn message_json(message: &ChatMessage) -> Value {
    match message {
        ChatMessage::User(text) => json!({ "role": "user", "content": text }),
        ChatMessage::Assistant { text, tool_calls } => {
            let mut value = json!({
                "role": "assistant",
                "content": text.clone().unwrap_or_default(),
            });
            if !tool_calls.is_empty() {
                value["tool_calls"] = json!(tool_calls
                    .iter()
                    .map(|call| json!({
                        "function": {
                            "name": call.name,
                            "arguments": call.arguments,
                        }
                    }))
                    .collect::<Vec<_>>());
            }
            value
        }
        ChatMessage::ToolResult { tool, content, .. } => json!({
            "role": "tool",
            "tool_name": tool,
            "content": content,
        }),
    }
}

fn number(response: &Value, key: &str) -> u64 {
    response.get(key).and_then(Value::as_u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recording {
        text: String,
    }

    impl DeltaSink for Recording {
        fn text(&mut self, delta: &str) {
            self.text.push_str(delta);
        }
    }

    /// Feed NDJSON lines through the parser as the reader would.
    fn run(lines: &[&str], sink: &mut Recording) -> Result<ChatResponse, AgentError> {
        let mut state = StreamState::default();
        for line in lines {
            let value: Value = serde_json::from_str(line).expect("fixture is valid JSON");
            if state.absorb(&value, sink)? {
                break;
            }
        }
        state.finish()
    }

    #[test]
    fn tool_calls_get_synthesized_ids() {
        let mut sink = Recording::default();
        let response = run(
            &[
                r#"{"message":{"content":"","tool_calls":[{"function":{"name":"entity_create","arguments":{"name":"Expense"}}}]},"done":false}"#,
                r#"{"message":{"content":""},"done":true,"done_reason":"stop","prompt_eval_count":5,"eval_count":3}"#,
            ],
            &mut sink,
        )
        .unwrap();
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "call_0");
        assert_eq!(response.tool_calls[0].arguments["name"], "Expense");
        assert_eq!(response.usage.tokens_in, 5);
        assert_eq!(response.usage.tokens_out, 3);
        // `done_reason` said "stop", but the turn ended on a tool call.
        assert_eq!(response.stop_reason, StopReason::ToolUse);
    }

    #[test]
    fn content_deltas_accumulate_and_reach_the_sink() {
        let mut sink = Recording::default();
        let response = run(
            &[
                r#"{"message":{"role":"assistant","content":"Do"},"done":false}"#,
                r#"{"message":{"role":"assistant","content":"ne."},"done":false}"#,
                r#"{"message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":2,"eval_count":1}"#,
            ],
            &mut sink,
        )
        .unwrap();
        assert_eq!(sink.text, "Done.");
        assert_eq!(response.text.as_deref(), Some("Done."));
        assert_eq!(response.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn an_error_line_fails_the_stream() {
        let mut sink = Recording::default();
        let error = run(&[r#"{"error":"model not found"}"#], &mut sink).unwrap_err();
        assert!(matches!(error, AgentError::Provider { .. }), "{error:?}");
    }
}
