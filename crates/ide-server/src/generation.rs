//! Code generation API: materialize a project's artifacts on demand.

use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;

use nocode_codegen::GeneratedProject;

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::projects::{require_project, sanitize_filename};
use crate::server::AppState;

/// One generated artifact in a listing.
#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub path: String,
    pub size: usize,
}

/// Summary of every artifact a project generates.
#[derive(Debug, Clone, Serialize)]
pub struct GeneratedFiles {
    pub project_id: String,
    pub file_count: usize,
    pub total_bytes: usize,
    pub files: Vec<FileEntry>,
}

/// The generation routes, merged into the main router.
pub fn generation_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/projects/:id/generate", post(generate))
        .route("/api/v1/projects/:id/files", get(files))
        .route("/api/v1/projects/:id/archive", get(archive))
}

/// Load an owned project and generate its artifacts in memory.
///
/// Returns the generated project together with its slug for download naming.
async fn build(
    state: &AppState,
    user_id: &str,
    id: &str,
) -> Result<(GeneratedProject, String), ApiError> {
    let project = require_project(&state.db, user_id, id).await?;
    let slug = project.slug.clone();
    let generated = nocode_codegen::generate_project_in_memory(&project)
        .await
        .map_err(ApiError::from)?;
    Ok((generated, slug))
}

/// Build a listing summary from generated artifacts.
fn summarize(project_id: String, generated: &GeneratedProject) -> GeneratedFiles {
    let mut total_bytes = 0usize;
    let mut files: Vec<FileEntry> = Vec::new();
    for (path, content) in generated.files() {
        total_bytes += content.len();
        files.push(FileEntry {
            path: path.to_string_lossy().replace('\\', "/"),
            size: content.len(),
        });
    }
    GeneratedFiles {
        project_id,
        file_count: files.len(),
        total_bytes,
        files,
    }
}

/// `POST /api/v1/projects/:id/generate` — regenerate and list the artifacts.
async fn generate(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<GeneratedFiles>, ApiError> {
    let (generated, _slug) = build(&state, &user.id, &id).await?;
    Ok(Json(summarize(id, &generated)))
}

/// `GET /api/v1/projects/:id/files` — list the artifacts without downloading.
async fn files(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<GeneratedFiles>, ApiError> {
    let (generated, _slug) = build(&state, &user.id, &id).await?;
    Ok(Json(summarize(id, &generated)))
}

/// `GET /api/v1/projects/:id/archive` — download the artifacts as a ZIP.
async fn archive(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let (generated, slug) = build(&state, &user.id, &id).await?;
    let bytes = generated.to_zip().map_err(ApiError::from)?;
    let filename = sanitize_filename(&slug);
    let headers = [
        (header::CONTENT_TYPE, "application/zip".to_string()),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}.zip\""),
        ),
    ];
    Ok((headers, bytes).into_response())
}
