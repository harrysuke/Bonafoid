//! HTTP DTOs and handlers for tenant-scoped connectors.
//!
//! Secrets enter only through the `secret` field of a mutation body and are
//! sealed immediately with the credential vault. No DTO in this module has a
//! field that can hold plaintext; every config is passed through recursive
//! redaction before it is stored or returned, and outbound error text passes
//! through `nocode_deploy::redact`.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use nocode_deploy::{redact, CredentialVault, SealedCredential};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{require_csrf, ApiError},
    server::AppState,
};

use super::{
    runtime::{self, BreakerState},
    store,
};

/// The five connector kinds exposed by the catalog.
pub const CONNECTOR_KINDS: [&str; 5] = ["http_rest", "webhook", "oauth2", "sql", "message_queue"];
/// Hard bound on a submitted secret (prevents unbounded vault input).
const MAX_SECRET_CHARS: usize = 4096;

/// Per-tenant token bucket: key -> (tokens, last refill epoch ms).
static RATE_BUCKETS: OnceLock<Mutex<HashMap<String, (f64, i64)>>> = OnceLock::new();
/// Per-instance breaker: key -> failure count and opening time.
static BREAKERS: OnceLock<Mutex<HashMap<String, BreakerEntry>>> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
struct BreakerEntry {
    failures: i32,
    opened_at: Option<i64>,
}

fn rate_buckets() -> &'static Mutex<HashMap<String, (f64, i64)>> {
    RATE_BUCKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn breakers() -> &'static Mutex<HashMap<String, BreakerEntry>> {
    BREAKERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Calls per minute, overridable by the operator. Invalid values fall back to
/// the safe default instead of disabling throttling.
fn rate_per_min() -> f64 {
    std::env::var("CONNECTOR_RATE_LIMIT")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(60.0)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// Build the vault from the environment, with operator-facing guidance when
/// the master key is absent. A missing key is a setup step, not a 500.
pub fn vault() -> Result<CredentialVault, ApiError> {
    CredentialVault::from_env().map_err(|error| {
        tracing::warn!(%error, "connector secrets disabled");
        ApiError::ServiceUnavailable("set IDE_SECRET_KEY to manage connector secrets".to_string())
    })
}

fn allow_request(tenant_id: &str) -> Result<(), ApiError> {
    let mut buckets = rate_buckets().lock().map_err(|_| ApiError::Internal)?;
    if runtime::rate_allow(&mut buckets, tenant_id, now_ms(), rate_per_min()) {
        Ok(())
    } else {
        Err(ApiError::ServiceUnavailable(
            "connector rate limit exceeded; retry later".to_string(),
        ))
    }
}

fn check_breaker(key: &str) -> Result<(), ApiError> {
    let breakers = breakers().lock().map_err(|_| ApiError::Internal)?;
    if let Some(entry) = breakers.get(key) {
        if runtime::breaker_state(entry.failures, entry.opened_at, now_ms()) == BreakerState::Open {
            return Err(ApiError::ServiceUnavailable(
                "connector circuit breaker is open; retry later".to_string(),
            ));
        }
    }
    Ok(())
}

fn record_failure(key: &str) {
    if let Ok(mut breakers) = breakers().lock() {
        let entry = breakers.entry(key.to_string()).or_insert(BreakerEntry {
            failures: 0,
            opened_at: None,
        });
        entry.failures = entry.failures.saturating_add(1);
        if entry.failures >= runtime::FAILURE_THRESHOLD {
            // Each failure while open restarts the 30-second window.
            entry.opened_at = Some(now_ms());
        }
    }
}

fn record_success(key: &str) {
    if let Ok(mut breakers) = breakers().lock() {
        breakers.remove(key);
    }
}

fn breaker_key(tenant_id: &str, id: &str) -> String {
    format!("{tenant_id}:{id}")
}

/// One entry in `GET /api/v1/connectors/catalog`.
#[derive(Debug, Serialize)]
pub struct CatalogEntry {
    pub kind: &'static str,
    pub description: &'static str,
}

/// Redacted view of a stored connector. There is deliberately no field for a
/// secret: `secret_set` only reports whether a sealed blob exists.
#[derive(Debug, Serialize)]
pub struct ConnectorResponse {
    pub id: String,
    pub tenant_id: String,
    pub kind: String,
    pub name: String,
    pub config: Value,
    pub enabled: bool,
    pub secret_set: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl ConnectorResponse {
    fn from_row(row: &store::ConnectorRow) -> Result<Self, ApiError> {
        let config = store::parse_config(&row.config_json)?;
        Ok(Self {
            id: row.id.clone(),
            tenant_id: row.tenant_id.clone(),
            kind: row.kind.clone(),
            name: row.name.clone(),
            config: runtime::redact_config(&config),
            enabled: row.enabled,
            secret_set: row.has_secret,
            created_at: row.created_at.clone(),
            updated_at: row.updated_at.clone(),
        })
    }
}

/// Outcome of `POST /api/v1/connectors/:id/test`.
#[derive(Debug, Serialize)]
pub struct TestResult {
    pub ok: bool,
    pub message: String,
    pub latency_ms: i64,
}

/// A JSON proxy reply from `POST /api/v1/connectors/:id/invoke`.
#[derive(Debug, Serialize)]
pub struct InvokeResponse {
    pub status: u16,
    pub body: Value,
}

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub kind: String,
    pub name: String,
    pub config: Value,
    pub secret: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct PatchRequest {
    pub name: Option<String>,
    pub config: Option<Value>,
    pub secret: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InvokeRequest {
    pub method: String,
    pub path: String,
    pub body: Option<Value>,
    pub headers: Option<Value>,
}

fn validate_kind(kind: &str) -> Result<(), ApiError> {
    if CONNECTOR_KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "unknown connector kind: {}",
            redact(kind)
        )))
    }
}

