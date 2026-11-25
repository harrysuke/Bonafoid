//! Conversation memory: what the assistant said, and what it may remember.
//!
//! The project JSON carries **state**; this module carries **intent**. A
//! follow-up like "now make the threshold 1000" can only resolve *the
//! threshold* if the previous turn is still in the prompt, and the project
//! cannot supply that: it records that a threshold is 500, not that anyone
//! discussed it (plan §15.3).
//!
//! # The rule this module enforces
//!
//! **The transcript supplies intent; the session supplies truth.** Nothing here
//! is ever rehydrated into a project. If the transcript says the threshold is
//! 500 and the project says 750, the project wins — the card is rebuilt from the
//! live session on every run and the system prompt says so explicitly.
//!
//! # Failure policy
//!
//! Memory must never fail a run. Every write here is best-effort at the call
//! site (the handler logs and continues), and compaction has a deterministic
//! fallback so a provider outage degrades the summary instead of stopping the
//! work.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value};

use nocode_agent::llm::{ChatMessage, ChatRequest, LlmProvider};
use nocode_agent::memory;

use crate::error::ApiError;

/// Compact once a conversation holds this many messages.
const COMPACT_AT_MESSAGES: usize = 40;

/// Compact once the stored transcript is roughly this many tokens.
const COMPACT_AT_TOKENS: usize = 12_000;

/// Turns kept verbatim after a compaction.
const KEEP_AFTER_COMPACT: usize = 12;

/// How many messages a run is given.
const CONTEXT_MESSAGES: usize = 24;

/// Token budget for the messages handed to a run.
const CONTEXT_TOKENS: usize = 8_000;

/// Longest stored title.
const MAX_TITLE_CHARS: usize = 60;

/// Longest accepted conversation id / project id (they are UUIDs).
const MAX_ID_CHARS: usize = 64;

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn text(value: &str) -> Value {
    Value::from(value.to_string())
}

fn opt_text(value: Option<&str>) -> Value {
    Value::String(value.map(|v| Box::new(v.to_string())))
}

/// One conversation, as the drawer's history menu needs it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Conversation {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub message_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// One message, flattened for the drawer.
///
/// The wire form is deliberately *not* the stored `ChatMessage` JSON: the
/// drawer renders text and tool names, and should not be coupled to the
/// storage encoding of a Rust enum.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConversationMessage {
    pub seq: i64,
    /// `user`, `assistant` or `tool`.
    pub role: String,
    pub text: String,
    /// Present on a tool result: the tool that produced it.
    pub tool: Option<String>,
    pub created_at: String,
}

impl ConversationMessage {
    fn from_stored(seq: i64, message: &ChatMessage, created_at: String) -> Self {
        let (role, text, tool) = match message {
            ChatMessage::User(text) => ("user".to_string(), text.clone(), None),
            ChatMessage::Assistant { text, tool_calls } => {
                let body = text.clone().unwrap_or_default();
                let tools: Vec<&str> = tool_calls.iter().map(|call| call.name.as_str()).collect();
                let label = if tools.is_empty() {
                    body
                } else {
                    format!("{body}{}{}", if body.is_empty() { "" } else { "\n" }, tools.join(", "))
                };
                ("assistant".to_string(), label, None)
            }
            ChatMessage::ToolResult { tool, content, .. } => {
                ("tool".to_string(), content.clone(), Some(tool.clone()))
            }
        };
        Self {
            seq,
            role,
            text,
            tool,
            created_at,
        }
    }
}

