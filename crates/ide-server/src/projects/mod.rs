//! Project CRUD, autosave, duplicate and import/export routes.

pub mod handlers;
pub mod store;

use axum::routing::{get, post};
use axum::Router;
use nocode_model::Project;
use sea_orm::DatabaseConnection;

use crate::error::ApiError;
use crate::server::AppState;

/// The project routes, merged into the main router.
pub fn projects_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/projects",
            get(handlers::list).post(handlers::create),
        )
        .route("/api/v1/projects/import", post(handlers::import))
        .route(
            "/api/v1/projects/:id",
            get(handlers::get)
                .put(handlers::update)
                .delete(handlers::delete),
        )
        .route(
            "/api/v1/projects/:id/duplicate",
            post(handlers::duplicate),
        )
        .route("/api/v1/projects/:id/export", get(handlers::export))
}

/// Load a project owned by `user_id` or return `NotFound`.
pub async fn require_project(
    db: &DatabaseConnection,
    user_id: &str,
    id: &str,
) -> Result<Project, ApiError> {
    let row = store::get_project(db, user_id, id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let project: Project = serde_json::from_str(&row.data).map_err(|error| {
        tracing::error!(error = %error, project_id = %id, "stored project JSON is corrupt");
        ApiError::Internal
    })?;
    Ok(project)
}

/// Convert a display name into a slug: lowercase, alphanumeric runs preserved,
/// every other run collapsed to a single `-`.
pub fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut pending_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if slug.is_empty() {
        "project".to_string()
    } else {
        slug
    }
}

/// Sanitize a value for use as a filename in `Content-Disposition`.
pub fn sanitize_filename(value: &str) -> String {
    let mut name = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            name.push(ch);
        } else {
            name.push('-');
        }
    }
    if name.is_empty() {
        "project".to_string()
    } else {
        name
    }
}
