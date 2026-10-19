//! Preview build API: start, poll, cancel and inspect hosted builds.
//!
//! Route map (all project-scoped routes require an authenticated owner):
//!
//! - `POST   /api/v1/projects/:id/preview/build` -> start a build
//! - `GET    /api/v1/preview/capabilities`      -> host toolchain probe
//! - `GET    /api/v1/builds`                    -> builds owned by the caller
//! - `GET    /api/v1/builds/:id`                -> one build + logs
//! - `DELETE /api/v1/builds/:id`                -> cancel and drop the workspace
//!
//! Static serving of `dist/` and the API reverse proxy live in
//! [`crate::preview_server`] because they need the process manager's port.

//! Preview subsystem: build a project, run it, serve it, proxy to it.
//!
//! Build routes (authenticated, owner-scoped):
//!
//! - `POST   /api/v1/projects/:id/preview/build` -> start a build
//! - `GET    /api/v1/preview/capabilities`      -> host toolchain probe
//! - `GET    /api/v1/builds`                    -> builds owned by the caller
//! - `GET    /api/v1/builds/:id`                -> one build + logs
//! - `DELETE /api/v1/builds/:id`                -> cancel and drop the workspace
//!
//! Run routes (a ready build's backend can be started and stopped):
//!
//! - `POST   /api/v1/builds/:id/run`            -> start the generated backend
//! - `GET    /api/v1/runs`                       -> running previews
//! - `POST   /api/v1/builds/:id/stop`            -> stop it
//!
//! Serving routes (also owner-scoped, see [`serve`]):
//!
//! - `GET /preview/{build_id}/*`                 -> the built frontend
//! - `ALL /preview/{build_id}/api/*`             -> proxy to the running backend

pub mod build;
pub mod manager;
pub mod runs;
pub mod serve;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::projects::{require_project, slugify};
use crate::server::AppState;

pub use build::{capabilities, run_build};
pub use manager::{
    BuildManager, BuildRecord, BuildState, BuildTarget, PreviewCapabilities, PreviewConfig,
};
pub use runs::{RunConfig, RunManager, RunRecord, RunStatus};
pub use serve::preview_server_router;

/// Body of `POST /api/v1/projects/:id/preview/build`.
#[derive(Debug, Default, Deserialize)]
pub struct StartBuildRequest {
    /// Which artifacts to build; defaults to backend + frontend.
    #[serde(default)]
    pub target: Option<BuildTarget>,
}

/// The preview routes, merged into the main router.
pub fn preview_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/projects/:id/preview/build", post(start_build))
        .route("/api/v1/preview/capabilities", get(capabilities_handler))
        .route("/api/v1/builds", get(list_builds))
        .route("/api/v1/builds/:id", get(get_build).delete(cancel_build))
        .route("/api/v1/builds/:id/run", post(start_run))
        .route("/api/v1/builds/:id/stop", post(stop_run))
        .route("/api/v1/runs", get(list_runs))
}

/// `POST /api/v1/projects/:id/preview/build` — queue a build and return at once.
async fn start_build(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(project_id): Path<String>,
    Json(body): Json<StartBuildRequest>,
) -> Result<(axum::http::StatusCode, Json<BuildRecord>), ApiError> {
    crate::error::require_csrf(&headers)?;

    let project = require_project(&state.db, &user.id, &project_id).await?;
    let target = body.target.unwrap_or(BuildTarget::All);
    let build_id = Uuid::new_v4().to_string();

    // Cargo cannot relink a binary that is still executing, so a rebuild
    // always invalidates the project's running preview.
    let stopped = state.runs.stop_for_project(&project.id.to_string()).await;

    let record = state.preview.enqueue(
        build_id.clone(),
        project.id.to_string(),
        user.id.clone(),
        target,
    );
    if !stopped.is_empty() {
        state.preview.append_logs(
            &build_id,
            vec![format!(
                "stopped {} running preview(s) for this project: {}",
                stopped.len(),
                stopped.join(", ")
            )],
        );
    }

    let manager = state.preview.clone();
    let project_for_task = project.clone();
    tokio::spawn(async move {
        run_build(manager, build_id, project_for_task, target).await;
    });

    tracing::info!(
        project = %project.name,
        slug = %slugify(&project.name),
        "preview build queued"
    );
    Ok((axum::http::StatusCode::ACCEPTED, Json(record)))
}

