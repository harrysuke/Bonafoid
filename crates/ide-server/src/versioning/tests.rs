//! Integration tests for the versioning API.
//!
//! Each test builds its own in-memory SQLite database, ephemeral RSA keys and
//! signed bearer tokens. Bearer requests are CSRF-exempt, which keeps fixture
//! setup short; one dedicated test covers cookie sessions with a CSRF token.

use std::sync::{Arc, Once, OnceLock};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response,
    Router,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value as DbValue};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::{
    auth::{
        jwt::{now_secs, Claims},
        JwtKeys,
    },
    db::connect_sqlite_memory,
    server::AppState,
};

use super::versioning_router;

const TEST_KEY_HEX: &str = concat!(
    "00000000", "00000000", "00000000", "00000000", "00000000", "00000000", "00000000", "00000000"
);
const TOKEN_A: &str = "u1";
const TOKEN_B: &str = "u2";
const TENANT_A: &str = "tenant-a";
const TENANT_B: &str = "tenant-b";
const PROJECT_A: &str = "p1";
const PROJECT_B: &str = "p2";

static INIT: Once = Once::new();

fn init_env() {
    INIT.call_once(|| {
        assert_eq!(TEST_KEY_HEX.len(), 64);
        std::env::set_var("IDE_SECRET_KEY", TEST_KEY_HEX);
    });
}

fn keys() -> Arc<JwtKeys> {
    static KEYS: OnceLock<Arc<JwtKeys>> = OnceLock::new();
    KEYS.get_or_init(|| Arc::new(JwtKeys::generate_ephemeral().expect("test keys")))
        .clone()
}

fn raw_token(user: &str, tenant: &str) -> String {
    let claims = Claims::access_for(
        user,
        "user@example.com",
        tenant.to_string(),
        vec!["owner".to_string()],
        None,
        "jti-versioning",
        now_secs(),
    );
    keys().encode(&claims).unwrap()
}

fn bearer(user: &str, tenant: &str) -> String {
    format!("Bearer {}", raw_token(user, tenant))
}

fn token_a() -> String {
    bearer(TOKEN_A, TENANT_A)
}

fn token_b() -> String {
    bearer(TOKEN_B, TENANT_B)
}

async fn fixture_db() -> DatabaseConnection {
    init_env();
    let db = connect_sqlite_memory().await.unwrap();
    for (id, slug) in [(TENANT_A, "tenant-a"), (TENANT_B, "tenant-b")] {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO tenants (id, name, slug) VALUES (?, ?, ?)",
            vec![DbValue::from(id), DbValue::from(id), DbValue::from(slug)],
        ))
        .await
        .unwrap();
    }
    for (id, tenant) in [(TOKEN_A, TENANT_A), (TOKEN_B, TENANT_B)] {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO users (id, email, password_hash, tenant_id) VALUES (?, ?, ?, ?)",
            vec![
                DbValue::from(id),
                DbValue::from(format!("{id}@example.com")),
                DbValue::from("hash"),
                DbValue::from(tenant),
            ],
        ))
        .await
        .unwrap();
    }
    for (id, owner, tenant) in [
        (PROJECT_A, TOKEN_A, TENANT_A),
        (PROJECT_B, TOKEN_B, TENANT_B),
    ] {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO projects (id, user_id, name, slug, version, data, tenant_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            vec![
                DbValue::from(id),
                DbValue::from(owner),
                DbValue::from("Project"),
                DbValue::from(id),
                DbValue::from("1"),
                DbValue::from("{}"),
                DbValue::from(tenant),
            ],
        ))
        .await
        .unwrap();
    }
    db
}

async fn router(db: DatabaseConnection) -> Router {
    Router::<AppState>::new()
        .merge(versioning_router())
        .with_state(AppState::new(db, keys()))
}

fn request(method: &str, uri: &str, token: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", token);
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    builder
        .body(Body::from(
            body.map(|value| value.to_string()).unwrap_or_default(),
        ))
        .unwrap()
}

