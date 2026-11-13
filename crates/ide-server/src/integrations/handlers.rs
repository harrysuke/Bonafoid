//! HTTP handlers for integrations and deployments.
//!
//! CSRF is enforced by `integrations_router`'s `csrf_before_body` layer before
//! any extractor runs; every mutation also calls `require_csrf` as defence in
//! depth. Every database read/write carries the authenticated `user_id`. No
//! response body in this file can contain a token: the only secret-bearing
//! values are sealed blobs, which are opened into `Zeroizing` strings and
//! never returned.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use nocode_deploy::{
    classify_run, safe_error, valid_api_base, ComposeConfig, DeployConfig, DeployError,
    DeployOptions, DeployService, GitHubClient, GitHubConfig, GitHubConfigResponse, GitHubError,
    RegistryConfig, RegistryConfigResponse, VaultError, WorkflowRun,
};
use serde::Deserialize;
use std::sync::Arc;
use zeroize::Zeroizing;

use crate::auth::AuthUser;
use crate::error::{require_csrf, ApiError};
use crate::projects::require_project;
use crate::server::AppState;

use super::store::{self, DeploymentRow, KIND_DOCKERHUB, KIND_GITHUB};
use super::{require_deploy, DeploymentResponse};

/// A PAT is a short string; bounding it prevents a request body from being
/// used to make the server seal an arbitrary amount of data.
const MAX_TOKEN_CHARS: usize = 512;

fn default_branch() -> String {
    "main".to_string()
}

fn default_registry() -> String {
    "docker.io".to_string()
}

fn public_api_base() -> String {
    std::env::var("IDE_PUBLIC_API_BASE").unwrap_or_else(|_| "http://localhost:8081".to_string())
}

fn vault_failure(error: VaultError) -> ApiError {
    // Vault errors never contain key material, but keep them out of the
    // response: a rotated master key is an operator problem, not an API one.
    tracing::error!(%error, "credential vault error");
    ApiError::Internal
}

fn stored_config<T: serde::de::DeserializeOwned>(json: &str, label: &str) -> Result<T, ApiError> {
    serde_json::from_str(json).map_err(|error| {
        tracing::error!(%error, label, "stored integration config is corrupt");
        ApiError::Internal
    })
}

#[derive(Debug, Deserialize)]
pub struct GitHubConfigRequest {
    pub owner: String,
    pub repo: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    /// The PAT. Accepted only in the request body, never in a URL, argv,
    /// log or generated file, and immediately moved into `Zeroizing`.
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct DockerHubConfigRequest {
    pub namespace: String,
    #[serde(default = "default_registry")]
    pub registry: String,
    /// Omit on update to keep the already-sealed token.
    pub token: Option<String>,
}

/// Body of `POST /api/v1/projects/:id/deploy`.
///
/// `api_base` is the public URL baked into the generated frontend bundle at
/// build time (Phase 15 / 15.4): it is a *deploy input*, not a runtime
/// setting. Omitting it keeps the operator's `IDE_PUBLIC_API_BASE` default.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct DeployRequest {
    pub api_base: Option<String>,
}

/// `GET /api/v1/integrations/github` — redacted config, never a token.
pub async fn get_github(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<GitHubConfigResponse>, ApiError> {
    let _service = require_deploy(&state)?;
    let Some(row) = store::get_integration(&state.db, &user.id, KIND_GITHUB).await? else {
        return Ok(Json(GitHubConfigResponse::absent()));
    };
    let config: GitHubConfig = stored_config(&row.config_json, "github")?;
    let mut response = GitHubConfigResponse::from(&config);
    if row.verified {
        response = response.verified();
    }
    Ok(Json(response))
}

/// `POST /api/v1/integrations/github` — validate, seal, verify, store.
pub async fn set_github(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Json(body): Json<GitHubConfigRequest>,
) -> Result<(StatusCode, Json<GitHubConfigResponse>), ApiError> {
    require_csrf(&headers)?;
    let service = require_deploy(&state)?;

    let token = Zeroizing::new(body.token.trim().to_string());
    if token.is_empty() || token.chars().count() > MAX_TOKEN_CHARS {
        return Err(ApiError::BadRequest(
            "a GitHub personal access token is required".into(),
        ));
    }
    let config = GitHubConfig {
        owner: body.owner,
        repo: body.repo,
        branch: body.branch,
    };
    config
        .validate()
        .map_err(|error| ApiError::BadRequest(safe_error(&error)))?;

    let sealed = service
        .vault()
        .seal(token.as_str())
        .map_err(vault_failure)?;

    // Verify before persisting: a token with no access to the repo must not
    // leave a "verified" row behind. The client shares the service's base
    // URL, so tests can point this at a local fake instead of GitHub.
    let client = GitHubClient::with_base_url(
        token.as_str().to_string(),
        config.clone(),
        service.github_base_url(),
    )
    .map_err(github_failure)?;
    client.verify_repo().await.map_err(github_failure)?;

    let config_json = serde_json::to_string(&config)?;
    store::upsert_integration(
        &state.db,
        &user.id,
        KIND_GITHUB,
        &config_json,
        Some(&sealed),
        true,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(GitHubConfigResponse::from(&config).verified()),
    ))
}