fn validate_name(name: &str) -> Result<String, ApiError> {
    let trimmed = name.trim();
    if (1..=128).contains(&trimmed.chars().count()) {
        Ok(trimmed.to_string())
    } else {
        Err(ApiError::BadRequest(
            "connector name must be 1 to 128 characters".to_string(),
        ))
    }
}

fn validate_secret(secret: &str) -> Result<String, ApiError> {
    if secret.chars().all(char::is_whitespace) || secret.chars().count() > MAX_SECRET_CHARS {
        return Err(ApiError::BadRequest(
            "secret must be a non-empty string of at most 4096 characters".to_string(),
        ));
    }
    Ok(secret.to_string())
}

fn required_string<'a>(config: &'a Map<String, Value>, key: &str) -> Result<&'a str, ApiError> {
    config
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::BadRequest(format!("config.{key} must be a non-empty string")))
}

/// Validate only the shape of a config. Credentials belong in `secret`, so a
/// DSN/password in `config` is neither required nor accepted here.
fn validate_config(kind: &str, config: &Value) -> Result<(), ApiError> {
    let object = config
        .as_object()
        .ok_or_else(|| ApiError::BadRequest("config must be a JSON object".to_string()))?;
    match kind {
        "http_rest" | "webhook" => {
            let url = required_string(object, "url")?;
            if !runtime::is_safe_target(url) {
                return Err(ApiError::BadRequest(format!(
                    "unsafe connector target: {}",
                    redact(url)
                )));
            }
        }
        "oauth2" => {
            let token_url = required_string(object, "token_url")?;
            if !runtime::is_safe_target(token_url) {
                return Err(ApiError::BadRequest(format!(
                    "unsafe connector target: {}",
                    redact(token_url)
                )));
            }
            required_string(object, "client_id")?;
        }
        "sql" => {
            required_string(object, "host")?;
            required_string(object, "database")?;
        }
        "message_queue" => {
            required_string(object, "broker")?;
            required_string(object, "topic")?;
        }
        other => {
            return Err(ApiError::BadRequest(format!(
                "unknown connector kind: {}",
                redact(other)
            )));
        }
    }
    Ok(())
}

fn seal_secret(secret: &str, vault: &CredentialVault) -> Result<SealedCredential, ApiError> {
    vault.seal(secret).map_err(|error| {
        tracing::error!(%error, "connector secret could not be sealed");
        ApiError::Internal
    })
}

async fn load(
    state: &AppState,
    user: &AuthUser,
    id: &str,
) -> Result<store::ConnectorRow, ApiError> {
    store::get(&state.db, &user.tenant_id, id)
        .await?
        .ok_or(ApiError::NotFound)
}

fn parse_headers(value: Option<&Value>) -> Result<Vec<(String, String)>, ApiError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let object = value
        .as_object()
        .ok_or_else(|| ApiError::BadRequest("headers must be a JSON object".to_string()))?;
    let mut headers = Vec::with_capacity(object.len());
    for (name, value) in object {
        let text = value
            .as_str()
            .ok_or_else(|| ApiError::BadRequest("header values must be strings".to_string()))?;
        if text.chars().any(char::is_control) {
            return Err(ApiError::BadRequest(
                "header value contains a control character".to_string(),
            ));
        }
        headers.push((name.clone(), text.to_string()));
    }
    Ok(headers)
}

fn latency_since(started: Instant) -> i64 {
    started.elapsed().as_millis() as i64
}

