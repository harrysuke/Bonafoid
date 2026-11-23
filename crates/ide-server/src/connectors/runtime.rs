//! Pure connector-safety helpers and the outbound HTTP transport.
//!
//! # Transport limitation
//! The legacy hyper client is built on a plain `HttpConnector`. It performs
//! HTTP/1.1 only and never follows redirects, so a connector cannot be used
//! as an open redirect proxy. HTTPS targets are refused explicitly instead of
//! being attempted with a TLS-less connector that could be mistaken for a
//! successful call.
//!
//! Every string that could carry a caller-supplied URL or an upstream error
//! is passed through `nocode_deploy::redact` before it reaches `ApiError`.

use bytes::Bytes;
use hyper::{Method, Request, StatusCode};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use crate::error::ApiError;

/// Default per-call deadline for connector requests.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
/// Upper bound on a proxied response body (1 MiB).
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// Consecutive failures before a connector's circuit breaker opens.
pub const FAILURE_THRESHOLD: i32 = 5;
/// How long an open breaker blocks calls before allowing one probe.
pub const OPEN_DURATION_MS: i64 = 30_000;
/// Operator-facing reason an HTTPS target is refused.
pub const HTTPS_MESSAGE: &str =
    "HTTPS connector calls require a TLS transport; configure a local HTTPS proxy";

/// Accept only absolute http(s) URLs with an authority and no userinfo.
///
/// Control characters and whitespace are rejected so a URL cannot smuggle a
/// second header or command into an error/log line. Userinfo (`user@host`) is
/// rejected because it is a common way to hide a credential in a URL.
pub fn is_safe_target(url: &str) -> bool {
    if url.is_empty() || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    let Some(rest) = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !authority.is_empty()
        && !authority.contains('@')
        && !authority.contains('\\')
        && !authority.starts_with(':')
}

/// Whether a JSON object key denotes a credential.
///
/// Punctuation is ignored so `apiKey`, `API-KEY`, `client_secret` and
/// `access_token` are all recognised, case-insensitively.
fn is_secret_key(key: &str) -> bool {
    let compact: String = key
        .chars()
        .filter(|c| !matches!(c, '_' | '-' | ' '))
        .flat_map(char::to_lowercase)
        .collect();
    const SECRET_KEYS: [&str; 5] = ["token", "secret", "password", "apikey", "authorization"];
    SECRET_KEYS
        .iter()
        .any(|marker| compact == *marker || compact.ends_with(marker))
}

/// Recursively replace every credential-looking value with a redaction marker.
///
/// Arrays and nested objects are walked; scalar values are cloned. The
/// returned value is safe to persist as `config_json` and to serialise into a
/// response DTO.
pub fn redact_config(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut redacted = serde_json::Map::with_capacity(map.len());
            for (key, inner) in map {
                let value = if is_secret_key(key) {
                    Value::String(nocode_deploy::REDACTED.to_string())
                } else {
                    redact_config(inner)
                };
                redacted.insert(key.clone(), value);
            }
            Value::Object(redacted)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_config).collect()),
        scalar => scalar.clone(),
    }
}

/// Join an invoke path onto a connector's base URL.
///
/// Absolute paths supplied as `path` are ignored: the stored base URL is the
/// only authority a caller may select. The result is re-checked with
/// [`is_safe_target`] so userinfo cannot be assembled from two pieces.
pub fn join_target(base: &str, path: &str) -> Result<String, ApiError> {
    if !is_safe_target(base) {
        return Err(ApiError::BadRequest(format!(
            "unsafe connector target: {}",
            nocode_deploy::redact(base)
        )));
    }
    if path.is_empty()
        || path.chars().any(|c| c.is_control())
        || path.contains("://")
        || path.starts_with("//")
    {
        return Err(ApiError::BadRequest(
            "invalid connector request path".to_string(),
        ));
    }
    let joined = format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    if !is_safe_target(&joined) {
        return Err(ApiError::BadRequest(
            "unsafe connector request path".to_string(),
        ));
    }
    Ok(joined)
}

/// One token-bucket refill/decision step.
///
/// `bucket` maps a key to `(available tokens, last refill in epoch ms)`. The
/// bucket capacity equals `rate_per_min`, refilled continuously. A non-positive
/// rate denies every request. The caller holds the lock; this function is pure.
pub fn rate_allow(
    bucket: &mut HashMap<String, (f64, i64)>,
    key: &str,
    now_ms: i64,
    rate_per_min: f64,
) -> bool {
    if !rate_per_min.is_finite() || rate_per_min <= 0.0 {
        return false;
    }
    let capacity = rate_per_min.max(1.0);
    let entry = bucket.entry(key.to_string()).or_insert((capacity, now_ms));
    let elapsed_ms = now_ms.saturating_sub(entry.1).max(0) as f64;
    entry.0 = (entry.0 + elapsed_ms * rate_per_min / 60_000.0).min(capacity);
    entry.1 = now_ms;
    if entry.0 >= 1.0 {
        entry.0 -= 1.0;
        true
    } else {
        false
    }
}