fn github_failure(error: GitHubError) -> ApiError {
    ApiError::BadRequest(safe_error(&error))
}

/// `GET /api/v1/integrations/dockerhub` — redacted config, never a token.
pub async fn get_dockerhub(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<RegistryConfigResponse>, ApiError> {
    let _service = require_deploy(&state)?;
    let Some(row) = store::get_integration(&state.db, &user.id, KIND_DOCKERHUB).await? else {
        return Ok(Json(RegistryConfigResponse::absent()));
    };
    let config: RegistryConfig = stored_config(&row.config_json, "dockerhub")?;
    let mut response = RegistryConfigResponse::from(&config);
    if row.verified {
        response = response.verified();
    }
    Ok(Json(response))
}

/// `POST /api/v1/integrations/dockerhub` — store only; no live registry call
/// in this stage. The token, when supplied, is sealed before it touches SQL.
pub async fn set_dockerhub(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Json(body): Json<DockerHubConfigRequest>,
) -> Result<(StatusCode, Json<RegistryConfigResponse>), ApiError> {
    require_csrf(&headers)?;
    let service = require_deploy(&state)?;

    if !nocode_deploy::valid_repo_component(&body.namespace)
        || !nocode_deploy::valid_repo_component(&body.registry)
    {
        return Err(ApiError::BadRequest(
            "namespace and registry must be plain identifiers".into(),
        ));
    }

    let existing = store::get_integration(&state.db, &user.id, KIND_DOCKERHUB).await?;
    let supplied = body
        .token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty());
    if supplied.is_some_and(|token| token.chars().count() > MAX_TOKEN_CHARS) {
        return Err(ApiError::BadRequest("registry token is too long".into()));
    }
    let sealed = match supplied {
        Some(token) => {
            let owned = Zeroizing::new(token.to_string());
            Some(
                service
                    .vault()
                    .seal(owned.as_str())
                    .map_err(vault_failure)?,
            )
        }
        None => existing.as_ref().and_then(|row| row.sealed()),
    };
    let sealed = sealed.ok_or_else(|| {
        ApiError::BadRequest("a Docker Hub token is required on first configuration".into())
    })?;

    let config = RegistryConfig {
        namespace: body.namespace,
        registry: body.registry,
        // Derived from the project slug at deploy time, never from input.
        backend_image: String::new(),
        frontend_image: String::new(),
    };
    let config_json = serde_json::to_string(&config)?;
    store::upsert_integration(
        &state.db,
        &user.id,
        KIND_DOCKERHUB,
        &config_json,
        Some(&sealed),
        true,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(RegistryConfigResponse::from(&config).verified()),
    ))
}

/// `POST /api/v1/projects/:id/deploy` — generate, commit and push.
///
/// Returns 202 with `status: pushed` and `deployed: false`. A repeat of an
/// unchanged generated tree returns the existing row with 200 and does not
/// push a second time.
pub async fn deploy_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(project_id): Path<String>,
    Json(body): Json<DeployRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let service = require_deploy(&state)?;
    let project = require_project(&state.db, &user.id, &project_id).await?;

    // 15.4: the public API base URL is captured at build time, so it must be
    // correct *before* this deploy. Validate it before doing any work; the
    // operator default is used when the caller does not supply one.
    let api_base = match body.api_base.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => {
            if !valid_api_base(value) {
                return Err(ApiError::BadRequest(
                    "api_base must be a plain absolute http(s) URL without credentials or query"
                        .into(),
                ));
            }
            value.trim_end_matches('/').to_string()
        }
        _ => public_api_base(),
    };

    let github_row = store::get_integration(&state.db, &user.id, KIND_GITHUB)
        .await?
        .ok_or_else(|| ApiError::BadRequest("configure the GitHub integration first".into()))?;
    let registry_row = store::get_integration(&state.db, &user.id, KIND_DOCKERHUB)
        .await?
        .ok_or_else(|| ApiError::BadRequest("configure the Docker Hub integration first".into()))?;
    if !github_row.verified || !registry_row.verified {
        return Err(ApiError::BadRequest(
            "both integrations must be verified before deploying".into(),
        ));
    }

    let github: GitHubConfig = stored_config(&github_row.config_json, "github")?;
    let registry: RegistryConfig = stored_config(&registry_row.config_json, "dockerhub")?;
    let sealed = github_row
        .sealed()
        .ok_or_else(|| ApiError::BadRequest("the GitHub credential is missing".into()))?;

    let branch = github.branch.clone();
    let branch_created_by_deploy =
        store::has_deployment_on_branch(&state.db, &user.id, &project_id, &branch).await?;
    let config = DeployConfig {
        github,
        compose: ComposeConfig {
            project_slug: project.slug.clone(),
            registry,
            api_base: api_base.clone(),
        },
        api_base,
    };

    let deployment = match service
        .deploy(
            &project,
            &config,
            &sealed,
            DeployOptions {
                branch_created_by_deploy,
            },
        )
        .await
    {
        Ok(deployment) => deployment,
        Err(DeployError::Idempotent) => {
            // The remote already has exactly this generated tree. Return the
            // existing record instead of manufacturing a second one.
            let existing = store::latest_deployment(&state.db, &user.id, &project_id)
                .await?
                .ok_or(ApiError::Conflict(
                    "this generated commit is already deployed".into(),
                ))?;
            return Ok((
                StatusCode::OK,
                Json(DeploymentResponse::from_row(&existing)),
            )
                .into_response());
        }
        Err(error) => return Err(error.into()),
    };

    let row = DeploymentRow::from_deployment(&user.id, &project_id, &branch, &deployment);
    if !store::insert_deployment(&state.db, &row).await? {
        // The partial unique index caught a concurrent duplicate push.
        let existing = store::active_deployment_for_commit(
            &state.db,
            &user.id,
            &project_id,
            &deployment.commit_sha,
        )
        .await?
        .unwrap_or(row);
        return Ok((
            StatusCode::OK,
            Json(DeploymentResponse::from_row(&existing)),
        )
            .into_response());
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(DeploymentResponse::from_row(&row)),
    )
        .into_response())
}

