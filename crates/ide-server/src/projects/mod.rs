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
        .route("/api/v1/projects/:id/duplicate", post(handlers::duplicate))
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

/// Maximum length of a project name, in characters.
pub const MAX_PROJECT_NAME: usize = 120;

/// Validate a user-supplied project name and return it trimmed.
///
/// The name is interpolated into the generated `Cargo.toml` and into generated
/// Rust source. Codegen escapes what it emits (a quote and a newline used to be
/// enough to inject TOML sections), but escaping is the second line of defence:
/// a control character in a project name has no legitimate use, so it is
/// refused at the API boundary rather than stored and neutralised later.
///
/// Length is bounded because the name travels into manifests, file headers and
/// the IDE's own list views.
pub fn validate_project_name(name: &str) -> Result<String, ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("project name is required".into()));
    }
    if trimmed.chars().count() > MAX_PROJECT_NAME {
        return Err(ApiError::BadRequest(format!(
            "project name must be at most {MAX_PROJECT_NAME} characters"
        )));
    }
    if let Some(bad) = trimmed.chars().find(|ch| ch.is_control()) {
        return Err(ApiError::BadRequest(format!(
            "project name must not contain control characters (found U+{:04X})",
            bad as u32
        )));
    }
    Ok(trimmed.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_is_accepted_and_trimmed() {
        assert_eq!(
            validate_project_name("  Sample Commerce  ").expect("valid"),
            "Sample Commerce"
        );
        assert_eq!(
            validate_project_name("Ünïcode 名前").expect("valid"),
            "Ünïcode 名前"
        );
    }

    #[test]
    fn an_empty_name_is_refused() {
        for bad in ["", "   ", "\t\n"] {
            let error = validate_project_name(bad).expect_err("empty name");
            assert!(matches!(error, ApiError::BadRequest(_)), "{bad:?}");
        }
    }

    #[test]
    fn an_overlong_name_is_refused() {
        let too_long = "a".repeat(MAX_PROJECT_NAME + 1);
        assert!(validate_project_name(&too_long).is_err());
        let at_limit = "a".repeat(MAX_PROJECT_NAME);
        assert!(validate_project_name(&at_limit).is_ok());
    }

    /// The name reaches the generated Cargo.toml and Rust source. A newline is
    /// what turns a quoted TOML string into injected sections, so control
    /// characters are refused before the project is ever stored.
    #[test]
    fn control_characters_are_refused() {
        for bad in [
            "evil\"\n[dependencies]\nmalicious = \"1\"",
            "line\nbreak",
            "carriage\rreturn",
            "nul\u{0}byte",
            "tab\tinside",
        ] {
            let error = validate_project_name(bad).expect_err("control character");
            assert!(
                format!("{error}").contains("control characters"),
                "{bad:?}: {error}"
            );
        }
    }
}