async fn call(router: &Router, req: Request<Body>) -> (StatusCode, String) {
    let response: Response = router.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

async fn commit_revision(
    router: &Router,
    token: &str,
    project: &str,
    branch: &str,
    message: &str,
    data: Value,
) -> Value {
    let (status, body) = call(
        router,
        request(
            "POST",
            "/api/v1/versioning/commits",
            Some(token),
            Some(json!({
                "project_id": project,
                "branch": branch,
                "message": message,
                "data": data,
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "commit failed: {body}");
    parsed(&body)
}

async fn head_of(router: &Router, token: &str, project: &str, branch: &str) -> String {
    let (status, body) = call(
        router,
        request(
            "GET",
            &format!("/api/v1/versioning/branches?project_id={project}"),
            Some(token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let branches = parsed(&body);
    branches
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == branch)
        .expect("branch missing")["head_revision_id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn create_branch(
    router: &Router,
    token: &str,
    project: &str,
    name: &str,
    from: Option<&str>,
) -> (StatusCode, String) {
    let mut body = json!({"project_id": project, "name": name});
    if let Some(from) = from {
        body["from_revision_id"] = Value::String(from.to_string());
    }
    call(
        router,
        request(
            "POST",
            "/api/v1/versioning/branches",
            Some(token),
            Some(body),
        ),
    )
    .await
}

async fn create_environment(router: &Router, token: &str, project: &str, name: &str) -> String {
    let (status, body) = call(
        router,
        request(
            "POST",
            "/api/v1/versioning/environments",
            Some(token),
            Some(json!({"project_id": project, "name": name, "config": {}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    parsed(&body)["name"].as_str().unwrap().to_string()
}

async fn sql_text(db: &DatabaseConnection, sql: &str, values: Vec<DbValue>) -> Option<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            sql,
            values,
        ))
        .await
        .unwrap()?;
    row.try_get::<String>("", "value").ok()
}

#[tokio::test]
async fn revisions_are_immutable_and_form_a_chain() {
    let db = fixture_db().await;
    let router = router(db).await;
    let token = token_a();
    let first = commit_revision(&router, &token, PROJECT_A, "main", "first", json!({"v": 1})).await;
    let second = commit_revision(
        &router,
        &token,
        PROJECT_A,
        "main",
        "second",
        json!({"v": 2}),
    )
    .await;

    assert!(first["parent_id"].is_null());
    assert_eq!(second["parent_id"], first["id"]);
    assert_ne!(first["id"], second["id"]);

    let (status, body) = call(
        &router,
        request(
            "GET",
            &format!("/api/v1/versioning/history?project_id={PROJECT_A}&branch=main"),
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let history = parsed(&body);
    assert_eq!(history[0]["id"], second["id"]);
    assert_eq!(history[1]["id"], first["id"]);
    assert_eq!(history[1]["data"], json!({"v": 1}));

    // There is deliberately no update route for a revision.
    let (status, _) = call(
        &router,
        request(
            "PATCH",
            &format!(
                "/api/v1/versioning/revisions/{}",
                first["id"].as_str().unwrap()
            ),
            Some(&token),
            Some(json!({"message": "tampered"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    let (status, body) = call(
        &router,
        request(
            "GET",
            &format!(
                "/api/v1/versioning/revisions/{}",
                first["id"].as_str().unwrap()
            ),
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parsed(&body)["message"], json!("first"));
}

#[tokio::test]
async fn commit_on_one_branch_does_not_move_another_branch_head() {
    let db = fixture_db().await;
    let router = router(db).await;
    let token = token_a();
    let root = commit_revision(&router, &token, PROJECT_A, "main", "root", json!({"v": 0})).await;
    let (status, body) = create_branch(
        &router,
        &token,
        PROJECT_A,
        "dev",
        Some(root["id"].as_str().unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(parsed(&body)["head_revision_id"], root["id"]);

    let main_next = commit_revision(
        &router,
        &token,
        PROJECT_A,
        "main",
        "main work",
        json!({"v": 1}),
    )
    .await;
    assert_eq!(head_of(&router, &token, PROJECT_A, "dev").await, root["id"]);
    assert_eq!(
        head_of(&router, &token, PROJECT_A, "main").await,
        main_next["id"]
    );

    let dev_next = commit_revision(
        &router,
        &token,
        PROJECT_A,
        "dev",
        "dev work",
        json!({"v": 2}),
    )
    .await;
    assert_eq!(
        head_of(&router, &token, PROJECT_A, "dev").await,
        dev_next["id"]
    );
    assert_eq!(
        head_of(&router, &token, PROJECT_A, "main").await,
        main_next["id"]
    );
    assert_eq!(dev_next["parent_id"], main_next["id"]);

    // The chain is shared; the branch POINTERS are isolated.
    let (status, body) = call(
        &router,
        request(
            "GET",
            &format!("/api/v1/versioning/history?project_id={PROJECT_A}&branch=dev"),
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parsed(&body).as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn environment_config_and_secrets_never_leak_plaintext() {
    let db = fixture_db().await;
    let router = router(db).await;
    let token = token_a();
    let (status, body) = call(
        &router,
        request(
            "POST",
            "/api/v1/versioning/environments",
            Some(&token),
            Some(json!({
                "project_id": PROJECT_A,
                "name": "staging",
                "config": {
                    "region": "eu-west-1",
                    "api_key": "ghp_supersecret",
                    "nested": {"password": "hunter2"},
                    "note": "auth with Bearer ghp_inline here"
                },
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = call(
        &router,
        request(
            "GET",
            &format!("/api/v1/versioning/environments?project_id={PROJECT_A}"),
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("supersecret") && !body.contains("hunter2"));
    assert!(!body.contains("ghp_inline"));
    let config = parsed(&body)[0]["config"].clone();
    assert_eq!(config["region"], json!("eu-west-1"));
    assert_eq!(config["api_key"], json!(nocode_deploy::REDACTED));
    assert_eq!(config["nested"]["password"], json!(nocode_deploy::REDACTED));
    assert!(config["note"]
        .as_str()
        .unwrap()
        .contains(nocode_deploy::REDACTED));

    let (status, body) = call(
        &router,
        request(
            "POST",
            "/api/v1/versioning/environments/staging/secrets",
            Some(&token),
            Some(json!({"project_id": PROJECT_A, "key": "DB_URL", "value": "postgres://super-secret"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(!body.contains("super-secret"));
    assert_eq!(parsed(&body)["key"], json!("DB_URL"));

    let uri = "/api/v1/versioning/environments/staging/secrets?project_id=p1";
    let (status, body) = call(&router, request("GET", uri, Some(&token), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("super-secret"));
    assert_eq!(parsed(&body)[0]["key"], json!("DB_URL"));

    let (status, _) = call(
        &router,
        request(
            "DELETE",
            "/api/v1/versioning/environments/staging/secrets/DB_URL?project_id=p1",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = call(&router, request("GET", uri, Some(&token), None)).await;
    assert_eq!(parsed(&body), json!([]));
}

#[tokio::test]
async fn promote_and_rollback_track_the_exact_revision() {
    let db = fixture_db().await;
    let router = router(db.clone()).await;
    let token = token_a();
    let first = commit_revision(&router, &token, PROJECT_A, "main", "r1", json!({"v": 1})).await;
    let second = commit_revision(&router, &token, PROJECT_A, "main", "r2", json!({"v": 2})).await;
    create_environment(&router, &token, PROJECT_A, "prod").await;

    let uri = "/api/v1/versioning/environments/prod/promote";
    let (status, body) = call(
        &router,
        request(
            "POST",
            uri,
            Some(&token),
            Some(json!({"project_id": PROJECT_A, "revision_id": first["id"]})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parsed(&body)["revision_id"], first["id"]);
    assert_eq!(parsed(&body)["status"], json!("deployed"));
    let audit = sql_text(
        &db,
        "SELECT status AS value FROM environment_deployments WHERE revision_id = ?",
        vec![DbValue::from(first["id"].as_str().unwrap())],
    )
    .await;
    assert_eq!(audit.as_deref(), Some("deployed"));

    let uri = "/api/v1/versioning/environments/prod/rollback";
    let (status, body) = call(
        &router,
        request(
            "POST",
            uri,
            Some(&token),
            Some(json!({"project_id": PROJECT_A, "revision_id": second["id"]})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parsed(&body)["revision_id"], second["id"]);
    assert_eq!(parsed(&body)["status"], json!("rolled_back"));
    let audit = sql_text(
        &db,
        "SELECT status AS value FROM environment_deployments WHERE revision_id = ?",
        vec![DbValue::from(second["id"].as_str().unwrap())],
    )
    .await;
    assert_eq!(audit.as_deref(), Some("rolled_back"));

    let (status, _) = call(
        &router,
        request(
            "POST",
            uri,
            Some(&token),
            Some(json!({"project_id": PROJECT_A, "revision_id": "not-a-revision"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A revision from the other tenant's project is not promotable here.
    let foreign = commit_revision(
        &router,
        &token_b(),
        PROJECT_B,
        "main",
        "foreign",
        json!({"v": 9}),
    )
    .await;
    let (status, _) = call(
        &router,
        request(
            "POST",
            "/api/v1/versioning/environments/prod/promote",
            Some(&token),
            Some(json!({"project_id": PROJECT_A, "revision_id": foreign["id"]})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tenant_a_cannot_see_tenant_b_versioning_data() {
    let db = fixture_db().await;
    let router = router(db).await;
    let owned = commit_revision(
        &router,
        &token_a(),
        PROJECT_A,
        "main",
        "owned",
        json!({"v": 1}),
    )
    .await;
    create_branch(
        &router,
        &token_a(),
        PROJECT_A,
        "dev",
        Some(owned["id"].as_str().unwrap()),
    )
    .await;
    create_environment(&router, &token_a(), PROJECT_A, "staging").await;

    let foreign = token_b();
    let (status, body) = call(
        &router,
        request(
            "GET",
            &format!("/api/v1/versioning/branches?project_id={PROJECT_A}"),
            Some(&foreign),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parsed(&body), json!([]));

    let (status, _) = call(
        &router,
        request(
            "GET",
            &format!("/api/v1/versioning/history?project_id={PROJECT_A}&branch=main"),
            Some(&foreign),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = call(
        &router,
        request(
            "GET",
            &format!("/api/v1/versioning/environments?project_id={PROJECT_A}"),
            Some(&foreign),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parsed(&body), json!([]));

    let uri = format!(
        "/api/v1/versioning/revisions/{}",
        owned["id"].as_str().unwrap()
    );
    let (status, _) = call(&router, request("GET", &uri, Some(&foreign), None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&router, request("GET", &uri, Some(&token_a()), None)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn diff_is_deterministic_and_tenant_scoped() {
    let db = fixture_db().await;
    let router = router(db).await;
    let token = token_a();
    let first = commit_revision(
        &router,
        &token,
        PROJECT_A,
        "main",
        "r1",
        json!({"a": 1, "b": [1, 2]}),
    )
    .await;
    let second = commit_revision(
        &router,
        &token,
        PROJECT_A,
        "main",
        "r2",
        json!({"a": 2, "b": [1, 3]}),
    )
    .await;
    let foreign = commit_revision(
        &router,
        &token_b(),
        PROJECT_B,
        "main",
        "foreign",
        json!({"a": 99}),
    )
    .await;

    let uri = format!(
        "/api/v1/versioning/diff?project_id={PROJECT_A}&from={}&to={}",
        first["id"].as_str().unwrap(),
        second["id"].as_str().unwrap()
    );
    let (status, one) = call(&router, request("GET", &uri, Some(&token), None)).await;
    assert_eq!(status, StatusCode::OK);
    let (_, two) = call(&router, request("GET", &uri, Some(&token), None)).await;
    assert_eq!(one, two);
    let diff = parsed(&one);
    assert_eq!(diff["from"], first["id"]);
    assert_eq!(diff["to"], second["id"]);
    let kinds: Vec<&str> = diff["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"add"));
    assert!(kinds.contains(&"remove"));
    assert!(kinds.contains(&"context"));

    let uri = format!(
        "/api/v1/versioning/diff?project_id={PROJECT_A}&from={}&to={}",
        foreign["id"].as_str().unwrap(),
        second["id"].as_str().unwrap()
    );
    let (status, _) = call(&router, request("GET", &uri, Some(&token), None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn fast_forward_requires_ancestry_and_conflicts_are_explicit() {
    let db = fixture_db().await;
    let router = router(db.clone()).await;
    let token = token_a();
    let root = commit_revision(
        &router,
        &token,
        PROJECT_A,
        "main",
        "root",
        json!({"winner": "root"}),
    )
    .await;
    create_branch(
        &router,
        &token,
        PROJECT_A,
        "feature",
        Some(root["id"].as_str().unwrap()),
    )
    .await;
    let feature = commit_revision(
        &router,
        &token,
        PROJECT_A,
        "feature",
        "feature work",
        json!({"winner": "feature"}),
    )
    .await;

    let merge = |target: &str, source: &str, strategy: &str| {
        let path = format!("/api/v1/versioning/branches/{target}/merge");
        let body = json!({"project_id": PROJECT_A, "source_branch": source, "strategy": strategy});
        request("POST", &path, Some(&token), Some(body))
    };

    let (status, body) = call(&router, merge("main", "feature", "fast_forward")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        head_of(&router, &token, PROJECT_A, "main").await,
        feature["id"]
    );
    let shared = sql_text(
        &db,
        "SELECT data AS value FROM projects WHERE id = ?",
        vec![DbValue::from(PROJECT_A)],
    )
    .await;
    assert!(shared.unwrap().contains("feature"));

    create_branch(
        &router,
        &token,
        PROJECT_A,
        "other",
        Some(feature["id"].as_str().unwrap()),
    )
    .await;
    let other = commit_revision(
        &router,
        &token,
        PROJECT_A,
        "other",
        "other work",
        json!({"winner": "other"}),
    )
    .await;
    let main = commit_revision(
        &router,
        &token,
        PROJECT_A,
        "main",
        "main work",
        json!({"winner": "main"}),
    )
    .await;
    let (status, body) = call(&router, merge("main", "other", "fast_forward")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let (status, body) = call(&router, merge("main", "other", "theirs")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let merged = parsed(&body);
    assert_eq!(merged["data"], json!({"winner": "other"}));
    assert!(merged["message"].as_str().unwrap().contains("conflict"));
    assert_ne!(merged["id"], main["id"]);
    assert_eq!(
        head_of(&router, &token, PROJECT_A, "main").await,
        merged["id"]
    );

    create_branch(
        &router,
        &token,
        PROJECT_A,
        "third",
        Some(merged["id"].as_str().unwrap()),
    )
    .await;
    commit_revision(
        &router,
        &token,
        PROJECT_A,
        "third",
        "third work",
        json!({"winner": "third"}),
    )
    .await;
    let (status, body) = call(&router, merge("main", "third", "ours")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parsed(&body)["data"], json!({"winner": "other"}));

    let (status, _) = call(&router, merge("main", "third", "last-write-wins")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn invalid_branch_names_are_rejected() {
    let db = fixture_db().await;
    let router = router(db).await;
    let token = token_a();
    commit_revision(&router, &token, PROJECT_A, "main", "root", json!({"v": 1})).await;
    let too_long = "a".repeat(65);
    for bad in ["Upper", "bad_name", "has space", too_long.as_str()] {
        let (status, _) = create_branch(&router, &token, PROJECT_A, bad, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "accepted {bad}");
    }
    let (status, body) = create_branch(&router, &token, PROJECT_A, "dev-1", None).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, _) = create_branch(&router, &token, PROJECT_A, "dev-1", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) =
        create_branch(&router, &token, PROJECT_A, "bad-parent", Some("missing")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn commits_require_ownership_and_update_project_data() {
    let db = fixture_db().await;
    let router = router(db.clone()).await;
    let token = token_a();
    let (status, _) = call(
        &router,
        request(
            "POST",
            "/api/v1/versioning/commits",
            Some(&token_b()),
            Some(json!({
                "project_id": PROJECT_A, "branch": "main", "message": "x", "data": {}
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    commit_revision(
        &router,
        &token,
        PROJECT_A,
        "main",
        "written",
        json!({"title": "committed"}),
    )
    .await;
    let stored = sql_text(
        &db,
        "SELECT data AS value FROM projects WHERE id = ?",
        vec![DbValue::from(PROJECT_A)],
    )
    .await
    .unwrap();
    assert!(stored.contains("committed"));

    let too_long = "m".repeat(257);
    for bad_message in ["", too_long.as_str()] {
        let (status, _) = call(
            &router,
            request(
                "POST",
                "/api/v1/versioning/commits",
                Some(&token),
                Some(json!({
                    "project_id": PROJECT_A, "branch": "main", "message": bad_message, "data": {}
                })),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn restore_appends_a_new_revision_without_rewriting_history() {
    let db = fixture_db().await;
    let router = router(db).await;
    let token = token_a();
    let first = commit_revision(&router, &token, PROJECT_A, "main", "r1", json!({"v": 1})).await;
    let second = commit_revision(&router, &token, PROJECT_A, "main", "r2", json!({"v": 2})).await;
    let (status, body) = call(
        &router,
        request(
            "POST",
            "/api/v1/versioning/restore",
            Some(&token),
            Some(json!({"project_id": PROJECT_A, "revision_id": first["id"]})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let restored = parsed(&body);
    assert_ne!(restored["id"], first["id"]);
    assert_eq!(restored["parent_id"], second["id"]);
    assert_eq!(restored["data"], json!({"v": 1}));
    assert_eq!(restored["branch"], json!("main"));

    let (_, body) = call(
        &router,
        request(
            "GET",
            &format!(
                "/api/v1/versioning/revisions/{}",
                first["id"].as_str().unwrap()
            ),
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(parsed(&body)["message"], json!("r1"));
    let (_, body) = call(
        &router,
        request(
            "GET",
            &format!("/api/v1/versioning/history?project_id={PROJECT_A}&branch=main"),
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(parsed(&body).as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn cookie_sessions_require_a_matching_csrf_token() {
    let db = fixture_db().await;
    let router = router(db).await;
    let body = json!({
        "project_id": PROJECT_A, "branch": "main", "message": "cookie", "data": {"v": 1}
    })
    .to_string();
    let token = raw_token(TOKEN_A, TENANT_A);

    let mut req = Request::builder()
        .method("POST")
        .uri("/api/v1/versioning/commits")
        .header("content-type", "application/json")
        .header(
            "cookie",
            format!("nocode_access={token}; nocode_csrf=csrf-1"),
        )
        .body(Body::from(body.clone()))
        .unwrap();
    let (status, response) = call(&router, req).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{response}");

    req = Request::builder()
        .method("POST")
        .uri("/api/v1/versioning/commits")
        .header("content-type", "application/json")
        .header(
            "cookie",
            format!("nocode_access={token}; nocode_csrf=csrf-1"),
        )
        .header("x-csrf-token", "csrf-1")
        .body(Body::from(body))
        .unwrap();
    let (status, response) = call(&router, req).await;
    assert_eq!(status, StatusCode::CREATED, "{response}");
}

#[tokio::test]
async fn branch_creation_uses_requested_or_current_head() {
    let db = fixture_db().await;
    let router = router(db).await;
    let token = token_a();
    let root = commit_revision(&router, &token, PROJECT_A, "main", "root", json!({"v": 1})).await;
    let (status, body) = create_branch(
        &router,
        &token,
        PROJECT_A,
        "from-root",
        Some(root["id"].as_str().unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(parsed(&body)["head_revision_id"], root["id"]);

    let (status, body) = create_branch(&router, &token, PROJECT_A, "from-head", None).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(parsed(&body)["head_revision_id"], root["id"]);
}
