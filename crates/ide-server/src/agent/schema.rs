//! Audit trail for agent runs (migration 0010).
//!
//! One row per run, written when the run starts and completed when it ends. The
//! row is what an operator needs to answer "who spent tokens on what, and did it
//! save": the prompt, the mode, the outcome, the revision, the token counts and
//! the error.
//!
//! Deliberately *not* stored: the model's messages, the tool arguments and the
//! API key. The first two are unbounded and contain user data; the third must
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