/// `GET /api/v1/connectors/catalog` — the five supported kinds.
pub async fn catalog(_user: AuthUser) -> Result<Json<Vec<CatalogEntry>>, ApiError> {
    Ok(Json(vec![
        CatalogEntry {
            kind: "http_rest",
            description: "Plain HTTP REST endpoint (GET/POST/PUT/PATCH/DELETE proxy).",
        },
        CatalogEntry {
            kind: "webhook",
            description: "Outbound webhook; test payloads can be HMAC-signed with a secret.",
        },
        CatalogEntry {
            kind: "oauth2",
            description: "OAuth 2.0 client configuration; live token exchange is not implemented.",
        },
        CatalogEntry {
            kind: "sql",
            description: "SQL connection settings; no query transport is available in this build.",
        },
        CatalogEntry {
            kind: "message_queue",
            description:
                "Broker and topic settings; no publish transport is available in this build.",
        },
    ]))
}

/// `GET /api/v1/connectors` — this tenant's instances, always redacted.
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<ConnectorResponse>>, ApiError> {
    let rows = store::list(&state.db, &user.tenant_id).await?;
    let items = rows
        .iter()
        .map(ConnectorResponse::from_row)
        .collect::<Result<Vec<_>, ApiError>>()?;
    Ok(Json(items))
}

/// `GET /api/v1/connectors/{id}` — one instance, scoped to the caller's
/// tenant. Another tenant's id is indistinguishable from a missing one.
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    user: AuthUser,
) -> Result<Json<ConnectorResponse>, ApiError> {
    let row = load(&state, &user, &id).await?;
    Ok(Json(ConnectorResponse::from_row(&row)?))
}

/// `POST /api/v1/connectors` — validate, redact, seal and store.
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<ConnectorResponse>), ApiError> {
    require_csrf(&headers)?;
    validate_kind(&body.kind)?;
    let name = validate_name(&body.name)?;
    validate_config(&body.kind, &body.config)?;

    let sealed = match body.secret {
        Some(secret) => {
            let secret = validate_secret(&secret)?;
            Some(seal_secret(&secret, &vault()?)?)
        }
        None => None,
    };
    let config_json = serde_json::to_string(&runtime::redact_config(&body.config))?;
    let id = Uuid::new_v4().to_string();
    store::create(
        &state.db,
        &user.tenant_id,
        &id,
        &body.kind,
        &name,
        &config_json,
        sealed.as_ref(),
    )
    .await?;
    let row = load(&state, &user, &id).await?;
    Ok((
        StatusCode::CREATED,
        Json(ConnectorResponse::from_row(&row)?),
    ))
}

/// `PATCH /api/v1/connectors/:id` — partial, tenant-scoped update.
pub async fn patch(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<PatchRequest>,
) -> Result<Json<ConnectorResponse>, ApiError> {
    require_csrf(&headers)?;
    let existing = load(&state, &user, &id).await?;
    let name = match body.name {
        Some(name) => Some(validate_name(&name)?),
        None => None,
    };
    let config_json = match body.config {
        Some(config) => {
            validate_config(&existing.kind, &config)?;
            Some(serde_json::to_string(&runtime::redact_config(&config))?)
        }
        None => None,
    };
    let sealed = match body.secret {
        Some(secret) => {
            let secret = validate_secret(&secret)?;
            Some(seal_secret(&secret, &vault()?)?)
        }
        None => None,
    };
    if name.is_some() || config_json.is_some() || sealed.is_some() {
        store::update(
            &state.db,
            &user.tenant_id,
            &id,
            name.as_deref(),
            config_json.as_deref(),
            sealed.as_ref(),
        )
        .await?;
    }
    let row = load(&state, &user, &id).await?;
    Ok(Json(ConnectorResponse::from_row(&row)?))
}

