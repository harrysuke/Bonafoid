//! Raw-SQL persistence for integration credentials and deployment history.
//!
//! # Isolation
//! Every query in this module carries `user_id` in its `WHERE` clause. A
//! project id or deployment id alone is never enough to read or change a
//! row, even if a caller forgets to check ownership first.
//!
//! # Secret discipline
//! `config_json` holds only the redacted configuration (owner/repo/branch,
//! registry/namespace). PATs and registry tokens are stored solely in the
//! `sealed_nonce` / `sealed_ciphertext` columns, encrypted with the server
//! master key. No function here returns plaintext.

use chrono::{DateTime, Utc};
use nocode_deploy::{Deployment, DeploymentStatus, SealedCredential};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, Value,
};
use uuid::Uuid;

use crate::error::ApiError;

pub const KIND_GITHUB: &str = "github";
pub const KIND_DOCKERHUB: &str = "dockerhub";

/// Statuses that represent a live deployment. Failed ones are excluded from
/// the partial unique index, so a failed attempt can be retried.
pub const ACTIVE_STATUSES: &str =
    "'generating', 'committing', 'pushing', 'pushed', 'ci_queued', 'ci_running', 'ci_succeeded'";

const INTEGRATION_COLUMNS: &str =
    "user_id, kind, config_json, sealed_nonce, sealed_ciphertext, verified, created_at, updated_at";
const DEPLOYMENT_COLUMNS: &str = "id, user_id, project_id, commit_sha, branch, status, github_run_id, github_run_url, error_message, created_at, updated_at";

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn text(value: &str) -> Value {
    Value::from(value.to_string())
}

fn opt_text(value: Option<&str>) -> Value {
    Value::from(value)
}