/// `GET /api/v1/projects/:id/deployments` — ownership-scoped history.
///
/// Non-terminal rows are refreshed best-effort from a SINGLE GitHub call;
/// `classify_run` filters by commit SHA, so a run belonging to another
/// commit can never mark this deployment as succeeded.
pub async fn list_deployments(
    State(state): State<AppState>,
    user: AuthUser,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<DeploymentResponse>>, ApiError> {
    let service = require_deploy(&state)?;
    let _project = require_project(&state.db, &user.id, &project_id).await?;
    let mut rows = store::list_deployments(&state.db, &user.id, &project_id).await?;
    refresh_statuses(&state, &service, &user.id, &mut rows).await;
    Ok(Json(
        rows.iter().map(DeploymentResponse::from_row).collect(),
    ))
}

async fn refresh_statuses(
    state: &AppState,
    service: &Arc<DeployService>,
    user_id: &str,
    rows: &mut [DeploymentRow],
) {
    let has_pending = rows
        .iter()
        .any(|row| !store::parse_status(&row.status).is_terminal());
    if !has_pending {
        return;
    }

    let runs = match fetch_runs(state, service, user_id).await {
        Ok(runs) => runs,
        Err(error) => {
            // Polling is best-effort: a GitHub outage must not hide the
            // deployment history the server already has.
            tracing::warn!(%error, "could not refresh deployment CI status");
            return;
        }
    };
    let branch = match store::get_integration(&state.db, user_id, KIND_GITHUB).await {
        Ok(Some(row)) => stored_config::<GitHubConfig>(&row.config_json, "github")
            .map(|config| config.branch)
            .unwrap_or_default(),
        _ => return,
    };

    for row in rows.iter_mut() {
        if store::parse_status(&row.status).is_terminal() {
            continue;
        }
        let Some((status, run_id, run_url)) = classify_run(&runs, &row.commit_sha, &branch) else {
            continue;
        };
        row.status = store::status_str(status).to_string();
        row.github_run_id = i64::try_from(run_id).ok();
        row.github_run_url = Some(run_url);
        row.updated_at = chrono::Utc::now().to_rfc3339();
        if let Err(error) = store::update_deployment(&state.db, row).await {
            tracing::warn!(%error, "could not persist refreshed deployment status");
        }
    }
}

async fn fetch_runs(
    state: &AppState,
    service: &Arc<DeployService>,
    user_id: &str,
) -> Result<Vec<WorkflowRun>, ApiError> {
    let row = store::get_integration(&state.db, user_id, KIND_GITHUB)
        .await?
        .ok_or(ApiError::Internal)?;
    let config: GitHubConfig = stored_config(&row.config_json, "github")?;
    let sealed = row.sealed().ok_or(ApiError::Internal)?;
    let token = Zeroizing::new(service.vault().open(&sealed).map_err(vault_failure)?);
    let client = GitHubClient::with_base_url(
        token.as_str().to_string(),
        config,
        service.github_base_url(),
    )
    .map_err(github_failure)?;
    client.list_workflow_runs().await.map_err(github_failure)
}
