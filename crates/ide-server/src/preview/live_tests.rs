//! End-to-end verification of a generated app, through the preview proxy.
//!
//! The ignored compile fixtures prove that the generated code COMPILES.
//! They never run it, so they cannot catch a route that does not answer, a
//! migration that does not apply, or a payload the backend rejects. This
//! module does the other half: it generates a project, builds its backend,
//! runs it, and drives the generated API through the IDE server's preview
//! proxy — the same path a user's browser takes.
//!
//! It is `#[ignore]`d because it needs cargo, the network (the generated
//! crate has its own dependency tree) and a couple of minutes. Run it with:
//!
//! ```text
//! cargo test -p nocode-ide-server -- --ignored live --test-threads=1
//! ```
//!
//! The backend target is built, not the frontend: the frontend needs trunk
//! and the wasm32 target, and this is about the API contract. The page
//! rendering leg is covered by the manual procedure in
//! `docs/verify-preview.md`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use nocode_model::entity::{EntityField, EntityFieldId, EntityFieldType};
use nocode_model::form::{FieldType, FormField, FormFieldId};
use nocode_model::page::{Page, PageComponent};
use nocode_model::{DatabaseDriver, Entity, Form, Project};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tower::ServiceExt;

use crate::preview::manager::PreviewConfig;
use crate::server::AppState;

/// A project with one entity and one form, wired so the generated backend
/// has something to serve.
fn demo_project() -> Project {
    let mut project = Project::new("Live Check");
    project.settings.database.driver = DatabaseDriver::SQLite;

    let mut product = Entity::new("Product");
    product.fields.push(EntityField {
        id: EntityFieldId::new(),
        name: "id".to_string(),
        field_type: EntityFieldType::Uuid,
        nullable: false,
        unique: true,
        default: None,
        auto_generate: None,
        validation: None,
        comment: None,
        order: 0,
    });
    product.fields.push(EntityField {
        id: EntityFieldId::new(),
        name: "name".to_string(),
        field_type: EntityFieldType::String {
            max_length: Some(120),
        },
        nullable: false,
        unique: false,
        default: None,
        auto_generate: None,
        validation: None,
        comment: None,
        order: 1,
    });
    project.entities.insert(product.id.clone(), product);

    let mut form = Form::new("Contact");
    form.fields.push(FormField {
        id: FormFieldId::new(),
        name: "email".to_string(),
        label: "Email".to_string(),
        field_type: FieldType::Email,
        required: true,
        default_value: None,
        validation_rules: vec![],
        conditional_visibility: None,
        ui_props: Default::default(),
        order: 0,
    });
    project.forms.insert(form.id.clone(), form);

    let mut page = Page::new("Home");
    page.path = "/".to_string();
    page.components.push(PageComponent {
        id: nocode_model::page::ComponentId::new(),
        component: nocode_model::page::ComponentType::Html {
            content: "Live check".to_string(),
            sanitize: true,
        },
        props: Default::default(),
        bindings: vec![],
        events: vec![],
        conditions: vec![],
        slots: Default::default(),
        style_overrides: Default::default(),
    });
    project.pages.insert(page.id.clone(), page);

    project
}

/// A router wired the way the server is, with a preview root under the
/// temp directory so the test never touches a developer's real workspaces.
async fn test_router() -> Router {
    static KEYS: OnceLock<Arc<crate::auth::JwtKeys>> = OnceLock::new();
    let keys = KEYS
        .get_or_init(|| Arc::new(crate::auth::JwtKeys::generate_ephemeral().expect("test keys")))
        .clone();
    let db = crate::db::connect_sqlite_memory().await.expect("db");
    crate::migration::run(&db).await.expect("migrations");
    let root = std::env::temp_dir().join("nocode-preview-live");
    // The generated crate is its own workspace, so it needs its own target
    // directory — but the workspace's target already holds the compiled
    // axum/sea-orm/tokio the generated backend depends on, and reusing it
    // turns a 277-crate cold build into a short one.
    let cargo_target_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join("preview-live");
    let state = AppState::with_preview(
        db,
        keys,
        PreviewConfig {
            root: root.clone(),
            api_base: API_BASE.into(),
            // A cold `cargo build` of the generated backend takes a while,
            // especially the first time when its dependencies have to be
            // fetched. The production default is 900s.
            timeout: Duration::from_secs(900),
            max_parallel: 1,
            cargo_target_dir,
            trunk_offline: false,
            preview_database_url: None,
        },
    );
    crate::server::build_router(state, "apps/ide-web/dist")
}

