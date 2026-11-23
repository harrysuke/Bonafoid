//! Stripe provider abstraction and webhook signature verification.
//!
//! # Honest limitation
//!
//! This crate has no HTTPS client dependency: `hyper` is present but `hyper-rustls`
//! (or any other TLS stack) is not, and Stripe only accepts HTTPS. Constructing
//! a fake `CheckoutSession { url }` would be a lie and would send users to a
//! non-existent page, so [`StripeProvider`] builds the exact request payloads
//! but returns `ServiceUnavailable` until a real client is wired in. Webhook
//! intake has no such dependency: signature verification is a pure HMAC-SHA256
//! computation over the raw request body and works today.
//!
//! No secret is ever returned in an error or log line.

use async_trait::async_trait;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::ApiError;

/// Inputs for a hosted subscription checkout.
#[derive(Debug, Clone)]
pub struct CheckoutParams {
    pub plan_id: String,
    pub price_id: String,
    pub seat_count: i64,
    pub tenant_id: String,
    pub user_id: String,
    pub email: String,
    pub success_url: String,
    pub cancel_url: String,
}

/// Inputs for the billing portal.
#[derive(Debug, Clone)]
pub struct PortalParams {
    pub return_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckoutSession {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortalSession {
    pub id: String,
    pub url: String,
}

/// Provider-neutral billing operations used by the HTTP layer.
#[async_trait]
pub trait BillingProvider: Send + Sync {
    async fn create_checkout_session(
        &self,
        params: CheckoutParams,
    ) -> Result<CheckoutSession, ApiError>;

    async fn create_portal_session(&self, params: PortalParams) -> Result<PortalSession, ApiError>;
}

const CHECKOUT_UNAVAILABLE: &str =
    "Stripe checkout requires an HTTP client dependency; webhook intake is available";
const PORTAL_UNAVAILABLE: &str =
    "Stripe billing portal requires an HTTP client dependency; webhook intake is available";
const MISSING_KEY: &str = "Stripe checkout is not configured. Set STRIPE_SECRET_KEY to enable hosted checkout; webhook intake is available independently";

/// Stripe-backed provider. The key is held privately and never formatted into
/// a response, error or trace event.
pub struct StripeProvider {
    secret_key: String,
}

impl StripeProvider {
    /// Build from `STRIPE_SECRET_KEY`, or explain the setup step when absent.
    pub fn from_env() -> Result<Self, ApiError> {
        let secret_key = std::env::var("STRIPE_SECRET_KEY")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ApiError::ServiceUnavailable(MISSING_KEY.to_string()))?;
        Ok(Self { secret_key })
    }

    /// The exact form body a real HTTP client would POST to
    /// `https://api.stripe.com/v1/checkout/sessions`. Kept pure so it can be
    /// reviewed and tested while the transport is unavailable.
    pub fn build_checkout_payload(params: &CheckoutParams) -> serde_json::Value {
        serde_json::json!({
            "mode": "subscription",
            "line_items": [{
                "price": params.price_id,
                "quantity": params.seat_count,
            }],
            "success_url": params.success_url,
            "cancel_url": params.cancel_url,
            "client_reference_id": params.tenant_id,
            "customer_email": params.email,
            "metadata": {
                "tenant_id": params.tenant_id,
                "user_id": params.user_id,
                "plan_id": params.plan_id,
            },
            "subscription_data": {
                "metadata": {
                    "tenant_id": params.tenant_id,
                    "plan_id": params.plan_id,
                }
            },
        })
    }

    /// The exact form body for `https://api.stripe.com/v1/billing_portal/sessions`.
    pub fn build_portal_payload(params: &PortalParams) -> serde_json::Value {
        serde_json::json!({ "return_url": params.return_url })
    }

    /// A non-secret description of the configuration state, safe to log.
    pub fn configuration_hint(&self) -> &'static str {
        if self.secret_key.is_empty() {
            "unconfigured"
        } else {
            "configured"
        }
    }
}

#[async_trait]
impl BillingProvider for StripeProvider {
    async fn create_checkout_session(
        &self,
        _params: CheckoutParams,
    ) -> Result<CheckoutSession, ApiError> {
        // `self.secret_key` is deliberately not read into any message.
        Err(ApiError::ServiceUnavailable(
            CHECKOUT_UNAVAILABLE.to_string(),
        ))
    }