/// `GET /api/v1/preview/capabilities` — probe the host toolchain.
async fn capabilities_handler(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Json<PreviewCapabilities> {
    Json(capabilities(&state.preview).await)
}

/// `GET /api/v1/builds` — builds owned by the caller, newest first.
async fn list_builds(State(state): State<AppState>, user: AuthUser) -> Json<Vec<BuildRecord>> {
    Json(state.preview.list_for_user(&user.id))
}

/// `GET /api/v1/builds/:id` — one build plus its logs.
async fn get_build(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<BuildRecord>, ApiError> {
    state
        .preview
        .get_owned(&id, &user.id)
        .map(Json)
        .ok_or(ApiError::NotFound)
}

/// `DELETE /api/v1/builds/:id` — cancel, remove the record and clean up disk.
async fn cancel_build(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    crate::error::require_csrf(&headers)?;

    let record = state
        .preview
        .get_owned(&id, &user.id)
        .ok_or(ApiError::NotFound)?;

    // Flag first: the worker checks this between phases and stops instead of
    // marking the (soon removed) record as ready.
    state.preview.mark_cancelled(&id);
    state.preview.remove(&id);

    // Drop the workspace. Failures here are logged, not surfaced: the record
    // is gone either way and a stale directory is harmless.
    if let Err(error) = tokio::fs::remove_dir_all(&record.workspace).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(build_id = %id, %error, "failed to remove preview workspace");
        }
    }

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// `POST /api/v1/builds/:id/run` — start the generated backend of a ready build.
async fn start_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<RunRecord>, ApiError> {
    crate::error::require_csrf(&headers)?;

    let build = state
        .preview
        .get_owned(&id, &user.id)
        .ok_or(ApiError::NotFound)?;
    if build.state != BuildState::Ready {
        return Err(ApiError::Conflict(format!(
            "build is not ready (state {:?})",
            build.state
        )));
    }

    // Idempotent: an already-running preview is returned as-is.
    if let Some(existing) = state.runs.running_for(&id, &user.id) {
        return Ok(Json(existing));
    }

    let binary = build.backend_binary.clone().ok_or_else(|| {
        ApiError::Conflict(
            "this build has no backend binary (build the backend target first)".into(),
        )
    })?;

    // The generated backend is compiled for the project's driver, so the run
    // must speak the same one. SQLite previews get a throwaway database inside
    // the build workspace; other drivers need the project's configured URL.
    let project = require_project(&state.db, &user.id, &build.project_id).await?;
    let database_url = match project.settings.database.driver {
        nocode_model::DatabaseDriver::SQLite => "sqlite://preview.db?mode=rwc".to_string(),
        _ => project.settings.database.url.trim().to_string(),
    };
    if database_url.is_empty() {
        return Err(ApiError::Conflict(
            "this project needs a database URL before it can run (Project Settings)".into(),
        ));
    }

    let record = state
        .runs
        .start(
            &id,
            &user.id,
            &build.project_id,
            binary,
            &[],
            build.workspace.clone(),
            vec![
                ("DATABASE_URL".to_string(), database_url),
                ("NOCODE_PREVIEW_BUILD".to_string(), id.clone()),
            ],
        )
        .await
        .map_err(ApiError::Conflict)?;

    tracing::info!(build_id = %id, port = record.port, "preview backend starting");
    Ok(Json(record))
}

/// `POST /api/v1/builds/:id/stop` — stop the running backend and free its port.
async fn stop_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<RunRecord>, ApiError> {
    crate::error::require_csrf(&headers)?;

    if state.preview.get_owned(&id, &user.id).is_none() {
        return Err(ApiError::NotFound);
    }
    if !state.runs.running_for(&id, &user.id).is_some() {
        return Err(ApiError::Conflict("preview backend is not running".into()));
    }
    let stopped = state.runs.stop(&id).await;
    tracing::info!(build_id = %id, stopped, "preview backend stopped");

    let now = chrono::Utc::now();
    Ok(Json(RunRecord {
        build_id: id,
        user_id: user.id.clone(),
        project_id: String::new(),
        port: 0,
        pid: None,
        status: RunStatus::Stopped,
        exit_code: None,
        started_at: now,
        last_used_at: now,
        requests: 0,
    }))
}

/// `GET /api/v1/runs` — previews the caller currently has running.
///
/// Each entry is refreshed first so a crashed preview shows up as `failed`
/// instead of lingering in `starting`.
async fn list_runs(State(state): State<AppState>, user: AuthUser) -> Json<Vec<RunRecord>> {
    for record in state.runs.list_for_user(&user.id) {
        state.runs.refresh(&record.build_id).await;
    }
    Json(state.runs.list_for_user(&user.id))
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use std::sync::{Arc, OnceLock};
    use std::time::Duration;
    use tower::ServiceExt;

    async fn test_router() -> Router {
        static KEYS: OnceLock<Arc<crate::auth::JwtKeys>> = OnceLock::new();
        let keys = KEYS
            .get_or_init(|| {
                Arc::new(crate::auth::JwtKeys::generate_ephemeral().expect("test keys"))
            })
            .clone();
        let db = crate::db::connect_sqlite_memory().await.expect("db");
        crate::migration::run(&db).await.expect("migrations");
        let state = AppState::with_preview(
            db,
            keys,
            PreviewConfig {
                root: std::env::temp_dir().join("nocode-preview-routes"),
                api_base: "http://127.0.0.1/preview".into(),
                timeout: Duration::from_secs(30),
                max_parallel: 1,
                cargo_target_dir: std::env::temp_dir().join("nocode-preview-routes-target"),
                trunk_offline: false,
            },
        );
        crate::server::build_router(state, "apps/ide-web/dist")
    }

    /// Register a user and return (cookie header value, csrf token).
    async fn register(app: &Router) -> (String, String) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"email":"routes@example.com","password":"supersecret"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        let cookies: Vec<String> = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .map(|value| value.to_str().unwrap().to_string())
            .collect();
        let csrf = cookies
            .iter()
            .find(|cookie| cookie.starts_with("nocode_csrf="))
            .map(|cookie| {
                cookie["nocode_csrf=".len()..]
                    .split(';')
                    .next()
                    .unwrap()
                    .to_string()
            })
            .expect("csrf cookie");
        (cookies.join("; "), csrf)
    }

    async fn json_body(response: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).expect("JSON body")
    }

    #[tokio::test]
    async fn capabilities_requires_authentication() {
        let app = test_router().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/preview/capabilities")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
    }

    #[tokio::test]
    async fn capabilities_reports_the_toolchain() {
        let app = test_router().await;
        let (cookies, _) = register(&app).await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/preview/capabilities")
                    .header("cookie", &cookies)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = json_body(response).await;
        assert!(body.get("cargo").and_then(|v| v.as_bool()).is_some());
        assert!(body.get("trunk").and_then(|v| v.as_bool()).is_some());
    }

    #[tokio::test]
    async fn build_routes_require_csrf_and_return_an_accepted_record() {
        let app = test_router().await;
        let (cookies, csrf) = register(&app).await;

        let create = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("cookie", &cookies)
                    .header("x-csrf-token", &csrf)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"Routes"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create.status(), 201);
        let created = json_body(create).await;
        let project_id = created["id"].as_str().expect("project id").to_string();

        let no_csrf = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/preview/build"))
                    .header("cookie", &cookies)
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            no_csrf.status(),
            403,
            "missing CSRF header must be rejected"
        );

        let accepted = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/preview/build"))
                    .header("cookie", &cookies)
                    .header("x-csrf-token", &csrf)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"target":"backend"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(accepted.status(), 202);
        let record = json_body(accepted).await;
        assert_eq!(record["state"], "queued");
        assert_eq!(record["target"], "backend");
        assert!(record["id"].as_str().is_some());
    }

    #[tokio::test]
    async fn builds_are_owner_scoped() {
        let app = test_router().await;
        let (cookies, _) = register(&app).await;

        let unknown = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/builds/does-not-exist")
                    .header("cookie", &cookies)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unknown.status(), 404);
    }

    #[tokio::test]
    async fn preview_serving_rejects_unknown_builds() {
        let app = test_router().await;
        let (cookies, _) = register(&app).await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/preview/some-build-id/")
                    .header("cookie", &cookies)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 404, "unknown build must not be served");
    }

    #[tokio::test]
    async fn proxying_requires_a_running_backend() {
        let app = test_router().await;
        let (cookies, _) = register(&app).await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/preview/some-build-id/api/health")
                    .header("cookie", &cookies)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
    }
}
