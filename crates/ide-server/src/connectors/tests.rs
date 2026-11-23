//! Connector tests: tenant isolation, secret discipline, safety helpers and a
//! real local HTTP round trip. Bearer tokens are used so CSRF is exempt (the
//! same convention as the billing tests).

use std::collections::HashMap;
use std::sync::{Arc, Once, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response,
    Router,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value as DbValue};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tower::ServiceExt;

use crate::{
    auth::{
        jwt::{now_secs, Claims},
        JwtKeys,
    },
    db::connect_sqlite_memory,
    server::AppState,
};

use super::{connectors_router, handlers, runtime, store};

const TEST_KEY_HEX: &str = concat!(
    "00000000", "00000000", "00000000", "00000000", "00000000", "00000000", "00000000", "00000000"
);
const SECRET: &str = "SUPER_SECRET_123";

static INIT: Once = Once::new();

/// The handlers build their vault from the environment. Set the test key once
/// for the whole test binary; all stores and reads use this same process.
fn init_env() {
    INIT.call_once(|| {
        assert_eq!(TEST_KEY_HEX.len(), 64);
        std::env::set_var("IDE_SECRET_KEY", TEST_KEY_HEX);
    });
}

fn test_keys() -> Arc<JwtKeys> {
    static KEYS: OnceLock<Arc<JwtKeys>> = OnceLock::new();
    KEYS.get_or_init(|| Arc::new(JwtKeys::generate_ephemeral().expect("test keys")))
        .clone()
}

fn bearer(tenant_id: &str) -> String {
    let claims = Claims::access_for(
        "u1",
        "a@b.com",
        tenant_id.to_string(),
        vec!["owner".to_string()],
        None,
        "jti-1",
        now_secs(),
    );
    format!("Bearer {}", test_keys().encode(&claims).unwrap())
}

async fn test_router(db: DatabaseConnection) -> Router {
    Router::<AppState>::new()
        .merge(connectors_router())
        .with_state(AppState::new(db, test_keys()))
}

async fn db_with_tenants(ids: &[&str]) -> DatabaseConnection {
    init_env();
    let db = connect_sqlite_memory().await.unwrap();
    for id in ids {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO tenants (id, name, slug) VALUES (?, ?, ?)",
            vec![DbValue::from(*id), DbValue::from(*id), DbValue::from(*id)],
        ))
        .await
        .unwrap();
    }
    db
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
        .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
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

async fn create_connector(
    router: &Router,
    tenant: &str,
    kind: &str,
    config: Value,
    secret: Option<&str>,
) -> (StatusCode, String) {
    let mut body = json!({
        "kind": kind,
        "name": "Test connector",
        "config": config,
    });
    if let Some(secret) = secret {
        body["secret"] = Value::String(secret.to_string());
    }
    call(
        router,
        request(
            "POST",
            "/api/v1/connectors",
            Some(&bearer(tenant)),
            Some(body),
        ),
    )
    .await
}