/// `DELETE /api/v1/connectors/:id` — tenant-scoped removal.
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_csrf(&headers)?;
    if !store::remove(&state.db, &user.tenant_id, &id).await? {
        return Err(ApiError::NotFound);
    }
    record_success(&breaker_key(&user.tenant_id, &id));
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/connectors/:id/test` — real HTTP probe for http_rest/webhook;
/// an honest `ok: false` for connector kinds without a transport.
pub async fn test(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<TestResult>, ApiError> {
    require_csrf(&headers)?;
    let row = load(&state, &user, &id).await?;
    let config = store::parse_config(&row.config_json)?;
    let started = Instant::now();
    let key = breaker_key(&user.tenant_id, &id);
    allow_request(&user.tenant_id)?;
    check_breaker(&key)?;

    let result = match row.kind.as_str() {
        "http_rest" | "webhook" => {
            let url = required_string(
                config
                    .as_object()
                    .ok_or_else(|| ApiError::Internal)?,
                "url",
            )?;
            if !runtime::is_safe_target(url) {
                return Err(ApiError::BadRequest(format!(
                    "unsafe connector target: {}",
                    redact(url)
                )));
            }
            if url.to_ascii_lowercase().starts_with("https://") {
                return Err(ApiError::BadRequest(runtime::HTTPS_MESSAGE.to_string()));
            }
            let (method, payload, outgoing_headers) = if row.kind == "webhook" {
                let payload = serde_json::json!({
                    "event": "connector_test",
                    "connector_id": id,
                });
                let mut headers = Vec::new();
                if let Some(sealed) = row.sealed() {
                    let vault = vault()?;
                    let secret = vault.open(&sealed).map_err(|error| {
                        tracing::error!(%error, "connector test secret could not be opened");
                        ApiError::ServiceUnavailable(
                            "connector secret could not be opened; check IDE_SECRET_KEY".to_string(),
                        )
                    })?;
                    let signature = runtime::hmac_sha256(secret.as_bytes(), payload.to_string().as_bytes());
                    headers.push((
                        "x-connector-signature".to_string(),
                        format!("sha256={}", runtime::hex_encode(&signature)),
                    ));
                }
                ("POST", payload.to_string(), headers)
            } else {
                ("GET", String::new(), Vec::new())
            };
            match runtime::ConnectorRuntime::shared()
                .send(url, method, &payload, &outgoing_headers)
                .await
            {
                Ok(reply) => TestResult {
                    ok: reply.status.is_success(),
                    message: if reply.status.is_success() {
                        format!("connector test succeeded (HTTP {})", reply.status.as_u16())
                    } else {
                        redact(&format!(
                            "connector test returned HTTP {}",
                            reply.status.as_u16()
                        ))
                    },
                    latency_ms: latency_since(started),
                },
                Err(error) => TestResult {
                    ok: false,
                    message: redact(&error.to_string()),
                    latency_ms: latency_since(started),
                },
            }
        }
        other => match validate_config(other, &config) {
            Ok(()) => TestResult {
                ok: false,
                message: format!(
                    "{other} connection tests are not implemented in this build; validate credentials out of band"
                ),
                latency_ms: latency_since(started),
            },
            Err(error) => TestResult {
                ok: false,
                message: redact(&error.to_string()),
                latency_ms: latency_since(started),
            },
        },
    };

    if result.ok {
        record_success(&key);
    } else {
        record_failure(&key);
    }
    Ok(Json(result))
}

/// `POST /api/v1/connectors/:id/invoke` — proxy one JSON request to the
/// connector's stored base URL. Redirects are never followed.
pub async fn invoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<InvokeRequest>,
) -> Result<Json<InvokeResponse>, ApiError> {
    require_csrf(&headers)?;
    let method = body.method.trim().to_ascii_uppercase();
    if !["GET", "POST", "PUT", "PATCH", "DELETE"].contains(&method.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "unsupported connector method: {}",
            redact(&method)
        )));
    }
    let row = load(&state, &user, &id).await?;
    if row.kind != "http_rest" && row.kind != "webhook" {
        return Err(ApiError::BadRequest(
            "invoke is only supported for http_rest and webhook connectors".to_string(),
        ));
    }
    let config = store::parse_config(&row.config_json)?;
    let base = config
        .as_object()
        .and_then(|object| object.get("url"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::Internal)?;
    let key = breaker_key(&user.tenant_id, &id);
    allow_request(&user.tenant_id)?;
    check_breaker(&key)?;

    let target = runtime::join_target(base, &body.path)?;
    if target.to_ascii_lowercase().starts_with("https://") {
        return Err(ApiError::BadRequest(runtime::HTTPS_MESSAGE.to_string()));
    }
    let outgoing_headers = parse_headers(body.headers.as_ref())?;
    let payload = body.body.map(|value| value.to_string()).unwrap_or_default();
    match runtime::ConnectorRuntime::shared()
        .send(&target, &method, &payload, &outgoing_headers)
        .await
    {
        Ok(reply) => {
            if reply.status.is_success() {
                record_success(&key);
            } else if reply.status.is_server_error() {
                record_failure(&key);
            }
            let text = String::from_utf8_lossy(&reply.body);
            let safe = redact(&text);
            let parsed = serde_json::from_str::<Value>(&safe)
                .unwrap_or_else(|_| Value::String(safe.to_string()));
            Ok(Json(InvokeResponse {
                status: reply.status.as_u16(),
                body: parsed,
            }))
        }
        Err(error) => {
            record_failure(&key);
            Err(ApiError::BadRequest(redact(&error.to_string())))
        }
    }
}

/// Exposed for tests that need a shorter deadline than the shared runtime.
pub fn runtime_with_timeout(timeout: Duration) -> runtime::ConnectorRuntime {
    runtime::ConnectorRuntime::new(timeout)
}
