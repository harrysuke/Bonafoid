//! Anthropic Messages API (`POST /v1/messages`).

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::events::Usage;
use crate::llm::http::HttpClient;
use crate::llm::{
    parse_arguments, ChatMessage, ChatRequest, ChatResponse, DeltaSink, LlmProvider, NullSink,
    StopReason, ToolCall,
};

const PROVIDER: &str = "anthropic";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    client: HttpClient,
    model: String,
    base_url: String,
    api_key: String,
    timeout: std::time::Duration,
}

impl AnthropicProvider {
    pub fn new(config: &AgentConfig, client: HttpClient) -> Self {
        Self {
            client,
            model: config.model.clone(),
            base_url: config.base_url.clone(),
            // `AgentConfig::from_env` refuses to build without a key for this
            // provider, so an empty string here would be a programming error,
            // not a user error. The request fails with a 401 and a clear body.
            api_key: config.api_key.clone().unwrap_or_default(),
            timeout: config.request_timeout,
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }

    fn headers(&self) -> [(&'static str, String); 2] {
        [
            ("x-api-key", self.api_key.clone()),
            ("anthropic-version", ANTHROPIC_VERSION.to_string()),
        ]
    }
}

#[async_trait::async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &'static str {
        PROVIDER
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, request: ChatRequest) -> Result<ChatResponse, AgentError> {
        // One parse path. A second, non-streaming parser would drift from this
        // one the first time the API changed a field.
        self.stream(request, &mut NullSink).await
    }

    async fn stream(
        &self,
        request: ChatRequest,
        sink: &mut (dyn DeltaSink + Send),
    ) -> Result<ChatResponse, AgentError> {
        let body = json!({
            "model": self.model,
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "system": request.system,
            "stream": true,
            "messages": request.messages.iter().map(message_json).collect::<Vec<_>>(),
            "tools": request.tools.iter().map(|tool| json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema,
            })).collect::<Vec<_>>(),
        });

        let mut reader = self
            .client
            .post_stream(
                PROVIDER,
                &self.endpoint(),
                &self.headers(),
                &body,
                self.timeout,
            )
            .await?;

        let mut state = StreamState::default();
        while let Some(line) = reader.next_line().await? {
            // Checked per line, not every N: `closed()` is one atomic load, and
            // a client that has gone is the difference between paying for the
            // rest of a generation and not.
            if sink.closed() {
                // Returning the half-read response would be worse than useless:
                // a truncated `tool_use` block parses to `{}`, and the run would
                // execute a tool with no arguments.
                return Err(AgentError::Cancelled);
            }
            let Some(data) = payload(&line) else { continue };
            let value: Value = serde_json::from_str(data).map_err(|_| AgentError::BadResponse {
                provider: PROVIDER,
                message: "malformed stream payload".into(),
            })?;
            state.absorb(&value, sink)?;
        }
        state.finish()
    }
}

/// The JSON of a `data:` line, or `None` for anything else in the stream.
///
/// `event:` lines, blank lines between frames and `: ping` keep-alives all
/// carry no payload; a reader that tried to parse them would fail on every
/// frame boundary.
fn payload(line: &str) -> Option<&str> {
    let data = line.strip_prefix("data:")?.trim();
    if data.is_empty() {
        None
    } else {
        Some(data)
    }
}

/// One tool call being assembled from `input_json_delta` fragments.
struct PartialCall {
    id: String,
    name: String,
    /// Accumulated as `&str` fragments, never as bytes: a fragment boundary is
    /// always a character boundary, so the string is valid at every step.
    json: String,
}

/// Everything a streamed response accumulates.
#[derive(Default)]
struct StreamState {
    text: String,
    /// Keyed by content-block index, because fragments for two tool calls can
    /// interleave in one response.
    calls: BTreeMap<u64, PartialCall>,
    /// Completed calls, in the order their blocks closed.
    tool_calls: Vec<ToolCall>,
    tokens_in: u64,
    tokens_out: u64,
    stop_reason: Option<StopReason>,
}

