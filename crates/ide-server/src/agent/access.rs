//! Owner allowlist for the assistant.
//!
//! The migrated credentials are sealed at rest, but a sealed key is still
//! spendable by anyone the server lets reach the agent endpoint. This module
//! is the single gate: a user may run a turn, list models or read conversation
//! history only when their **signed-in user id** has an enabled row in
//! `agent_key_access`.
//!
//! The allowlist is keyed by id, not by email: emails can be changed and an
//! `AuthUser.email` is only a signed claim convenience. The table is written
//! by the operator (or the migration script), never by a public endpoint.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value};

use crate::auth::AuthUser;
use crate::error::ApiError;

const DENIED: &str = "the AI assistant is restricted to its owner";

/// Require that this authenticated user may use the assistant.
pub async fn require_owner(db: &DatabaseConnection, user: &AuthUser) -> Result<(), ApiError> {
    let statement = Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT 1 FROM agent_key_access WHERE user_id = ? AND enabled = 1 LIMIT 1",
        vec![Value::from(user.id.clone())],
    );
    let allowed = db
        .query_all(statement)
        .await
        .map_err(|error| {
            tracing::error!(%error, "could not check agent owner allowlist");
            ApiError::Internal
        })?
        .into_iter()
        .next()
        .is_some();

    if allowed {
        Ok(())
    } else {
        Err(ApiError::Forbidden(DENIED.into()))
    }
}

/// Whether a user id is on the allowlist. Used by local tooling at startup;
/// requests must still go through [`require_owner`].
pub async fn is_owner(db: &DatabaseConnection, user_id: &str) -> Result<bool, ApiError> {
    let statement = Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT 1 FROM agent_key_access WHERE user_id = ? AND enabled = 1 LIMIT 1",
        vec![Value::from(user_id.to_string())],
    );
    Ok(db
        .query_all(statement)
        .await
        .map_err(|error| {
            tracing::error!(%error, "could not check agent owner allowlist");
            ApiError::Internal
        })?
        .into_iter()
        .next()
        .is_some())
}
