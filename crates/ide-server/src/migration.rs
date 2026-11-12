//! Versioned, idempotent schema migrations applied through the SeaORM connection.
//!
//! Each migration is applied once and recorded in `schema_migrations`. The DDL is
//! written for SQLite (the current IDE database); revisit when Postgres becomes
//! the production store.

use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};

/// Ordered `(version, sql)` pairs. Never edit or reorder an applied entry; append
/// new migrations instead.
const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_initial", INITIAL_SCHEMA),
    ("0002_plugins", PLUGINS_SCHEMA),
    ("0003_integrations", INTEGRATIONS_SCHEMA),
];

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

/// The plugins a user has installed.
///
/// The manifest is stored as JSON; the WebAssembly module lives on disk
/// next to it, because a module is large and is never queried. The primary
/// key is (user_id, id), so two users can install a plugin with the same
/// id and neither sees the other's.
const PLUGINS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS plugins (
    id         TEXT NOT NULL,
    user_id    TEXT NOT NULL,
    manifest   TEXT NOT NULL,
    enabled    INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_plugins_user_id ON plugins(user_id);
"#;

/// Integration credentials and deployment history (Phase 14).
///
/// Secret discipline is enforced by the schema: `config_json` is the ONLY
/// freely-readable config column and callers must write redacted JSON into
/// it. PATs and registry tokens live exclusively in the sealed columns,
/// encrypted with the server master key. Nothing in this schema can be
/// selected into an API response without an explicit, deliberate read.
///
/// The partial unique index gives idempotency at the storage layer: two
/// successful/in-flight deployments of the same commit for the same project
/// and user cannot coexist, while a failed attempt may be retried.
const INTEGRATIONS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS integrations (
    user_id           TEXT NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('github', 'dockerhub')),
    config_json       TEXT NOT NULL,
    sealed_nonce      TEXT,
    sealed_ciphertext TEXT,
    verified          INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, kind),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_integrations_user_id ON integrations(user_id);

CREATE TABLE IF NOT EXISTS deployments (
    id              TEXT PRIMARY KEY NOT NULL,
    user_id         TEXT NOT NULL,
    project_id      TEXT NOT NULL,
    commit_sha      TEXT NOT NULL,
    branch          TEXT NOT NULL,
    status          TEXT NOT NULL,
    github_run_id   INTEGER,
    github_run_url  TEXT,
    error_message   TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_deployments_user_project ON deployments(user_id, project_id, created_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_deployments_active_commit
    ON deployments(user_id, project_id, commit_sha)
    WHERE status NOT IN ('failed', 'ci_failed');
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
        assert!(
            names.contains(&"users".to_string()),
            "missing users: {names:?}"
        );
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
        assert!(
            names.contains(&"plugins".to_string()),
            "missing plugins: {names:?}"
        );
        assert!(
            names.contains(&"integrations".to_string()),
            "missing integrations: {names:?}"
        );
        assert!(
            names.contains(&"deployments".to_string()),
            "missing deployments: {names:?}"
        );
    }

    /// The sealed columns must exist and be separate from the readable
    /// config column, and the idempotency index must be a *partial* one.
    #[tokio::test]
    async fn integration_schema_keeps_secrets_in_sealed_columns() {
        let db = connect_sqlite_memory().await.unwrap();

        let columns: Vec<String> = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(integrations)",
            ))
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.try_get::<String>("", "name").unwrap())
            .collect();
        for expected in [
            "user_id",
            "kind",
            "config_json",
            "sealed_nonce",
            "sealed_ciphertext",
            "verified",
        ] {
            assert!(
                columns.contains(&expected.to_string()),
                "integrations is missing column {expected}: {columns:?}"
            );
        }

        let index_sql: String = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT sql FROM sqlite_master WHERE name = 'idx_deployments_active_commit'",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<String>("", "sql")
            .unwrap();
        assert!(index_sql.contains("UNIQUE INDEX"), "index: {index_sql}");
        assert!(
            index_sql.contains("WHERE"),
            "index must be partial: {index_sql}"
        );
    }

    async fn seed_user_and_project(db: &DatabaseConnection) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO users (id, email, password_hash) VALUES (?, ?, ?)",
            [
                Value::from("u1"),
                Value::from("u1@example.com"),
                Value::from("hash"),
            ],
        ))
        .await
        .unwrap();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO projects (id, user_id, name, slug, version, data) VALUES (?, ?, ?, ?, ?, ?)",
            [
                Value::from("p1"),
                Value::from("u1"),
                Value::from("Project"),
                Value::from("project"),
                Value::from("1"),
                Value::from("{}"),
            ],
        ))
        .await
        .unwrap();
    }

    async fn insert_deployment(db: &DatabaseConnection, id: &str, status: &str) -> bool {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO deployments (id, user_id, project_id, commit_sha, branch, status) VALUES (?, ?, ?, ?, ?, ?)",
            [
                Value::from(id),
                Value::from("u1"),
                Value::from("p1"),
                Value::from("same-commit-sha"),
                Value::from("main"),
                Value::from(status),
            ],
        ))
        .await
        .is_ok()
    }

    /// A second push of the same generated commit must be refused at the
    /// storage layer, but a failed attempt has to be retryable.
    #[tokio::test]
    async fn duplicate_active_commit_is_rejected_but_failures_are_retryable() {
        let db = connect_sqlite_memory().await.unwrap();
        seed_user_and_project(&db).await;

        assert!(insert_deployment(&db, "d1", "pushed").await);
        assert!(!insert_deployment(&db, "d2", "pushed").await);
        assert!(!insert_deployment(&db, "d3", "ci_succeeded").await);
        assert!(insert_deployment(&db, "d4", "failed").await);
        assert!(insert_deployment(&db, "d5", "failed").await);
        assert!(insert_deployment(&db, "d6", "ci_failed").await);
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
        assert_eq!(
            count,
            MIGRATIONS.len(),
            "migrations recorded more than once"
        );
    }
}
