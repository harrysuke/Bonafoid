//! Audit trail for agent runs (migration 0010) and conversation memory
//! (migration 0011).
//!
//! `agent_runs` is one row per run, written when the run starts and completed
//! when it ends. The row is what an operator needs to answer "who spent tokens
//! on what, and did it save": the prompt, the mode, the outcome, the revision,
//! the token counts and the error.
//!
//! `agent_conversations` and `agent_messages` are the transcript (plan §15.3).
//! They store the **normalized** `ChatMessage`, never a provider's raw payload:
//! a raw payload differs per vendor and would pin the table to a provider
//! version. What stays absent from `agent_runs` is the API key, which must
//! never be written anywhere.

/// DDL for the `agent_runs` table.
pub const AGENT_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS agent_runs (
    id          TEXT PRIMARY KEY NOT NULL,
    tenant_id   TEXT NOT NULL,
    project_id  TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    mode        TEXT NOT NULL,
    prompt      TEXT NOT NULL,
    status      TEXT NOT NULL,
    saved       INTEGER NOT NULL DEFAULT 0,
    revision    INTEGER,
    tokens_in   INTEGER NOT NULL DEFAULT 0,
    tokens_out  INTEGER NOT NULL DEFAULT 0,
    tool_calls  INTEGER NOT NULL DEFAULT 0,
    turns       INTEGER NOT NULL DEFAULT 0,
    error       TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_agent_runs_project
    ON agent_runs(tenant_id, project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_agent_runs_user
    ON agent_runs(tenant_id, user_id, created_at);
"#;

/// DDL for conversation memory (migration 0011).
///
/// `project_id` carries `ON DELETE CASCADE` so deleting a project takes its
/// conversations with it — but SQLite only enforces that with
/// `PRAGMA foreign_keys = ON`, so the delete path in `projects` also removes
/// them explicitly rather than relying on the pragma.
///
/// `agent_runs.conversation_id` is added by the same migration so an audit row
/// can be traced to its turn. It is nullable: rows written before this
/// migration, and runs from a host with no conversation store, have no turn.
pub const CONVERSATION_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS agent_conversations (
    id          TEXT PRIMARY KEY NOT NULL,
    tenant_id   TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    project_id  TEXT NOT NULL,
    title       TEXT NOT NULL,
    summary     TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_agent_conversations_project
    ON agent_conversations(tenant_id, project_id, updated_at);

CREATE TABLE IF NOT EXISTS agent_messages (
    id              TEXT PRIMARY KEY NOT NULL,
    conversation_id TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    role            TEXT NOT NULL,
    content         TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES agent_conversations(id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_messages_seq
    ON agent_messages(conversation_id, seq);

ALTER TABLE agent_runs ADD COLUMN conversation_id TEXT;
"#;
