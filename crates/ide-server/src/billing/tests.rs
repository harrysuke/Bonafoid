//! Integration tests for the billing API, using a migrated in-memory SQLite
//! database and signed bearer tokens (which are CSRF-exempt).

use std::sync::{Arc, OnceLock};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response,
    Router,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value};
use tower::ServiceExt;

use crate::{
    auth::{
        jwt::{now_secs, Claims},
        JwtKeys,
    },
    db::connect_sqlite_memory,
    server::AppState,
};

use super::{billing_router, handlers, store, stripe};

fn test_keys() -> Arc<JwtKeys> {
    static KEYS: OnceLock<Arc<JwtKeys>> = OnceLock::new();
    KEYS.get_or_init(|| Arc::new(JwtKeys::generate_ephemeral().expect("test keys")))
        .clone()
}

fn bearer(tenant_id: &str, roles: Vec<String>, platform_role: Option<String>) -> String {
    let claims = Claims::access_for(
        "u1",
        "a@b.com",
        tenant_id.to_string(),
        roles,
        platform_role,
        "jti-1",
        now_secs(),
    );
    format!("Bearer {}", test_keys().encode(&claims).unwrap())
}

fn owner(tenant_id: &str) -> String {
    bearer(tenant_id, vec!["owner".to_string()], None)
}

fn super_admin(tenant_id: &str) -> String {
    bearer(tenant_id, Vec::new(), Some("super_admin".to_string()))
}

async fn test_router(db: DatabaseConnection) -> Router {
    Router::<AppState>::new()
        .merge(billing_router())
        .with_state(AppState::new(db, test_keys()))
}

async fn seed_tenant(db: &DatabaseConnection, id: &str) {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT OR IGNORE INTO tenants (id, name, slug) VALUES (?, ?, ?)",
        vec![Value::from(id), Value::from(id), Value::from(id)],
    ))
    .await
    .unwrap();
}

async fn seed_plan(db: &DatabaseConnection) -> String {
    let id = "plan-pro".to_string();
    store::insert_plan(
        db,
        &id,
        "Pro",
        None,
        "USD",
        "month",
        1900,
        None,
        "2024-01-01T00:00:00Z",
        "2024-01-01T00:00:00Z",
    )
    .await
    .unwrap();
    id
}

async fn seed_subscription(db: &DatabaseConnection, tenant_id: &str, id: &str) {
    store::upsert_subscription(
        db,
        id,
        tenant_id,
        "plan-pro",
        "price_plan-pro",
        "active",
        3,
        "2024-01-01T00:00:00Z",
        "2024-02-01T00:00:00Z",
        "2024-01-01T00:00:00Z",
        "2024-01-01T00:00:00Z",
    )
    .await
    .unwrap();
}

fn post(uri: &str, token: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", token)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn get(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header("authorization", token)
        .body(Body::empty())
        .unwrap()
}

