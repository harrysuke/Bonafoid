//! Database connection helpers for the IDE server.

use anyhow::{Context, Result};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};

/// Default database URL (SQLite file next to the process working directory).
pub fn default_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://nocode-ide.db?mode=rwc".to_string())
}

/// Open a database connection and run pending migrations.
pub async fn connect(url: &str) -> Result<DatabaseConnection> {
    let db = Database::connect(url)
        .await
        .with_context(|| format!("failed to connect to database: {url}"))?;
    crate::migration::run(&db).await?;
    tracing::info!("database ready");
    Ok(db)
}

/// Open a single-connection in-memory SQLite database and run migrations.
///
/// A single connection keeps the in-memory schema visible to every query,
/// which makes this suitable for tests.
pub async fn connect_sqlite_memory() -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new("sqlite::memory:");
    options.max_connections(1);
    let db = Database::connect(options)
        .await
        .context("failed to connect to in-memory sqlite")?;
    crate::migration::run(&db).await?;
    Ok(db)
}