    async fn create_portal_session(
        &self,
        _params: PortalParams,
    ) -> Result<PortalSession, ApiError> {
        Err(ApiError::ServiceUnavailable(PORTAL_UNAVAILABLE.to_string()))
    }
}

/// Manual HMAC-SHA256 (RFC 2104) built on `sha2` only.
///
/// Block size is 64 bytes for SHA-256; longer keys are hashed down first.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;

    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let shortened = Sha256::digest(key);
        key_block[..32].copy_from_slice(&shortened);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner_input = Vec::with_capacity(BLOCK + data.len());
    let mut outer_input = Vec::with_capacity(BLOCK + 32);
    for &byte in &key_block {
        inner_input.push(byte ^ 0x36);
        outer_input.push(byte ^ 0x5c);
    }
    inner_input.extend_from_slice(data);
    let inner = Sha256::digest(&inner_input);
    outer_input.extend_from_slice(&inner);
    let outer = Sha256::digest(&outer_input);

    let mut output = [0u8; 32];
    output.copy_from_slice(&outer);
    output
}

/// Constant-time comparison of equal-length digests. Lengths are public (a
/// signature is always 32 bytes), so an early length check leaks no secret.
fn constant_time_eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in a.iter().zip(b.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || value.len() % 2 != 0 {
        return None;
    }
    let mut output = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        output.push((hex_value(pair[0])? << 4) | hex_value(pair[1])?);
    }
    Some(output)
}

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

/// Maximum acceptable age of a `stripe-signature` timestamp, in seconds.
pub const SIGNATURE_TOLERANCE_SECS: i64 = 5 * 60;

/// Verify a Stripe signature over the *raw* body.
///
/// The header format is `t=<unix seconds>,v1=<hex>` with optional additional
/// `v1` values during key rotation. Stale timestamps are refused to bound
/// replay windows; the event id is separately deduplicated in the store.
pub fn verify_signature(secret: &str, header: &str, body: &[u8], now: i64) -> Result<(), ApiError> {
    let mut timestamp: Option<i64> = None;
    let mut signatures: Vec<&str> = Vec::new();
    for part in header.split(',') {
        let part = part.trim();
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "t" => timestamp = value.parse::<i64>().ok(),
            "v1" => signatures.push(value),
            _ => {}
        }
    }

    let timestamp = timestamp.ok_or_else(|| {
        ApiError::BadRequest("stripe-signature is missing a valid timestamp".to_string())
    })?;
    // i128 avoids overflowing on a hostile 64-bit timestamp value.
    let skew = now as i128 - timestamp as i128;
    if skew > SIGNATURE_TOLERANCE_SECS as i128 {
        return Err(ApiError::BadRequest(
            "stripe-signature timestamp is too old".to_string(),
        ));
    }
    if skew < -(SIGNATURE_TOLERANCE_SECS as i128) {
        return Err(ApiError::BadRequest(
            "stripe-signature timestamp is too far in the future".to_string(),
        ));
    }

    let mut signed = Vec::with_capacity(timestamp.to_string().len() + 1 + body.len());
    signed.extend_from_slice(timestamp.to_string().as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(body);
    let expected = hmac_sha256(secret.as_bytes(), &signed);

    // Do not short-circuit on the first match: evaluating every candidate
    // avoids leaking which rotation key matched via response timing.
    let mut matched = false;
    for candidate in &signatures {
        let decoded = hex_decode(candidate);
        let equals = decoded
            .as_deref()
            .map(|bytes| constant_time_eq_bytes(bytes, &expected))
            .unwrap_or(false);
        matched |= equals;
    }
    if matched {
        Ok(())
    } else {
        Err(ApiError::BadRequest(
            "stripe-signature verification failed".to_string(),
        ))
    }
}