async fn json_body(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn billing_routes_require_authentication() {
    let db = connect_sqlite_memory().await.unwrap();
    let router = test_router(db).await;
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/billing/plans")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn plan_creation_requires_platform_super_admin() {
    let db = connect_sqlite_memory().await.unwrap();
    let router = test_router(db).await;
    let body = serde_json::json!({
        "name": "Pro",
        "currency": "USD",
        "interval": "month",
        "amount_cents": 1900,
    });

    // A tenant owner is not a platform admin.
    let response = router
        .clone()
        .oneshot(post(
            "/api/v1/billing/plans",
            &owner("tenant-a"),
            body.clone(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // The platform super admin may create a plan; Bearer is CSRF-exempt.
    let response = router
        .clone()
        .oneshot(post(
            "/api/v1/billing/plans",
            &super_admin("tenant-a"),
            body,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await;
    assert_eq!(created["name"], "Pro");
    assert_eq!(created["amount_cents"], 1900);
    assert_eq!(created["is_active"], true);

    let response = router
        .oneshot(get("/api/v1/billing/plans", &owner("tenant-a")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let plans = json_body(response).await;
    assert_eq!(plans.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn subscribe_grants_entitlements_scoped_to_the_tenant() {
    let db = connect_sqlite_memory().await.unwrap();
    seed_tenant(&db, "tenant-a").await;
    seed_tenant(&db, "tenant-b").await;
    seed_plan(&db).await;
    let router = test_router(db).await;

    let response = router
        .clone()
        .oneshot(post(
            "/api/v1/billing/subscribe",
            &owner("tenant-a"),
            serde_json::json!({ "plan_id": "plan-pro", "seat_count": 3 }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let subscription = json_body(response).await;
    assert_eq!(subscription["status"], "trialing");
    assert_eq!(subscription["seat_count"], 3);

    let response = router
        .clone()
        .oneshot(get("/api/v1/billing/entitlements", &owner("tenant-a")))
        .await
        .unwrap();
    let grants = json_body(response).await;
    let seats = grants
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["feature_key"] == "seats")
        .unwrap();
    assert_eq!(seats["limit_value"], 3);

    // Tenant B sees none of A's subscription, invoices or entitlements.
    for uri in [
        "/api/v1/billing/subscription",
        "/api/v1/billing/entitlements",
        "/api/v1/billing/invoices",
        "/api/v1/billing/usage",
    ] {
        let response = router
            .clone()
            .oneshot(get(uri, &owner("tenant-b")))
            .await
            .unwrap();
        let body = json_body(response).await;
        match body {
            serde_json::Value::Null => {}
            other => assert_eq!(other, serde_json::json!([]), "unexpected body for {uri}"),
        }
    }
}

#[tokio::test]
async fn usage_recording_is_tenant_scoped_and_enforces_entitlements() {
    let db = connect_sqlite_memory().await.unwrap();
    seed_tenant(&db, "tenant-a").await;
    seed_tenant(&db, "tenant-b").await;
    seed_plan(&db).await;
    let router = test_router(db.clone()).await;

    let response = router
        .clone()
        .oneshot(post(
            "/api/v1/billing/subscribe",
            &owner("tenant-a"),
            serde_json::json!({ "plan_id": "plan-pro", "seat_count": 2 }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = router
        .clone()
        .oneshot(post(
            "/api/v1/billing/record-usage",
            &owner("tenant-a"),
            serde_json::json!({ "meter_key": "api_calls", "quantity": 7 }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let recorded = json_body(response).await;
    assert_eq!(recorded["quantity"], 7);

    // The pure enforcement helper is the API-side check: 2 seats allows one
    // more seat, but not a third.
    let grants = store::list_entitlements(&db, "tenant-a").await.unwrap();
    assert!(super::engine::entitlement_allows(&grants, "seats", 1));
    assert!(!super::engine::entitlement_allows(&grants, "seats", 2));
    assert!(!super::engine::entitlement_allows(
        &grants,
        "nonexistent",
        0
    ));

    let response = router
        .clone()
        .oneshot(get("/api/v1/billing/usage", &owner("tenant-b")))
        .await
        .unwrap();
    assert_eq!(json_body(response).await, serde_json::json!([]));
}

#[tokio::test]
async fn checkout_and_portal_are_honest_about_the_missing_http_client() {
    let db = connect_sqlite_memory().await.unwrap();
    seed_tenant(&db, "tenant-a").await;
    seed_plan(&db).await;
    let router = test_router(db).await;

    // Both outcomes (key absent, or key present but no TLS transport) are 503
    // with an actionable message; neither can return a fabricated URL.
    for (uri, body) in [
        (
            "/api/v1/billing/checkout",
            serde_json::json!({
                "plan_id": "plan-pro",
                "success_url": "https://example.com/ok",
                "cancel_url": "https://example.com/no",
            }),
        ),
        (
            "/api/v1/billing/portal",
            serde_json::json!({ "return_url": "https://example.com/back" }),
        ),
    ] {
        let response = router
            .clone()
            .oneshot(post(uri, &owner("tenant-a"), body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{uri}");
        let body = json_body(response).await;
        assert_eq!(body["error"], "service_unavailable");
        assert!(body["message"].as_str().unwrap().contains("Stripe"));
    }
}

#[tokio::test]
async fn webhook_signature_is_verified_before_any_state_change() {
    let db = connect_sqlite_memory().await.unwrap();
    seed_tenant(&db, "tenant-a").await;
    seed_plan(&db).await;
    seed_subscription(&db, "tenant-a", "sub-a").await;

    let body = serde_json::to_vec(&serde_json::json!({
        "id": "evt_bad",
        "type": "invoice.paid",
        "data": { "object": { "id": "inv_bad", "customer": "cus_1", "amount_paid": 500 } },
    }))
    .unwrap();
    let bad_signature = "t=1,v1=0000000000000000000000000000000000000000000000000000000000000000";
    assert!(
        handlers::handle_webhook(&db, "whsec_test", bad_signature, &body)
            .await
            .is_err()
    );

    let stale = now_secs() - stripe::SIGNATURE_TOLERANCE_SECS - 1;
    let stale_signature = stripe::test_signature("whsec_test", stale, &body);
    assert!(
        handlers::handle_webhook(&db, "whsec_test", &stale_signature, &body)
            .await
            .is_err()
    );

    let count: i64 = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM webhook_events",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn webhook_replay_is_idempotent_and_unknown_events_are_recorded() {
    let db = connect_sqlite_memory().await.unwrap();
    seed_tenant(&db, "tenant-a").await;
    seed_plan(&db).await;
    seed_subscription(&db, "tenant-a", "sub-a").await;
    let secret = "whsec_replay_test";
    let now = now_secs();

    let checkout = serde_json::to_vec(&serde_json::json!({
        "id": "evt_checkout",
        "type": "checkout.session.completed",
        "data": { "object": {
            "customer": "cus_1",
            "client_reference_id": "tenant-a",
            "subscription": "sub_stripe_1",
        } },
    }))
    .unwrap();
    let signature = stripe::test_signature(secret, now, &checkout);
    let ack = handlers::handle_webhook(&db, secret, &signature, &checkout)
        .await
        .unwrap();
    assert!(!ack.0.duplicate);
    let customer: Option<String> = store::get_subscription(&db, "tenant-a", "sub-a")
        .await
        .unwrap()
        .unwrap()
        .external_customer_id;
    assert_eq!(customer.as_deref(), Some("cus_1"));

    let invoice = serde_json::to_vec(&serde_json::json!({
        "id": "inv_1",
        "type": "invoice.paid",
        "data": { "object": {
            "id": "inv_1", "customer": "cus_1",
            "amount_paid": 1900, "currency": "usd",
        } },
    }))
    .unwrap();
    let signature = stripe::test_signature(secret, now, &invoice);
    let ack = handlers::handle_webhook(&db, secret, &signature, &invoice)
        .await
        .unwrap();
    assert!(!ack.0.duplicate);
    assert_eq!(
        store::list_invoices(&db, "tenant-a").await.unwrap().len(),
        1
    );

    // Identical event id and body: a verified replay is a no-op.
    let ack = handlers::handle_webhook(&db, secret, &signature, &invoice)
        .await
        .unwrap();
    assert!(ack.0.duplicate);
    assert_eq!(
        store::list_invoices(&db, "tenant-a").await.unwrap().len(),
        1
    );

    // Unknown types are still recorded, and never mutate billing rows.
    let unknown = serde_json::to_vec(&serde_json::json!({
        "id": "evt_unknown", "type": "payment_intent.succeeded",
        "data": { "object": {} },
    }))
    .unwrap();
    let signature = stripe::test_signature(secret, now, &unknown);
    let ack = handlers::handle_webhook(&db, secret, &signature, &unknown)
        .await
        .unwrap();
    assert!(!ack.0.duplicate);
    let count: i64 = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM webhook_events",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn webhook_status_change_uses_the_legal_state_machine() {
    let db = connect_sqlite_memory().await.unwrap();
    seed_tenant(&db, "tenant-a").await;
    seed_plan(&db).await;
    seed_subscription(&db, "tenant-a", "sub-a").await;
    let secret = "whsec_transition_test";
    let now = now_secs();

    // First link the provider customer, then drive active -> past_due -> active.
    let checkout = serde_json::to_vec(&serde_json::json!({
        "id": "evt_link",
        "type": "checkout.session.completed",
        "data": { "object": { "customer": "cus_2", "client_reference_id": "tenant-a" } },
    }))
    .unwrap();
    let signature = stripe::test_signature(secret, now, &checkout);
    handlers::handle_webhook(&db, secret, &signature, &checkout)
        .await
        .unwrap();

    for (event_id, status, expected) in [
        ("evt_past_due", "past_due", "past_due"),
        ("evt_active", "active", "active"),
    ] {
        let body = serde_json::to_vec(&serde_json::json!({
            "id": event_id,
            "type": "customer.subscription.updated",
            "data": { "object": { "customer": "cus_2", "status": status } },
        }))
        .unwrap();
        let signature = stripe::test_signature(secret, now, &body);
        handlers::handle_webhook(&db, secret, &signature, &body)
            .await
            .unwrap();
        assert_eq!(
            store::get_subscription(&db, "tenant-a", "sub-a")
                .await
                .unwrap()
                .unwrap()
                .status,
            expected
        );
    }
}