/// One stored integration. The sealed columns are ciphertext, never a token.
#[derive(Debug, Clone)]
pub struct IntegrationRow {
    pub user_id: String,
    pub kind: String,
    pub config_json: String,
    pub sealed_nonce: Option<String>,
    pub sealed_ciphertext: Option<String>,
    pub verified: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl IntegrationRow {
    /// Reassemble the encrypted blob, if this integration stores a secret.
    pub fn sealed(&self) -> Option<SealedCredential> {
        Some(SealedCredential {
            nonce: self.sealed_nonce.clone()?,
            ciphertext: self.sealed_ciphertext.clone()?,
        })
    }
}

fn map_integration(row: &QueryResult) -> Result<IntegrationRow, ApiError> {
    Ok(IntegrationRow {
        user_id: row.try_get("", "user_id").map_err(ApiError::from)?,
        kind: row.try_get("", "kind").map_err(ApiError::from)?,
        config_json: row.try_get("", "config_json").map_err(ApiError::from)?,
        sealed_nonce: row
            .try_get::<Option<String>>("", "sealed_nonce")
            .map_err(ApiError::from)?,
        sealed_ciphertext: row
            .try_get::<Option<String>>("", "sealed_ciphertext")
            .map_err(ApiError::from)?,
        verified: row.try_get::<i64>("", "verified").map_err(ApiError::from)? != 0,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
        updated_at: row.try_get("", "updated_at").map_err(ApiError::from)?,
    })
}

pub async fn get_integration(
    db: &DatabaseConnection,
    user_id: &str,
    kind: &str,
) -> Result<Option<IntegrationRow>, ApiError> {
    let rows = db
        .query_all(statement(
            &format!(
                "SELECT {INTEGRATION_COLUMNS} FROM integrations WHERE user_id = ? AND kind = ?"
            ),
            vec![text(user_id), text(kind)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.first().map(map_integration).transpose()
}

/// Insert or replace one integration. `config_json` must be redacted config;
/// the secret, when present, is passed as an already-sealed blob.
pub async fn upsert_integration(
    db: &DatabaseConnection,
    user_id: &str,
    kind: &str,
    config_json: &str,
    sealed: Option<&SealedCredential>,
    verified: bool,
) -> Result<(), ApiError> {
    db.execute(statement(
        "INSERT INTO integrations \
         (user_id, kind, config_json, sealed_nonce, sealed_ciphertext, verified) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT(user_id, kind) DO UPDATE SET \
           config_json = excluded.config_json, \
           sealed_nonce = excluded.sealed_nonce, \
           sealed_ciphertext = excluded.sealed_ciphertext, \
           verified = excluded.verified, \
           updated_at = datetime('now')",
        vec![
            text(user_id),
            text(kind),
            text(config_json),
            opt_text(sealed.map(|s| s.nonce.as_str())),
            opt_text(sealed.map(|s| s.ciphertext.as_str())),
            Value::from(if verified { 1i64 } else { 0i64 }),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct DeploymentRow {
    pub id: String,
    pub user_id: String,
    pub project_id: String,
    pub commit_sha: String,
    pub branch: String,
    pub status: String,
    pub github_run_id: Option<i64>,
    pub github_run_url: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl DeploymentRow {
    pub fn from_deployment(
        user_id: &str,
        project_id: &str,
        branch: &str,
        deployment: &Deployment,
    ) -> Self {
        Self {
            id: deployment.id.to_string(),
            user_id: user_id.to_string(),
            project_id: project_id.to_string(),
            commit_sha: deployment.commit_sha.clone(),
            branch: branch.to_string(),
            status: status_str(deployment.status).to_string(),
            github_run_id: deployment.github_run_id.map(|id| id as i64),
            github_run_url: deployment.github_run_url.clone(),
            error_message: deployment.error_message.clone(),
            created_at: deployment.created_at.to_rfc3339(),
            updated_at: deployment.updated_at.to_rfc3339(),
        }
    }

    /// Rebuild the deploy crate's record so `deployed()` and `ci_state()`
    /// stay the single source of truth for what the API reports.
    pub fn to_deployment(&self) -> Deployment {
        Deployment {
            id: Uuid::parse_str(&self.id).unwrap_or_else(|_| Uuid::new_v4()),
            project_id: self.project_id.clone(),
            commit_sha: self.commit_sha.clone(),
            status: parse_status(&self.status),
            created_at: parse_time(&self.created_at),
            updated_at: parse_time(&self.updated_at),
            github_run_id: self.github_run_id.and_then(|id| u64::try_from(id).ok()),
            github_run_url: self.github_run_url.clone(),
            error_message: self.error_message.clone(),
        }
    }
}

fn parse_time(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

/// Explicit mapping keeps an unknown stored status from ever being reported
/// as a successful deployment.
pub fn status_str(status: DeploymentStatus) -> &'static str {
    match status {
        DeploymentStatus::Generating => "generating",
        DeploymentStatus::Committing => "committing",
        DeploymentStatus::Pushing => "pushing",
        DeploymentStatus::Pushed => "pushed",
        DeploymentStatus::CiQueued => "ci_queued",
        DeploymentStatus::CiRunning => "ci_running",
        DeploymentStatus::CiSucceeded => "ci_succeeded",
        DeploymentStatus::CiFailed => "ci_failed",
        DeploymentStatus::Failed => "failed",
    }
}

pub fn parse_status(value: &str) -> DeploymentStatus {
    match value {
        "generating" => DeploymentStatus::Generating,
        "committing" => DeploymentStatus::Committing,
        "pushing" => DeploymentStatus::Pushing,
        "pushed" => DeploymentStatus::Pushed,
        "ci_queued" => DeploymentStatus::CiQueued,
        "ci_running" => DeploymentStatus::CiRunning,
        "ci_succeeded" => DeploymentStatus::CiSucceeded,
        "ci_failed" => DeploymentStatus::CiFailed,
        _ => DeploymentStatus::Failed,
    }
}

fn map_deployment(row: &QueryResult) -> Result<DeploymentRow, ApiError> {
    Ok(DeploymentRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        user_id: row.try_get("", "user_id").map_err(ApiError::from)?,
        project_id: row.try_get("", "project_id").map_err(ApiError::from)?,
        commit_sha: row.try_get("", "commit_sha").map_err(ApiError::from)?,
        branch: row.try_get("", "branch").map_err(ApiError::from)?,
        status: row.try_get("", "status").map_err(ApiError::from)?,
        github_run_id: row
            .try_get::<Option<i64>>("", "github_run_id")
            .map_err(ApiError::from)?,
        github_run_url: row
            .try_get::<Option<String>>("", "github_run_url")
            .map_err(ApiError::from)?,
        error_message: row
            .try_get::<Option<String>>("", "error_message")
            .map_err(ApiError::from)?,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
        updated_at: row.try_get("", "updated_at").map_err(ApiError::from)?,
    })
}

/// Insert a deployment. Returns `false` when the partial unique index
/// refuses a duplicate active commit (another request already pushed it).
pub async fn insert_deployment(
    db: &DatabaseConnection,
    row: &DeploymentRow,
) -> Result<bool, ApiError> {
    let result = db
        .execute(statement(
            &format!(
                "INSERT INTO deployments ({DEPLOYMENT_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT DO NOTHING"
            ),
            vec![
                text(&row.id),
                text(&row.user_id),
                text(&row.project_id),
                text(&row.commit_sha),
                text(&row.branch),
                text(&row.status),
                Value::BigInt(row.github_run_id),
                opt_text(row.github_run_url.as_deref()),
                opt_text(row.error_message.as_deref()),
                text(&row.created_at),
                text(&row.updated_at),
            ],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() > 0)
}

pub async fn active_deployment_for_commit(
    db: &DatabaseConnection,
    user_id: &str,
    project_id: &str,
    commit_sha: &str,
) -> Result<Option<DeploymentRow>, ApiError> {
    let rows = db
        .query_all(statement(
            &format!(
                "SELECT {DEPLOYMENT_COLUMNS} FROM deployments \
                 WHERE user_id = ? AND project_id = ? AND commit_sha = ? \
                   AND status IN ({ACTIVE_STATUSES}) LIMIT 1"
            ),
            vec![text(user_id), text(project_id), text(commit_sha)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.first().map(map_deployment).transpose()
}

pub async fn latest_deployment(
    db: &DatabaseConnection,
    user_id: &str,
    project_id: &str,
) -> Result<Option<DeploymentRow>, ApiError> {
    let rows = db
        .query_all(statement(
            &format!(
                "SELECT {DEPLOYMENT_COLUMNS} FROM deployments \
                 WHERE user_id = ? AND project_id = ? \
                 ORDER BY created_at DESC, id DESC LIMIT 1"
            ),
            vec![text(user_id), text(project_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.first().map(map_deployment).transpose()
}

pub async fn list_deployments(
    db: &DatabaseConnection,
    user_id: &str,
    project_id: &str,
) -> Result<Vec<DeploymentRow>, ApiError> {
    let rows = db
        .query_all(statement(
            &format!(
                "SELECT {DEPLOYMENT_COLUMNS} FROM deployments \
                 WHERE user_id = ? AND project_id = ? \
                 ORDER BY created_at DESC, id DESC"
            ),
            vec![text(user_id), text(project_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_deployment).collect()
}

/// Whether this project has already pushed to `branch` through the IDE.
/// Without such a row an existing remote branch is refused, so a deploy can
/// never fast-forward onto a branch the user did not create here.
pub async fn has_deployment_on_branch(
    db: &DatabaseConnection,
    user_id: &str,
    project_id: &str,
    branch: &str,
) -> Result<bool, ApiError> {
    let rows = db
        .query_all(statement(
            &format!(
                "SELECT 1 FROM deployments \
                 WHERE user_id = ? AND project_id = ? AND branch = ? \
                   AND status IN ({ACTIVE_STATUSES}) LIMIT 1"
            ),
            vec![text(user_id), text(project_id), text(branch)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(!rows.is_empty())
}

pub async fn update_deployment(
    db: &DatabaseConnection,
    row: &DeploymentRow,
) -> Result<(), ApiError> {
    db.execute(statement(
        "UPDATE deployments SET status = ?, github_run_id = ?, github_run_url = ?, \
         error_message = ?, updated_at = datetime('now') \
         WHERE id = ? AND user_id = ?",
        vec![
            text(&row.status),
            Value::BigInt(row.github_run_id),
            opt_text(row.github_run_url.as_deref()),
            opt_text(row.error_message.as_deref()),
            text(&row.id),
            text(&row.user_id),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}
