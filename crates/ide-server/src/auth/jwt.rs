//! RS256 JSON Web Token issuing and verification.
//!
//! Access and refresh tokens are both RS256-signed by the same key pair. The
//! `typ` claim distinguishes them so a refresh token cannot be replayed as an
//! access token (and vice versa).

use anyhow::{bail, Context, Result};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

/// Access token lifetime (15 minutes).
pub const ACCESS_TTL_SECS: i64 = 15 * 60;
/// Refresh token lifetime (30 days).
pub const REFRESH_TTL_SECS: i64 = 30 * 24 * 60 * 60;
/// Token issuer recorded in (and required by) every token.
pub const ISSUER: &str = "nocode-ide";
/// `typ` claim value for access tokens.
pub const TOKEN_TYPE_ACCESS: &str = "access";
/// `typ` claim value for refresh tokens.
pub const TOKEN_TYPE_REFRESH: &str = "refresh";

/// Claims carried by both access and refresh tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject: the user id.
    pub sub: String,
    /// User email (for convenience; not used for authorization).
    pub email: String,
    /// Unique token id; the refresh row is keyed by this.
    pub jti: String,
    /// Issuer.
    pub iss: String,
    /// Token type: `access` or `refresh`.
    pub typ: String,
    /// Issued-at (unix seconds).
    pub iat: i64,
    /// Expiry (unix seconds).
    pub exp: i64,
}

impl Claims {
    /// Build an access token's claims.
    pub fn access(sub: &str, email: &str, jti: &str, now: i64) -> Self {
        Self {
            sub: sub.to_string(),
            email: email.to_string(),
            jti: jti.to_string(),
            iss: ISSUER.to_string(),
            typ: TOKEN_TYPE_ACCESS.to_string(),
            iat: now,
            exp: now + ACCESS_TTL_SECS,
        }
    }

    /// Build a refresh token's claims.
    pub fn refresh(sub: &str, email: &str, jti: &str, now: i64) -> Self {
        Self {
            sub: sub.to_string(),
            email: email.to_string(),
            jti: jti.to_string(),
            iss: ISSUER.to_string(),
            typ: TOKEN_TYPE_REFRESH.to_string(),
            iat: now,
            exp: now + REFRESH_TTL_SECS,
        }
    }
}

/// Current unix time in seconds.
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// An RSA key pair used to sign and verify tokens.
pub struct JwtKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl std::fmt::Debug for JwtKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtKeys").finish_non_exhaustive()
    }
}

impl JwtKeys {
    /// Load a PEM key pair from `dir`, generating and persisting one if absent.
    pub fn load_or_generate(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let private_path = dir.join("private.pem");
        let public_path = dir.join("public.pem");

        if private_path.exists() && public_path.exists() {
            let private_pem = std::fs::read_to_string(&private_path)
                .with_context(|| format!("failed to read {}", private_path.display()))?;
            let public_pem = std::fs::read_to_string(&public_path)
                .with_context(|| format!("failed to read {}", public_path.display()))?;
            return Self::from_pem(&private_pem, &public_pem);
        }

        std::fs::create_dir_all(dir)
            .with_context(|| format!("failed to create key directory {}", dir.display()))?;
        let (private_pem, public_pem) = generate_pem()?;
        std::fs::write(&private_path, private_pem.as_bytes())
            .with_context(|| format!("failed to write {}", private_path.display()))?;
        std::fs::write(&public_path, public_pem.as_bytes())
            .with_context(|| format!("failed to write {}", public_path.display()))?;
        tracing::info!(dir = %dir.display(), "generated new RSA signing keys");

        Self::from_pem(&private_pem, &public_pem)
    }

    /// Build a key pair from PEM text.
    pub fn from_pem(private_pem: &str, public_pem: &str) -> Result<Self> {
        let encoding = EncodingKey::from_rsa_pem(private_pem.as_bytes())
            .context("invalid RSA private key PEM")?;
        let decoding = DecodingKey::from_rsa_pem(public_pem.as_bytes())
            .context("invalid RSA public key PEM")?;
        Ok(Self { encoding, decoding })
    }