/// Reject an id that cannot be one of ours before it reaches SQL.
///
/// Not a security boundary (values are bound, never interpolated) but a cheap
/// way to turn a malformed path segment into a 404 instead of a query.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_CHARS
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Create a conversation and return its id.
pub async fn create(
    db: &DatabaseConnection,
    tenant_id: &str,
    user_id: &str,
    project_id: &str,
    title: &str,
) -> Result<String, ApiError> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let title = memory::title_from_prompt(title, MAX_TITLE_CHARS);
    db.execute(statement(
        "INSERT INTO agent_conversations \
         (id, tenant_id, user_id, project_id, title, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        vec![
            text(&id),
            text(tenant_id),
            text(user_id),
            text(project_id),
            text(&title),
            text(&now),
            text(&now),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(id)
}

/// One conversation, scoped by tenant and user.
///
/// A conversation belonging to somebody else is `None`, which the handler turns
/// into a 404 — never a 403, so the endpoint cannot be used to probe existence.
pub async fn get(
    db: &DatabaseConnection,
    tenant_id: &str,
    user_id: &str,
    project_id: &str,
    conversation_id: &str,
) -> Result<Option<Conversation>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT id, project_id, title, created_at, updated_at, \
             (SELECT COUNT(*) FROM agent_messages m WHERE m.conversation_id = c.id) AS message_count \
             FROM agent_conversations c \
             WHERE id = ? AND tenant_id = ? AND user_id = ? AND project_id = ?",
            vec![
                text(conversation_id),
                text(tenant_id),
                text(user_id),
                text(project_id),
            ],
        ))
        .await
        .map_err(ApiError::from)?;

    Ok(rows.first().map(|row| Conversation {
        id: row.try_get("", "id").unwrap_or_default(),
        project_id: row.try_get("", "project_id").unwrap_or_default(),
        title: row.try_get("", "title").unwrap_or_default(),
        message_count: row.try_get("", "message_count").unwrap_or(0),
        created_at: row.try_get("", "created_at").unwrap_or_default(),
        updated_at: row.try_get("", "updated_at").unwrap_or_default(),
    }))
}

/// Newest-first conversations for one project.
pub async fn list(
    db: &DatabaseConnection,
    tenant_id: &str,
    user_id: &str,
    project_id: &str,
    limit: i64,
) -> Result<Vec<Conversation>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT id, project_id, title, created_at, updated_at, \
             (SELECT COUNT(*) FROM agent_messages m WHERE m.conversation_id = c.id) AS message_count \
             FROM agent_conversations c \
             WHERE tenant_id = ? AND user_id = ? AND project_id = ? \
             ORDER BY updated_at DESC LIMIT ?",
            vec![
                text(tenant_id),
                text(user_id),
                text(project_id),
                Value::from(limit.clamp(1, 100)),
            ],
        ))
        .await
        .map_err(ApiError::from)?;

    Ok(rows
        .iter()
        .map(|row| Conversation {
            id: row.try_get("", "id").unwrap_or_default(),
            project_id: row.try_get("", "project_id").unwrap_or_default(),
            title: row.try_get("", "title").unwrap_or_default(),
            message_count: row.try_get("", "message_count").unwrap_or(0),
            created_at: row.try_get("", "created_at").unwrap_or_default(),
            updated_at: row.try_get("", "updated_at").unwrap_or_default(),
        })
        .collect())
}