impl StreamState {
    /// Fold one SSE payload into the response.
    fn absorb(
        &mut self,
        value: &Value,
        sink: &mut (dyn DeltaSink + Send),
    ) -> Result<(), AgentError> {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let index = value.get("index").and_then(Value::as_u64).unwrap_or(0);

        match kind {
            "message_start" => {
                if let Some(tokens) = at(value, &["message", "usage", "input_tokens"]) {
                    self.tokens_in = tokens;
                }
            }
            "content_block_start" => {
                let block = value.get("content_block");
                if block.and_then(|b| b.get("type")).and_then(Value::as_str) == Some("tool_use") {
                    self.calls.insert(
                        index,
                        PartialCall {
                            id: text_of(block, "id"),
                            name: text_of(block, "name"),
                            json: String::new(),
                        },
                    );
                }
            }
            "content_block_delta" => {
                let delta = value.get("delta");
                match delta.and_then(|d| d.get("type")).and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(text) =
                            delta.and_then(|d| d.get("text")).and_then(Value::as_str)
                        {
                            self.text.push_str(text);
                            sink.text(text);
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(text) = delta
                            .and_then(|d| d.get("thinking"))
                            .and_then(Value::as_str)
                        {
                            sink.thinking(text);
                        }
                    }
                    Some("input_json_delta") => {
                        // Never parsed here: a `tool_use` input is only valid
                        // once the block closes, and acting on a truncated
                        // object would run a tool with the wrong arguments.
                        if let Some(partial) = delta
                            .and_then(|d| d.get("partial_json"))
                            .and_then(Value::as_str)
                        {
                            if let Some(call) = self.calls.get_mut(&index) {
                                call.json.push_str(partial);
                            }
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => self.close_block(index),
            "message_delta" => {
                if let Some(reason) = value
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    self.stop_reason = Some(stop_reason(reason));
                }
                if let Some(tokens) = at(value, &["usage", "output_tokens"]) {
                    self.tokens_out = tokens;
                }
            }
            "error" => {
                let message = value
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("the provider reported an error");
                return Err(AgentError::Provider {
                    provider: PROVIDER,
                    message: message.chars().take(300).collect(),
                });
            }
            // `ping`, `message_stop`: nothing to accumulate.
            _ => {}
        }
        Ok(())
    }

    /// Parse a finished `tool_use` block.
    fn close_block(&mut self, index: u64) {
        if let Some(call) = self.calls.remove(&index) {
            self.push_call(call);
        }
    }

    fn push_call(&mut self, call: PartialCall) {
        self.tool_calls.push(ToolCall {
            id: call.id,
            name: call.name,
            // Empty buffer means the model called the tool with no arguments.
            arguments: parse_arguments(Value::String(call.json)),
        });
    }

    fn finish(mut self) -> Result<ChatResponse, AgentError> {
        // A stream cut short by a disconnect can end without `content_block_stop`.
        // Keeping the partial call is better than dropping a tool the model did
        // finish emitting, and `parse_arguments` treats an empty buffer as `{}`.
        for (_, call) in std::mem::take(&mut self.calls) {
            self.push_call(call);
        }

        let stop_reason = self.stop_reason.unwrap_or(if self.tool_calls.is_empty() {
            StopReason::EndTurn
        } else {
            StopReason::ToolUse
        });

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

/// `value[a][b][c]`, as a number.
fn at(value: &Value, path: &[&str]) -> Option<u64> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(*key)?;
    }
    cursor.as_u64()
}

fn text_of(value: Option<&Value>, key: &str) -> String {
    value
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "end_turn" | "stop_sequence" => StopReason::EndTurn,
        _ => StopReason::Other,
    }
}

fn message_json(message: &ChatMessage) -> Value {
    match message {
        ChatMessage::User(text) => json!({
            "role": "user",
            "content": [{ "type": "text", "text": text }],
        }),
        ChatMessage::Assistant { text, tool_calls } => {
            let mut content = Vec::new();
            if let Some(text) = text.as_ref().filter(|text| !text.trim().is_empty()) {
                content.push(json!({ "type": "text", "text": text }));
            }
            for call in tool_calls {
                content.push(json!({
                    "type": "tool_use",
                    "id": call.id,
                    "name": call.name,
                    "input": call.arguments,
                }));
            }
            json!({ "role": "assistant", "content": content })
        }
        ChatMessage::ToolResult {
            call_id, content, ..
        } => json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": call_id,
                "content": content,
            }],
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sink that records what it was handed, so a test can prove the deltas
    /// and the final response agree.
    #[derive(Default)]
    struct Recording {
        text: String,
        thinking: String,
    }

    impl DeltaSink for Recording {
        fn text(&mut self, delta: &str) {
            self.text.push_str(delta);
        }

        fn thinking(&mut self, delta: &str) {
            self.thinking.push_str(delta);
        }
    }