/// Register a user and return (cookie header, csrf token).
async fn register(app: &Router) -> (String, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"email":"live@example.com","password":"supersecret"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let cookies: Vec<String> = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| {
            value
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    let csrf = cookies
        .iter()
        .find(|cookie| cookie.starts_with("nocode_csrf="))
        .expect("a csrf cookie")
        .trim_start_matches("nocode_csrf=")
        .to_string();
    (cookies.join("; "), csrf)
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

/// True when a build failed because the host locked a file cargo was
/// renaming, rather than because the generated code is wrong.
///
/// On Windows, antivirus and the search indexer hold files that cargo is
/// renaming, and the build dies with "os error 32". That is a property of
/// the host, not of the generated code, and retrying is what a developer
/// would do.
fn is_file_lock_error(logs: &str) -> bool {
    logs.contains("os error 32")
        || logs.contains("being used by another process")
        || logs.contains("Permission denied")
}

/// Queue a build and wait for it to finish.
///
/// Returns the build id and the build log. A build that failed because of
/// a file lock is retried; any other failure is returned as-is.
async fn build(
    app: &Router,
    cookie: &str,
    csrf: &str,
    project_id: &str,
    target: &str,
    attempts: usize,
) -> (String, String) {
    for attempt in 1..=attempts {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/preview/build"))
                    .header("content-type", "application/json")
                    .header("cookie", cookie)
                    .header("x-csrf-token", csrf)
                    .body(Body::from(format!(r#"{{"target":"{target}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let record = json_body(response).await;
        let build_id = record["id"].as_str().expect("a build id").to_string();

        let mut state = record["state"].as_str().unwrap_or("").to_string();
        let mut error = record["error"].as_str().unwrap_or("").to_string();
        let mut logs = String::new();
        for _ in 0..900 {
            if state == "ready" || state == "failed" {
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(format!("/api/v1/builds/{build_id}"))
                        .header("cookie", cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let record = json_body(response).await;
            state = record["state"].as_str().unwrap_or("").to_string();
            error = record["error"].as_str().unwrap_or("").to_string();
            if let Some(entries) = record["logs"].as_array() {
                logs = entries
                    .iter()
                    .map(|line| line.as_str().unwrap_or("").to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }

        if state == "ready" {
            return (build_id, logs);
        }

        assert!(
            is_file_lock_error(&logs) && attempt < attempts,
            "the build failed on attempt {attempt}: {error}\n{logs}"
        );
        eprintln!("retrying the build after a file lock (attempt {attempt})");
    }
    unreachable!("the loop above either returns or panics");
}

/// Generate, build, run, and drive the generated app's API.
#[tokio::test]
#[ignore = "needs cargo, the network and a few minutes"]
async fn a_generated_app_serves_its_api_through_the_preview_proxy() {
    let app = test_router().await;
    let (cookie, csrf) = register(&app).await;

    // 1. Create the project.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/projects")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .header("x-csrf-token", &csrf)
                .body(Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "name": "Live Check",
                        "data": demo_project(),
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await;
    let project_id = created["id"].as_str().expect("a project id").to_string();

    // 2. Build the backend and wait for it.
    let (build_id, build_log) = build(&app, &cookie, &csrf, &project_id, "backend", 3).await;
    eprintln!("build log:\n{build_log}");

    // 3. Run it.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/builds/{build_id}/run"))
                .header("cookie", &cookie)
                .header("x-csrf-token", &csrf)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the preview did not start"
    );
    let run = json_body(response).await;
    assert_eq!(run["status"].as_str(), Some("starting"), "{run}");

    // 4. Wait for the backend to answer on its port. A run starts in
    //    "starting" and is promoted to "running" the first time something
    //    connects to it, so the proxy would 404 until then.
    let mut status = "starting".to_string();
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/runs")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let runs = json_body(response).await;
        if let Some(entries) = runs.as_array() {
            if let Some(entry) = entries.first() {
                status = entry["status"].as_str().unwrap_or("").to_string();
                if status == "running" {
                    break;
                }
                if status == "failed" || status == "exited" {
                    panic!("the preview exited: {entry}");
                }
            }
        }
    }
    assert_eq!(status, "running", "the preview never became ready");

    // 5. Drive the generated API through the proxy. This is the same path
    //    the generated frontend takes: the proxy's route is
    //    /preview/{build}/api/* and the generated backend's API root is
    //    /api, so the segment appears in both.
    let proxy = format!("/preview/{build_id}/api");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("{proxy}/v1/product"))
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the entity list did not answer"
    );
    let list = json_body(response).await;
    assert_eq!(list.as_array().map(|items| items.len()), Some(0), "{list}");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{proxy}/v1/product"))
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from(r#"{"name":"Widget"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "the entity create did not answer"
    );
    let created = json_body(response).await;
    let entity_id = created["id"].as_str().expect("an entity id").to_string();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("{proxy}/v1/product/{entity_id}"))
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let fetched = json_body(response).await;
    assert_eq!(fetched["name"].as_str(), Some("Widget"), "{fetched}");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("{proxy}/v1/product/{entity_id}"))
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from(r#"{"name":"Gadget"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the entity update did not answer"
    );
    let updated = json_body(response).await;
    assert_eq!(updated["name"].as_str(), Some("Gadget"), "{updated}");

    // The generated form endpoint: a typed Submit struct per form.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{proxy}/v1/forms/contact"))
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from(r#"{"email":"live@example.com"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "the form submission did not answer"
    );

    // A required form field that is missing must be rejected, which proves
    // the typed Submit struct is enforced rather than ignored.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{proxy}/v1/forms/contact"))
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        response.status().is_client_error(),
        "a form submission missing a required field was accepted"
    );

    // 6. And the entity can be deleted.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("{proxy}/v1/product/{entity_id}"))
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "the entity delete did not answer: {}",
        response.status()
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("{proxy}/v1/product"))
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let list = json_body(response).await;
    assert_eq!(list.as_array().map(|items| items.len()), Some(0), "{list}");
}

/// Build the generated FRONTEND and check that the bundle is served and
/// that it knows which build it belongs to.
///
/// The generated frontend calls `{API_BASE}/api/v1/{entity}`. `API_BASE` is
/// baked into the bundle at compile time, so it has to be this build's own
/// preview URL, including the build id. Without the id the bundle called
/// `/preview/api/v1/...`, which the proxy read as a build called "api" and
/// answered 404. The bundle is a WASM binary, so the literal is checked
/// inside it.
#[tokio::test]
#[ignore = "needs cargo, trunk, the wasm32 target and several minutes"]
async fn a_generated_apps_frontend_is_served_with_its_own_api_base() {
    let app = test_router().await;
    let (cookie, csrf) = register(&app).await;

    // 1. Create the project.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/projects")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .header("x-csrf-token", &csrf)
                .body(Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "name": "Live Frontend",
                        "data": demo_project(),
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await;
    let project_id = created["id"].as_str().expect("a project id").to_string();

    // 2. Build the frontend. The backend is skipped: this is about the
    //    bundle, not the API.
    let (build_id, build_log) = build(&app, &cookie, &csrf, &project_id, "frontend", 4).await;
    eprintln!("build log:\n{build_log}");

    // 3. The index is served.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/preview/{build_id}/"))
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the frontend was not served"
    );
    let index = String::from_utf8(
        axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("index")
            .to_vec(),
    )
    .expect("utf-8");
    assert!(index.contains("<!DOCTYPE html>"), "{index}");

    // 4. The bundle knows its own preview URL.
    let dist = std::env::temp_dir()
        .join("nocode-preview-live")
        .join(&build_id)
        .join("frontend")
        .join("dist");
    let wasm = std::fs::read_dir(&dist)
        .expect("the dist directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("wasm"))
        .expect("a wasm bundle in the dist directory");
    let bytes = std::fs::read(&wasm).expect("read the bundle");
    let haystack = String::from_utf8_lossy(&bytes);
    let expected = format!("{API_BASE}/{build_id}");
    assert!(
        haystack.contains(&expected),
        "the bundle does not carry its own API_BASE {expected}"
    );
}

/// The API base the preview configuration bakes into a frontend bundle.
const API_BASE: &str = "http://127.0.0.1/preview";
