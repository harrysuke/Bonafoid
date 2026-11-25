//! What survives a run, and what must not.
//!
//! Two transformations sit between a run and the conversation store, and both
//! live here rather than in `nocode-ide-server` for one reason: the shapes they
//! operate on are [`ChatMessage`]s, which only this crate owns. A store that
//! re-implemented them would drift from the orchestrator the first time a
//! message variant changed.
//!
//! - [`redact_message`] removes credentials from tool arguments before a row is
//!   written (plan §15.3.6).
//! - [`compact_tool_result`] replaces a large tool result with
//!   `{ok, summary, ids}` (plan §15.3.1).
//!
//! Neither may fail a run: a message that cannot be redacted is dropped, and a
//! result that cannot be summarised is truncated. Memory is an optimisation for
//! the *next* turn; it must never break the current one.

use serde_json::{json, Value};

use crate::llm::ChatMessage;

/// Tool results above this size are replaced by a summary.
///
/// 4 KB is roughly 1k tokens: large enough that a real domain map survives
/// intact, small enough that twelve of them cannot fill a context window.
pub const MAX_TOOL_RESULT_CHARS: usize = 4 * 1024;

/// How much of a large result is kept in its summary.
const SUMMARY_CHARS: usize = 400;

/// Keys whose values are dropped from stored tool arguments.
///
/// A substring test rather than a regex: the dependency-free version is
/// auditable at a glance, and the failure mode of a *wider* match (dropping a
/// benign `token_count`) is a slightly less useful transcript, while the
/// failure mode of a narrower one is a leaked credential.
const SENSITIVE_KEYS: &[&str] = &[
    "secret",
    "token",
    "password",
    "passwd",
    "api_key",
    "apikey",
    "api-key",
    "authorization",
    "credential",
    "private_key",
];

/// Whether a JSON key names something that must not be persisted.
pub fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    let collapsed = normalized.replace('_', "");
    SENSITIVE_KEYS
        .iter()
        .any(|needle| normalized.contains(needle) || collapsed.contains(&needle.replace('_', "")))
}

/// Remove sensitive values from a JSON tree, in place.
///
/// Replaced with `"[redacted]"` rather than deleted: the model reading a
/// restored transcript should see that a field existed, so it does not try to
/// re-supply one it already sent.
fn redact_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *child = Value::String("[redacted]".into());
                } else {
                    redact_value(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_value),
        _ => {}
    }
}

/// A message as it may be stored.
///
/// User text is kept verbatim — it is the intent the next turn has to resolve,
/// and redacting it would remove exactly the context memory exists to provide.
pub fn redact_message(message: &ChatMessage) -> ChatMessage {
    match message {
        ChatMessage::User(text) => ChatMessage::User(text.clone()),
        ChatMessage::Assistant { text, tool_calls } => ChatMessage::Assistant {
            text: text.clone(),
            tool_calls: tool_calls
                .iter()
                .map(|call| {
                    let mut arguments = call.arguments.clone();
                    redact_value(&mut arguments);
                    crate::llm::ToolCall {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments,
                    }
                })
                .collect(),
        },
        ChatMessage::ToolResult {
            call_id,
            tool,
            content,
        } => ChatMessage::ToolResult {
            call_id: call_id.clone(),
            tool: tool.clone(),
            // Not scrubbed, only bounded — by `compact_tool_result`. Scrubbing
            // free text with a pattern match would mangle a legitimate result
            // and still miss a secret that was reformatted; the guarantee this
            // module offers is on the arguments, where the value has a name.
            content: content.clone(),
        },
    }
}