    /// Feed `data:` lines through the parser exactly as the reader would.
    fn run(lines: &[&str], sink: &mut Recording) -> Result<ChatResponse, AgentError> {
        let mut state = StreamState::default();
        for line in lines {
            if let Some(data) = payload(line) {
                let value: Value = serde_json::from_str(data).expect("fixture is valid JSON");
                state.absorb(&value, sink)?;
            }
        }
        state.finish()
    }

    #[test]
    fn a_tool_input_split_across_three_fragments_parses() {
        let mut sink = Recording::default();
        let response = run(
            &[
                r#"data: {"type":"message_start","message":{"usage":{"input_tokens":12}}}"#,
                r#"event: content_block_start"#,
                r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Creating "}}"#,
                r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"the entity."}}"#,
                r#"data: {"type":"content_block_stop","index":0}"#,
                r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_1","name":"entity_create","input":{}}}"#,
                r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"na"}}"#,
                r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"me\":\"Expe"}}"#,
                r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"nse\"}"}}"#,
                r#"data: {"type":"content_block_stop","index":1}"#,
                r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":7}}"#,
                r#"data: {"type":"message_stop"}"#,
                "",
                ": ping",
            ],
            &mut sink,
        )
        .unwrap();

        // The deltas and the final text agree: streaming never changes the result.
        assert_eq!(sink.text, "Creating the entity.");
        assert_eq!(response.text.as_deref(), Some("Creating the entity."));
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "tu_1");
        assert_eq!(response.tool_calls[0].name, "entity_create");
        assert_eq!(response.tool_calls[0].arguments["name"], "Expense");
        assert_eq!(response.usage.tokens_in, 12);
        assert_eq!(response.usage.tokens_out, 7);
        assert_eq!(response.stop_reason, StopReason::ToolUse);
    }

    #[test]
    fn a_tool_use_block_with_no_arguments_is_an_empty_object() {
        let mut sink = Recording::default();
        let response = run(
            &[
                r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t","name":"project_get","input":{}}}"#,
                r#"data: {"type":"content_block_stop","index":0}"#,
            ],
            &mut sink,
        )
        .unwrap();
        assert_eq!(response.tool_calls[0].arguments, json!({}));
        // No `message_delta` means no stop reason: tool use is inferred.
        assert_eq!(response.stop_reason, StopReason::ToolUse);
    }

    #[test]
    fn thinking_deltas_are_not_assistant_text() {
        let mut sink = Recording::default();
        let response = run(
            &[
                r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"weighing options"}}"#,
                r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Done."}}"#,
            ],
            &mut sink,
        )
        .unwrap();
        assert_eq!(sink.thinking, "weighing options");
        assert_eq!(sink.text, "Done.");
        assert_eq!(response.text.as_deref(), Some("Done."));
    }

    #[test]
    fn an_error_frame_fails_the_stream() {
        let mut sink = Recording::default();
        let error = run(
            &[r#"data: {"type":"error","error":{"message":"overloaded"}}"#],
            &mut sink,
        )
        .unwrap_err();
        assert!(matches!(error, AgentError::Provider { .. }), "{error:?}");
    }

    #[test]
    fn a_stream_that_ends_mid_block_keeps_the_partial_call() {
        let mut sink = Recording::default();
        let response = run(
            &[
                r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t","name":"entity_create","input":{}}}"#,
                r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"name\":\"Ex\"}"}}"#,
                // No `content_block_stop`: the connection dropped.
            ],
            &mut sink,
        )
        .unwrap();
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].arguments["name"], "Ex");
    }

    #[test]
    fn tool_results_are_sent_as_user_turns() {
        let message = ChatMessage::ToolResult {
            call_id: "tu_1".into(),
            tool: "entity_create".into(),
            content: "{\"entity_id\":\"e\"}".into(),
        };
        let value = message_json(&message);
        assert_eq!(value["role"], "user");
        assert_eq!(value["content"][0]["type"], "tool_result");
        assert_eq!(value["content"][0]["tool_use_id"], "tu_1");
    }

    #[test]
    fn an_assistant_turn_keeps_text_and_calls_together() {
        let message = ChatMessage::Assistant {
            text: Some("thinking".into()),
            tool_calls: vec![ToolCall {
                id: "1".into(),
                name: "project_get".into(),
                arguments: json!({}),
            }],
        };
        let value = message_json(&message);
        assert_eq!(value["content"][0]["type"], "text");
        assert_eq!(value["content"][1]["type"], "tool_use");
    }
}