/// Circuit breaker state for a connector instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed,
    Open,
    /// The open window elapsed; exactly one probe should be allowed.
    HalfOpen,
}

/// Derive the breaker state from a failure count and the time it opened.
///
/// `opened_at` is `None` until the failure threshold is reached. After
/// [`OPEN_DURATION_MS`] an open breaker becomes half-open so a single probe can
/// close it again.
pub fn breaker_state(failures: i32, opened_at: Option<i64>, now_ms: i64) -> BreakerState {
    if failures < FAILURE_THRESHOLD {
        return BreakerState::Closed;
    }
    match opened_at {
        None => BreakerState::Open,
        Some(opened) if now_ms.saturating_sub(opened) >= OPEN_DURATION_MS => BreakerState::HalfOpen,
        Some(_) => BreakerState::Open,
    }
}

/// Hex-encode bytes without adding a dependency on the `hex` crate.
pub fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// HMAC-SHA256, used to sign webhook test payloads with the stored secret.
///
/// Implemented on top of `sha2` because no `hmac` crate is available to this
/// crate. The block size follows RFC 2104; keys longer than 64 bytes are first
/// hashed.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut shortened = [0u8; 32];
    let key = if key.len() > BLOCK {
        shortened.copy_from_slice(&Sha256::digest(key));
        &shortened[..]
    } else {
        key
    };
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for (index, byte) in key.iter().enumerate() {
        ipad[index] ^= byte;
        opad[index] ^= byte;
    }
    let inner = Sha256::new()
        .chain_update(ipad)
        .chain_update(message)
        .finalize();
    let outer = Sha256::new()
        .chain_update(opad)
        .chain_update(inner)
        .finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&outer);
    output
}

/// The status and collected body of one proxied response.
#[derive(Debug)]
pub struct HttpReply {
    pub status: StatusCode,
    pub body: Bytes,
}

/// Plain-HTTP connector transport.
///
/// `Client` is clonable and holds a connection pool; one shared instance is
/// used by handlers, while tests construct one with a short deadline.
pub struct ConnectorRuntime {
    client: Client<HttpConnector, String>,
    timeout: Duration,
}

impl ConnectorRuntime {
    /// Build a runtime that gives up on a call after `timeout`.
    pub fn new(timeout: Duration) -> Self {
        let client: Client<HttpConnector, String> =
            Client::builder(TokioExecutor::new()).build(HttpConnector::new());
        Self { client, timeout }
    }

