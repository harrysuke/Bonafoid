//! Raw-SQL persistence for revisions, branches, environments and secrets.
//!
//! Every query that touches owned data carries `tenant_id = ?` derived from
//! the SIGNED JWT claim, never from a query string. Revisions are strictly
//! append-only: this module exposes no UPDATE or DELETE for
//! `project_revisions`. Because the schema has a UNIQUE index on `parent_id`,
//! new commits always point at the project's newest revision, keeping the
//! history a single chain and making double-child conflicts impossible.

use std::collections::HashSet;

use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, Value,
};
use serde::Serialize;

use crate::error::ApiError;

pub use super::vcs::redact_env_config;

const REVISION_COLUMNS: &str =
    "id, tenant_id, project_id, parent_id, branch, message, author_id, data, created_at";
const ENVIRONMENT_COLUMNS: &str =
    "tenant_id, project_id, name, config_json, revision_id, status, updated_at";

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn text(value: &str) -> Value {
    Value::from(value.to_string())
}

fn opt_text(value: Option<&str>) -> Value {
    Value::String(value.map(|value| Box::new(value.to_string())))
}

/// An immutable commit row. `data` is the serialized project JSON document.
#[derive(Debug, Clone, Serialize)]
pub struct RevisionRow {
    pub id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub parent_id: Option<String>,
    pub branch: String,
    pub message: String,
    pub author_id: String,
    pub data: String,
    pub created_at: String,
}

/// A branch pointer into the revision chain.
#[derive(Debug, Clone, Serialize)]
pub struct BranchRow {
    pub tenant_id: String,
    pub project_id: String,
    pub name: String,
    pub head_revision_id: String,
    pub created_at: String,
}

/// A deployment environment with its redacted configuration and pointer.
#[derive(Debug, Clone, Serialize)]
pub struct EnvironmentRow {
    pub tenant_id: String,
    pub project_id: String,
    pub name: String,
    pub config_json: String,
    pub revision_id: Option<String>,
    pub status: String,
    pub updated_at: String,
}

/// An audit record for a promotion or rollback.
#[derive(Debug, Clone, Serialize)]
pub struct DeploymentRow {
    pub id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub environment_name: String,
    pub revision_id: String,
    pub status: String,
    pub deployed_at: String,
}

fn map_revision(row: &QueryResult) -> Result<RevisionRow, ApiError> {
    Ok(RevisionRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        tenant_id: row.try_get("", "tenant_id").map_err(ApiError::from)?,
        project_id: row.try_get("", "project_id").map_err(ApiError::from)?,
        parent_id: row.try_get("", "parent_id").map_err(ApiError::from)?,
        branch: row.try_get("", "branch").map_err(ApiError::from)?,
        message: row.try_get("", "message").map_err(ApiError::from)?,
        author_id: row.try_get("", "author_id").map_err(ApiError::from)?,
        data: row.try_get("", "data").map_err(ApiError::from)?,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
    })
}

fn map_branch(row: &QueryResult) -> Result<BranchRow, ApiError> {
    Ok(BranchRow {
        tenant_id: row.try_get("", "tenant_id").map_err(ApiError::from)?,
        project_id: row.try_get("", "project_id").map_err(ApiError::from)?,
        name: row.try_get("", "name").map_err(ApiError::from)?,
        head_revision_id: row
            .try_get("", "head_revision_id")
            .map_err(ApiError::from)?,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
    })
}

fn map_environment(row: &QueryResult) -> Result<EnvironmentRow, ApiError> {
    Ok(EnvironmentRow {
        tenant_id: row.try_get("", "tenant_id").map_err(ApiError::from)?,
        project_id: row.try_get("", "project_id").map_err(ApiError::from)?,
        name: row.try_get("", "name").map_err(ApiError::from)?,
        config_json: row.try_get("", "config_json").map_err(ApiError::from)?,
        revision_id: row.try_get("", "revision_id").map_err(ApiError::from)?,
        status: row.try_get("", "status").map_err(ApiError::from)?,
        updated_at: row.try_get("", "updated_at").map_err(ApiError::from)?,
    })
}

