//! Versioned, idempotent schema migrations applied through the SeaORM connection.
//!
//! Each migration is applied once and recorded in `schema_migrations`. The DDL is
//! written for SQLite (the current IDE database); revisit when Postgres becomes
//! the production store.

use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};

/// Ordered `(version, sql)` pairs. Never edit or reorder an applied entry; append
/// new migrations instead.
const MIGRATIONS: &[(&str, &str)] = &[("0001_initial", INITIAL_SCHEMA)];

const INITIAL_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id            TEXT PRIMARY KEY NOT NULL,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    is_active     INTEGER NOT NULL DEFAULT 1,
    is_verified   INTEGER NOT NULL DEFAULT 0,
    last_login_at TEXT,
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users(email);

CREATE TABLE IF NOT EXISTS projects (
    id          TEXT PRIMARY KEY NOT NULL,
    user_id     TEXT NOT NULL,
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL,
    description TEXT,
    version     TEXT NOT NULL,
    data        TEXT NOT NULL,
    revision    INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_projects_user_id ON projects(user_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_projects_user_slug ON projects(user_id, slug);

CREATE TABLE IF NOT EXISTS refresh_tokens (
    id         TEXT PRIMARY KEY NOT NULL,
    user_id    TEXT NOT NULL,
    jti        TEXT NOT NULL UNIQUE,
    token_hash TEXT NOT NULL,
    user_agent TEXT,
    ip_address TEXT,
    expires_at TEXT NOT NULL,
    revoked_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user_id ON refresh_tokens(user_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_refresh_tokens_jti ON refresh_tokens(jti);
"#;

/// Apply every migration that has not yet been recorded.
pub async fn run(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        "CREATE TABLE IF NOT EXISTS schema_migrations (\
         version TEXT PRIMARY KEY NOT NULL, \
         applied_at TEXT NOT NULL DEFAULT (datetime('now')))",
    )
    .await
    .context("failed to create schema_migrations table")?;

    for (version, sql) in MIGRATIONS {
        if is_applied(db, version).await? {
            continue;
        }
        for statement in split_statements(sql) {
            db.execute_unprepared(&statement)
                .await
                .with_context(|| format!("migration {version} failed on: {statement}"))?;
        }
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO schema_migrations (version) VALUES (?)",
            [Value::from(*version)],
        ))
        .await
        .with_context(|| format!("failed to record migration {version}"))?;
        tracing::info!(version, "applied migration");
    }

    Ok(())
}

async fn is_applied(db: &DatabaseConnection, version: &str) -> Result<bool> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM schema_migrations WHERE version = ?",
            [Value::from(version)],
        ))
        .await
        .context("failed to read schema_migrations")?;
    Ok(!rows.is_empty())
}

/// Split a migration script into individual statements on `;`.
fn split_statements(sql: &str) -> Vec<String> {
    sql.split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_sqlite_memory;

    async fn table_names(db: &DatabaseConnection) -> Vec<String> {
        db.query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect()
    }

    #[tokio::test]
    async fn creates_expected_tables() {
        let db = connect_sqlite_memory().await.unwrap();
        let names = table_names(&db).await;
        assert!(names.contains(&"users".to_string()), "missing users: {names:?}");
        assert!(
            names.contains(&"projects".to_string()),
            "missing projects: {names:?}"
        );
        assert!(
            names.contains(&"refresh_tokens".to_string()),
            "missing refresh_tokens: {names:?}"
        );
        assert!(
            names.contains(&"schema_migrations".to_string()),
            "missing schema_migrations: {names:?}"
        );
    }

    #[tokio::test]
    async fn run_is_idempotent() {
        let db = connect_sqlite_memory().await.unwrap();
        run(&db).await.unwrap();
        let count = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT version FROM schema_migrations",
            ))
            .await
            .unwrap()
            .len();
        assert_eq!(count, MIGRATIONS.len(), "migrations recorded more than once");
    }
}