    /// Process-wide runtime used by API handlers (five-second deadline).
    pub fn shared() -> &'static Self {
        static RUNTIME: OnceLock<ConnectorRuntime> = OnceLock::new();
        RUNTIME.get_or_init(|| Self::new(DEFAULT_TIMEOUT))
    }

    /// Perform one request without following redirects.
    pub async fn send(
        &self,
        url: &str,
        method: &str,
        body: &str,
        headers: &[(String, String)],
    ) -> Result<HttpReply, ApiError> {
        if !is_safe_target(url) {
            return Err(ApiError::BadRequest(format!(
                "unsafe connector target: {}",
                nocode_deploy::redact(url)
            )));
        }
        if url.to_ascii_lowercase().starts_with("https://") {
            return Err(ApiError::BadRequest(HTTPS_MESSAGE.to_string()));
        }
        let method = Method::from_bytes(method.as_bytes()).map_err(|error| {
            ApiError::BadRequest(format!(
                "invalid connector method: {}",
                nocode_deploy::redact(&error.to_string())
            ))
        })?;

        let mut builder = Request::builder().method(method).uri(url);
        for (name, value) in headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        if !body.is_empty() {
            builder = builder.header(hyper::header::CONTENT_TYPE, "application/json");
        }
        let request = builder.body(body.to_string()).map_err(|error| {
            ApiError::BadRequest(format!(
                "invalid connector request: {}",
                nocode_deploy::redact(&error.to_string())
            ))
        })?;

        let response = tokio::time::timeout(self.timeout, self.client.request(request))
            .await
            .map_err(|_| ApiError::GatewayTimeout("connector did not respond in time".to_string()))?
            .map_err(|error| {
                ApiError::BadRequest(format!(
                    "connector request failed: {}",
                    nocode_deploy::redact(&error.to_string())
                ))
            })?;
        let status = response.status();
        let collected = tokio::time::timeout(
            self.timeout,
            axum::body::to_bytes(
                axum::body::Body::new(response.into_body()),
                MAX_RESPONSE_BYTES,
            ),
        )
        .await
        .map_err(|_| ApiError::GatewayTimeout("connector response timed out".to_string()))?
        .map_err(|error| {
            ApiError::BadRequest(format!(
                "connector response failed: {}",
                nocode_deploy::redact(&error.to_string())
            ))
        })?;
        Ok(HttpReply {
            status,
            body: collected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_safe_target_accepts_plain_http_and_rejects_dangerous_targets() {
        assert!(is_safe_target("http://127.0.0.1:8080/api"));
        assert!(is_safe_target("https://api.example.com/v1?x=1"));
        assert!(!is_safe_target("ftp://example.com"));
        assert!(!is_safe_target("javascript:alert(1)"));
        assert!(!is_safe_target("http://user:secret@example.com"));
        assert!(!is_safe_target("http://example.com\nX-Evil: 1"));
        assert!(!is_safe_target("http://example.com/a b"));
        assert!(!is_safe_target("http://"));
        assert!(!is_safe_target(""));
    }

    #[test]
    fn redact_config_walks_nested_objects_and_arrays() {
        let value = serde_json::json!({
            "url": "http://localhost:9000",
            "AUTHORIZATION": "Bearer abc",
            "nested": {
                "clientSecret": "hidden-1",
                "items": [{"password": "hidden-2", "keep": "visible"}]
            }
        });
        let redacted = redact_config(&value);
        let dumped = redacted.to_string();
        assert!(!dumped.contains("hidden-1"));
        assert!(!dumped.contains("hidden-2"));
        assert!(!dumped.contains("Bearer abc"));
        assert_eq!(redacted["url"], serde_json::json!("http://localhost:9000"));
        assert_eq!(
            redacted["nested"]["items"][0]["keep"],
            serde_json::json!("visible")
        );
    }

    #[test]
    fn rate_allow_enforces_capacity_and_refills() {
        let mut bucket: HashMap<String, (f64, i64)> = HashMap::new();
        assert!(rate_allow(&mut bucket, "tenant-a", 0, 60.0));
        assert!(rate_allow(&mut bucket, "tenant-a", 1, 60.0));
        assert!(!rate_allow(&mut bucket, "tenant-a", 1, 60.0));
        // Other tenants have their own bucket.
        assert!(rate_allow(&mut bucket, "tenant-b", 1, 60.0));
        // 1 token per second at 60/min: one refill after a full second.
        assert!(rate_allow(&mut bucket, "tenant-a", 1001, 60.0));
        assert!(!rate_allow(&mut bucket, "tenant-a", 1001, 60.0));
        assert!(!rate_allow(&mut bucket, "x", 0, 0.0));
    }

    #[test]
    fn breaker_opens_after_five_failures_and_half_opens_after_window() {
        assert_eq!(breaker_state(0, None, 1000), BreakerState::Closed);
        assert_eq!(breaker_state(4, None, 1000), BreakerState::Closed);
        assert_eq!(breaker_state(5, None, 1000), BreakerState::Open);
        assert_eq!(breaker_state(5, Some(1000), 29_999), BreakerState::Open);
        assert_eq!(breaker_state(5, Some(1000), 31_000), BreakerState::HalfOpen);
    }

    #[test]
    fn hmac_matches_rfc4231_test_vector() {
        let key = [0x0bu8; 20];
        let digest = hmac_sha256(&key, b"Hi There");
        let expected: [u8; 32] = [
            0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b,
            0xf1, 0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c,
            0x2e, 0x32, 0xcf, 0xf7,
        ];
        assert_eq!(digest, expected);
        assert_eq!(
            hex_encode(&digest),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn join_target_keeps_the_stored_authority() {
        assert_eq!(
            join_target("http://127.0.0.1:9000/api/", "/items?x=1").unwrap(),
            "http://127.0.0.1:9000/api/items?x=1"
        );
        assert!(join_target("http://127.0.0.1:9000", "http://evil.example").is_err());
        assert!(join_target("http://127.0.0.1:9000", "//evil.example").is_err());
        assert!(join_target("ftp://127.0.0.1:9000", "/ok").is_err());
    }
}