/// Append a revision. This is the ONLY write to `project_revisions`.
#[allow(clippy::too_many_arguments)]
pub async fn insert_revision(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    parent_id: Option<&str>,
    branch: &str,
    message: &str,
    author_id: &str,
    data: &str,
    created_at: &str,
) -> Result<(), ApiError> {
    let sql = "INSERT INTO project_revisions \
               (id, tenant_id, project_id, parent_id, branch, message, author_id, data, created_at) \
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)";
    db.execute(statement(
        sql,
        vec![
            text(&uuid::Uuid::new_v4().to_string()),
            text(tenant_id),
            text(project_id),
            opt_text(parent_id),
            text(branch),
            text(message),
            text(author_id),
            text(data),
            text(created_at),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// The newest revision of a project. Its id is the only legal parent for the
/// next commit, which keeps the UNIQUE(parent_id) index satisfied.
pub async fn latest_revision(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
) -> Result<Option<RevisionRow>, ApiError> {
    let sql = format!(
        "SELECT {REVISION_COLUMNS} FROM project_revisions \
         WHERE tenant_id = ? AND project_id = ? ORDER BY rowid DESC LIMIT 1"
    );
    let row = db
        .query_one(statement(&sql, vec![text(tenant_id), text(project_id)]))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_revision).transpose()
}

/// Every revision of a project in insertion order, used to build the parent
/// lookup for ancestry checks.
pub async fn list_revisions(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
) -> Result<Vec<RevisionRow>, ApiError> {
    let sql = format!(
        "SELECT {REVISION_COLUMNS} FROM project_revisions \
         WHERE tenant_id = ? AND project_id = ? ORDER BY rowid ASC"
    );
    let rows = db
        .query_all(statement(&sql, vec![text(tenant_id), text(project_id)]))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_revision).collect()
}

/// Fetch one revision by id, scoped to the signed tenant.
pub async fn get_revision(
    db: &DatabaseConnection,
    tenant_id: &str,
    id: &str,
) -> Result<Option<RevisionRow>, ApiError> {
    let sql =
        format!("SELECT {REVISION_COLUMNS} FROM project_revisions WHERE id = ? AND tenant_id = ?");
    let row = db
        .query_one(statement(&sql, vec![text(id), text(tenant_id)]))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_revision).transpose()
}

/// Fetch one revision, additionally scoped to a project. Used by promote and
/// rollback, where a revision from a sibling project must be a 404.
pub async fn get_project_revision(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    id: &str,
) -> Result<Option<RevisionRow>, ApiError> {
    let sql = format!(
        "SELECT {REVISION_COLUMNS} FROM project_revisions \
         WHERE id = ? AND tenant_id = ? AND project_id = ?"
    );
    let row = db
        .query_one(statement(
            &sql,
            vec![text(id), text(tenant_id), text(project_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_revision).transpose()
}

/// Walk parents from a branch head, newest-first.
pub async fn history_from_head(
    db: &DatabaseConnection,
    tenant_id: &str,
    head: &str,
) -> Result<Vec<RevisionRow>, ApiError> {
    let mut history = Vec::new();
    let mut current = Some(head.to_string());
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if !seen.insert(id.clone()) || history.len() >= 10_000 {
            break;
        }
        let Some(row) = get_revision(db, tenant_id, &id).await? else {
            break;
        };
        current = row.parent_id.clone();
        history.push(row);
    }
    Ok(history)
}

/// List branches of a tenant's project, most recently created first.
pub async fn list_branches(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
) -> Result<Vec<BranchRow>, ApiError> {
    let sql = "SELECT tenant_id, project_id, name, head_revision_id, created_at \
               FROM project_branches WHERE tenant_id = ? AND project_id = ? \
               ORDER BY created_at DESC, name ASC";
    let rows = db
        .query_all(statement(&sql, vec![text(tenant_id), text(project_id)]))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_branch).collect()
}

pub async fn get_branch(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    name: &str,
) -> Result<Option<BranchRow>, ApiError> {
    let sql = "SELECT tenant_id, project_id, name, head_revision_id, created_at \
               FROM project_branches WHERE tenant_id = ? AND project_id = ? AND name = ?";
    let row = db
        .query_one(statement(
            &sql,
            vec![text(tenant_id), text(project_id), text(name)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_branch).transpose()
}

/// Insert a branch. Returns false if one with the same name already exists.
pub async fn insert_branch(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    name: &str,
    head_revision_id: &str,
) -> Result<bool, ApiError> {
    let sql = "INSERT INTO project_branches \
               (tenant_id, project_id, name, head_revision_id) \
               VALUES (?, ?, ?, ?) \
               ON CONFLICT (tenant_id, project_id, name) DO NOTHING";
    let result = db
        .execute(statement(
            sql,
            vec![
                text(tenant_id),
                text(project_id),
                text(name),
                text(head_revision_id),
            ],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

/// Move only the named branch pointer. Other branches are never touched.
pub async fn set_branch_head(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    name: &str,
    head_revision_id: &str,
) -> Result<bool, ApiError> {
    let sql = "UPDATE project_branches SET head_revision_id = ? \
               WHERE tenant_id = ? AND project_id = ? AND name = ?";
    let result = db
        .execute(statement(
            sql,
            vec![
                text(head_revision_id),
                text(tenant_id),
                text(project_id),
                text(name),
            ],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

pub async fn list_environments(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
) -> Result<Vec<EnvironmentRow>, ApiError> {
    let sql = format!(
        "SELECT {ENVIRONMENT_COLUMNS} FROM project_environments \
         WHERE tenant_id = ? AND project_id = ? ORDER BY name ASC"
    );
    let rows = db
        .query_all(statement(&sql, vec![text(tenant_id), text(project_id)]))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_environment).collect()
}

pub async fn get_environment(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    name: &str,
) -> Result<Option<EnvironmentRow>, ApiError> {
    let sql = format!(
        "SELECT {ENVIRONMENT_COLUMNS} FROM project_environments \
         WHERE tenant_id = ? AND project_id = ? AND name = ?"
    );
    let row = db
        .query_one(statement(
            &sql,
            vec![text(tenant_id), text(project_id), text(name)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_environment).transpose()
}

pub async fn insert_environment(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    name: &str,
    config_json: &str,
) -> Result<bool, ApiError> {
    let sql = "INSERT INTO project_environments \
               (tenant_id, project_id, name, config_json) VALUES (?, ?, ?, ?) \
               ON CONFLICT (tenant_id, project_id, name) DO NOTHING";
    let result = db
        .execute(statement(
            sql,
            vec![
                text(tenant_id),
                text(project_id),
                text(name),
                text(config_json),
            ],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

pub async fn update_environment_config(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    name: &str,
    config_json: &str,
) -> Result<bool, ApiError> {
    let sql = "UPDATE project_environments SET config_json = ?, \
               updated_at = datetime('now') \
               WHERE tenant_id = ? AND project_id = ? AND name = ?";
    let result = db
        .execute(statement(
            sql,
            vec![
                text(config_json),
                text(tenant_id),
                text(project_id),
                text(name),
            ],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

/// Set both the promoted revision and the environment status atomically.
pub async fn set_environment_state(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    name: &str,
    revision_id: &str,
    status: &str,
) -> Result<bool, ApiError> {
    let sql = "UPDATE project_environments SET revision_id = ?, status = ?, \
               updated_at = datetime('now') \
               WHERE tenant_id = ? AND project_id = ? AND name = ?";
    let result = db
        .execute(statement(
            sql,
            vec![
                text(revision_id),
                text(status),
                text(tenant_id),
                text(project_id),
                text(name),
            ],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

/// Store a sealed secret. Upserting lets an operator rotate a value without
/// leaving the old ciphertext behind.
pub async fn insert_secret(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    environment_name: &str,
    key: &str,
    sealed_nonce: &str,
    sealed_ciphertext: &str,
) -> Result<(), ApiError> {
    let sql = "INSERT INTO environment_secrets \
               (tenant_id, project_id, environment_name, key, sealed_nonce, sealed_ciphertext) \
               VALUES (?, ?, ?, ?, ?, ?) \
               ON CONFLICT (tenant_id, project_id, environment_name, key) DO UPDATE SET \
               sealed_nonce = excluded.sealed_nonce, \
               sealed_ciphertext = excluded.sealed_ciphertext";
    db.execute(statement(
        sql,
        vec![
            text(tenant_id),
            text(project_id),
            text(environment_name),
            text(key),
            text(sealed_nonce),
            text(sealed_ciphertext),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Only key names are ever exposed; ciphertext and plaintext never leave SQL.
pub async fn list_secret_keys(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    environment_name: &str,
) -> Result<Vec<String>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT key FROM environment_secrets \
             WHERE tenant_id = ? AND project_id = ? AND environment_name = ? ORDER BY key ASC",
            vec![text(tenant_id), text(project_id), text(environment_name)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter()
        .map(|row| row.try_get("", "key").map_err(ApiError::from))
        .collect()
}

pub async fn delete_secret(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    environment_name: &str,
    key: &str,
) -> Result<bool, ApiError> {
    let result = db
        .execute(statement(
            "DELETE FROM environment_secrets \
             WHERE tenant_id = ? AND project_id = ? AND environment_name = ? AND key = ?",
            vec![
                text(tenant_id),
                text(project_id),
                text(environment_name),
                text(key),
            ],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

/// Append a deployment audit row. `status` records whether this was a promote
/// or a rollback.
pub async fn insert_deployment(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
    environment_name: &str,
    revision_id: &str,
    status: &str,
) -> Result<(), ApiError> {
    let insert = "INSERT INTO environment_deployments \
                  (id, tenant_id, project_id, environment_name, revision_id, status) \
                  VALUES (?, ?, ?, ?, ?, ?)";
    db.execute(statement(
        insert,
        vec![
            text(&uuid::Uuid::new_v4().to_string()),
            text(tenant_id),
            text(project_id),
            text(environment_name),
            text(revision_id),
            text(status),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Whether a project exists in the signed tenant.
pub async fn project_in_tenant(
    db: &DatabaseConnection,
    tenant_id: &str,
    project_id: &str,
) -> Result<bool, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT 1 FROM projects WHERE id = ? AND tenant_id = ?",
            vec![text(project_id), text(tenant_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(row.is_some())
}

/// Whether the signed user owns the project in the signed tenant.
pub async fn project_owned(
    db: &DatabaseConnection,
    tenant_id: &str,
    user_id: &str,
    project_id: &str,
) -> Result<bool, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT 1 FROM projects WHERE id = ? AND user_id = ? AND tenant_id = ?",
            vec![text(project_id), text(user_id), text(tenant_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(row.is_some())
}

/// Mirror a committed JSON document into the projects row so the IDE sees the
/// committed state. Ownership and tenant are checked by the WHERE clause.
pub async fn write_project_data(
    db: &DatabaseConnection,
    tenant_id: &str,
    user_id: &str,
    project_id: &str,
    data: &str,
    updated_at: &str,
) -> Result<bool, ApiError> {
    let result = db
        .execute(statement(
            "UPDATE projects SET data = ?, updated_at = ? \
             WHERE id = ? AND user_id = ? AND tenant_id = ?",
            vec![
                text(data),
                text(updated_at),
                text(project_id),
                text(user_id),
                text(tenant_id),
            ],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}
