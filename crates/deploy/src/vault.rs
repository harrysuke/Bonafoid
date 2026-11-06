//! Credential vault: encrypts secrets at rest with ChaCha20-Poly1305.
//!
//! The server's master key comes from `IDE_SECRET_KEY`.
//! The desktop keeps a key file in its app-data directory.
//!
//! A plaintext secret is NEVER returned by any public API.

use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error};

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed — ciphertext may be tampered or key is wrong")]
    Decrypt,
    #[error("key must be exactly {KEY_LEN} bytes, got {0}")]
    KeyLength(usize),
    #[error("missing master key: set IDE_SECRET_KEY or provide a key file")]
    MissingKey,
    #[error("hex decode failed")]
    HexDecode,
}

/// An encrypted credential blob.
///
/// The nonce is stored alongside the ciphertext so the same key can decrypt
/// it later. The format is opaque to callers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SealedCredential {
    /// hex-encoded nonce
    pub nonce: String,
    /// hex-encoded ciphertext (includes the Poly1305 tag)
    pub ciphertext: String,
}

/// Encrypts and decrypts credentials.
///
/// Clone is cheap: the key lives behind an `Arc`.
#[derive(Clone)]
pub struct CredentialVault {
    cipher: Arc<ChaCha20Poly1305>,
}

impl CredentialVault {
    /// Create a vault from a raw 32-byte key.
    pub fn from_key(key: &[u8]) -> Result<Self, VaultError> {
        if key.len() != KEY_LEN {
            return Err(VaultError::KeyLength(key.len()));
        }
        let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|_| VaultError::Encrypt)?;
        Ok(Self {
            cipher: Arc::new(cipher),
        })
    }

    /// Create a vault from a hex-encoded key string.
    pub fn from_hex(key_hex: &str) -> Result<Self, VaultError> {
        let key = hex::decode(key_hex).map_err(|_| VaultError::HexDecode)?;
        Self::from_key(&key)
    }

    /// Create a vault from the `IDE_SECRET_KEY` environment variable.
    pub fn from_env() -> Result<Self, VaultError> {
        let key_hex = std::env::var("IDE_SECRET_KEY").map_err(|_| VaultError::MissingKey)?;
        Self::from_hex(&key_hex)
    }

    /// Seal a plaintext secret.
    pub fn seal(&self, plaintext: &str) -> Result<SealedCredential, VaultError> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| VaultError::Encrypt)?;
        debug!("sealed credential ({} bytes)", ciphertext.len());
        Ok(SealedCredential {
            nonce: hex::encode(nonce),
            ciphertext: hex::encode(ciphertext),
        })
    }

    /// Open a sealed credential. This is the ONLY way to recover plaintext.
    ///
    /// Callers MUST NOT log or return the result.
    pub fn open(&self, sealed: &SealedCredential) -> Result<String, VaultError> {
        let nonce = hex::decode(&sealed.nonce).map_err(|_| VaultError::Decrypt)?;
        let nonce = Nonce::from_slice(&nonce);
        let ciphertext = hex::decode(&sealed.ciphertext).map_err(|_| VaultError::Decrypt)?;
        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|_| VaultError::Decrypt)?;
        String::from_utf8(plaintext).map_err(|_| VaultError::Decrypt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> Vec<u8> {
        vec![0u8; 32]
    }

    #[test]
    fn round_trip() {
        let vault = CredentialVault::from_key(&test_key()).unwrap();
        let secret = "ghp_xxxxxxxxxxxxxxxxxxxx";
        let sealed = vault.seal(secret).unwrap();
        let opened = vault.open(&sealed).unwrap();
        assert_eq!(opened, secret);
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let vault = CredentialVault::from_key(&test_key()).unwrap();
        let mut sealed = vault.seal("secret").unwrap();
        // Flip one hex nibble
        let last = sealed.ciphertext.pop().unwrap();
        sealed.ciphertext.push(if last == '0' { '1' } else { '0' });
        assert!(vault.open(&sealed).is_err());
    }

    #[test]
    fn wrong_key_fails() {
        let vault_a = CredentialVault::from_key(&test_key()).unwrap();
        let sealed = vault_a.seal("secret").unwrap();
        let mut other_key = test_key();
        other_key[0] = 1;
        let vault_b = CredentialVault::from_key(&other_key).unwrap();
        assert!(vault_b.open(&sealed).is_err());
    }

    #[test]
    fn key_length_enforced() {
        assert!(CredentialVault::from_key(&[0u8; 31]).is_err());
        assert!(CredentialVault::from_key(&[0u8; 33]).is_err());
    }
}
