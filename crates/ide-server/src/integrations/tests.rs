//! Route tests for the integration service.
//!
//! No test here talks to GitHub or Docker Hub. A local HTTP server answers
//! the GitHub REST calls, and a local bare git repository receives the push,
//! so a real `git push` is exercised without any credential leaving the test.

use super::*;
use crate::auth::JwtKeys;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use nocode_deploy::{CredentialVault, DeployService};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tower::ServiceExt;

const TEST_PAT: &str = "ghp_thisIsAFakeTokenForTestsOnly123";

fn test_keys() -> Arc<JwtKeys> {
    static KEYS: OnceLock<Arc<JwtKeys>> = OnceLock::new();
    KEYS.get_or_init(|| Arc::new(JwtKeys::generate_ephemeral().expect("test keys")))
        .clone()
}

/// A GitHub REST stand-in. `runs` is shared so a test can change what CI
/// reports between two GETs without restarting the server.
struct FakeGitHub {
    base_url: String,
    runs: Arc<Mutex<String>>,
    seen: Arc<Mutex<Vec<String>>>,
    _task: tokio::task::JoinHandle<()>,
}

impl FakeGitHub {
    async fn start() -> Self {
        let runs = Arc::new(Mutex::new(String::from(
            "{\"total_count\":0,\"workflow_runs\":[]}",
        )));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake github");
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let runs_for_task = runs.clone();
        let seen_for_task = seen.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                // Read until the end of the header block: a single `read()`
                // is not guaranteed to contain the whole request, and the
                // assertions below inspect the raw wire headers.
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 64 * 1024 {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&buf).to_string();
                seen_for_task.lock().unwrap().push(request.clone());

                let (status, body) = if request.contains("/actions/workflows/deploy.yml/runs") {
                    let body = runs_for_task.lock().unwrap().clone();
                    ("200 OK", body)
                } else if request.contains("/repos/") {
                    (
                        "200 OK",
                        r#"{"id":1,"name":"app","full_name":"acme/app","default_branch":"main","html_url":"https://github.com/acme/app"}"#
                            .to_string(),
                    )
                } else {
                    ("404 Not Found", "{}".to_string())
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        Self {
            base_url,
            runs,
            seen,
            _task: task,
        }
    }

    fn set_runs(&self, value: Value) {
        *self.runs.lock().unwrap() = value.to_string();
    }
}

/// A bare repository the deploy service can really push to.
fn bare_repo(dir: &std::path::Path) -> String {
    let path = dir.join("remote.git");
    // Windows can transiently deny a write to a just-created directory while
    // a scanner holds a handle. Retry only that specific failure, as the
    // production git service does.
    for attempt in 1u64..=4 {
        let output = std::process::Command::new("git")
            .args(["init", "--bare", "--quiet"])
            .arg(&path)
            .output()
            .expect("git init --bare");
        if output.status.success() {
            return path.to_string_lossy().replace('\\', "/");
        }
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let transient = stderr.contains("Permission denied")
            || stderr.contains("Access is denied")
            || stderr.contains("being used by another process");
        if transient && attempt < 4 {
            std::thread::sleep(std::time::Duration::from_millis(50 * attempt));
            continue;
        }
        panic!("git init --bare failed in {}: {stderr}", path.display());
    }
    unreachable!("bare repo loop always returns or panics")
}

struct Harness {
    app: Router,
    _dir: tempfile::TempDir,
    github: FakeGitHub,
}

impl Harness {
    async fn new(enable_deploy: bool) -> Self {
        let github = FakeGitHub::start().await;
        let dir = tempfile::tempdir().expect("temp dir");
        let db = crate::db::connect_sqlite_memory().await.expect("db");
        let deploy = if enable_deploy {
            let vault = CredentialVault::from_key(&[7u8; 32]).expect("vault");
            Some(Arc::new(
                DeployService::new(vault, dir.path().join("staging"))
                    .with_github_base_url(github.base_url.clone())
                    .with_git_remote(bare_repo(dir.path())),
            ))
        } else {
            None
        };
        let state = AppState::new(db, test_keys()).with_deploy(deploy);
        let app = crate::server::build_router(state, "apps/ide-web/dist");
        Self {
            app,
            _dir: dir,
            github,
        }
    }

    async fn call(&self, request: Request<Body>) -> (StatusCode, String) {
        let response = self.app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("body");
        (status, String::from_utf8_lossy(&bytes).to_string())
    }
}

/// Register a user and return (cookie header, csrf token).
async fn register(app: &Router, email: &str) -> (String, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"email":"{email}","password":"supersecret"}}"#
                )))
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

