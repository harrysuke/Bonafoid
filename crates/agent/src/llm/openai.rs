//! OpenAI Chat Completions (`POST /v1/chat/completions`).
//!
//! This one adapter drives every OpenAI-shaped platform in
//! [`crate::llm::catalog`] — Groq, Cerebras, Zhipu, OpenRouter, Cloudflare and
//! the rest. What differs between them is the base URL, whether a credential is
//! sent at all, and a few extra headers; all three come from the catalog, so a
//! new gateway is a table entry and never a new provider.

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

const PROVIDER: &str = "openai";

pub struct OpenAiProvider {
    client: HttpClient,
    model: String,
    base_url: String,
    /// `None` for a keyless gateway: no `Authorization` header is sent, because
    /// a placeholder value would be worse than none at all.
    api_key: Option<String>,
    /// Static per-platform headers from the catalog (OpenRouter's attribution
    /// headers, for instance).
    extra_headers: Vec<(String, String)>,
    timeout: std::time::Duration,
}

impl OpenAiProvider {
    pub fn new(config: &AgentConfig, client: HttpClient) -> Self {
        Self {
            client,
            model: config.model.clone(),
            base_url: config.base_url.clone(),
            api_key: config
                .api_key
                .clone()
                .filter(|key| !key.trim().is_empty()),
            extra_headers: config.headers.clone(),
            timeout: config.request_timeout,
        }
    }

    fn endpoint(&self) -> String {
        chat_endpoint(&self.base_url)
    }

    /// `Authorization` first, then the platform's own headers.
    ///
    /// Built per request because `HttpClient` takes borrowed pairs, and the
    /// owned strings have to outlive the call. Cheap: three or four entries.
    fn headers(&self) -> Vec<(&str, String)> {
        let mut headers: Vec<(&str, String)> = Vec::with_capacity(self.extra_headers.len() + 1);
        if let Some(key) = self.api_key.as_deref() {
            headers.push(("authorization", format!("Bearer {key}")));
        }
        for (name, value) in &self.extra_headers {
            headers.push((name.as_str(), value.clone()));
        }
        headers
    }
}

/// Where an OpenAI-compatible gateway's chat route lives.
///
/// Most gateways are `{base}/v1/chat/completions`, but not all of them:
/// OpenRouter is `/api/v1/...`, Zhipu is `/api/paas/v4/chat/completions`, and
/// some (GitHub Models, Gemini's compatibility layer) publish a path that no
/// suffix rule can guess. The catalog therefore stores a *complete* endpoint
/// for those, and the first branch below leaves it alone. A base URL an
/// operator typed on a key row gets the forgiving treatment, because there is
/// no way to know which shape they meant.
fn chat_endpoint(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        return base.to_string();
    }
    // `…/v1` or `…/v4`: the version is already part of the base.
    let versioned = base
        .rsplit('/')
        .next()
        .map(|segment| {
            let mut chars = segment.chars();
            matches!(chars.next(), Some('v'))
                && chars.clone().next().is_some()
                && chars.all(|c| c.is_ascii_digit())
        })
        .unwrap_or(false);
    if versioned {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    }
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiProvider {
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
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "messages": messages,
            "tools": request.tools.iter().map(|tool| json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                }
            })).collect::<Vec<_>>(),
            "tool_choice": "auto",
            "stream": true,
            // Without this the stream carries no token counts at all, and the
            // run's usage would report zero for every OpenAI run.
            "stream_options": { "include_usage": true },
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
            if sink.closed() {
                // See `anthropic.rs`: a half-read response must not be executed.
                return Err(AgentError::Cancelled);
            }
            let Some(data) = payload(&line) else { continue };
            if data == "[DONE]" {
                break;
            }
            let value: Value = serde_json::from_str(data).map_err(|_| AgentError::BadResponse {
                provider: PROVIDER,
                message: "malformed stream payload".into(),
            })?;
            state.absorb(&value, sink)?;
        }
        state.finish()
    }
}

/// The JSON of a `data:` line, or `None` for anything else.
fn payload(line: &str) -> Option<&str> {
    let data = line.strip_prefix("data:")?.trim();
    if data.is_empty() {
        None
    } else {
        Some(data)
    }
}

/// One tool call being assembled from `arguments` fragments.
struct PartialCall {
    id: String,
    name: String,
    /// Fragments are `&str`, never bytes: OpenAI splits argument text mid-token,
    /// and a byte buffer reassembled at the end would corrupt multi-byte
    /// characters.
    arguments: String,
}

#[derive(Default)]
struct StreamState {
    text: String,
    calls: BTreeMap<u64, PartialCall>,
    tool_calls: Vec<ToolCall>,
    tokens_in: u64,
    tokens_out: u64,
    finish_reason: Option<StopReason>,
}