    /// Generate an in-memory key pair (used by tests; never persisted).
    pub fn generate_ephemeral() -> Result<Self> {
        let (private_pem, public_pem) = generate_pem()?;
        Self::from_pem(&private_pem, &public_pem)
    }

    /// Encode claims into a signed RS256 token.
    pub fn encode(&self, claims: &Claims) -> Result<String> {
        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".to_string());
        encode(&header, claims, &self.encoding).context("failed to encode token")
    }

    /// Verify and decode an access token.
    pub fn decode_access(&self, token: &str) -> Result<Claims> {
        self.decode(token, TOKEN_TYPE_ACCESS)
    }

    /// Verify and decode a refresh token.
    pub fn decode_refresh(&self, token: &str) -> Result<Claims> {
        self.decode(token, TOKEN_TYPE_REFRESH)
    }

    fn decode(&self, token: &str, expected_type: &str) -> Result<Claims> {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[ISSUER]);
        validation.validate_exp = true;
        validation.validate_aud = false;

        let data = decode::<Claims>(token, &self.decoding, &validation).context("invalid token")?;
        if data.claims.typ != expected_type {
            bail!("unexpected token type");
        }
        Ok(data.claims)
    }
}

fn generate_pem() -> Result<(Zeroizing<String>, String)> {
    let mut rng = rand::thread_rng();
    let key = RsaPrivateKey::new(&mut rng, 2048).context("failed to generate RSA key")?;
    let private_pem = key
        .to_pkcs8_pem(LineEnding::LF)
        .context("failed to encode private key")?;
    let public_pem = RsaPublicKey::from(&key)
        .to_public_key_pem(LineEnding::LF)
        .context("failed to encode public key")?;
    Ok((private_pem, public_pem))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> JwtKeys {
        JwtKeys::generate_ephemeral().expect("keygen")
    }

    #[test]
    fn access_token_round_trips() {
        let keys = keys();
        let now = now_secs();
        let claims = Claims::access("user-1", "a@b.com", "jti-1", now);
        let token = keys.encode(&claims).unwrap();
        let decoded = keys.decode_access(&token).unwrap();
        assert_eq!(decoded.sub, "user-1");
        assert_eq!(decoded.email, "a@b.com");
        assert_eq!(decoded.jti, "jti-1");
        assert_eq!(decoded.typ, TOKEN_TYPE_ACCESS);
        assert_eq!(decoded.exp, now + ACCESS_TTL_SECS);
    }

    #[test]
    fn refresh_token_round_trips() {
        let keys = keys();
        let claims = Claims::refresh("user-2", "c@d.com", "jti-2", now_secs());
        let token = keys.encode(&claims).unwrap();
        let decoded = keys.decode_refresh(&token).unwrap();
        assert_eq!(decoded.typ, TOKEN_TYPE_REFRESH);
    }

    #[test]
    fn refresh_token_is_rejected_as_access() {
        let keys = keys();
        let claims = Claims::refresh("user-2", "c@d.com", "jti-2", now_secs());
        let token = keys.encode(&claims).unwrap();
        assert!(keys.decode_access(&token).is_err());
    }

    #[test]
    fn tampered_token_is_rejected() {
        let keys = keys();
        let claims = Claims::access("user-1", "a@b.com", "jti-1", now_secs());
        let token = keys.encode(&claims).unwrap();
        let mut bytes = token.into_bytes();
        let mid = bytes.len() / 2;
        bytes[mid] = if bytes[mid] == b'a' { b'b' } else { b'a' };
        let tampered = String::from_utf8(bytes).unwrap();
        assert!(keys.decode_access(&tampered).is_err());
    }

    #[test]
    fn foreign_key_token_is_rejected() {
        let signer = keys();
        let verifier = keys();
        let claims = Claims::access("user-1", "a@b.com", "jti-1", now_secs());
        let token = signer.encode(&claims).unwrap();
        assert!(verifier.decode_access(&token).is_err());
    }
}
