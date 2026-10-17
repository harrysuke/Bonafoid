//! HTTP handlers for the project CRUD, autosave, duplicate and import/export API.

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use nocode_model::{Project, ProjectId};

use crate::auth::AuthUser;
use crate::error::{require_csrf, ApiError};
use crate::server::AppState;

use super::{sanitize_filename, slugify, store};

/// Listing projection returned for each project.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub version: String,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&store::ProjectSummaryRow> for ProjectSummary {
    fn from(row: &store::ProjectSummaryRow) -> Self {
        Self {
            id: row.id.clone(),
            name: row.name.clone(),
            slug: row.slug.clone(),
            description: row.description.clone(),
            version: row.version.clone(),
            revision: row.revision,
            created_at: row.created_at.clone(),
            updated_at: row.updated_at.clone(),
        }
    }
}

/// A project plus its metadata, flattened into one JSON object.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectDetail {
    #[serde(flatten)]
    pub summary: ProjectSummary,
    pub data: Project,
}

/// Body of `POST /api/v1/projects`.
#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub data: Option<Project>,
}

/// Body of `PUT /api/v1/projects/:id` (optimistic-concurrency write).
#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    pub revision: i64,
    pub data: Project,
}

fn format_timestamp(value: &DateTime<Utc>) -> String {
    value.to_rfc3339()
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

/// Insert a freshly built project under a unique slug and return the 201 detail.
async fn insert_with_unique_slug(
    state: &AppState,
    user_id: &str,
    mut project: Project,
    slug_base: &str,
) -> Result<Response, ApiError> {
    let slug = store::unique_slug(&state.db, user_id, slug_base, None).await?;
    let now = Utc::now();
    project.slug = slug.clone();
    project.created_at = now;
    project.updated_at = now;

    let data = serde_json::to_string(&project).map_err(ApiError::from)?;
    let id = project.id.0.to_string();
    store::insert_project(
        &state.db,
        &id,
        user_id,
        &project.name,
        &slug,
        project.description.as_deref(),
        &project.version,
        &data,
        &format_timestamp(&now),
        &format_timestamp(&now),
    )
    .await?;

    let summary = ProjectSummary {
        id,
        name: project.name.clone(),
        slug,
        description: project.description.clone(),
        version: project.version.clone(),
        revision: 1,
        created_at: format_timestamp(&now),
        updated_at: format_timestamp(&now),
    };
    Ok((
        StatusCode::CREATED,
        Json(ProjectDetail {
            summary,
            data: project,
        }),
    )
        .into_response())
}

/// `GET /api/v1/projects`
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<ProjectSummary>>, ApiError> {
    let rows = store::list_projects(&state.db, &user.id).await?;
    let summaries = rows.iter().map(ProjectSummary::from).collect();
    Ok(Json(summaries))
}

/// `POST /api/v1/projects`
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Json(body): Json<CreateProjectRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("project name is required".into()));
    }

    let mut project = body.data.unwrap_or_else(|| Project::new(name));
    project.name = name.to_string();
    if project.description.is_none() {
        project.description = body.description;
    }
    let slug_base = slugify(name);
    insert_with_unique_slug(&state, &user.id, project, &slug_base).await
}

/// `GET /api/v1/projects/:id`
pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<ProjectDetail>, ApiError> {
    let row = store::get_project(&state.db, &user.id, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let data: Project = serde_json::from_str(&row.data).map_err(|error| {
        tracing::error!(error = %error, project_id = %id, "stored project JSON is corrupt");
        ApiError::Internal
    })?;
    Ok(Json(ProjectDetail {
        summary: ProjectSummary::from(&row.summary),
        data,
    }))
}

/// `PUT /api/v1/projects/:id`
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateProjectRequest>,
) -> Result<Json<ProjectDetail>, ApiError> {
    require_csrf(&headers)?;

    let existing = store::get_project(&state.db, &user.id, &id)
        .await?
        .ok_or(ApiError::NotFound)?;

    let mut project = body.data;
    let name = project.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("project name is required".into()));
    }
    project.name = name.to_string();

    let parsed_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    project.id = ProjectId(parsed_id);
    project.slug = existing.summary.slug.clone();
    project.created_at = parse_timestamp(&existing.summary.created_at).unwrap_or_else(Utc::now);
    let now = Utc::now();
    project.updated_at = now;

    let data = serde_json::to_string(&project).map_err(ApiError::from)?;
    let written = store::update_project(
        &state.db,
        &id,
        &user.id,
        &project.name,
        project.description.as_deref(),
        &project.version,
        &data,
        body.revision,
        &format_timestamp(&now),
    )
    .await?;

    if !written {
        return Err(ApiError::Conflict(
            "project was modified by another session".into(),
        ));
    }

    let summary = ProjectSummary {
        id: existing.summary.id,
        name: project.name.clone(),
        slug: existing.summary.slug,
        description: project.description.clone(),
        version: project.version.clone(),
        revision: body.revision + 1,
        created_at: existing.summary.created_at,
        updated_at: format_timestamp(&now),
    };
    Ok(Json(ProjectDetail {
        summary,
        data: project,
    }))
}

/// `DELETE /api/v1/projects/:id`
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_csrf(&headers)?;
    let removed = store::delete_project(&state.db, &user.id, &id).await?;
    if !removed {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/projects/:id/duplicate`
pub async fn duplicate(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let existing = store::get_project(&state.db, &user.id, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let mut project: Project = serde_json::from_str(&existing.data).map_err(|error| {
        tracing::error!(error = %error, project_id = %id, "stored project JSON is corrupt");
        ApiError::Internal
    })?;

    project.id = ProjectId::new();
    project.name = format!("{} Copy", existing.summary.name);
    project.published_at = None;
    let slug_base = slugify(&format!("{}-copy", existing.summary.slug));
    insert_with_unique_slug(&state, &user.id, project, &slug_base).await
}

/// `POST /api/v1/projects/import`
pub async fn import(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Json(mut project): Json<Project>,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;
    let name = project.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::BadRequest("imported project has no name".into()));
    }
    project.name = name.clone();
    project.id = ProjectId::new();
    project.published_at = None;
    let slug_base = slugify(&name);
    insert_with_unique_slug(&state, &user.id, project, &slug_base).await
}

/// `GET /api/v1/projects/:id/export`
pub async fn export(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let row = store::get_project(&state.db, &user.id, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let project: Project = serde_json::from_str(&row.data).map_err(|error| {
        tracing::error!(error = %error, project_id = %id, "stored project JSON is corrupt");
        ApiError::Internal
    })?;
    let body = serde_json::to_string_pretty(&project).map_err(ApiError::from)?;
    let filename = format!("{}.json", sanitize_filename(&row.summary.slug));
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response())
}