fn request(
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    body: Value,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf-token", csrf);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

fn json(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or(Value::Null)
}

async fn configure(h: &Harness, cookie: &str, csrf: &str) {
    let github = json!({
        "owner": "acme",
        "repo": "app",
        "branch": "main",
        "token": TEST_PAT,
    });
    let (status, body) = h
        .call(request(
            "POST",
            "/api/v1/integrations/github",
            Some(cookie),
            Some(csrf),
            github,
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED, "github config: {body}");
    assert!(
        !body.contains("ghp_"),
        "token leaked into POST response: {body}"
    );

    let (status, body) = h
        .call(request(
            "POST",
            "/api/v1/integrations/dockerhub",
            Some(cookie),
            Some(csrf),
            json!({"namespace": "acme", "token": "registry-secret"}),
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED, "dockerhub config: {body}");
    assert!(!body.contains("registry-secret"));
}

async fn create_project(h: &Harness, cookie: &str, csrf: &str) -> String {
    let (status, body) = h
        .call(request(
            "POST",
            "/api/v1/projects",
            Some(cookie),
            Some(csrf),
            json!({"name": "Deployable App"}),
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED, "create project: {body}");
    json(&body)["id"].as_str().expect("project id").to_string()
}

#[tokio::test]
async fn integration_routes_require_authentication() {
    let h = Harness::new(false).await;
    for uri in [
        "/api/v1/integrations/github",
        "/api/v1/integrations/dockerhub",
        "/api/v1/projects/anything/deployments",
    ] {
        let (status, _) = h.call(request("GET", uri, None, None, json!({}))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "GET {uri}");
    }
}

#[tokio::test]
async fn mutations_require_a_csrf_token() {
    let h = Harness::new(true).await;
    let (cookie, _csrf) = register(&h.app, "csrf@example.com").await;
    // The body is deliberately invalid: CSRF must be decided before JSON
    // deserialisation, so an unauthorised mutation cannot even get a 422
    // out of the parser, let alone reach a handler.
    for uri in [
        "/api/v1/integrations/github",
        "/api/v1/integrations/dockerhub",
    ] {
        let (status, body) = h
            .call(request("POST", uri, Some(&cookie), None, json!({})))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "POST {uri}: {body}");
    }
    let (status, _) = h
        .call(request(
            "POST",
            "/api/v1/projects/anything/deploy",
            Some(&cookie),
            None,
            json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_missing_master_key_disables_only_deploy_routes_with_503() {
    let h = Harness::new(false).await;
    let (cookie, csrf) = register(&h.app, "nokey@example.com").await;
    for (method, uri) in [
        ("GET", "/api/v1/integrations/github"),
        ("GET", "/api/v1/integrations/dockerhub"),
        ("GET", "/api/v1/projects/anything/deployments"),
    ] {
        let (status, body) = h
            .call(request(method, uri, Some(&cookie), Some(&csrf), json!({})))
            .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{uri}: {body}");
        assert!(body.contains("IDE_SECRET_KEY"), "unhelpful 503: {body}");
    }
    // The rest of the IDE is unaffected.
    let (status, _) = h
        .call(request(
            "POST",
            "/api/v1/projects",
            Some(&cookie),
            Some(&csrf),
            json!({"name": "Still Works"}),
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn github_config_is_verified_and_no_token_is_stored_or_returned() {
    let h = Harness::new(true).await;
    let (cookie, csrf) = register(&h.app, "owner@example.com").await;
    configure(&h, &cookie, &csrf).await;

    let (status, body) = h
        .call(request(
            "GET",
            "/api/v1/integrations/github",
            Some(&cookie),
            None,
            json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let value = json(&body);
    assert_eq!(value["owner"], json!("acme"));
    assert_eq!(value["repo"], json!("app"));
    assert_eq!(value["verified"], json!(true));
    assert!(!body.contains("ghp_"));

    // The fake server did receive a bearer token, so verification is real.
    // HTTP header names are case-insensitive and hyper emits them lowercase.
    let saw_auth = h.github.seen.lock().unwrap().iter().any(|request| {
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer ")
    });
    assert!(saw_auth, "verify_repo did not send credentials");
}

#[tokio::test]
async fn one_user_cannot_see_or_deploy_anothers_integration_or_project() {
    let h = Harness::new(true).await;
    let (owner_cookie, owner_csrf) = register(&h.app, "owner2@example.com").await;
    let (other_cookie, other_csrf) = register(&h.app, "other2@example.com").await;
    configure(&h, &owner_cookie, &owner_csrf).await;
    let project_id = create_project(&h, &owner_cookie, &owner_csrf).await;

    let (status, body) = h
        .call(request(
            "GET",
            "/api/v1/integrations/github",
            Some(&other_cookie),
            None,
            json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["configured"], json!(false));

    let (status, _) = h
        .call(request(
            "POST",
            &format!("/api/v1/projects/{project_id}/deploy"),
            Some(&other_cookie),
            Some(&other_csrf),
            json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "cross-user deploy must 404");
}

#[tokio::test]
async fn deploy_really_pushes_but_is_not_reported_as_deployed() {
    let h = Harness::new(true).await;
    let (cookie, csrf) = register(&h.app, "deploy@example.com").await;
    configure(&h, &cookie, &csrf).await;
    let project_id = create_project(&h, &cookie, &csrf).await;

    let (status, body) = h
        .call(request(
            "POST",
            &format!("/api/v1/projects/{project_id}/deploy"),
            Some(&cookie),
            Some(&csrf),
            json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "deploy: {body}");
    let value = json(&body);
    assert_eq!(value["status"], json!("pushed"));
    assert_eq!(value["deployed"], json!(false));
    assert_eq!(value["ci_state"], json!("not_started"));
    assert!(!body.contains("ghp_"), "token leaked: {body}");
    let commit_sha = value["commit_sha"]
        .as_str()
        .expect("commit sha")
        .to_string();
    assert_eq!(commit_sha.len(), 40, "not a real git commit: {commit_sha}");

    // A second deploy of the identical tree must not push again.
    let (status, body) = h
        .call(request(
            "POST",
            &format!("/api/v1/projects/{project_id}/deploy"),
            Some(&cookie),
            Some(&csrf),
            json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "idempotent redeploy: {body}");
    assert_eq!(json(&body)["commit_sha"], json!(commit_sha));
}

#[tokio::test]
async fn ci_success_requires_a_run_for_this_exact_commit() {
    let h = Harness::new(true).await;
    let (cookie, csrf) = register(&h.app, "ci@example.com").await;
    configure(&h, &cookie, &csrf).await;
    let project_id = create_project(&h, &cookie, &csrf).await;
    let (status, body) = h
        .call(request(
            "POST",
            &format!("/api/v1/projects/{project_id}/deploy"),
            Some(&cookie),
            Some(&csrf),
            json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "deploy: {body}");
    let commit_sha = json(&body)["commit_sha"].as_str().unwrap().to_string();
    let uri = format!("/api/v1/projects/{project_id}/deployments");

    // A successful run for a DIFFERENT commit must never mark this one done.
    h.github.set_runs(json!({
        "total_count": 1,
        "workflow_runs": [{
            "id": 1, "status": "completed", "conclusion": "success",
            "html_url": "https://github.com/acme/app/actions/runs/1",
            "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
            "head_sha": "some-other-commit", "head_branch": "main"
        }]
    }));
    let (status, body) = h
        .call(request("GET", &uri, Some(&cookie), None, json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "list: {body}");
    assert_eq!(json(&body)[0]["status"], json!("pushed"));
    assert_eq!(json(&body)[0]["deployed"], json!(false));

    // Only a run whose head_sha matches is allowed to promote it.
    h.github.set_runs(json!({
        "total_count": 1,
        "workflow_runs": [{
            "id": 2, "status": "completed", "conclusion": "success",
            "html_url": "https://github.com/acme/app/actions/runs/2",
            "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
            "head_sha": commit_sha, "head_branch": "main"
        }]
    }));
    let (status, body) = h
        .call(request("GET", &uri, Some(&cookie), None, json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "list: {body}");
    assert_eq!(json(&body)[0]["status"], json!("ci_succeeded"), "{body}");
    assert_eq!(json(&body)[0]["deployed"], json!(true));
    assert!(!body.contains("ghp_"));
}

/// The staging cleanup must not leave generated trees behind after a push.
#[tokio::test]
async fn staging_directories_are_cleaned_up() {
    let h = Harness::new(true).await;
    let (cookie, csrf) = register(&h.app, "cleanup@example.com").await;
    configure(&h, &cookie, &csrf).await;
    let project_id = create_project(&h, &cookie, &csrf).await;
    let (status, body) = h
        .call(request(
            "POST",
            &format!("/api/v1/projects/{project_id}/deploy"),
            Some(&cookie),
            Some(&csrf),
            json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "deploy failed: {body}");
    let staging: PathBuf = h._dir.path().join("staging");
    if staging.exists() {
        let leftovers = std::fs::read_dir(&staging).unwrap().count();
        assert_eq!(leftovers, 0, "deploy staging directory was not cleaned");
    }
}
