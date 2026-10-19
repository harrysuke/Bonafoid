//! Raw-SQL persistence for users and refresh tokens.
//!
//! Queries use SeaORM's [`Statement`] + [`QueryResult`] interfaces directly (the
//! same pattern proven in `migration.rs`) instead of the entity /
//! `FromQueryResult` derive surface. Booleans are stored as SQLite integers and
//! decoded with `!= 0`.

use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, Value};
use serde::Serialize;

/// Columns selected when loading a user row.
const USER_COLUMNS: &str = "id, email, password_hash, is_active, is_verified, created_at";

/// A row of the `users` table.
#[derive(Debug, Clone, Serialize)]
pub struct UserRow {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub is_active: bool,
    pub is_verified: bool,
    pub created_at: String,
}

/// A row of the `refresh_tokens` table (projection used for verification).
#[derive(Debug, Clone)]
pub struct RefreshRow {
    pub user_id: String,
    pub token_hash: String,
    pub revoked: bool,
}

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

fn text(value: &str) -> Value {
    Value::from(value.to_string())
}

fn map_user(row: &QueryResult) -> Result<UserRow> {
    Ok(UserRow {
        id: row.try_get("", "id").context("users.id")?,
        email: row.try_get("", "email").context("users.email")?,
        password_hash: row
            .try_get("", "password_hash")
            .context("users.password_hash")?,
        is_active: row
            .try_get::<i64>("", "is_active")
            .context("users.is_active")?
            != 0,
        is_verified: row
            .try_get::<i64>("", "is_verified")
            .context("users.is_verified")?
            != 0,
        created_at: row.try_get("", "created_at").context("users.created_at")?,
    })
}

/// Insert a new user. `created_at` / `updated_at` are defaulted in SQL.
pub async fn insert_user(
    db: &DatabaseConnection,
    id: &str,
    email: &str,
    password_hash: &str,
) -> Result<()> {
    db.execute(statement(
        "INSERT INTO users (id, email, password_hash) VALUES (?, ?, ?)",
        vec![text(id), text(email), text(password_hash)],
    ))
    .await
    .context("failed to insert user")?;
    Ok(())
}

/// Find a user by email. Callers normalize the address first.
pub async fn find_user_by_email(db: &DatabaseConnection, email: &str) -> Result<Option<UserRow>> {
    let sql = format!("SELECT {USER_COLUMNS} FROM users WHERE email = ?");
    let rows = db
        .query_all(statement(&sql, vec![text(email)]))
        .await
        .context("failed to query user by email")?;
    rows.first().map(map_user).transpose()
}

/// Find a user by id.
pub async fn find_user_by_id(db: &DatabaseConnection, id: &str) -> Result<Option<UserRow>> {
    let sql = format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?");
    let rows = db
        .query_all(statement(&sql, vec![text(id)]))
        .await
        .context("failed to query user by id")?;
    rows.first().map(map_user).transpose()
}

/// Record a successful login timestamp.
pub async fn update_last_login(db: &DatabaseConnection, id: &str) -> Result<()> {
    db.execute(statement(
        "UPDATE users SET last_login_at = datetime('now'), updated_at = datetime('now') WHERE id = ?",
        vec![text(id)],
    ))
    .await
    .context("failed to update last_login_at")?;
    Ok(())
}

/// Persist a refresh token. `expires_at` is an RFC 3339 timestamp.
pub async fn insert_refresh_token(
    db: &DatabaseConnection,
    id: &str,
    user_id: &str,
    jti: &str,
    token_hash: &str,
    expires_at: &str,
) -> Result<()> {
    db.execute(statement(
        "INSERT INTO refresh_tokens (id, user_id, jti, token_hash, expires_at) \
         VALUES (?, ?, ?, ?, ?)",
        vec![
            text(id),
            text(user_id),
            text(jti),
            text(token_hash),
            text(expires_at),
        ],
    ))
    .await
    .context("failed to insert refresh token")?;
    Ok(())
}

/// Look up a refresh token by its `jti`.
pub async fn find_refresh_token_by_jti(
    db: &DatabaseConnection,
    jti: &str,
) -> Result<Option<RefreshRow>> {
    let rows = db
        .query_all(statement(
            "SELECT user_id, token_hash, (revoked_at IS NOT NULL) AS revoked \
             FROM refresh_tokens WHERE jti = ?",
            vec![text(jti)],
        ))
        .await
        .context("failed to query refresh token")?;
    rows.first()
        .map(|row| -> Result<RefreshRow> {
            Ok(RefreshRow {
                user_id: row
                    .try_get("", "user_id")
                    .context("refresh_tokens.user_id")?,
                token_hash: row
                    .try_get("", "token_hash")
                    .context("refresh_tokens.token_hash")?,
                revoked: row
                    .try_get::<i64>("", "revoked")
                    .context("refresh_tokens.revoked")?
                    != 0,
            })
        })
        .transpose()
}

/// Mark a refresh token as revoked (idempotent).
pub async fn revoke_refresh_token(db: &DatabaseConnection, jti: &str) -> Result<()> {
    db.execute(statement(
        "UPDATE refresh_tokens SET revoked_at = datetime('now') \
         WHERE jti = ? AND revoked_at IS NULL",
        vec![text(jti)],
    ))
    .await
    .context("failed to revoke refresh token")?;
    Ok(())
}