impl StreamState {
    fn absorb(
        &mut self,
        value: &Value,
        sink: &mut (dyn DeltaSink + Send),
    ) -> Result<(), AgentError> {
        // A gateway can report a failure inside a 200 stream.
        if let Some(error) = value.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
                .unwrap_or("the provider reported an error");
            return Err(AgentError::Provider {
                provider: PROVIDER,
                message: message.chars().take(300).collect(),
            });
        }

        if let Some(tokens) = at(value, &["usage", "prompt_tokens"]) {
            self.tokens_in = tokens;
        }
        if let Some(tokens) = at(value, &["usage", "completion_tokens"]) {
            self.tokens_out = tokens;
        }

        let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            // The usage-only frame that `include_usage` adds has no choices.
            return Ok(());
        };

        if let Some(delta) = choice.get("delta") {
            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                self.text.push_str(text);
                sink.text(text);
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    self.absorb_call(call);
                }
            }
        }

        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(match reason {
                "tool_calls" => StopReason::ToolUse,
                "length" => StopReason::MaxTokens,
                "stop" => StopReason::EndTurn,
                _ => StopReason::Other,
            });
        }
        Ok(())
    }

    /// Merge one fragment. `id` and `name` arrive once, on the first fragment
    /// for an index; the arguments arrive in pieces, sometimes one character at
    /// a time.
    fn absorb_call(&mut self, call: &Value) {
        let index = call.get("index").and_then(Value::as_u64).unwrap_or(0);
        let entry = self.calls.entry(index).or_insert_with(|| PartialCall {
            id: String::new(),
            name: String::new(),
            arguments: String::new(),
        });
        if let Some(id) = call.get("id").and_then(Value::as_str) {
            if !id.is_empty() {
                entry.id = id.to_string();
            }
        }
        let function = call.get("function");
        if let Some(name) = function.and_then(|f| f.get("name")).and_then(Value::as_str) {
            if !name.is_empty() {
                entry.name = name.to_string();
            }
        }
        if let Some(fragment) = function
            .and_then(|f| f.get("arguments"))
            .and_then(Value::as_str)
        {
            entry.arguments.push_str(fragment);
        }
    }

    fn finish(mut self) -> Result<ChatResponse, AgentError> {
        for (_, call) in std::mem::take(&mut self.calls) {
            self.tool_calls.push(ToolCall {
                id: call.id,
                name: call.name,
                // Only parsed here: a partial argument string is not JSON, and
                // parsing it early would run a tool with truncated arguments.
                arguments: parse_arguments(Value::String(call.arguments)),
            });
        }

        let stop_reason = self.finish_reason.unwrap_or(if self.tool_calls.is_empty() {
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

fn at(value: &Value, path: &[&str]) -> Option<u64> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(*key)?;
    }
    cursor.as_u64()
}

fn message_json(message: &ChatMessage) -> Value {
    match message {
        ChatMessage::User(text) => json!({ "role": "user", "content": text }),
        ChatMessage::Assistant { text, tool_calls } => {
            let content = match text {
                Some(text) => json!(text),
                None => Value::Null,
            };
            let mut value = json!({ "role": "assistant", "content": content });
            if !tool_calls.is_empty() {
                value["tool_calls"] = json!(tool_calls
                    .iter()
                    .map(|call| json!({
                        "id": call.id,
                        "type": "function",
                        "function": {
                            "name": call.name,
                            // The API requires a JSON *string* here.
                            "arguments": call.arguments.to_string(),
                        }
                    }))
                    .collect::<Vec<_>>());
            }
            value
        }
        ChatMessage::ToolResult {
            call_id, content, ..
        } => json!({
            "role": "tool",
            "tool_call_id": call_id,
            "content": content,
        }),
    }
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

    fn run(lines: &[&str], sink: &mut Recording) -> Result<ChatResponse, AgentError> {
        let mut state = StreamState::default();
        for line in lines {
            let Some(data) = payload(line) else { continue };
            if data == "[DONE]" {
                break;
            }
            let value: Value = serde_json::from_str(data).expect("fixture is valid JSON");
            state.absorb(&value, sink)?;
        }
        state.finish()
    }

    #[test]
    fn argument_fragments_are_concatenated_before_parsing() {
        let mut sink = Recording::default();
        let response = run(
            &[
                r#"data: {"choices":[{"delta":{"role":"assistant","content":null}}]}"#,
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"entity_create","arguments":"{\"na"}}]}}]}"#,
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"me\":\"Expense\"}"}}]}}]}"#,
                r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
                r#"data: {"choices":[],"usage":{"prompt_tokens":9,"completion_tokens":4}}"#,
                "data: [DONE]",
            ],
            &mut sink,
        )
        .unwrap();

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "call_1");
        assert_eq!(response.tool_calls[0].name, "entity_create");
        assert_eq!(response.tool_calls[0].arguments["name"], "Expense");
        assert_eq!(response.usage.tokens_in, 9);
        assert_eq!(response.usage.tokens_out, 4);
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert!(response.text.is_none());
    }

    #[test]
    fn a_fragment_split_inside_a_multibyte_character_still_parses() {
        let mut sink = Recording::default();
        let response = run(
            &[
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":"note","arguments":"{\"text\":\"party "}}]}}]}"#,
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"🎉\"}"}}]}}]}"#,
            ],
            &mut sink,
        )
        .unwrap();
        assert_eq!(response.tool_calls[0].arguments["text"], "party 🎉");
    }

    #[test]
    fn two_tool_calls_assemble_by_index() {
        let mut sink = Recording::default();
        let response = run(
            &[
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"one","arguments":"{}"}},{"index":1,"id":"b","function":{"name":"two","arguments":"{}"}}]}}]}"#,
            ],
            &mut sink,
        )
        .unwrap();
        assert_eq!(response.tool_calls.len(), 2);
        assert_eq!(response.tool_calls[0].name, "one");
        assert_eq!(response.tool_calls[1].name, "two");
    }

    #[test]
    fn text_deltas_reach_the_sink_in_order() {
        let mut sink = Recording::default();
        let response = run(
            &[
                r#"data: {"choices":[{"delta":{"content":"Done"}}]}"#,
                r#"data: {"choices":[{"delta":{"content":"."}}]}"#,
                r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            ],
            &mut sink,
        )
        .unwrap();
        assert_eq!(sink.text, "Done.");
        assert_eq!(response.text.as_deref(), Some("Done."));
        assert_eq!(response.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn an_error_frame_fails_the_stream() {
        let mut sink = Recording::default();
        let error = run(
            &[r#"data: {"error":{"message":"rate limited"}}"#],
            &mut sink,
        )
        .unwrap_err();
        assert!(matches!(error, AgentError::Provider { .. }), "{error:?}");
    }

    #[test]
    fn gateway_endpoints_follow_the_base_url_shape() {
        assert_eq!(
            chat_endpoint("https://api.openai.com"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_endpoint("https://openrouter.ai/api/"),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        assert_eq!(
            chat_endpoint("https://api.groq.com/openai"),
            "https://api.groq.com/openai/v1/chat/completions"
        );
        // A version already in the base is not repeated.
        assert_eq!(
            chat_endpoint("https://open.bigmodel.cn/api/paas/v4"),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
        // A base that is the full path is used as-is.
        assert_eq!(
            chat_endpoint("https://models.github.ai/inference/chat/completions"),
            "https://models.github.ai/inference/chat/completions"
        );
    }

    #[test]
    fn every_openai_shaped_platform_produces_a_usable_endpoint() {
        use crate::config::ProviderKind;
        use crate::llm::catalog;
        for spec in catalog::CATALOG {
            // Anthropic and a local Ollama have their own adapters, Cloudflare
            // and `custom` get their endpoint from the caller, not from here.
            if spec.kind != ProviderKind::OpenAi
                || spec.account_scoped
                || spec.base_url.is_empty()
            {
                continue;
            }
            let endpoint = chat_endpoint(spec.base_url);
            assert!(
                endpoint.ends_with("/chat/completions"),
                "{} produced {endpoint}",
                spec.platform
            );
            // No doubled version segment: the bug that makes Google and GitHub
            // 404 when a base is treated as a bare host.
            assert!(
                !endpoint.contains("/v1/v1/") && !endpoint.contains("/v4/v1/"),
                "{} produced {endpoint}",
                spec.platform
            );
        }
    }

    #[test]
    fn a_keyless_platform_sends_no_authorization_header() {
        let config = crate::config::AgentConfig::for_platform("kilo", "", None, None)
            .expect("kilo is keyless");
        let provider = OpenAiProvider::new(&config, HttpClient::new());
        let headers = provider.headers();
        assert!(
            !headers.iter().any(|(name, _)| *name == "authorization"),
            "{headers:?}"
        );
    }

    #[test]
    fn catalog_headers_ride_along_with_the_bearer_token() {
        let config =
            crate::config::AgentConfig::for_platform("openrouter", "", None, Some("sk-or".into()))
                .unwrap();
        let provider = OpenAiProvider::new(&config, HttpClient::new());
        let headers = provider.headers();
        assert_eq!(headers[0].0, "authorization");
        assert_eq!(headers[0].1, "Bearer sk-or");
        assert!(headers.iter().any(|(name, _)| *name == "http-referer"));
        assert!(headers.iter().any(|(name, _)| *name == "x-title"));
    }

    #[test]
    fn assistant_tool_calls_are_sent_as_json_strings() {
        let message = ChatMessage::Assistant {
            text: None,
            tool_calls: vec![ToolCall {
                id: "1".into(),
                name: "project_get".into(),
                arguments: json!({"view": "summary"}),
            }],
        };
        let value = message_json(&message);
        let arguments = value["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        let reparsed: Value = serde_json::from_str(arguments).unwrap();
        assert_eq!(reparsed["view"], "summary");
    }
}