/// Rename a conversation. Returns false when it is not the caller's.
pub async fn rename(
    db: &DatabaseConnection,
    tenant_id: &str,
    user_id: &str,
    project_id: &str,
    conversation_id: &str,
    title: &str,
) -> Result<bool, ApiError> {
    let title = memory::title_from_prompt(title, MAX_TITLE_CHARS);
    let result = db
        .execute(statement(
            "UPDATE agent_conversations SET title = ?, updated_at = ? \
             WHERE id = ? AND tenant_id = ? AND user_id = ? AND project_id = ?",
            vec![
                text(&title),
                text(&Utc::now().to_rfc3339()),
                text(conversation_id),
                text(tenant_id),
                text(user_id),
                text(project_id),
            ],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() > 0)
}

/// Delete a conversation and its messages.
pub async fn delete(
    db: &DatabaseConnection,
    tenant_id: &str,
    user_id: &str,
    project_id: &str,
    conversation_id: &str,
) -> Result<bool, ApiError> {
    // Messages first: the FK cascade needs `PRAGMA foreign_keys`, which is not
    // guaranteed on every connection, and an orphaned transcript would outlive
    // the retention policy that justified keeping it.
    let owned = get(db, tenant_id, user_id, project_id, conversation_id).await?;
    if owned.is_none() {
        return Ok(false);
    }
    db.execute(statement(
        "DELETE FROM agent_messages WHERE conversation_id = ?",
        vec![text(conversation_id)],
    ))
    .await
    .map_err(ApiError::from)?;
    db.execute(statement(
        "DELETE FROM agent_conversations WHERE id = ? AND tenant_id = ? AND user_id = ?",
        vec![text(conversation_id), text(tenant_id), text(user_id)],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(true)
}

/// Remove every conversation of a project.
///
/// Called by the project delete handler so a deleted project cannot leave a
/// transcript behind.
pub async fn delete_for_project(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
) -> Result<(), ApiError> {
    db.execute(statement(
        "DELETE FROM agent_messages WHERE conversation_id IN \
         (SELECT id FROM agent_conversations WHERE tenant_id = ? AND project_id = ?)",
        vec![text(tenant_id), text(project_id)],
    ))
    .await
    .map_err(ApiError::from)?;
    db.execute(statement(
        "DELETE FROM agent_conversations WHERE tenant_id = ? AND project_id = ?",
        vec![text(tenant_id), text(project_id)],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Append one message, redacted and bounded.
///
/// Returns the sequence number it was given.
pub async fn append(
    db: &DatabaseConnection,
    conversation_id: &str,
    message: &ChatMessage,
) -> Result<i64, ApiError> {
    let stored = memory::compact_tool_result(&memory::redact_message(message));
    let content = serde_json::to_string(&stored).map_err(|error| {
        tracing::error!(%error, "could not serialise a conversation message");
        ApiError::Internal
    })?;
    let now = Utc::now().to_rfc3339();

    // `MAX(seq)+1` inside one statement: the run holds the project lease, so
    // there is exactly one writer per conversation at a time.
    db.execute(statement(
        "INSERT INTO agent_messages (id, conversation_id, seq, role, content, created_at) \
         SELECT ?, ?, COALESCE(MAX(seq), 0) + 1, ?, ?, ? FROM agent_messages \
         WHERE conversation_id = ?",
        vec![
            text(&uuid::Uuid::new_v4().to_string()),
            text(conversation_id),
            text(message.role()),
            text(&content),
            text(&now),
            text(conversation_id),
        ],
    ))
    .await
    .map_err(ApiError::from)?;

    db.execute(statement(
        "UPDATE agent_conversations SET updated_at = ? WHERE id = ?",
        vec![text(&now), text(conversation_id)],
    ))
    .await
    .map_err(ApiError::from)?;

    let seq = db
        .query_all(statement(
            "SELECT COALESCE(MAX(seq), 0) AS seq FROM agent_messages WHERE conversation_id = ?",
            vec![text(conversation_id)],
        ))
        .await
        .map_err(ApiError::from)?
        .first()
        .and_then(|row| row.try_get::<i64>("", "seq").ok())
        .unwrap_or(0);
    Ok(seq)
}

/// Append a whole run's transcript, newest last.
///
/// A failure here is logged by the caller and never fails the run: the user
/// already has their answer and the project is already saved.
pub async fn append_transcript(
    db: &DatabaseConnection,
    conversation_id: &str,
    transcript: &[ChatMessage],
) -> Result<usize, ApiError> {
    let mut written = 0;
    for message in transcript {
        append(db, conversation_id, message).await?;
        written += 1;
    }
    Ok(written)
}

/// The messages of one conversation, oldest first, for the drawer.
pub async fn messages(
    db: &DatabaseConnection,
    conversation_id: &str,
    limit: i64,
) -> Result<Vec<ConversationMessage>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT seq, content, created_at FROM agent_messages \
             WHERE conversation_id = ? ORDER BY seq ASC LIMIT ?",
            vec![text(conversation_id), Value::from(limit.clamp(1, 500))],
        ))
        .await
        .map_err(ApiError::from)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let seq: i64 = row.try_get("", "seq").unwrap_or(0);
        let raw: String = row.try_get("", "content").unwrap_or_default();
        let created_at: String = row.try_get("", "created_at").unwrap_or_default();
        // A row that no longer parses is skipped rather than failing the whole
        // restore: one bad message must not hide the other forty.
        if let Ok(message) = serde_json::from_str::<ChatMessage>(&raw) {
            out.push(ConversationMessage::from_stored(seq, &message, created_at));
        }
    }
    Ok(out)
}

/// What a run needs from memory.
#[derive(Debug, Default)]
pub struct Memory {
    pub summary: Option<String>,
    pub history: Vec<ChatMessage>,
}

/// Assemble the context for the next run.
///
/// Newest-first selection, then reversed: the budget must spend on the turns
/// closest to the new prompt, because those are the ones a follow-up refers to.
pub async fn context(
    db: &DatabaseConnection,
    conversation_id: &str,
) -> Result<Memory, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT content FROM agent_messages WHERE conversation_id = ? \
             ORDER BY seq DESC LIMIT ?",
            vec![text(conversation_id), Value::from(CONTEXT_MESSAGES as i64)],
        ))
        .await
        .map_err(ApiError::from)?;

    let mut history: Vec<ChatMessage> = Vec::new();
    let mut tokens = 0usize;
    for row in &rows {
        let raw: String = row.try_get("", "content").unwrap_or_default();
        let Ok(message) = serde_json::from_str::<ChatMessage>(&raw) else {
            continue;
        };
        let cost = message.approx_tokens();
        // Always keep the most recent turn, even if it alone exceeds the
        // budget: dropping it would remove the very thing being followed up.
        if !history.is_empty() && tokens + cost > CONTEXT_TOKENS {
            break;
        }
        tokens += cost;
        history.push(message);
    }
    history.reverse();

    let summary = summary_of(db, conversation_id).await?;
    Ok(Memory { summary, history })
}

