//! Phase 7 tests: tenant CRUD, membership, roles and cross-tenant isolation.

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value};
use std::sync::{Arc, OnceLock};
use tower::ServiceExt;

use crate::{
    auth::{jwt, JwtKeys},
    db::connect_sqlite_memory,
    migration,
    server::{build_router, AppState},
};

use super::schema::DEFAULT_TENANT_ID;

fn keys() -> Arc<JwtKeys> {
    static KEYS: OnceLock<Arc<JwtKeys>> = OnceLock::new();
    KEYS.get_or_init(|| Arc::new(JwtKeys::generate_ephemeral().expect("keygen")))
        .clone()
}

async fn app() -> Router {
    app_and_db().await.0
}

/// Build the router and keep a handle to the same database, so a test can seed
/// the rows a forged token implies (a real login would have created them).
async fn app_and_db() -> (Router, sea_orm::DatabaseConnection) {
    let db = connect_sqlite_memory().await.expect("in-memory db");
    let router = build_router(AppState::new(db.clone(), keys()), "apps/ide-web/dist");
    (router, db)
}

/// Insert the tenant, user and membership rows that a JWT claims.
///
/// Tests forge tokens directly instead of registering, so nothing else creates
/// these rows; without them `projects.user_id` fails its foreign key and
/// membership-scoped queries return nothing.
async fn seed_member(
    db: &sea_orm::DatabaseConnection,
    user_id: &str,
    tenant_id: &str,
    role: &str,
) {
    for (sql, values) in [
        (
            "INSERT OR IGNORE INTO tenants (id, name, slug) VALUES (?, ?, ?)",
            vec![
                Value::from(tenant_id.to_string()),
                Value::from(tenant_id.to_string()),
                Value::from(tenant_id.to_string()),
            ],
        ),
        (
            "INSERT OR IGNORE INTO users (id, email, password_hash, tenant_id) VALUES (?, ?, 'hash', ?)",
            vec![
                Value::from(user_id.to_string()),
                Value::from(format!("{user_id}@example.com")),
                Value::from(tenant_id.to_string()),
            ],
        ),
        (
            "INSERT OR IGNORE INTO tenant_members (tenant_id, user_id, role) VALUES (?, ?, ?)",
            vec![
                Value::from(tenant_id.to_string()),
                Value::from(user_id.to_string()),
                Value::from(role.to_string()),
            ],
        ),
    ] {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            sql,
            values,
        ))
        .await
        .expect("seed member");
    }
}

fn bearer(user: &str, tenant: &str, roles: &[&str], platform: Option<&str>) -> String {
    let now = jwt::now_secs();
    let claims = jwt::Claims::access_for(
        user,
        &format!("{user}@example.com"),
        tenant.to_string(),
        roles.iter().map(|role| (*role).to_string()).collect(),
        platform.map(str::to_string),
        "jti-tenant-test",
        now,
    );
    format!("Bearer {}", keys().encode(&claims).expect("token"))
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    // Empty bodies (204, redirects) are not JSON; callers treat them as null.
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, token);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let request = builder
        .body(Body::from(
            body.map(|value| value.to_string()).unwrap_or_default(),
        ))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let json = body_json(response).await;
    (status, json)
}

async fn seed_project(
    db: &sea_orm::DatabaseConnection,
    id: &str,
    tenant_id: &str,
    user_id: &str,
    name: &str,
) {
    // `projects.user_id` is a foreign key, so the owner must exist first.
    seed_member(db, user_id, tenant_id, "owner").await;
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO projects (id, tenant_id, user_id, name, slug, version, data) \
         VALUES (?, ?, ?, ?, ?, '1', '{}')",
        vec![
            Value::from(id.to_string()),
            Value::from(tenant_id.to_string()),
            Value::from(user_id.to_string()),
            Value::from(name.to_string()),
            Value::from(name.to_lowercase()),
        ],
    ))
    .await
    .expect("seed project");
}

#[tokio::test]
async fn migrations_create_tenancy_tables_and_backfill() {
    let db = connect_sqlite_memory().await.unwrap();
    migration::run(&db).await.unwrap();
    let tenant = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM tenants WHERE id = ?",
            [Value::from(DEFAULT_TENANT_ID)],
        ))
        .await
        .unwrap()
        .expect("default tenant");
    let id: String = tenant.try_get("", "id").unwrap();
    assert_eq!(id, DEFAULT_TENANT_ID);

    // A user created before tenancy lands in the default tenant and has a
    // membership row, so their old projects remain reachable.
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO users (id, email, password_hash, tenant_id) VALUES ('legacy', 'legacy@example.com', 'hash', 'default-tenant')",
        [],
    ))
    .await
    .unwrap();
    let role = super::store::active_tenant(&db, "legacy").await.unwrap();
    assert_eq!(role, DEFAULT_TENANT_ID);
}

