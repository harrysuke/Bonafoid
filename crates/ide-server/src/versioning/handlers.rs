//! HTTP DTOs and handlers for version control and environments.
//!
//! Every handler takes the tenant id from the signed [`AuthUser`] claim. No
//! caller-supplied tenant id is ever used for authorization. Mutations require
//! double-submit CSRF for cookie sessions (bearer requests are exempt by
//! design). Secrets are sealed with the credential vault and never serialized
//! into a response.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use nocode_deploy::{redact, CredentialVault, SealedCredential};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::{
    auth::AuthUser,
    error::{require_csrf, ApiError},
    server::AppState,
};

use super::{store, vcs};

/// Body of `POST /api/v1/versioning/branches`.
#[derive(Debug, Deserialize)]
pub struct CreateBranchRequest {
    pub project_id: String,
    pub name: String,
    pub from_revision_id: Option<String>,
}

/// Body of `POST /api/v1/versioning/commits`.
#[derive(Debug, Deserialize)]
pub struct CommitRequest {
    pub project_id: String,
    pub branch: String,
    pub message: String,
    pub data: Value,
}

/// Body of `POST /api/v1/versioning/branches/:name/merge`.
#[derive(Debug, Deserialize)]
pub struct MergeRequest {
    pub project_id: String,
    pub source_branch: String,
    pub strategy: String,
}

/// Body of `POST /api/v1/versioning/restore`.
#[derive(Debug, Deserialize)]
pub struct RestoreRequest {
    pub project_id: String,
    pub revision_id: String,
}

/// Body of `POST /api/v1/versioning/environments`.
#[derive(Debug, Deserialize)]
pub struct CreateEnvironmentRequest {
    pub project_id: String,
    pub name: String,
    pub config: Value,
}

/// Body of `PATCH /api/v1/versioning/environments/:name`.
#[derive(Debug, Deserialize)]
pub struct PatchEnvironmentRequest {
    pub project_id: String,
    pub config: Value,
}

/// Body of promote and rollback requests.
#[derive(Debug, Deserialize)]
pub struct RevisionTargetRequest {
    pub project_id: String,
    pub revision_id: String,
}

/// Body of `POST /api/v1/versioning/environments/:name/secrets`.
#[derive(Debug, Deserialize)]
pub struct CreateSecretRequest {
    pub project_id: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct ProjectQuery {
    pub project_id: String,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub project_id: String,
    pub branch: String,
}

#[derive(Debug, Deserialize)]
pub struct DiffQuery {
    pub project_id: String,
    pub from: String,
    pub to: String,
}

/// A revision as returned by the API. `data` is a JSON value, not a string.
#[derive(Debug, Serialize)]
pub struct RevisionResponse {
    pub id: String,
    pub project_id: String,
    pub parent_id: Option<String>,
    pub branch: String,
    pub message: String,
    pub author_id: String,
    pub data: Value,
    pub created_at: String,
}

impl TryFrom<&store::RevisionRow> for RevisionResponse {
    type Error = ApiError;

