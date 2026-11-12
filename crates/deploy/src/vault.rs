//! Credential vault: encrypts secrets at rest with ChaCha20-Poly1305.
//!
//! The server's master key comes from `IDE_SECRET_KEY`.
//! The desktop keeps a key file in its app-data directory.
//!
//! A plaintext secret is NEVER returned by any response DTO. [`open`] is the
//! single internal recovery point; callers must not log or serialise its
//! result.

use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;

const KEY_LEN: usize = 32;

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
    #[error("key file error: {0}")]
    KeyFile(String),
}

/// An encrypted credential blob.
///
/// The nonce is stored alongside the ciphertext so the same key can decrypt
/// it later. The format is opaque to callers. Both fields are hex, never
/// plaintext.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SealedCredential {
    /// hex-encoded nonce
    pub nonce: String,
    /// hex-encoded ciphertext (includes the Poly1305 tag)
    pub ciphertext: String,
}

/// Encrypts and decrypts credentials.
///
/// Clone is cheap: the key lives behind an `Arc`. `Debug` is intentionally
/// NOT derived — the key must never reach a log line.
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
        let key = hex::decode(key_hex.trim()).map_err(|_| VaultError::HexDecode)?;
        Self::from_key(&key)
    }

    /// Create a vault from the `IDE_SECRET_KEY` environment variable.
    ///
    /// There is deliberately no generated fallback: a key that changes or
    /// disappears between restarts makes every stored credential
    /// undecryptable, which is worse than refusing to start the feature.
    pub fn from_env() -> Result<Self, VaultError> {
        let key_hex = std::env::var("IDE_SECRET_KEY").map_err(|_| VaultError::MissingKey)?;
        Self::from_hex(&key_hex)
    }

    /// Create (on first use) or read a key file for a local desktop host.
    ///
    /// The file holds a hex-encoded 32-byte key. On Unix it is mode 0600;
    /// on Windows it inherits the ACL of the app-data directory, which is
    /// already user-scoped. It must never be committed or synced.
    pub fn from_key_file(path: &Path) -> Result<Self, VaultError> {
        if path.is_file() {
            let contents =
                std::fs::read_to_string(path).map_err(|e| VaultError::KeyFile(e.to_string()))?;
            return Self::from_hex(&contents);
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| VaultError::KeyFile(e.to_string()))?;
        }
        let mut key = [0u8; KEY_LEN];
        OsRng.fill_bytes(&mut key);
        std::fs::write(path, hex::encode(key)).map_err(|e| VaultError::KeyFile(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| VaultError::KeyFile(e.to_string()))?;
        }
        Self::from_key(&key)
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
    /// Callers MUST NOT log, serialise or return the result.
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

    #[test]
    fn sealed_blob_never_contains_the_plaintext() {
        let vault = CredentialVault::from_key(&test_key()).unwrap();
        let sealed = vault.seal("ghp_topsecretvalue").unwrap();
        let as_json = serde_json::to_string(&sealed).unwrap();
        assert!(!as_json.contains("topsecretvalue"));
    }

    #[test]
    fn key_file_is_created_once_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("vault.key");
        let first = CredentialVault::from_key_file(&path).unwrap();
        let sealed = first.seal("persisted").unwrap();
        let second = CredentialVault::from_key_file(&path).unwrap();
        assert_eq!(second.open(&sealed).unwrap(), "persisted");
        assert!(path.is_file());
    }
}