/// Replace a large tool result with the three things the next turn needs.
///
/// `ok` says whether the call succeeded, `summary` says what happened in one
/// line, and `ids` lists the objects the model may refer to later. Everything
/// else — the full domain map — is re-readable from the project session, which
/// is also fresher than a transcript.
pub fn compact_tool_result(message: &ChatMessage) -> ChatMessage {
    let ChatMessage::ToolResult {
        call_id,
        tool,
        content,
    } = message
    else {
        return message.clone();
    };

    if content.chars().count() <= MAX_TOOL_RESULT_CHARS {
        return message.clone();
    }

    let parsed: Option<Value> = serde_json::from_str(content).ok();
    let failed = parsed
        .as_ref()
        .and_then(|value| value.get("isError"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || content.starts_with("error:");

    let mut ids: Vec<String> = Vec::new();
    if let Some(value) = parsed.as_ref() {
        collect_ids(value, &mut ids, 0);
    }
    ids.sort();
    ids.dedup();
    ids.truncate(40);

    let summary: String = content.chars().take(SUMMARY_CHARS).collect();
    let compacted = json!({
        "ok": !failed,
        "summary": summary,
        "ids": ids,
        "truncated": true,
        "original_chars": content.chars().count(),
    });

    ChatMessage::ToolResult {
        call_id: call_id.clone(),
        tool: tool.clone(),
        content: compacted.to_string(),
    }
}

/// Gather `id`-shaped values from a JSON tree, bounded by depth.
///
/// Depth-limited because a domain map is a tree the model wrote: an
/// accidentally self-referential structure is impossible in JSON, but a deeply
/// nested one is not, and the transcript must not become a stack test.
fn collect_ids(value: &Value, out: &mut Vec<String>, depth: usize) {
    if depth > 6 || out.len() >= 200 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == "id" || key.ends_with("_id") {
                    if let Some(id) = child.as_str() {
                        out.push(id.to_string());
                    }
                } else {
                    collect_ids(child, out, depth + 1);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_ids(item, out, depth + 1)),
        _ => {}
    }
}

/// A one-line headline for a prompt, used as the conversation title.
///
/// Truncated on a character boundary, not a byte one: a title that splits a
/// multi-byte character would be an invalid UTF-8 string in the drawer.
pub fn title_from_prompt(prompt: &str, max_chars: usize) -> String {
    let first_line = prompt
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("New conversation");
    if first_line.chars().count() <= max_chars {
        return first_line.to_string();
    }
    let truncated: String = first_line.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", truncated.trim_end())
}

/// A deterministic digest of what a conversation touched.
///
/// The compaction fallback when the summariser is unavailable (plan §15.3.3).
/// It is worse than a model summary and better than nothing: it names the
/// objects, which is what a follow-up is most likely to reference.
pub fn digest(messages: &[ChatMessage]) -> String {
    let mut tools: Vec<String> = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    let mut prompts: Vec<String> = Vec::new();

    for message in messages {
        match message {
            ChatMessage::User(text) => {
                let line = text.lines().next().unwrap_or("").trim();
                if !line.is_empty() {
                    prompts.push(title_from_prompt(line, 80));
                }
            }
            ChatMessage::Assistant { tool_calls, .. } => {
                for call in tool_calls {
                    tools.push(call.name.clone());
                    collect_ids(&call.arguments, &mut ids, 0);
                }
            }
            ChatMessage::ToolResult { .. } => {}
        }
    }

    tools.sort();
    tools.dedup();
    ids.sort();
    ids.dedup();
    ids.truncate(30);

    let mut out = String::from("Earlier turns (deterministic digest):\n");
    if !prompts.is_empty() {
        out.push_str("- asked: ");
        out.push_str(&prompts.join(" | "));
        out.push('\n');
    }
    if !tools.is_empty() {
        out.push_str("- tools used: ");
        out.push_str(&tools.join(", "));
        out.push('\n');
    }
    if !ids.is_empty() {
        out.push_str("- objects touched: ");
        out.push_str(&ids.join(", "));
        out.push('\n');
    }
    if out.ends_with("digest):\n") {
        out.push_str("- (no structured content)\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::ToolCall;

    fn tool_call(arguments: Value) -> ToolCall {
        ToolCall {
            id: "call_1".into(),
            name: "connector_invoke".into(),
            arguments,
        }
    }

    #[test]
    fn sensitive_keys_are_recognised_in_their_common_spellings() {
        for key in [
            "api_key",
            "apiKey",
            "API-KEY",
            "authorization",
            "client_secret",
            "password",
            "access_token",
        ] {
            assert!(is_sensitive_key(key), "{key} should be sensitive");
        }
        for key in ["name", "amount", "description", "entity_id"] {
            assert!(!is_sensitive_key(key), "{key} should not be sensitive");
        }
    }

    #[test]
    fn redaction_reaches_nested_arguments() {
        let message = ChatMessage::Assistant {
            text: Some("calling".into()),
            tool_calls: vec![tool_call(json!({
                "url": "https://example.test",
                "headers": { "Authorization": "Bearer sk-live-123" },
                "body": { "api_key": "sk-live-456", "amount": 10 }
            }))],
        };

        let stored = serde_json::to_string(&redact_message(&message)).unwrap();
        assert!(!stored.contains("sk-live-123"), "{stored}");
        assert!(!stored.contains("sk-live-456"), "{stored}");
        // The non-secret sibling survives, so the transcript stays useful.
        assert!(stored.contains("amount"), "{stored}");
    }

    #[test]
    fn a_small_tool_result_is_stored_verbatim() {
        let message = ChatMessage::ToolResult {
            call_id: "c".into(),
            tool: "project_get".into(),
            content: "{\"entities\":{}}".into(),
        };
        assert_eq!(
            serde_json::to_string(&compact_tool_result(&message)).unwrap(),
            serde_json::to_string(&message).unwrap()
        );
    }

    #[test]
    fn a_large_tool_result_keeps_only_ok_summary_and_ids() {
        let big = json!({
            "ok": true,
            "entities": { "e1": { "id": "e1", "fields": "x".repeat(9_000) } },
            "forms": { "f1": { "id": "f1" } }
        })
        .to_string();
        assert!(big.len() > MAX_TOOL_RESULT_CHARS);

        let message = ChatMessage::ToolResult {
            call_id: "c".into(),
            tool: "project_get".into(),
            content: big,
        };
        let ChatMessage::ToolResult { content, .. } = compact_tool_result(&message) else {
            panic!("wrong variant");
        };
        let value: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(value["ok"], json!(true));
        assert_eq!(value["truncated"], json!(true));
        let ids = value["ids"].as_array().unwrap();
        assert!(ids.contains(&json!("e1")));
        assert!(ids.contains(&json!("f1")));
        assert!(content.len() < MAX_TOOL_RESULT_CHARS);
    }

    #[test]
    fn a_failed_result_is_reported_as_failed() {
        let message = ChatMessage::ToolResult {
            call_id: "c".into(),
            tool: "entity_create".into(),
            content: format!("error: {}", "boom ".repeat(2_000)),
        };
        let ChatMessage::ToolResult { content, .. } = compact_tool_result(&message) else {
            panic!("wrong variant");
        };
        let value: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(value["ok"], json!(false));
    }

    #[test]
    fn titles_never_split_a_character() {
        let prompt = "É".repeat(120);
        let title = title_from_prompt(&prompt, 60);
        assert!(title.chars().count() <= 60);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn the_digest_names_tools_and_ids_without_a_model() {
        let messages = vec![
            ChatMessage::User("Build an expense tracker".into()),
            ChatMessage::Assistant {
                text: None,
                tool_calls: vec![tool_call(json!({ "entity_id": "e-42" }))],
            },
        ];
        let digest = digest(&messages);
        assert!(digest.contains("expense tracker"), "{digest}");
        assert!(digest.contains("connector_invoke"), "{digest}");
        assert!(digest.contains("e-42"), "{digest}");
    }

    #[test]
    fn an_empty_conversation_still_produces_a_digest() {
        assert!(digest(&[]).contains("no structured content"));
    }
}
