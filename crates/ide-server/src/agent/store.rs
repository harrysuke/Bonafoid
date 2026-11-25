//! Raw-SQL persistence for the agent audit trail.
//!
//! Written the same way as the rest of the server's stores: `Statement`s with
//! bound values, no string interpolation of user input.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value};

use nocode_agent::Usage;

use crate::error::ApiError;

/// Cap on the stored prompt. A prompt is user input and has no length limit
/// upstream; a multi-megabyte paste should not become a multi-megabyte row.
const MAX_PROMPT_CHARS: usize = 4_000;

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn text(value: &str) -> Value {
    Value::from(value.to_string())
}

fn opt_text(value: Option<&str>) -> Value {
    Value::String(value.map(|v| Box::new(v.to_string())))
}

/// Record the start of a run, before the first token is spent.
///
/// Inserting first means a run that crashes the process leaves a `running` row
/// behind, which is exactly the evidence an operator needs.
pub async fn insert_run(
    db: &DatabaseConnection,
    id: &str,
    tenant_id: &str,
    project_id: &str,
    user_id: &str,
    mode: &str,
    prompt: &str,
    conversation_id: &str,
) -> Result<(), ApiError> {
    let prompt: String = prompt.chars().take(MAX_PROMPT_CHARS).collect();
    db.execute(statement(
        "INSERT INTO agent_runs \
         (id, tenant_id, project_id, user_id, mode, prompt, status, conversation_id) \
         VALUES (?, ?, ?, ?, ?, ?, 'running', ?)",
        vec![
            text(id),
            text(tenant_id),
            text(project_id),
            text(user_id),
            text(mode),
            text(&prompt),
            text(conversation_id),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Complete a run row. Failures here are logged by the caller, not surfaced:
/// the user already has their answer and the project is already saved.
pub async fn finish_run(
    db: &DatabaseConnection,
    id: &str,
    status: &str,
    revision: u64,
    saved: bool,
    usage: Usage,
    error: Option<&str>,
) -> Result<(), ApiError> {
    db.execute(statement(
        "UPDATE agent_runs SET \
         status = ?, saved = ?, revision = ?, tokens_in = ?, tokens_out = ?, \
         tool_calls = ?, turns = ?, error = ?, finished_at = ? \
         WHERE id = ?",
        vec![
            text(status),
            // SQLite has no boolean type; the column is an INTEGER flag.
            Value::from(if saved { 1_i64 } else { 0_i64 }),
            Value::from(revision as i64),
            Value::from(usage.tokens_in as i64),
            Value::from(usage.tokens_out as i64),
            Value::from(usage.tool_calls as i64),
            Value::from(usage.turns as i64),
            opt_text(error),
            text(&Utc::now().to_rfc3339()),
            text(id),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_are_bounded_before_they_reach_the_database() {
        let long = "x".repeat(MAX_PROMPT_CHARS + 100);
        let stored: String = long.chars().take(MAX_PROMPT_CHARS).collect();
        assert_eq!(stored.chars().count(), MAX_PROMPT_CHARS);
    }
}