#[tokio::test]
async fn tenant_create_list_and_members_work() {
    let (app, db) = app_and_db().await;
    seed_member(&db, "owner", DEFAULT_TENANT_ID, "owner").await;
    let token = bearer("owner", DEFAULT_TENANT_ID, &["owner"], None);
    let (status, list) = request(&app, Method::GET, "/api/v1/tenants", &token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(list
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"] == DEFAULT_TENANT_ID));

    let (status, created) = request(
        &app,
        Method::POST,
        "/api/v1/tenants",
        &token,
        Some(serde_json::json!({ "name": "Acme", "slug": "acme" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["role"], "owner");

    let (status, members) = request(
        &app,
        Method::GET,
        &format!("/api/v1/tenants/{id}/members"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(members.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn tenant_admin_is_required_for_mutations() {
    let app = app().await;
    let member = bearer("member", DEFAULT_TENANT_ID, &["member"], None);
    let (status, _) = request(
        &app,
        Method::POST,
        "/api/v1/tenants",
        &member,
        Some(serde_json::json!({ "name": "Nope", "slug": "nope" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let admin = bearer("admin", DEFAULT_TENANT_ID, &["admin"], None);
    let (status, _) = request(
        &app,
        Method::POST,
        "/api/v1/tenants",
        &admin,
        Some(serde_json::json!({ "name": "Yes", "slug": "yes" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn super_admin_role_is_separate_from_tenant_roles() {
    let app = app().await;
    let tenant_admin = bearer("ta", DEFAULT_TENANT_ID, &["owner"], None);
    let (status, _) = request(
        &app,
        Method::GET,
        "/api/v1/admin/analytics",
        &tenant_admin,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "tenant admin must not reach the platform panel"
    );

    let super_admin = bearer("sa", DEFAULT_TENANT_ID, &["member"], Some("super_admin"));
    let (status, _) = request(
        &app,
        Method::GET,
        "/api/v1/admin/analytics",
        &super_admin,
        None,
    )
    .await;
    assert_ne!(
        status,
        StatusCode::FORBIDDEN,
        "super_admin can reach the platform panel"
    );
}

#[tokio::test]
async fn cross_tenant_project_is_invisible() {
    let db = connect_sqlite_memory().await.unwrap();
    seed_project(&db, "project-a", "tenant-a", "user-a", "Alpha").await;
    seed_project(&db, "project-b", "tenant-b", "user-b", "Beta").await;

    let a = bearer("user-a", "tenant-a", &["owner"], None);
    let own = crate::projects::store::list_projects(&db, "tenant-a", "user-a")
        .await
        .unwrap();
    assert_eq!(own.len(), 1);
    let other = crate::projects::store::get_project(&db, "tenant-a", "user-a", "project-b")
        .await
        .unwrap();
    assert!(
        other.is_none(),
        "a project from another tenant was readable"
    );
    let removed = crate::projects::store::delete_project(&db, "tenant-a", "user-a", "project-b")
        .await
        .unwrap();
    assert!(!removed, "a project from another tenant was deletable");

    let app = build_router(AppState::new(db, keys()), "apps/ide-web/dist");
    let (status, _) = request(&app, Method::GET, "/api/v1/projects", &a, None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn tenant_switch_requires_membership() {
    let app = app().await;
    let token = bearer("owner", DEFAULT_TENANT_ID, &["owner"], None);
    let (status, _) = request(
        &app,
        Method::POST,
        "/api/v1/tenants/not-a-member/switch",
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn legacy_rows_are_scoped_to_default_tenant() {
    let db = connect_sqlite_memory().await.unwrap();
    let row = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id FROM tenants WHERE id = 'default-tenant'",
        ))
        .await
        .unwrap()
        .expect("default tenant exists");
    let id: String = row.try_get("", "id").unwrap();
    assert_eq!(id, DEFAULT_TENANT_ID);

    let tokens = keys()
        .encode(&jwt::Claims::access(
            "u",
            "u@example.com",
            "jti",
            jwt::now_secs(),
        ))
        .expect("legacy token");
    let decoded = keys()
        .decode_access(&tokens)
        .expect("legacy token still verifies");
    assert_eq!(decoded.tenant_id, DEFAULT_TENANT_ID);
}

#[tokio::test]
async fn auth_tokens_carry_the_active_tenant() {
    let db = connect_sqlite_memory().await.unwrap();
    let app = build_router(AppState::new(db, keys()), "apps/ide-web/dist");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"email":"tenant-token@example.com","password":"password123"}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("register");
    assert_eq!(response.status(), StatusCode::CREATED);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    let access = cookie
        .split(';')
        .find_map(|part| part.trim().strip_prefix("nocode_access="))
        .unwrap();
    let claims = keys().decode_access(access).unwrap();
    assert_eq!(claims.tenant_id, DEFAULT_TENANT_ID);
    assert!(claims.roles.iter().any(|role| role == "owner"));
}