/// The stored compaction summary, if any.
pub async fn summary_of(
    db: &DatabaseConnection,
    conversation_id: &str,
) -> Result<Option<String>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT summary FROM agent_conversations WHERE id = ?",
            vec![text(conversation_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(rows
        .first()
        .and_then(|row| row.try_get::<Option<String>>("", "summary").ok().flatten())
        .filter(|summary| !summary.trim().is_empty()))
}

/// Summarise the oldest turns when the conversation has grown too large.
///
/// Returns `true` when a compaction ran. Never returns `Err` for a provider
/// problem: a summariser outage degrades to a deterministic digest and the run
/// continues (plan §15.3.3).
pub async fn compact(
    db: &DatabaseConnection,
    provider: Option<&dyn LlmProvider>,
    conversation_id: &str,
) -> Result<bool, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT seq, content FROM agent_messages WHERE conversation_id = ? ORDER BY seq ASC",
            vec![text(conversation_id)],
        ))
        .await
        .map_err(ApiError::from)?;

    let mut all: Vec<(i64, ChatMessage)> = Vec::with_capacity(rows.len());
    for row in &rows {
        let seq: i64 = row.try_get("", "seq").unwrap_or(0);
        let raw: String = row.try_get("", "content").unwrap_or_default();
        if let Ok(message) = serde_json::from_str::<ChatMessage>(&raw) {
            all.push((seq, message));
        }
    }

    let tokens: usize = all.iter().map(|(_, m)| m.approx_tokens()).sum();
    if all.len() < COMPACT_AT_MESSAGES && tokens < COMPACT_AT_TOKENS {
        return Ok(false);
    }
    // Nothing to summarise if the kept tail is the whole conversation.
    if all.len() <= KEEP_AFTER_COMPACT {
        return Ok(false);
    }

    let split = all.len() - KEEP_AFTER_COMPACT;
    let (older, kept) = all.split_at(split);
    let older_messages: Vec<ChatMessage> = older.iter().map(|(_, m)| m.clone()).collect();

    let existing = summary_of(db, conversation_id).await?;
    let summary = match provider {
        Some(provider) => summarise(provider, existing.as_deref(), &older_messages)
            .await
            .unwrap_or_else(|| memory::digest(&older_messages)),
        None => memory::digest(&older_messages),
    };

    let first_kept = kept.first().map(|(seq, _)| *seq).unwrap_or(i64::MAX);
    db.execute(statement(
        "DELETE FROM agent_messages WHERE conversation_id = ? AND seq < ?",
        vec![text(conversation_id), Value::from(first_kept)],
    ))
    .await
    .map_err(ApiError::from)?;
    db.execute(statement(
        "UPDATE agent_conversations SET summary = ?, updated_at = ? WHERE id = ?",
        vec![
            text(&summary),
            text(&Utc::now().to_rfc3339()),
            text(conversation_id),
        ],
    ))
    .await
    .map_err(ApiError::from)?;

    Ok(true)
}