/// Build a signature header for tests and local Stripe CLI replay.
#[cfg(test)]
pub(crate) fn test_signature(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let mut signed = Vec::with_capacity(timestamp.to_string().len() + 1 + body.len());
    signed.extend_from_slice(timestamp.to_string().as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(body);
    format!(
        "t={timestamp},v1={}",
        hex_encode(&hmac_sha256(secret.as_bytes(), &signed))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_rfc4231_test_vector() {
        // RFC 4231, test case 1: key = 0x0b * 20, data = "Hi There".
        let key = [0x0bu8; 20];
        let digest = hmac_sha256(&key, b"Hi There");
        assert_eq!(
            hex_encode(&digest),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn verify_signature_accepts_valid_header() {
        let secret = "whsec_test";
        let body = br#"{"id":"evt_1","type":"invoice.paid"}"#;
        let now = 1_700_000_000;
        let header = test_signature(secret, now, body);
        assert!(verify_signature(secret, &header, body, now).is_ok());
    }

    #[test]
    fn verify_signature_accepts_rotation_candidates() {
        let secret = "whsec_rotating";
        let body = b"payload";
        let now = 1_700_000_000;
        let valid = test_signature(secret, now, body);
        let v1 = valid.split("v1=").nth(1).unwrap();
        let header = format!("t={now},v1=deadbeef,v1={v1}");
        assert!(verify_signature(secret, &header, body, now).is_ok());
    }

    #[test]
    fn verify_signature_rejects_tampered_body() {
        let secret = "whsec_test";
        let body = br#"{"id":"evt_1","amount":100}"#;
        let now = 1_700_000_000;
        let header = test_signature(secret, now, body);
        let tampered = br#"{"id":"evt_1","amount":999}"#;
        assert!(verify_signature(secret, &header, tampered, now).is_err());
    }

    #[test]
    fn verify_signature_rejects_wrong_secret_and_malformed_hex() {
        let now = 1_700_000_000;
        let body = b"payload";
        let header = test_signature("whsec_right", now, body);
        assert!(verify_signature("whsec_wrong", &header, body, now).is_err());
        assert!(verify_signature("whsec_right", &format!("t={now},v1=xyz"), body, now).is_err());
        assert!(verify_signature("whsec_right", "v1=abc", body, now).is_err());
        assert!(verify_signature("whsec_right", "", body, now).is_err());
    }

    #[test]
    fn verify_signature_rejects_stale_timestamp() {
        let secret = "whsec_test";
        let body = b"payload";
        let now = 1_700_000_000;
        let old = now - SIGNATURE_TOLERANCE_SECS - 1;
        let header = test_signature(secret, old, body);
        assert!(verify_signature(secret, &header, body, now).is_err());
        // Exactly at the boundary is still acceptable.
        let boundary = test_signature(secret, now - SIGNATURE_TOLERANCE_SECS, body);
        assert!(verify_signature(secret, &boundary, body, now).is_ok());
        // A far-future timestamp must be refused as well.
        let future = test_signature(secret, now + SIGNATURE_TOLERANCE_SECS + 1, body);
        assert!(verify_signature(secret, &future, body, now).is_err());
    }

    #[test]
    fn payload_builders_never_contain_secrets() {
        let params = CheckoutParams {
            plan_id: "plan_1".into(),
            price_id: "price_1".into(),
            seat_count: 4,
            tenant_id: "tenant-a".into(),
            user_id: "u1".into(),
            email: "a@b.com".into(),
            success_url: "https://example.com/ok".into(),
            cancel_url: "https://example.com/no".into(),
        };
        let payload = StripeProvider::build_checkout_payload(&params);
        assert_eq!(payload["mode"], "subscription");
        assert_eq!(payload["line_items"][0]["price"], "price_1");
        assert_eq!(payload["line_items"][0]["quantity"], 4);
        assert_eq!(payload["metadata"]["tenant_id"], "tenant-a");
        let serialized = payload.to_string();
        assert!(!serialized.contains("sk_"));
        assert!(!serialized.contains("whsec_"));

        let portal = StripeProvider::build_portal_payload(&PortalParams {
            return_url: "https://example.com/back".into(),
        });
        assert_eq!(portal["return_url"], "https://example.com/back");
    }

    #[tokio::test]
    async fn provider_errors_are_actionable_and_secret_free() {
        let params = CheckoutParams {
            plan_id: "p".into(),
            price_id: "price".into(),
            seat_count: 1,
            tenant_id: "t".into(),
            user_id: "u".into(),
            email: "a@b.com".into(),
            success_url: "https://example.com/ok".into(),
            cancel_url: "https://example.com/no".into(),
        };
        // Construct directly so the test never depends on process env.
        let provider = StripeProvider {
            secret_key: "sk_test_super_secret_value".into(),
        };
        assert_eq!(provider.configuration_hint(), "configured");
        let error = provider.create_checkout_session(params).await.unwrap_err();
        let message = error.to_string();
        assert!(message.contains("HTTP client dependency"));
        assert!(!message.contains("sk_test"));
        let error = provider
            .create_portal_session(PortalParams {
                return_url: "https://example.com".into(),
            })
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("sk_test"));
    }
}
