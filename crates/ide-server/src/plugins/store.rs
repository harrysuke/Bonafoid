//! Raw-SQL persistence for installed plugins.
//!
//! The `plugins` table records what a user has installed and whether it is
//! enabled. The manifest itself is stored as JSON in the `manifest` column;
//! the WebAssembly module lives on disk next to it, because a module is
//! large and is never queried.

use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, Value,
};

use crate::error::ApiError;

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn text(value: &str) -> Value {
    Value::from(value.to_string())
}

/// One installed plugin.
#[derive(Debug, Clone)]
pub struct PluginRow {
    pub id: String,
    pub manifest: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

fn map_row(row: &QueryResult) -> Result<PluginRow, ApiError> {
    Ok(PluginRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        manifest: row.try_get("", "manifest").map_err(ApiError::from)?,
        enabled: row.try_get::<i64>("", "enabled").map_err(ApiError::from)? != 0,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
        updated_at: row.try_get("", "updated_at").map_err(ApiError::from)?,
    })
}

/// Every plugin installed by `user_id`, oldest first.
pub async fn list_plugins(
    db: &DatabaseConnection,
    user_id: &str,
) -> Result<Vec<PluginRow>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT id, manifest, enabled, created_at, updated_at FROM plugins \
             WHERE user_id = ? ORDER BY created_at ASC, id ASC",
            vec![text(user_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_row).collect()
}

/// One plugin installed by `user_id`.
pub async fn get_plugin(
    db: &DatabaseConnection,
    user_id: &str,
    id: &str,
) -> Result<Option<PluginRow>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT id, manifest, enabled, created_at, updated_at FROM plugins \
             WHERE user_id = ? AND id = ?",
            vec![text(user_id), text(id)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.first().map(map_row).transpose()
}

/// Record a newly installed plugin.
pub async fn insert_plugin(
    db: &DatabaseConnection,
    user_id: &str,
    id: &str,
    manifest: &str,
) -> Result<(), ApiError> {
    db.execute(statement(
        "INSERT INTO plugins (id, user_id, manifest, enabled) VALUES (?, ?, ?, 1)",
        vec![text(id), text(user_id), text(manifest)],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Turn a plugin on or off.
pub async fn set_enabled(
    db: &DatabaseConnection,
    user_id: &str,
    id: &str,
    enabled: bool,
) -> Result<bool, ApiError> {
    let result = db
        .execute(statement(
            "UPDATE plugins SET enabled = ?, updated_at = datetime('now') \
             WHERE user_id = ? AND id = ?",
            vec![
                Value::from(if enabled { 1i64 } else { 0i64 }),
                text(user_id),
                text(id),
            ],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() > 0)
}

/// Forget a plugin.
pub async fn delete_plugin(
    db: &DatabaseConnection,
    user_id: &str,
    id: &str,
) -> Result<bool, ApiError> {
    let result = db
        .execute(statement(
            "DELETE FROM plugins WHERE user_id = ? AND id = ?",
            vec![text(user_id), text(id)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() > 0)
}