/// One cheap completion that turns old turns into decisions, names and
/// rejected options.
///
/// `None` on any failure, which sends the caller to the digest fallback.
async fn summarise(
    provider: &dyn LlmProvider,
    existing: Option<&str>,
    messages: &[ChatMessage],
) -> Option<String> {
    let mut transcript = String::new();
    for message in messages {
        match message {
            ChatMessage::User(text) => {
                transcript.push_str("USER: ");
                transcript.push_str(&memory::title_from_prompt(text, 400));
                transcript.push('\n');
            }
            ChatMessage::Assistant { text, tool_calls } => {
                if let Some(text) = text {
                    transcript.push_str("ASSISTANT: ");
                    transcript.push_str(&memory::title_from_prompt(text, 400));
                    transcript.push('\n');
                }
                for call in tool_calls {
                    transcript.push_str("TOOL: ");
                    transcript.push_str(&call.name);
                    transcript.push('\n');
                }
            }
            ChatMessage::ToolResult { tool, .. } => {
                transcript.push_str("RESULT: ");
                transcript.push_str(tool);
                transcript.push('\n');
            }
        }
    }
    // Bound the input: a summary prompt must not itself become the largest
    // thing in the window.
    if transcript.chars().count() > 24_000 {
        transcript = transcript.chars().take(24_000).collect();
    }

    let mut prompt = String::new();
    if let Some(existing) = existing {
        prompt.push_str("Summary so far:\n");
        prompt.push_str(existing);
        prompt.push_str("\n\n");
    }
    prompt.push_str(
        "Newer turns to fold in:\n",
    );
    prompt.push_str(&transcript);
    prompt.push_str(
        "\n\nRewrite the summary in at most 400 tokens. Keep: decisions taken, names chosen, \
         objects created or renamed, and options explicitly rejected. Drop pleasantries and \
         tool mechanics. Write plain prose, no headings.",
    );

    let response = provider
        .complete(ChatRequest {
            system: "You compress a conversation between a user and an app-building assistant \
                     into a durable memory. Reply with the summary only."
                .to_string(),
            messages: vec![ChatMessage::User(prompt)],
            tools: Vec::new(),
            max_tokens: 400,
            temperature: 0.0,
        })
        .await
        .ok()?;

    response
        .text
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use sea_orm::ConnectionTrait;

    use nocode_agent::error::AgentError;
    use nocode_agent::events::Usage;
    use nocode_agent::llm::{ChatResponse, StopReason};

    /// A provider that returns a fixed line, or fails — for the compaction
    /// paths, where the only thing that matters is whether a summary arrived.
    struct Stub {
        reply: Result<&'static str, ()>,
    }

    #[async_trait]
    impl LlmProvider for Stub {
        fn name(&self) -> &'static str {
            "stub"
        }

        fn model(&self) -> &str {
            "stub-1"
        }

        async fn complete(&self, _request: ChatRequest) -> Result<ChatResponse, AgentError> {
            match self.reply {
                Ok(text) => Ok(ChatResponse {
                    text: Some(text.to_string()),
                    tool_calls: Vec::new(),
                    usage: Usage::default(),
                    stop_reason: StopReason::EndTurn,
                }),
                Err(()) => Err(AgentError::Provider {
                    provider: "stub",
                    message: "down".into(),
                }),
            }
        }
    }

    /// A database with the tenant, user and project rows a conversation needs.
    async fn db() -> (DatabaseConnection, String) {
        let db = crate::db::connect_sqlite_memory().await.expect("db");
        db.execute_unprepared(
            "INSERT OR IGNORE INTO tenants (id, name, slug) VALUES ('t1', 'Tenant', 'tenant')",
        )
        .await
        .unwrap();
        db.execute_unprepared(
            "INSERT OR IGNORE INTO users (id, email, password_hash) VALUES ('u1', 'u1@example.com', 'x')",
        )
        .await
        .unwrap();
        let project_id = uuid::Uuid::new_v4().to_string();
        db.execute_unprepared(&format!(
            "INSERT INTO projects (id, tenant_id, user_id, name, slug, version, data) \
             VALUES ('{project_id}', 't1', 'u1', 'App', 'app', '1.0', '{{}}')"
        ))
        .await
        .unwrap();
        (db, project_id)
    }

    async fn conversation_with(db: &DatabaseConnection, project_id: &str, count: usize) -> String {
        let id = create(db, "t1", "u1", project_id, "Build an expense tracker")
            .await
            .expect("create");
        for index in 0..count {
            let message = if index % 2 == 0 {
                ChatMessage::User(format!("turn {index}"))
            } else {
                ChatMessage::Assistant {
                    text: Some(format!("reply {index}")),
                    tool_calls: Vec::new(),
                }
            };
            append(db, &id, &message).await.expect("append");
        }
        id
    }

    #[tokio::test]
    async fn ids_are_checked_before_they_reach_a_query() {
        assert!(valid_id("2f8a1c4e-0000-4000-8000-000000000000"));
        assert!(!valid_id(""));
        assert!(!valid_id("has space"));
        assert!(!valid_id(&"x".repeat(MAX_ID_CHARS + 1)));
    }

    #[test]
    fn a_tool_result_flattens_to_its_tool_and_text() {
        let message = ChatMessage::ToolResult {
            call_id: "c".into(),
            tool: "entity_create".into(),
            content: "{\"entity_id\":\"e1\"}".into(),
        };
        let flat = ConversationMessage::from_stored(3, &message, "now".into());
        assert_eq!(flat.role, "tool");
        assert_eq!(flat.tool.as_deref(), Some("entity_create"));
        assert_eq!(flat.seq, 3);
    }

    #[test]
    fn an_assistant_turn_names_the_tools_it_called() {
        let message = ChatMessage::Assistant {
            text: Some("Creating the entity".into()),
            tool_calls: vec![nocode_agent::llm::ToolCall {
                id: "c".into(),
                name: "entity_create".into(),
                arguments: serde_json::json!({}),
            }],
        };
        let flat = ConversationMessage::from_stored(1, &message, "now".into());
        assert!(flat.text.contains("Creating the entity"));
        assert!(flat.text.contains("entity_create"));
    }

    #[tokio::test]
    async fn a_credential_in_a_tool_argument_is_not_stored() {
        let (db, project_id) = db().await;
        let id = create(&db, "t1", "u1", &project_id, "Connect to Stripe")
            .await
            .unwrap();

        let message = ChatMessage::Assistant {
            text: None,
            tool_calls: vec![nocode_agent::llm::ToolCall {
                id: "call_1".into(),
                name: "connector_create".into(),
                arguments: serde_json::json!({
                    "kind": "http_rest",
                    "config": { "api_key": "sk-live-123", "url": "https://example.test" }
                }),
            }],
        };
        append(&db, &id, &message).await.unwrap();

        let rows = db
            .query_all(statement(
                "SELECT content FROM agent_messages WHERE conversation_id = ?",
                vec![text(&id)],
            ))
            .await
            .unwrap();
        let stored: String = rows[0].try_get("", "content").unwrap();
        assert!(!stored.contains("sk-live-123"), "{stored}");
        assert!(stored.contains("redacted"), "{stored}");
        // The non-secret sibling survives, so the transcript stays useful.
        assert!(stored.contains("example.test"), "{stored}");
    }

    #[tokio::test]
    async fn a_restore_returns_the_messages_in_order() {
        let (db, project_id) = db().await;
        let id = conversation_with(&db, &project_id, 4).await;

        let restored = messages(&db, &id, 200).await.unwrap();
        assert_eq!(restored.len(), 4);
        assert_eq!(restored[0].role, "user");
        assert_eq!(restored[0].text, "turn 0");
        assert_eq!(restored[1].role, "assistant");
        assert!(restored[1].text.contains("reply 1"));
        assert_eq!(restored[3].seq, 4);
    }

    #[tokio::test]
    async fn a_conversation_belonging_to_someone_else_is_invisible() {
        let (db, project_id) = db().await;
        let id = conversation_with(&db, &project_id, 2).await;

        assert!(get(&db, "t1", "u1", &project_id, &id).await.unwrap().is_some());
        // Another user of the same tenant: not found, never forbidden.
        assert!(get(&db, "t1", "someone-else", &project_id, &id)
            .await
            .unwrap()
            .is_none());
        // The right user, the wrong project.
        assert!(get(&db, "t1", "u1", "another-project", &id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn compaction_summarises_the_oldest_turns_and_keeps_the_tail() {
        let (db, project_id) = db().await;
        let id = conversation_with(&db, &project_id, 41).await;

        let stub = Stub {
            reply: Ok("The user chose Expense as the entity name."),
        };
        let ran = compact(&db, Some(&stub), &id).await.expect("compact");
        assert!(ran, "41 messages must trigger compaction");

        let remaining = messages(&db, &id, 500).await.unwrap();
        assert_eq!(remaining.len(), KEEP_AFTER_COMPACT, "only the tail is kept");
        assert_eq!(remaining[0].seq as usize, 41 - KEEP_AFTER_COMPACT + 1);

        let summary = summary_of(&db, &id).await.unwrap().expect("a summary");
        assert!(summary.contains("Expense"), "{summary}");
    }

    #[tokio::test]
    async fn a_failing_summariser_falls_back_to_a_digest() {
        let (db, project_id) = db().await;
        let id = conversation_with(&db, &project_id, 41).await;

        let stub = Stub { reply: Err(()) };
        let ran = compact(&db, Some(&stub), &id).await.expect("compact");
        assert!(ran, "a provider outage must not stop compaction");

        let summary = summary_of(&db, &id).await.unwrap().expect("a summary");
        assert!(summary.contains("digest"), "{summary}");
        // The run still has its tail, which is what the next turn reads.
        assert_eq!(messages(&db, &id, 500).await.unwrap().len(), KEEP_AFTER_COMPACT);
    }

    #[tokio::test]
    async fn a_short_conversation_is_not_compacted() {
        let (db, project_id) = db().await;
        let id = conversation_with(&db, &project_id, 5).await;

        let stub = Stub {
            reply: Ok("unused"),
        };
        assert!(!compact(&db, Some(&stub), &id).await.unwrap());
        assert!(summary_of(&db, &id).await.unwrap().is_none());
        assert_eq!(messages(&db, &id, 500).await.unwrap().len(), 5);
    }

    #[tokio::test]
    async fn deleting_a_conversation_removes_its_messages() {
        let (db, project_id) = db().await;
        let id = conversation_with(&db, &project_id, 3).await;

        assert!(delete(&db, "t1", "u1", &project_id, &id).await.unwrap());
        assert!(get(&db, "t1", "u1", &project_id, &id).await.unwrap().is_none());
        assert!(messages(&db, &id, 100).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_a_project_removes_its_conversations() {
        let (db, project_id) = db().await;
        let id = conversation_with(&db, &project_id, 3).await;

        delete_for_project(&db, "t1", &project_id).await.unwrap();
        assert!(get(&db, "t1", "u1", &project_id, &id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn context_is_newest_first_selection_reversed() {
        let (db, project_id) = db().await;
        let id = conversation_with(&db, &project_id, 30).await;

        let memory = context(&db, &id).await.unwrap();
        assert_eq!(memory.history.len(), CONTEXT_MESSAGES);
        // Oldest-first for the prompt: the last element is the newest turn.
        let last = memory.history.last().expect("a turn");
        assert!(matches!(last, ChatMessage::Assistant { text: Some(text), .. } if text == "reply 29"));
    }
}