fn connector_id(body: &str) -> String {
    serde_json::from_str::<Value>(body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn catalog_lists_five_kinds_with_descriptions() {
    let db = db_with_tenants(&["tenant-catalog"]).await;
    let router = test_router(db).await;
    let (status, body) = call(
        &router,
        request(
            "GET",
            "/api/v1/connectors/catalog",
            Some(&bearer("tenant-catalog")),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let entries = serde_json::from_str::<Value>(&body).unwrap();
    let entries = entries.as_array().unwrap();
    assert_eq!(entries.len(), 5);
    for entry in entries {
        assert!(entry["description"].as_str().unwrap().len() > 10);
    }
}

#[tokio::test]
async fn mutating_routes_require_authentication() {
    let db = db_with_tenants(&[]).await;
    let router = test_router(db).await;
    let (status, _) = call(&router, request("GET", "/api/v1/connectors", None, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(
        &router,
        request(
            "POST",
            "/api/v1/connectors",
            None,
            Some(json!({"kind": "http_rest", "name": "x", "config": {}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn tenant_a_cannot_list_or_get_tenant_b_connector() {
    let db = db_with_tenants(&["tenant-iso-a", "tenant-iso-b"]).await;
    let router = test_router(db).await;
    let (status, body) = create_connector(
        &router,
        "tenant-iso-a",
        "http_rest",
        json!({"url": "http://127.0.0.1:9"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = connector_id(&body);

    let (status, body) = call(
        &router,
        request(
            "GET",
            "/api/v1/connectors",
            Some(&bearer("tenant-iso-b")),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), json!([]));

    let (status, _) = call(
        &router,
        request(
            "GET",
            &format!("/api/v1/connectors/{id}"),
            Some(&bearer("tenant-iso-b")),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tenant_a_cannot_patch_or_delete_tenant_b_connector() {
    let db = db_with_tenants(&["tenant-mut-a", "tenant-mut-b"]).await;
    let router = test_router(db).await;
    let (status, body) = create_connector(
        &router,
        "tenant-mut-a",
        "http_rest",
        json!({"url": "http://127.0.0.1:9"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = connector_id(&body);
    let uri = format!("/api/v1/connectors/{id}");

    let (status, _) = call(
        &router,
        request(
            "PATCH",
            &uri,
            Some(&bearer("tenant-mut-b")),
            Some(json!({"name": "stolen"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &router,
        request("DELETE", &uri, Some(&bearer("tenant-mut-b")), None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = call(
        &router,
        request("GET", &uri, Some(&bearer("tenant-mut-a")), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["name"],
        json!("Test connector")
    );
}

#[tokio::test]
async fn create_list_and_get_never_expose_secrets() {
    let db = db_with_tenants(&["tenant-secret"]).await;
    let router = test_router(db).await;
    let (status, body) = create_connector(
        &router,
        "tenant-secret",
        "http_rest",
        json!({"url": "http://127.0.0.1:9", "api_key": "SUPER_SECRET_IN_CONFIG"}),
        Some(SECRET),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(
        !body.contains("SUPER_SECRET"),
        "create body leaked a secret"
    );
    let created = serde_json::from_str::<Value>(&body).unwrap();
    assert_eq!(created["secret_set"], json!(true));
    assert_eq!(created["config"]["api_key"], json!(nocode_deploy::REDACTED));
    let id = connector_id(&body);

    let (status, body) = call(
        &router,
        request(
            "GET",
            "/api/v1/connectors",
            Some(&bearer("tenant-secret")),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("SUPER_SECRET"), "list body leaked a secret");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()[0]["secret_set"],
        json!(true)
    );

    let (status, body) = call(
        &router,
        request(
            "GET",
            &format!("/api/v1/connectors/{id}"),
            Some(&bearer("tenant-secret")),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("SUPER_SECRET"), "get body leaked a secret");
}

#[tokio::test]
async fn test_result_never_exposes_secret_for_unimplemented_kind() {
    let db = db_with_tenants(&["tenant-oauth"]).await;
    let router = test_router(db).await;
    let (status, body) = create_connector(
        &router,
        "tenant-oauth",
        "oauth2",
        json!({
            "token_url": "http://127.0.0.1:9/token",
            "client_id": "client-1",
            "password": "SUPER_SECRET_IN_CONFIG"
        }),
        Some(SECRET),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = connector_id(&body);

    let (status, body) = call(
        &router,
        request(
            "POST",
            &format!("/api/v1/connectors/{id}/test"),
            Some(&bearer("tenant-oauth")),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("SUPER_SECRET"), "test body leaked a secret");
    let result = serde_json::from_str::<Value>(&body).unwrap();
    assert_eq!(result["ok"], json!(false));
    assert!(result["message"]
        .as_str()
        .unwrap()
        .contains("not implemented"));
    assert!(result["latency_ms"].is_i64());
}

#[tokio::test]
async fn invoke_error_never_exposes_secret() {
    let db = db_with_tenants(&["tenant-invoke-err"]).await;
    let router = test_router(db).await;
    let (status, body) = create_connector(
        &router,
        "tenant-invoke-err",
        "http_rest",
        json!({"url": "http://127.0.0.1:9"}),
        Some(SECRET),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = connector_id(&body);

    let (status, body) = call(
        &router,
        request(
            "POST",
            &format!("/api/v1/connectors/{id}/invoke"),
            Some(&bearer("tenant-invoke-err")),
            Some(json!({"method": "GET", "path": "/missing"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        !body.contains("SUPER_SECRET"),
        "invoke error leaked a secret"
    );
    assert!(!body.is_empty());
}

#[test]
fn redact_config_removes_nested_secrets() {
    let input = json!({
        "url": "http://localhost",
        "Authorization": "Bearer abc",
        "nested": {
            "ACCESS_TOKEN": "s1",
            "list": [{"client_secret": "s2", "visible": 7}]
        }
    });
    let output = runtime::redact_config(&input);
    let dumped = output.to_string();
    assert!(!dumped.contains("Bearer abc"));
    assert!(!dumped.contains("s1"));
    assert!(!dumped.contains("s2"));
    assert_eq!(output["nested"]["list"][0]["visible"], json!(7));
    // The canonical implementation is re-exported by the store as well.
    assert_eq!(store::redact_config(&input), output);
}

#[test]
fn is_safe_target_rejects_userinfo_ftp_and_control_chars() {
    assert!(runtime::is_safe_target("http://127.0.0.1:9000/api"));
    assert!(runtime::is_safe_target("https://example.com/v1"));
    assert!(!runtime::is_safe_target("ftp://example.com"));
    assert!(!runtime::is_safe_target("file:///etc/passwd"));
    assert!(!runtime::is_safe_target("http://user:pass@example.com"));
    assert!(!runtime::is_safe_target("http://example.com\rn"));
    assert!(!runtime::is_safe_target("http://example.com/a b"));
    assert!(!runtime::is_safe_target("http://"));
}

#[test]
fn rate_limiter_exhausts_and_refills() {
    let mut bucket: HashMap<String, (f64, i64)> = HashMap::new();
    let rate = 2.0; // 2 per minute -> one token every 30s
    assert!(runtime::rate_allow(&mut bucket, "tenant", 0, rate));
    assert!(runtime::rate_allow(&mut bucket, "tenant", 0, rate));
    assert!(!runtime::rate_allow(&mut bucket, "tenant", 0, rate));
    assert!(!runtime::rate_allow(&mut bucket, "tenant", 29_999, rate));
    assert!(runtime::rate_allow(&mut bucket, "tenant", 30_001, rate));
    assert!(runtime::rate_allow(&mut bucket, "other-tenant", 0, rate));
}

#[test]
fn breaker_opens_after_five_failures() {
    use runtime::BreakerState::*;
    assert_eq!(runtime::breaker_state(4, None, 0), Closed);
    assert_eq!(runtime::breaker_state(5, None, 0), Open);
    assert_eq!(runtime::breaker_state(5, Some(0), 29_999), Open);
    assert_eq!(runtime::breaker_state(5, Some(0), 30_001), HalfOpen);
}

#[tokio::test]
async fn dead_http_endpoint_produces_usable_error_within_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        // Accept the connection but never answer it.
        if let Ok((_socket, _)) = listener.accept().await {
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
    let runtime = handlers::runtime_with_timeout(Duration::from_millis(250));
    let started = Instant::now();
    let error = runtime
        .send(&format!("http://{address}"), "GET", "", &[])
        .await
        .unwrap_err();
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(2), "elapsed: {elapsed:?}");
    assert!(
        error.to_string().contains("did not respond in time"),
        "unexpected error: {error}"
    );
    assert!(!error.to_string().contains(SECRET));
    server.abort();
}

#[tokio::test]
async fn https_target_is_refused_with_clear_message() {
    let runtime = handlers::runtime_with_timeout(Duration::from_millis(250));
    let error = runtime
        .send("https://example.com/v1", "GET", "", &[])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("HTTPS connector calls require"));
}

#[tokio::test]
async fn invoke_rejects_unsafe_method_and_path() {
    let db = db_with_tenants(&["tenant-invoke-guard"]).await;
    let router = test_router(db).await;
    let (status, body) = create_connector(
        &router,
        "tenant-invoke-guard",
        "http_rest",
        json!({"url": "http://127.0.0.1:9"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = connector_id(&body);
    let uri = format!("/api/v1/connectors/{id}/invoke");
    let token = bearer("tenant-invoke-guard");

    let (status, _) = call(
        &router,
        request(
            "POST",
            &uri,
            Some(&token),
            Some(json!({"method": "TRACE", "path": "/x"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    for path in ["http://evil.example/x", "//evil.example/x", "/a\nX-Evil: 1"] {
        let (status, _) = call(
            &router,
            request(
                "POST",
                &uri,
                Some(&token),
                Some(json!({"method": "GET", "path": path})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "path was accepted: {path}");
    }
}

#[tokio::test]
async fn invoke_proxies_a_json_response_from_a_local_server() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 4096];
        let _ = socket.read(&mut buffer).await;
        let payload = b"{\"pong\":true}";
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(payload).await.unwrap();
    });

    let db = db_with_tenants(&["tenant-invoke-proxy"]).await;
    let router = test_router(db).await;
    let (status, body) = create_connector(
        &router,
        "tenant-invoke-proxy",
        "http_rest",
        json!({"url": format!("http://{address}")}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = connector_id(&body);

    let (status, body) = call(
        &router,
        request(
            "POST",
            &format!("/api/v1/connectors/{id}/invoke"),
            Some(&bearer("tenant-invoke-proxy")),
            Some(json!({"method": "GET", "path": "/ping"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "invoke failed: {body}");
    let reply = serde_json::from_str::<Value>(&body).unwrap();
    assert_eq!(reply["status"], json!(200));
    assert_eq!(reply["body"]["pong"], json!(true));
    server.abort();
}

#[tokio::test]
async fn patch_keeps_the_secret_flag_and_redacts_new_config() {
    let db = db_with_tenants(&["tenant-patch-secret"]).await;
    let router = test_router(db).await;
    let (status, body) = create_connector(
        &router,
        "tenant-patch-secret",
        "webhook",
        json!({"url": "http://127.0.0.1:9/hook"}),
        Some(SECRET),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = connector_id(&body);

    let (status, body) = call(
        &router,
        request(
            "PATCH",
            &format!("/api/v1/connectors/{id}"),
            Some(&bearer("tenant-patch-secret")),
            Some(json!({
                "name": "Renamed",
                "config": {"url": "http://127.0.0.1:9/hook", "secret": "SUPER_SECRET_NEW"}
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("SUPER_SECRET"), "patch body leaked a secret");
    let updated = serde_json::from_str::<Value>(&body).unwrap();
    assert_eq!(updated["name"], json!("Renamed"));
    assert_eq!(updated["secret_set"], json!(true));
    assert_eq!(updated["config"]["secret"], json!(nocode_deploy::REDACTED));
}

#[tokio::test]
async fn https_connector_calls_are_rejected_with_a_clear_reason() {
    let db = db_with_tenants(&["tenant-https"]).await;
    let router = test_router(db).await;
    let (status, body) = create_connector(
        &router,
        "tenant-https",
        "http_rest",
        json!({"url": "https://example.com/v1"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = connector_id(&body);
    let token = bearer("tenant-https");

    let (status, body) = call(
        &router,
        request(
            "POST",
            &format!("/api/v1/connectors/{id}/test"),
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("HTTPS connector calls require a TLS transport"));

    let (status, body) = call(
        &router,
        request(
            "POST",
            &format!("/api/v1/connectors/{id}/invoke"),
            Some(&token),
            Some(json!({"method": "GET", "path": "/items"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("configure a local HTTPS proxy"));
}