    fn try_from(row: &store::RevisionRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id.clone(),
            project_id: row.project_id.clone(),
            parent_id: row.parent_id.clone(),
            branch: row.branch.clone(),
            message: row.message.clone(),
            author_id: row.author_id.clone(),
            data: serde_json::from_str(&row.data)?,
            created_at: row.created_at.clone(),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct BranchResponse {
    pub name: String,
    pub head_revision_id: String,
    pub created_at: String,
}

impl From<&store::BranchRow> for BranchResponse {
    fn from(row: &store::BranchRow) -> Self {
        Self {
            name: row.name.clone(),
            head_revision_id: row.head_revision_id.clone(),
            created_at: row.created_at.clone(),
        }
    }
}

/// An environment view. `config` has passed through recursive redaction and
/// there is deliberately no field containing a secret value.
#[derive(Debug, Serialize)]
pub struct EnvironmentResponse {
    pub name: String,
    pub config: Value,
    pub revision_id: Option<String>,
    pub status: String,
    pub updated_at: String,
}

impl TryFrom<&store::EnvironmentRow> for EnvironmentResponse {
    type Error = ApiError;

    fn try_from(row: &store::EnvironmentRow) -> Result<Self, Self::Error> {
        let config: Value = serde_json::from_str(&row.config_json)?;
        Ok(Self {
            name: row.name.clone(),
            config: store::redact_env_config(&config),
            revision_id: row.revision_id.clone(),
            status: row.status.clone(),
            updated_at: row.updated_at.clone(),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct DiffResponse {
    pub from: String,
    pub to: String,
    pub lines: Vec<vcs::DiffLine>,
}

#[derive(Debug, Serialize)]
pub struct SecretKeyResponse {
    pub key: String,
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

/// The vault is a feature, not an error: a missing master key is an operator
/// setup step and reported as 503, never as 500.
fn vault() -> Result<CredentialVault, ApiError> {
    CredentialVault::from_env().map_err(|error| {
        tracing::warn!(%error, "versioning secrets disabled");
        ApiError::ServiceUnavailable("set IDE_SECRET_KEY to manage environment secrets".to_string())
    })
}

fn seal_secret(
    credential: &CredentialVault,
    plaintext: &str,
) -> Result<SealedCredential, ApiError> {
    credential.seal(plaintext).map_err(|error| {
        tracing::error!(%error, "failed to seal environment secret");
        ApiError::Internal
    })
}

async fn require_project(
    state: &AppState,
    user: &AuthUser,
    project_id: &str,
) -> Result<(), ApiError> {
    if !store::project_in_tenant(&state.db, &user.tenant_id, project_id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

async fn require_owner(
    state: &AppState,
    user: &AuthUser,
    project_id: &str,
) -> Result<(), ApiError> {
    if !store::project_owned(&state.db, &user.tenant_id, &user.id, project_id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

/// Pretty-print JSON for the line-based diff. Splitting on `\n` keeps the
/// output deterministic regardless of the platform's line endings.
fn pretty_lines(value: &Value) -> Result<Vec<String>, ApiError> {
    let pretty = serde_json::to_string_pretty(value)?;
    Ok(pretty.split('\n').map(str::to_string).collect())
}

/// Append a revision to the project's single chain and point `branch` at it.
/// The parent is always the newest revision, so the UNIQUE(parent_id) index
/// can never be violated — even when two branches commit in sequence.
async fn append_revision(
    state: &AppState,
    user: &AuthUser,
    project_id: &str,
    branch: &str,
    message: &str,
    data: &str,
) -> Result<store::RevisionRow, ApiError> {
    let parent = store::latest_revision(&state.db, &user.tenant_id, project_id)
        .await?
        .map(|row| row.id);
    let created_at = now();
    store::insert_revision(
        &state.db,
        &user.tenant_id,
        project_id,
        parent.as_deref(),
        branch,
        message,
        &user.id,
        data,
        &created_at,
    )
    .await?;
    let head = store::latest_revision(&state.db, &user.tenant_id, project_id)
        .await?
        .ok_or(ApiError::Internal)?
        .id;
    if store::get_branch(&state.db, &user.tenant_id, project_id, branch)
        .await?
        .is_some()
    {
        store::set_branch_head(&state.db, &user.tenant_id, project_id, branch, &head).await?;
    } else {
        store::insert_branch(&state.db, &user.tenant_id, project_id, branch, &head).await?;
    }
    store::get_revision(&state.db, &user.tenant_id, &head)
        .await?
        .ok_or(ApiError::Internal)
}

/// `GET /api/v1/versioning/branches`
pub async fn list_branches(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ProjectQuery>,
) -> Result<Json<Vec<BranchResponse>>, ApiError> {
    let rows = store::list_branches(&state.db, &user.tenant_id, &query.project_id).await?;
    Ok(Json(rows.iter().map(BranchResponse::from).collect()))
}

/// `POST /api/v1/versioning/branches`
pub async fn create_branch(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Json(body): Json<CreateBranchRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    if !vcs::validate_branch(&body.name) {
        return Err(ApiError::BadRequest(format!(
            "invalid branch name `{}`: expected 1-64 lowercase letters, digits or hyphens",
            body.name
        )));
    }
    require_project(&state, &user, &body.project_id).await?;
    if store::get_branch(&state.db, &user.tenant_id, &body.project_id, &body.name)
        .await?
        .is_some()
    {
        return Err(ApiError::Conflict(format!(
            "branch `{}` already exists",
            body.name
        )));
    }
    let head = match body.from_revision_id {
        Some(id) => {
            store::get_project_revision(&state.db, &user.tenant_id, &body.project_id, &id)
                .await?
                .ok_or(ApiError::NotFound)?
                .id
        }
        None => {
            store::latest_revision(&state.db, &user.tenant_id, &body.project_id)
                .await?
                .ok_or_else(|| ApiError::BadRequest("no revision to branch from".to_string()))?
                .id
        }
    };
    if !store::insert_branch(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &body.name,
        &head,
    )
    .await?
    {
        return Err(ApiError::Conflict(format!(
            "branch `{}` already exists",
            body.name
        )));
    }
    let row = store::get_branch(&state.db, &user.tenant_id, &body.project_id, &body.name)
        .await?
        .ok_or(ApiError::Internal)?;
    Ok((StatusCode::CREATED, Json(BranchResponse::from(&row))).into_response())
}

/// `GET /api/v1/versioning/history`
pub async fn history(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<RevisionResponse>>, ApiError> {
    let branch = store::get_branch(&state.db, &user.tenant_id, &query.project_id, &query.branch)
        .await?
        .ok_or(ApiError::NotFound)?;
    let rows =
        store::history_from_head(&state.db, &user.tenant_id, &branch.head_revision_id).await?;
    let revisions = rows
        .iter()
        .map(RevisionResponse::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(revisions))
}

/// `GET /api/v1/versioning/revisions/:id`
pub async fn get_revision(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<RevisionResponse>, ApiError> {
    let row = store::get_revision(&state.db, &user.tenant_id, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(RevisionResponse::try_from(&row)?))
}

/// `POST /api/v1/versioning/commits`
pub async fn commit(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Json(body): Json<CommitRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let message = body.message.trim().to_string();
    if message.is_empty() || message.chars().count() > 256 {
        return Err(ApiError::BadRequest(
            "commit message must be 1-256 characters".to_string(),
        ));
    }
    if !vcs::validate_branch(&body.branch) {
        return Err(ApiError::BadRequest(format!(
            "invalid branch name `{}`",
            body.branch
        )));
    }
    require_owner(&state, &user, &body.project_id).await?;
    let data = serde_json::to_string(&body.data)?;
    let row = append_revision(
        &state,
        &user,
        &body.project_id,
        &body.branch,
        &message,
        &data,
    )
    .await?;
    if !store::write_project_data(
        &state.db,
        &user.tenant_id,
        &user.id,
        &body.project_id,
        &data,
        &now(),
    )
    .await?
    {
        return Err(ApiError::NotFound);
    }
    Ok((StatusCode::CREATED, Json(RevisionResponse::try_from(&row)?)).into_response())
}

/// `GET /api/v1/versioning/diff`
pub async fn diff(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<DiffQuery>,
) -> Result<Json<DiffResponse>, ApiError> {
    let from =
        store::get_project_revision(&state.db, &user.tenant_id, &query.project_id, &query.from)
            .await?
            .ok_or(ApiError::NotFound)?;
    let to = store::get_project_revision(&state.db, &user.tenant_id, &query.project_id, &query.to)
        .await?
        .ok_or(ApiError::NotFound)?;
    let from_value: Value = serde_json::from_str(&from.data)?;
    let to_value: Value = serde_json::from_str(&to.data)?;
    let from_lines = pretty_lines(&from_value)?;
    let to_lines = pretty_lines(&to_value)?;
    Ok(Json(DiffResponse {
        from: query.from,
        to: query.to,
        lines: vcs::lcs_diff(&from_lines, &to_lines),
    }))
}

/// `POST /api/v1/versioning/branches/:name/merge`
pub async fn merge(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(name): Path<String>,
    Json(body): Json<MergeRequest>,
) -> Result<Json<RevisionResponse>, ApiError> {
    require_csrf(&headers)?;
    if !matches!(body.strategy.as_str(), "fast_forward" | "theirs" | "ours") {
        return Err(ApiError::BadRequest(format!(
            "invalid merge strategy `{}`: expected fast_forward, theirs or ours",
            body.strategy
        )));
    }
    require_owner(&state, &user, &body.project_id).await?;
    let target = store::get_branch(&state.db, &user.tenant_id, &body.project_id, &name)
        .await?
        .ok_or(ApiError::NotFound)?;
    let source = store::get_branch(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &body.source_branch,
    )
    .await?
    .ok_or(ApiError::NotFound)?;

    let revisions = store::list_revisions(&state.db, &user.tenant_id, &body.project_id).await?;
    let parents: HashMap<String, Option<String>> = revisions
        .into_iter()
        .map(|row| (row.id, row.parent_id))
        .collect();
    let parent_of = |id: &str| parents.get(id).cloned().flatten();

    if body.strategy == "fast_forward" {
        if !vcs::is_ancestor(
            &target.head_revision_id,
            &source.head_revision_id,
            &parent_of,
        ) {
            return Err(ApiError::Conflict(
                "fast forward refused: target head is not an ancestor of source head".to_string(),
            ));
        }
        if !store::set_branch_head(
            &state.db,
            &user.tenant_id,
            &body.project_id,
            &name,
            &source.head_revision_id,
        )
        .await?
        {
            return Err(ApiError::NotFound);
        }
        let source_revision = store::get_project_revision(
            &state.db,
            &user.tenant_id,
            &body.project_id,
            &source.head_revision_id,
        )
        .await?
        .ok_or(ApiError::Internal)?;
        store::write_project_data(
            &state.db,
            &user.tenant_id,
            &user.id,
            &body.project_id,
            &source_revision.data,
            &now(),
        )
        .await?;
        return Ok(Json(RevisionResponse::try_from(&source_revision)?));
    }

    let ours_row = store::get_project_revision(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &target.head_revision_id,
    )
    .await?
    .ok_or(ApiError::Internal)?;
    let theirs_row = store::get_project_revision(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &source.head_revision_id,
    )
    .await?
    .ok_or(ApiError::Internal)?;
    let ours: Value = serde_json::from_str(&ours_row.data)?;
    let theirs: Value = serde_json::from_str(&theirs_row.data)?;
    let merged =
        vcs::merge_strategy_wins(&body.strategy, &ours, &theirs).map_err(ApiError::BadRequest)?;
    let data = serde_json::to_string(&merged)?;
    let message = format!(
        "merge `{}` into `{}`: conflict resolved with `{}` policy",
        body.source_branch, name, body.strategy
    );
    let row = append_revision(&state, &user, &body.project_id, &name, &message, &data).await?;
    store::write_project_data(
        &state.db,
        &user.tenant_id,
        &user.id,
        &body.project_id,
        &data,
        &now(),
    )
    .await?;
    Ok(Json(RevisionResponse::try_from(&row)?))
}

/// `POST /api/v1/versioning/restore`
pub async fn restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Json(body): Json<RestoreRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    require_owner(&state, &user, &body.project_id).await?;
    let source = store::get_project_revision(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &body.revision_id,
    )
    .await?
    .ok_or(ApiError::NotFound)?;
    let branch = source.branch.clone();
    if store::get_branch(&state.db, &user.tenant_id, &body.project_id, &branch)
        .await?
        .is_none()
    {
        let head = store::latest_revision(&state.db, &user.tenant_id, &body.project_id)
            .await?
            .map(|row| row.id)
            .unwrap_or_else(|| source.id.clone());
        store::insert_branch(&state.db, &user.tenant_id, &body.project_id, &branch, &head).await?;
    }
    let message = format!("restore revision {}", source.id);
    let row = append_revision(
        &state,
        &user,
        &body.project_id,
        &branch,
        &message,
        &source.data,
    )
    .await?;
    store::write_project_data(
        &state.db,
        &user.tenant_id,
        &user.id,
        &body.project_id,
        &source.data,
        &now(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(RevisionResponse::try_from(&row)?)).into_response())
}

/// `GET /api/v1/versioning/environments`
pub async fn list_environments(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ProjectQuery>,
) -> Result<Json<Vec<EnvironmentResponse>>, ApiError> {
    let rows = store::list_environments(&state.db, &user.tenant_id, &query.project_id).await?;
    let environments = rows
        .iter()
        .map(EnvironmentResponse::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(environments))
}

/// `POST /api/v1/versioning/environments`
pub async fn create_environment(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Json(body): Json<CreateEnvironmentRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    if !vcs::validate_branch(&body.name) {
        return Err(ApiError::BadRequest(format!(
            "invalid environment name `{}`",
            body.name
        )));
    }
    require_project(&state, &user, &body.project_id).await?;
    let config = serde_json::to_string(&store::redact_env_config(&body.config))?;
    if !store::insert_environment(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &body.name,
        &config,
    )
    .await?
    {
        return Err(ApiError::Conflict(format!(
            "environment `{}` already exists",
            body.name
        )));
    }
    let row = store::get_environment(&state.db, &user.tenant_id, &body.project_id, &body.name)
        .await?
        .ok_or(ApiError::Internal)?;
    Ok((
        StatusCode::CREATED,
        Json(EnvironmentResponse::try_from(&row)?),
    )
        .into_response())
}

/// `PATCH /api/v1/versioning/environments/:name`
pub async fn patch_environment(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(name): Path<String>,
    Json(body): Json<PatchEnvironmentRequest>,
) -> Result<Json<EnvironmentResponse>, ApiError> {
    require_csrf(&headers)?;
    require_project(&state, &user, &body.project_id).await?;
    if store::get_environment(&state.db, &user.tenant_id, &body.project_id, &name)
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    let config = serde_json::to_string(&store::redact_env_config(&body.config))?;
    if !store::update_environment_config(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &name,
        &config,
    )
    .await?
    {
        return Err(ApiError::NotFound);
    }
    let row = store::get_environment(&state.db, &user.tenant_id, &body.project_id, &name)
        .await?
        .ok_or(ApiError::Internal)?;
    Ok(Json(EnvironmentResponse::try_from(&row)?))
}

/// `POST /api/v1/versioning/environments/:name/promote`
pub async fn promote(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(name): Path<String>,
    Json(body): Json<RevisionTargetRequest>,
) -> Result<Json<EnvironmentResponse>, ApiError> {
    require_csrf(&headers)?;
    require_project(&state, &user, &body.project_id).await?;
    store::get_project_revision(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &body.revision_id,
    )
    .await?
    .ok_or(ApiError::NotFound)?;
    if store::get_environment(&state.db, &user.tenant_id, &body.project_id, &name)
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    if !store::set_environment_state(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &name,
        &body.revision_id,
        "deployed",
    )
    .await?
    {
        return Err(ApiError::NotFound);
    }
    store::insert_deployment(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &name,
        &body.revision_id,
        "deployed",
    )
    .await?;
    let row = store::get_environment(&state.db, &user.tenant_id, &body.project_id, &name)
        .await?
        .ok_or(ApiError::Internal)?;
    Ok(Json(EnvironmentResponse::try_from(&row)?))
}

/// `POST /api/v1/versioning/environments/:name/rollback`
pub async fn rollback(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(name): Path<String>,
    Json(body): Json<RevisionTargetRequest>,
) -> Result<Json<EnvironmentResponse>, ApiError> {
    require_csrf(&headers)?;
    require_project(&state, &user, &body.project_id).await?;
    store::get_project_revision(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &body.revision_id,
    )
    .await?
    .ok_or(ApiError::NotFound)?;
    if store::get_environment(&state.db, &user.tenant_id, &body.project_id, &name)
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    if !store::set_environment_state(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &name,
        &body.revision_id,
        "rolled_back",
    )
    .await?
    {
        return Err(ApiError::NotFound);
    }
    store::insert_deployment(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &name,
        &body.revision_id,
        "rolled_back",
    )
    .await?;
    let row = store::get_environment(&state.db, &user.tenant_id, &body.project_id, &name)
        .await?
        .ok_or(ApiError::Internal)?;
    Ok(Json(EnvironmentResponse::try_from(&row)?))
}

/// `POST /api/v1/versioning/environments/:name/secrets`
pub async fn create_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(name): Path<String>,
    Json(body): Json<CreateSecretRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    if body.key.is_empty()
        || body.key.chars().count() > 128
        || body.key.chars().any(char::is_control)
    {
        return Err(ApiError::BadRequest(
            "secret key must be 1-128 characters with no control characters".to_string(),
        ));
    }
    require_project(&state, &user, &body.project_id).await?;
    if store::get_environment(&state.db, &user.tenant_id, &body.project_id, &name)
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    let credential = vault()?;
    let sealed = seal_secret(&credential, &body.value)?;
    store::insert_secret(
        &state.db,
        &user.tenant_id,
        &body.project_id,
        &name,
        &body.key,
        &sealed.nonce,
        &sealed.ciphertext,
    )
    .await?;
    tracing::info!(key = %redact(&body.key), environment = %name, "environment secret stored");
    Ok((
        StatusCode::CREATED,
        Json(SecretKeyResponse { key: body.key }),
    )
        .into_response())
}

/// `GET /api/v1/versioning/environments/:name/secrets`
pub async fn list_secrets(
    State(state): State<AppState>,
    user: AuthUser,
    Path(name): Path<String>,
    Query(query): Query<ProjectQuery>,
) -> Result<Json<Vec<SecretKeyResponse>>, ApiError> {
    if store::get_environment(&state.db, &user.tenant_id, &query.project_id, &name)
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    let keys =
        store::list_secret_keys(&state.db, &user.tenant_id, &query.project_id, &name).await?;
    Ok(Json(
        keys.into_iter()
            .map(|key| SecretKeyResponse { key })
            .collect(),
    ))
}

/// `DELETE /api/v1/versioning/environments/:name/secrets/:key`
pub async fn delete_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path((name, key)): Path<(String, String)>,
    Query(query): Query<ProjectQuery>,
) -> Result<StatusCode, ApiError> {
    require_csrf(&headers)?;
    if store::delete_secret(&state.db, &user.tenant_id, &query.project_id, &name, &key).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}
