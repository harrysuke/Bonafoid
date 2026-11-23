//! Integration tests for the administration panel.
//!
//! Each test gets its own in-memory database with the full migration set and
//! an ephemeral RSA key pair, so role claims and SQL state are isolated.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

use super::{admin_router, analytics, store};
use crate::{
    auth::jwt::{now_secs, Claims},
    db::connect_sqlite_memory,
    server::AppState,
    JwtKeys,
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

async fn setup() -> (AppState, Arc<JwtKeys>) {
    let db = connect_sqlite_memory().await.expect("in-memory sqlite");
    let keys = Arc::new(JwtKeys::generate_ephemeral().expect("ephemeral keys"));
    (AppState::new(db, keys.clone()), keys)
}

fn app(state: AppState) -> Router {
    Router::<AppState>::new()
        .merge(admin_router())
        .with_state(state)
}

fn bearer(
    keys: &JwtKeys,
    id: &str,
    email: &str,
    tenant: &str,
    roles: Vec<&str>,
    platform: Option<&str>,
) -> String {
    let claims = Claims::access_for(
        id,
        email,
        tenant.to_string(),
        roles.into_iter().map(str::to_string).collect(),
        platform.map(str::to_string),
        "jti-test",
        now_secs(),
    );
    format!("Bearer {}", keys.encode(&claims).expect("signed token"))
}

fn super_bearer(keys: &JwtKeys) -> String {
    bearer(
        keys,
        "admin-1",
        "admin@example.com",
        "default-tenant",
        vec!["owner"],
        Some("super_admin"),
    )
}

async fn call(
    router: Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", token);
    }
    let body = match body {
        Some(value) => {
            builder = builder.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = router
        .oneshot(builder.body(body).expect("request"))
        .await
        .expect("router responded");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

/// All values here are fixed test literals; no caller input reaches SQL.
async fn sql(db: &DatabaseConnection, statement: &str) {
    db.execute_unprepared(statement).await.expect("seed SQL");
}

async fn count(db: &DatabaseConnection, table: &str) -> i64 {
    let sql = format!("SELECT COUNT(*) AS count FROM {table}");
    let statement = Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql.as_str(), vec![]);
    let row = db
        .query_one(statement)
        .await
        .expect("count query")
        .expect("count row");
    row.try_get::<i64>("", "count").expect("count value")
}

async fn seed_user(db: &DatabaseConnection, id: &str, email: &str, platform: Option<&str>) {
    let role = match platform {
        Some(value) => format!("'{value}'"),
        None => "NULL".to_string(),
    };
    sql(
        db,
        &format!(
            "INSERT INTO users (id, email, password_hash, is_active, is_verified, platform_role, tenant_id) \
             VALUES ('{id}', '{email}', 'hash', 1, 0, {role}, 'default-tenant')"
        ),
    )
    .await;
}

async fn seed_tenant(db: &DatabaseConnection, id: &str, name: &str, slug: &str, status: &str) {
    sql(
        db,
        &format!(
            "INSERT INTO tenants (id, name, slug, status) VALUES ('{id}', '{name}', '{slug}', '{status}')"
        ),
    )
    .await;
}

async fn seed_flag(db: &DatabaseConnection, key: &str, enabled: bool, rollout: i64, kill: bool) {
    sql(
        db,
        &format!(
            "INSERT INTO platform_flags (key, enabled, rollout_percentage, kill_switch) \
             VALUES ('{key}', {}, {rollout}, {})",
            i64::from(enabled),
            i64::from(kill)
        ),
    )
    .await;
}

// ---------------------------------------------------------------------------
// Authorization gates
// ---------------------------------------------------------------------------

#[tokio::test]
async fn super_admin_gate_rejects_plain_user() {
    let (state, keys) = setup().await;
    let token = bearer(
        &keys,
        "u-1",
        "member@example.com",
        "default-tenant",
        vec!["member"],
        None,
    );
    let (status, _) = call(
        app(state),
        "GET",
        "/api/v1/admin/tenants",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn super_admin_gate_rejects_tenant_admin() {
    let (state, keys) = setup().await;
    let token = bearer(
        &keys,
        "u-2",
        "owner@example.com",
        "default-tenant",
        vec!["owner", "admin"],
        None,
    );
    let (status, _) = call(app(state), "GET", "/api/v1/admin/users", Some(&token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn missing_token_is_unauthorized() {
    let (state, _) = setup().await;
    let (status, _) = call(app(state), "GET", "/api/v1/admin/flags", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// Admin listings and pagination
// ---------------------------------------------------------------------------

#[tokio::test]
async fn super_admin_lists_tenants_and_clamps_pagination() {
    let (state, keys) = setup().await;
    let db = state.db.clone();
    seed_tenant(&db, "t-1", "Alpha", "alpha", "active").await;
    seed_tenant(&db, "t-2", "Beta", "beta", "suspended").await;
    seed_tenant(&db, "t-3", "Gamma", "gamma", "active").await;
    let token = super_bearer(&keys);

    let (status, body) = call(
        app(state.clone()),
        "GET",
        "/api/v1/admin/tenants?limit=2&offset=1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().expect("array").len(), 2);

    let (_, filtered) = call(
        app(state),
        "GET",
        "/api/v1/admin/tenants?search=alpha",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(filtered[0]["slug"], json!("alpha"));

    assert_eq!(store::clamp_limit(Some(9_999)), 200);
    assert_eq!(store::clamp_limit(Some(0)), 1);
    assert_eq!(store::clamp_limit(None), 50);
    assert_eq!(store::clamp_offset(Some(-5)), 0);
}

#[tokio::test]
async fn like_wildcards_in_search_are_literal() {
    let (state, keys) = setup().await;
    let db = state.db.clone();
    seed_tenant(&db, "t-1", "Alpha", "alpha", "active").await;
    let token = super_bearer(&keys);
    let (status, body) = call(
        app(state),
        "GET",
        "/api/v1/admin/tenants?search=%25",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().expect("array").len(), 0);
    assert!(!store::like_pattern("a_b").contains("a_b"));
}

// ---------------------------------------------------------------------------
// Public status surfaces
// ---------------------------------------------------------------------------

#[tokio::test]
async fn public_status_has_no_tenant_identifying_fields() {
    let (state, _) = setup().await;
    let (status, body) = call(app(state), "GET", "/api/v1/status", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let rendered = body.to_string().to_lowercase();
    for forbidden in [
        "tenant",
        "email",
        "user_id",
        "default-tenant",
        "plugin-workspaces",
        "@",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "status leaked {forbidden}: {rendered}"
        );
    }
    assert_eq!(body["components"]["database"], json!("ok"));
    assert!(body["components"].get("queue_depth").is_some());
    assert!(body["status"] == json!("operational") || body["status"] == json!("degraded"));
}

#[tokio::test]
async fn public_incidents_expose_only_allowlisted_fields() {
    let (state, _) = setup().await;
    let db = state.db.clone();
    sql(
        &db,
        "INSERT INTO status_incidents (id, title, status, message, component, opened_at, resolved_at) \
         VALUES ('inc-secret', 'Checkout slowdown', 'investigating', \
         'PRIVATE alice@example.com default-tenant /srv/secret', 'api', '2026-01-01 00:00:00', NULL)",
    )
    .await;

    let (status, body) = call(app(state), "GET", "/api/v1/status/incidents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().expect("array").len(), 1);
    let mut keys: Vec<String> = body[0]
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["component", "opened_at", "resolved_at", "status", "title"]
    );
    let rendered = body.to_string();
    assert!(rendered.contains("Checkout slowdown"));
    assert!(!rendered.contains("alice@example.com"));
    assert!(!rendered.contains("default-tenant"));
    assert!(!rendered.contains("inc-secret"));
    assert!(!rendered.contains("/srv/secret"));
}

// ---------------------------------------------------------------------------
// Flag evaluation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unknown_flag_fails_closed() {
    let (state, keys) = setup().await;
    let token = bearer(
        &keys,
        "u-3",
        "user@example.com",
        "default-tenant",
        vec!["member"],
        None,
    );
    let (status, body) = call(
        app(state),
        "GET",
        "/api/v1/admin/flags/evaluate?key=does-not-exist",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], json!(false));
}

#[tokio::test]
async fn kill_switch_overrides_everything() {
    let (state, keys) = setup().await;
    let db = state.db.clone();
    seed_flag(&db, "flag.kill", true, 100, true).await;
    sql(
        &db,
        "INSERT INTO tenant_flag_overrides (flag_key, tenant_id, enabled) \
         VALUES ('flag.kill', 'default-tenant', 1)",
    )
    .await;
    let token = bearer(
        &keys,
        "u-4",
        "user@example.com",
        "default-tenant",
        vec!["member"],
        None,
    );
    let (_, body) = call(
        app(state),
        "GET",
        "/api/v1/admin/flags/evaluate?key=flag.kill",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(body["enabled"], json!(false));
}

#[tokio::test]
async fn tenant_override_beats_global_and_rollout() {
    let (state, keys) = setup().await;
    let db = state.db.clone();
    seed_flag(&db, "flag.override", false, 0, false).await;
    let admin = super_bearer(&keys);
    let (status, _) = call(
        app(state.clone()),
        "POST",
        "/api/v1/admin/flags/overrides",
        Some(&admin),
        Some(json!({"flag_key": "flag.override", "tenant_id": "default-tenant", "enabled": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let member = bearer(
        &keys,
        "u-5",
        "user@example.com",
        "default-tenant",
        vec!["member"],
        None,
    );
    let (_, enabled) = call(
        app(state.clone()),
        "GET",
        "/api/v1/admin/flags/evaluate?key=flag.override",
        Some(&member),
        None,
    )
    .await;
    assert_eq!(enabled["enabled"], json!(true));

    let (_, disabled) = call(
        app(state),
        "GET",
        "/api/v1/admin/flags/evaluate?key=flag.override",
        Some(&bearer(
            &keys,
            "u-5",
            "user@example.com",
            "other-tenant",
            vec!["member"],
            None,
        )),
        None,
    )
    .await;
    assert_eq!(disabled["enabled"], json!(false));
}

#[tokio::test]
async fn rollout_is_deterministic_and_bounded() {
    let (state, keys) = setup().await;
    let db = state.db.clone();
    seed_flag(&db, "flag.rollout", false, 50, false).await;
    let token = bearer(
        &keys,
        "u-6",
        "user@example.com",
        "stable-tenant",
        vec!["member"],
        None,
    );
    let first = call(
        app(state.clone()),
        "GET",
        "/api/v1/admin/flags/evaluate?key=flag.rollout",
        Some(&token),
        None,
    )
    .await
    .1;
    let second = call(
        app(state),
        "GET",
        "/api/v1/admin/flags/evaluate?key=flag.rollout",
        Some(&token),
        None,
    )
    .await
    .1;
    assert_eq!(first["enabled"], second["enabled"]);

    let hash = store::stable_hash("stable-tenant");
    assert_eq!(hash, store::stable_hash("stable-tenant"));
    assert_ne!(hash, store::stable_hash("another-tenant"));
    assert!(!store::rollout_enabled(hash, 0));
    assert!(store::rollout_enabled(hash, 100));
    assert_eq!(store::rollout_enabled(hash, 50), (hash % 100) < 50);
}

// ---------------------------------------------------------------------------
// Audit trail and impersonation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mutations_write_audit_entries() {
    let (state, keys) = setup().await;
    let db = state.db.clone();
    seed_user(&db, "target-1", "target@example.com", None).await;
    let admin = super_bearer(&keys);

    let (status, _) = call(
        app(state.clone()),
        "PATCH",
        "/api/v1/admin/tenants/default-tenant/status",
        Some(&admin),
        Some(json!({"status": "suspended"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = call(
        app(state.clone()),
        "POST",
        "/api/v1/admin/users/target-1/impersonate",
        Some(&admin),
        Some(json!({"reason": "support ticket"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(count(&db, "audit_log").await, 2);
    assert_eq!(count(&db, "impersonations").await, 1);
    let (_, audit) = call(
        app(state.clone()),
        "GET",
        "/api/v1/admin/audit",
        Some(&admin),
        None,
    )
    .await;
    let mut actions: Vec<String> = audit
        .as_array()
        .expect("array")
        .iter()
        .map(|entry| entry["action"].as_str().unwrap().to_string())
        .collect();
    actions.sort();
    assert_eq!(actions, vec!["impersonate", "tenant_status_update"]);
    let impersonation = audit
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["action"] == json!("impersonate"))
        .unwrap();
    assert_eq!(impersonation["target_type"], json!("user"));
    assert_eq!(impersonation["target_id"], json!("target-1"));
    assert_eq!(impersonation["actor_email"], json!("admin@example.com"));
}

#[tokio::test]
async fn impersonation_response_never_contains_a_token() {
    let (state, keys) = setup().await;
    let db = state.db.clone();
    seed_user(&db, "target-2", "target2@example.com", None).await;
    let (status, body) = call(
        app(state),
        "POST",
        "/api/v1/admin/users/target-2/impersonate",
        Some(&super_bearer(&keys)),
        Some(json!({"reason": "investigation"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["impersonation_id"].as_str().unwrap().len() > 8);
    assert!(body["note"].as_str().unwrap().contains("no session token"));
    assert!(body.get("token").is_none());
    assert!(body.get("access_token").is_none());
}

// ---------------------------------------------------------------------------
// User administration invariants
// ---------------------------------------------------------------------------

#[tokio::test]
async fn last_super_admin_cannot_be_demoted() {
    let (state, keys) = setup().await;
    let db = state.db.clone();
    seed_user(&db, "admin-1", "admin@example.com", Some("super_admin")).await;
    let token = super_bearer(&keys);

    let (status, _) = call(
        app(state.clone()),
        "PATCH",
        "/api/v1/admin/users/admin-1",
        Some(&token),
        Some(json!({"platform_role": null})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    seed_user(&db, "admin-2", "admin2@example.com", Some("super_admin")).await;
    let (status, body) = call(
        app(state),
        "PATCH",
        "/api/v1/admin/users/admin-2",
        Some(&token),
        Some(json!({"platform_role": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["platform_role"], Value::Null);
}

#[tokio::test]
async fn omitted_platform_role_does_not_demote() {
    let (state, keys) = setup().await;
    let db = state.db.clone();
    seed_user(&db, "admin-2", "admin2@example.com", Some("super_admin")).await;
    let (status, body) = call(
        app(state),
        "PATCH",
        "/api/v1/admin/users/admin-2",
        Some(&super_bearer(&keys)),
        Some(json!({"is_active": false})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["platform_role"], json!("super_admin"));
    assert_eq!(body["is_active"], json!(false));
}

#[tokio::test]
async fn invalid_platform_role_is_rejected() {
    let (state, keys) = setup().await;
    let db = state.db.clone();
    seed_user(&db, "admin-2", "admin2@example.com", Some("super_admin")).await;
    let (status, _) = call(
        app(state),
        "PATCH",
        "/api/v1/admin/users/admin-2",
        Some(&super_bearer(&keys)),
        Some(json!({"platform_role": "admin"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Analytics and usage
// ---------------------------------------------------------------------------

#[tokio::test]
async fn analytics_uses_hand_computable_formulas() {
    assert_eq!(analytics::mrr_cents(&[]), 0);
    assert_eq!(analytics::arr_cents(1_000), 12_000);
    assert_eq!(analytics::churn_rate(0, 3), 0.0);
    assert_eq!(
        analytics::revenue_by_plan(&[("p".to_string(), 400, "paid".to_string())]).get("p"),
        Some(&400)
    );

    let (state, keys) = setup().await;
    let db = state.db.clone();
    sql(
        &db,
        "INSERT INTO plans (id, name, interval, amount_cents) VALUES ('plan-a', 'A', 'month', 1000)",
    )
    .await;
    sql(
        &db,
        "INSERT INTO subscriptions (id, tenant_id, plan_id, price_id, status, seat_count, period_start, period_end) \
         VALUES ('sub-1', 'default-tenant', 'plan-a', 'price-a', 'active', 1, '2026-01-01', '2026-02-01')",
    )
    .await;
    sql(
        &db,
        "INSERT INTO invoices (id, tenant_id, subscription_id, amount_cents, currency, status, issued_at, period_start, period_end) \
         VALUES ('inv-1', 'default-tenant', 'sub-1', 1000, 'USD', 'paid', '2026-01-15', '2026-01-01', '2026-01-31')",
    )
    .await;

    let (status, body) = call(
        app(state),
        "GET",
        "/api/v1/admin/analytics",
        Some(&super_bearer(&keys)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mrr_cents"], json!(1_000));
    assert_eq!(body["arr_cents"], json!(12_000));
    assert_eq!(body["revenue_by_plan"]["plan-a"], json!(1_000));
    assert_eq!(body["monthly_series"][0]["month"], json!("2026-01"));
    assert_eq!(body["monthly_series"][0]["revenue_cents"], json!(1_000));
    assert_eq!(body["churn_rate"], json!(0.0));
}

#[tokio::test]
async fn usage_counts_come_from_the_declared_tables() {
    let (state, keys) = setup().await;
    let db = state.db.clone();
    seed_tenant(&db, "t-1", "Alpha", "alpha", "active").await;
    seed_user(&db, "u-1", "user@example.com", None).await;
    sql(
        &db,
        "INSERT INTO tenant_members (tenant_id, user_id, role) VALUES ('default-tenant', 'u-1', 'member')",
    )
    .await;
    sql(
        &db,
        "INSERT INTO usage_events (id, tenant_id, kind, quantity) VALUES ('e-1', 'default-tenant', 'api_call', 3)",
    )
    .await;

    let (status, body) = call(
        app(state),
        "GET",
        "/api/v1/admin/usage",
        Some(&super_bearer(&keys)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["usage_events"], json!(1));
    assert_eq!(body["tenant_members"], json!(1));
    assert_eq!(body["audit_log"], json!(0));
    assert_eq!(body["projects"], json!(0));
    assert_eq!(body["integrations"], json!(0));
    assert_eq!(body["deployments"], json!(0));
}
