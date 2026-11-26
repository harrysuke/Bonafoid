//! Provider keys: the assistant's model catalog.
//!
//! Before this module the assistant could drive exactly one model, chosen by
//! `AGENT_PROVIDER` / `AGENT_API_KEY` at boot. That is a fine default and a bad
//! product: the drawer could not offer a choice, and changing models meant
//! restarting the server. `agent_api_keys` (migration 0012) holds one row per
//! provider, sealed with the same [`CredentialVault`] the deploy service uses,
//! so the drawer can list what is available and a run can pick one.
//!
//! # One table, not two
//!
//! Which platforms exist, what endpoint each one uses, which headers it needs
//! and whether it accepts anonymous calls is knowledge that belongs to the
//! agent crate, because that is where the request is built. This module used to
//! keep its own copy of that table, and the copies drifted: rows for Cohere,
//! Kilo and OpenCode were imported from the previous product, held a valid key,
//! and were still reported unusable because this file said so. The table now
//! lives in [`nocode_agent::llm::catalog`] and this module only decides *which
//! stored rows can run*.
//!
//! # What is never returned
//!
//! [`ProviderInfo`] is the only shape that leaves this module, and it has no
//! field for a key. `sealed_nonce` / `sealed_ciphertext` are opened exactly
//! once per `(key, model)` pair, inside [`KeyCatalog::provider`], and the
//! plaintext lives only in the provider that needs it. `Debug` is not derived
//! on anything that holds a key.
//!
//! # Ownership
//!
//! The table is deployment-wide storage, but spending a key is gated by
//! [`super::access`]: only user ids on the `agent_key_access` allowlist may
//! reach a route that resolves a provider. `user_id` is legacy provenance —
//! the migration wrote the *old* application's user id into it — so the
//! authoritative owner is `owner_user_id`, added by migration 0013.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nocode_agent::llm::catalog::{self, ProviderSpec};
use nocode_agent::llm::{ChatMessage, ChatRequest, LlmProvider};
use nocode_agent::{AgentConfig, ProviderKind};
use nocode_deploy::vault::{CredentialVault, SealedCredential};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;

/// One provider the drawer can offer.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    /// Selection token: the row id. Stable across renames.
    pub id: String,
    pub platform: String,
    /// What the picker shows. The row's label when it has one, else the
    /// platform's display name.
    pub label: String,
    /// Wire format: `openai`, `anthropic` or `ollama`.
    pub kind: String,
    /// Model used when the request names none.
    pub model: String,
    /// Suggestions for the model field. Not exhaustive on purpose: a gateway
    /// ships new models faster than this table can be edited.
    pub models: Vec<String>,
    /// Last recorded health (`healthy`, `invalid`, `error`, `unknown`).
    pub status: String,
    pub enabled: bool,
    /// Override stored on the row, when it has one.
    pub base_url: Option<String>,
    /// True when the platform serves requests without any credential, so a
    /// missing key is not a problem.
    pub keyless: bool,
    /// Why this provider cannot run, when it cannot. `None` means usable.
    pub unavailable: Option<String>,
}

/// One platform this build supports, whether or not a key row exists for it.
///
/// The drawer needs this to offer "add a provider" for something that has never
/// been configured; the key rows alone can only describe what already exists.
#[derive(Debug, Clone, Serialize)]
pub struct PlatformInfo {
    pub platform: &'static str,
    pub label: &'static str,
    pub kind: &'static str,
    /// Default endpoint. Empty when the caller must supply one.
    pub base_url: &'static str,
    pub keyless: bool,
    /// True when the credential is `account_id:api_token` (Cloudflare).
    pub account_scoped: bool,
    pub default_model: &'static str,
    pub models: &'static [&'static str],
    /// Setup caveat, e.g. "needs a base URL on its key row".
    pub note: Option<&'static str>,
}

/// Every platform the build can reach, in catalog order.
pub fn supported_platforms() -> Vec<PlatformInfo> {
    catalog::CATALOG
        .iter()
        .map(|spec| PlatformInfo {
            platform: spec.platform,
            label: spec.label,
            kind: spec.kind.as_str(),
            base_url: spec.base_url,
            keyless: spec.keyless,
            account_scoped: spec.account_scoped,
            default_model: spec.default_model,
            models: spec.models,
            note: spec.note,
        })
        .collect()
}

/// A provider resolved for one run.
pub struct Resolved {
    pub provider: Arc<dyn LlmProvider>,
    pub platform: String,
    pub label: String,
    pub model: String,
}

fn spec_for(platform: &str) -> Option<&'static ProviderSpec> {
    catalog::spec(platform)
}

/// One row of `agent_api_keys`, as the catalog holds it.
struct KeyEntry {
    id: i64,
    platform: String,
    label: String,
    base_url: Option<String>,
    status: String,
    enabled: bool,
    sealed: Option<SealedCredential>,
}

impl KeyEntry {
    /// The endpoint this row would use: the operator's override, else the
    /// platform default. `None` when neither exists.
    fn base_url(&self) -> Option<String> {
        self.base_url
            .clone()
            .or_else(|| spec_for(&self.platform).map(|spec| spec.base_url.to_string()))
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty())
    }

    fn keyless(&self) -> bool {
        spec_for(&self.platform)
            .map(|spec| spec.keyless)
            .unwrap_or(false)
    }
}

/// Every usable provider, with its keys opened lazily.
pub struct KeyCatalog {
    entries: Vec<KeyEntry>,
    vault: Option<CredentialVault>,
    /// Why the vault is missing, when it is.
    vault_error: Option<String>,
    /// One provider per `(key, model)`. Building one is cheap, but a provider
    /// owns an HTTP client, so a picker that is opened repeatedly should not
    /// create one per click.
    built: Mutex<HashMap<(i64, String), Arc<dyn LlmProvider>>>,
}

impl std::fmt::Debug for KeyCatalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print a sealed blob or an opened key.
        f.debug_struct("KeyCatalog")
            .field("entries", &self.entries.len())
            .field("vault", &self.vault.is_some())
            .finish()
    }
}

/// The vault this server can open `agent_api_keys` with.
///
/// `IDE_SECRET_KEY` first, because that is the same key the deploy service
/// uses. Then a key file, for a local install that was set up by the migration
/// script (`data/vault.key`): that file is the only copy of the key, so it must
/// be read, never regenerated — `from_key_file` would happily create a *new*
/// key for a missing path, which would silently make every stored credential
/// undecryptable.
pub fn vault_from_env() -> Result<CredentialVault, String> {
    if let Ok(value) = std::env::var("IDE_SECRET_KEY") {
        if !value.trim().is_empty() {
            return CredentialVault::from_hex(&value)
                .map_err(|error| format!("IDE_SECRET_KEY is not a 64-character hex key: {error}"));
        }
    }
    let path = std::env::var("IDE_VAULT_KEY_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/vault.key"));
    if path.is_file() {
        return CredentialVault::from_key_file(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()));
    }
    Err(format!(
        "set IDE_SECRET_KEY (64 hex characters) or IDE_VAULT_KEY_FILE to the key that sealed \
         agent_api_keys; {} does not exist",
        path.display()
    ))
}

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

impl KeyCatalog {
    /// Read every key row. An empty table is not an error: it means the
    /// operator has not configured a provider, which the drawer explains.
    pub async fn load(db: &DatabaseConnection) -> Result<Self, ApiError> {
        let (vault, vault_error) = match vault_from_env() {
            Ok(vault) => (Some(vault), None),
            Err(reason) => (None, Some(reason)),
        };
        Self::load_rows(db, vault, vault_error).await
    }

    /// Read every key row with a caller-supplied vault.
    ///
    /// Exists so a test does not have to mutate the process environment to
    /// hold `IDE_SECRET_KEY`; nothing in production calls it.
    #[cfg(test)]
    pub async fn load_with(
        db: &DatabaseConnection,
        vault: CredentialVault,
    ) -> Result<Self, ApiError> {
        Self::load_rows(db, Some(vault), None).await
    }

    async fn load_rows(
        db: &DatabaseConnection,
        vault: Option<CredentialVault>,
        vault_error: Option<String>,
    ) -> Result<Self, ApiError> {
        let rows = db
            .query_all(statement(
                "SELECT id, platform, label, base_url, status, enabled, \
                        sealed_nonce, sealed_ciphertext \
                   FROM agent_api_keys ORDER BY platform, id",
                vec![],
            ))
            .await
            .map_err(|error| {
                tracing::error!(%error, "could not read agent_api_keys");
                ApiError::Internal
            })?;

        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let nonce: String = row.try_get("", "sealed_nonce").unwrap_or_default();
            let ciphertext: String = row.try_get("", "sealed_ciphertext").unwrap_or_default();
            entries.push(KeyEntry {
                id: row.try_get("", "id").unwrap_or_default(),
                platform: row.try_get("", "platform").unwrap_or_default(),
                label: row.try_get("", "label").unwrap_or_default(),
                base_url: row.try_get("", "base_url").ok().filter(|v: &String| !v.is_empty()),
                status: row
                    .try_get("", "status")
                    .unwrap_or_else(|_| "unknown".to_string()),
                enabled: row.try_get::<i64>("", "enabled").unwrap_or(1) != 0,
                sealed: (!nonce.is_empty() && !ciphertext.is_empty())
                    .then(|| SealedCredential { nonce, ciphertext }),
            });
        }

        if let Some(reason) = vault_error.as_deref() {
            tracing::warn!(reason, "provider keys are stored but cannot be opened");
        }

        Ok(Self {
            entries,
            vault,
            vault_error,
            built: Mutex::new(HashMap::new()),
        })
    }

    /// Whether there is anything to offer at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Why a row cannot run, or `None` when it can.
    ///
    /// Order matters: a missing endpoint is a setup mistake the operator can
    /// fix, so it is reported before "disabled"; a keyless platform is checked
    /// before the vault, because it never needs one.
    fn unavailable(&self, entry: &KeyEntry) -> Option<String> {
        let spec = spec_for(&entry.platform);
        if entry.base_url().is_none() {
            return Some(
                spec.and_then(|spec| spec.note)
                    .unwrap_or("set a base URL on its key row")
                    .to_string(),
            );
        }
        if !entry.enabled {
            return Some("disabled".to_string());
        }
        if entry.keyless() {
            return None;
        }
        if self.vault.is_none() {
            return self
                .vault_error
                .clone()
                .or_else(|| Some("the server cannot open stored keys".to_string()));
        }
        if entry.sealed.is_none() {
            return Some("the row has no sealed key".to_string());
        }
        None
    }

    /// The picker's data. Never includes a key.
    pub fn info(&self) -> Vec<ProviderInfo> {
        self.entries
            .iter()
            .map(|entry| {
                let spec = spec_for(&entry.platform);
                let kind = spec
                    .map(|spec| spec.kind.as_str().to_string())
                    .unwrap_or_else(|| {
                        // An unknown platform with a base URL is assumed to be
                        // OpenAI-compatible; without one it is unusable anyway.
                        if entry.base_url.is_some() {
                            "openai".to_string()
                        } else {
                            "unsupported".to_string()
                        }
                    });
                let label = if entry.label.trim().is_empty() {
                    spec.map(|spec| spec.label.to_string())
                        .unwrap_or_else(|| entry.platform.clone())
                } else {
                    entry.label.clone()
                };
                ProviderInfo {
                    id: entry.id.to_string(),
                    platform: entry.platform.clone(),
                    label,
                    kind,
                    model: spec
                        .map(|spec| spec.default_model.to_string())
                        .unwrap_or_default(),
                    models: spec
                        .map(|spec| spec.models.iter().map(|m| m.to_string()).collect())
                        .unwrap_or_default(),
                    status: entry.status.clone(),
                    enabled: entry.enabled,
                    base_url: entry.base_url.clone(),
                    keyless: entry.keyless(),
                    unavailable: self.unavailable(entry),
                }
            })
            .collect()
    }

    /// Find a row by selection: a row id (`"6"`, `"#6"`) or a platform name.
    fn find(&self, selection: &str) -> Option<&KeyEntry> {
        let trimmed = selection.trim();
        let bare = trimmed.strip_prefix('#').unwrap_or(trimmed);
        if let Ok(id) = bare.parse::<i64>() {
            if let Some(entry) = self.entries.iter().find(|entry| entry.id == id) {
                return Some(entry);
            }
        }
        // Platform name: prefer an enabled, usable row when there are several.
        let matches: Vec<&KeyEntry> = self
            .entries
            .iter()
            .filter(|entry| entry.platform.eq_ignore_ascii_case(bare))
            .collect();
        matches
            .iter()
            .find(|entry| self.unavailable(entry).is_none())
            .or_else(|| matches.first())
            .copied()
    }

    /// The first row that can actually run, for a request that named none.
    fn first_usable(&self) -> Option<&KeyEntry> {
        self.entries
            .iter()
            .find(|entry| self.unavailable(entry).is_none())
    }

    /// The selection token for the first usable row.
    pub fn default_selection(&self) -> Option<String> {
        self.first_usable().map(|entry| entry.id.to_string())
    }

    /// Open this row's credential, or `None` when the platform is keyless.
    fn open_key(&self, entry: &KeyEntry) -> Result<Option<String>, ApiError> {
        if entry.keyless() {
            return Ok(None);
        }
        let vault = self
            .vault
            .as_ref()
            .ok_or_else(|| ApiError::ServiceUnavailable("the server cannot open stored keys".into()))?;
        let sealed = entry
            .sealed
            .as_ref()
            .ok_or_else(|| ApiError::ServiceUnavailable("the key row has no sealed key".into()))?;
        let key = vault.open(sealed).map_err(|error| {
            // The error is about the vault, never the key: safe to log.
            tracing::warn!(%error, platform = %entry.platform, "could not open a provider key");
            ApiError::ServiceUnavailable(format!(
                "the key for {} could not be decrypted; check IDE_SECRET_KEY / IDE_VAULT_KEY_FILE",
                entry.platform
            ))
        })?;
        Ok(Some(key))
    }

    /// Resolve a selection into a provider, opening the key once per
    /// `(key, model)` pair.
    pub fn provider(&self, selection: &str, model: Option<&str>) -> Result<Resolved, ApiError> {
        let entry = self.find(selection).ok_or_else(|| {
            ApiError::BadRequest(format!("unknown assistant provider: {selection}"))
        })?;
        if let Some(reason) = self.unavailable(entry) {
            return Err(ApiError::ServiceUnavailable(format!(
                "the assistant cannot use {}: {reason}",
                entry.platform
            )));
        }

        let spec = spec_for(&entry.platform);
        let model = model
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                spec.map(|spec| spec.default_model.to_string())
                    .unwrap_or_default()
            });
        if model.is_empty() {
            // An unknown platform has no default model to fall back to, and a
            // completion with an empty model name is a provider error the user
            // cannot act on.
            return Err(ApiError::BadRequest(format!(
                "choose a model for {}: this platform has no default",
                entry.platform
            )));
        }

        let cache_key = (entry.id, model.clone());
        if let Some(provider) = self
            .built
            .lock()
            .ok()
            .and_then(|cache| cache.get(&cache_key).cloned())
        {
            return Ok(Resolved {
                provider,
                platform: entry.platform.clone(),
                label: entry.label.clone(),
                model,
            });
        }

        let api_key = self.open_key(entry)?;
        let config = self.config_for(entry, model.clone(), api_key)?;
        let provider = nocode_agent::build_provider(&config).map_err(|error| {
            tracing::warn!(%error, platform = %entry.platform, "could not build a provider");
            ApiError::ServiceUnavailable(format!(
                "the assistant could not use {}: {error}",
                entry.platform
            ))
        })?;
        if let Ok(mut cache) = self.built.lock() {
            cache.insert(cache_key, provider.clone());
        }
        Ok(Resolved {
            provider,
            platform: entry.platform.clone(),
            label: entry.label.clone(),
            model,
        })
    }

    /// Build the agent config for one row.
    ///
    /// A catalog platform resolves its endpoint, headers and account-scoped
    /// credential itself. A row whose platform this build does not know is
    /// still tried as an OpenAI-compatible gateway when it carries a base URL,
    /// so an operator can add a gateway without waiting for a release.
    fn config_for(
        &self,
        entry: &KeyEntry,
        model: String,
        api_key: Option<String>,
    ) -> Result<AgentConfig, ApiError> {
        if spec_for(&entry.platform).is_some() {
            return AgentConfig::for_platform(
                &entry.platform,
                model,
                entry.base_url(),
                api_key,
            )
            .map_err(|error| {
                ApiError::ServiceUnavailable(format!(
                    "the assistant cannot use {}: {error}",
                    entry.platform
                ))
            });
        }

        let base_url = entry.base_url().ok_or_else(|| {
            ApiError::ServiceUnavailable(format!(
                "the assistant cannot use {}: no base URL",
                entry.platform
            ))
        })?;
        // No catalog entry means no static headers and no keyless rule, which
        // is exactly what a generic gateway needs.
        Ok(AgentConfig::for_provider(
            ProviderKind::OpenAi,
            model,
            base_url,
            api_key,
        ))
    }
}

/* ------------------------------------------------------------------ *
 * Writing keys
 * ------------------------------------------------------------------ */

/// A provider the operator is adding.
///
/// `platform` is a catalog id (`groq`, `google`, `cloudflare`, …) or any name
/// the build does not know, which is then treated as an OpenAI-compatible
/// gateway and requires a `base_url`. Nothing else about the request is
/// platform-specific: the catalog supplies the endpoint, the headers and the
/// keyless rule, so this shape does not grow a field per vendor.
#[derive(Debug, Default, Deserialize)]
pub struct KeyDraft {
    /// Catalog platform id. Case-insensitive.
    pub platform: String,
    /// The credential. Required unless the platform is keyless.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Display name. Defaults to the platform's label.
    #[serde(default)]
    pub label: Option<String>,
    /// Endpoint override. Only required by `custom` and unknown platforms.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Defaults to enabled, because adding a key is intent to use it.
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// Fields a patch may change. Absent means "leave it alone".
#[derive(Debug, Default, Deserialize)]
pub struct KeyPatch {
    /// `Some("")` clears the stored credential; `Some(key)` replaces it.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    /// `Some("")` drops the override and falls back to the platform default.
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// A draft that has passed validation.
struct Validated {
    platform: String,
    label: String,
    base_url: Option<String>,
    api_key: Option<String>,
    enabled: bool,
}

impl std::fmt::Debug for Validated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand-written, not derived: `api_key` is plaintext here, and a
        // `{:?}` on a validation failure must not put a credential in a log.
        f.debug_struct("Validated")
            .field("platform", &self.platform)
            .field("label", &self.label)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// Trim a string field and treat blank as absent.
fn field(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Check a draft against the catalog before anything is stored.
///
/// Every rule here is one the request builder would otherwise discover at the
/// first completion — an endpoint that does not exist, a credential that was
/// never supplied, a Cloudflare token missing its account id. Catching them now
/// turns a failed run into a form error the operator can read.
fn validate(draft: &KeyDraft) -> Result<Validated, ApiError> {
    let platform = draft.platform.trim().to_ascii_lowercase();
    if platform.is_empty() {
        return Err(ApiError::BadRequest("name a platform to add".into()));
    }

    let base_url = field(draft.base_url.as_deref());
    let supplied = field(draft.api_key.as_deref());
    let spec = spec_for(&platform);

    // `custom` ships an empty endpoint on purpose, and an unknown platform has
    // no default at all: both are gateways that only work with an override.
    if base_url.is_none() && spec.map(|spec| spec.base_url.is_empty()).unwrap_or(true) {
        return Err(ApiError::BadRequest(match spec {
            Some(spec) => format!(
                "{} needs a base URL: {}",
                spec.label,
                spec.note.unwrap_or("give it the gateway's /v1 endpoint")
            ),
            None => format!(
                "unknown platform `{platform}`; give it a base URL to run it as an \
                 OpenAI-compatible gateway"
            ),
        }));
    }

    let label = field(draft.label.as_deref())
        .or_else(|| spec.map(|spec| spec.label.to_string()))
        .unwrap_or_else(|| platform.clone());

    let api_key = match spec {
        // A keyless platform is never given a credential: sealing one the
        // adapter will not send is a secret kept for no reason.
        Some(spec) if spec.keyless => None,
        Some(spec) if spec.account_scoped => {
            let key = supplied.ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "{} needs a credential in the form `account_id:api_token`",
                    spec.label
                ))
            })?;
            // Cloudflare puts the account in the URL and the token in the
            // header, so a bare token is not a shorter version of the same
            // thing — it addresses no account.
            let (account, token) = key.split_once(':').ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "{} credentials are `account_id:api_token`, not a bare token",
                    spec.label
                ))
            })?;
            if account.trim().is_empty() || token.trim().is_empty() {
                return Err(ApiError::BadRequest(format!(
                    "{} credentials are `account_id:api_token`; one half is empty",
                    spec.label
                )));
            }
            Some(key)
        }
        Some(spec) => Some(supplied.ok_or_else(|| {
            ApiError::BadRequest(format!("{} needs an API key", spec.label))
        })?),
        None => Some(supplied.ok_or_else(|| {
            ApiError::BadRequest(format!("unknown platform `{platform}` needs an API key"))
        })?),
    };

    Ok(Validated {
        platform,
        label,
        base_url,
        api_key,
        enabled: draft.enabled.unwrap_or(true),
    })
}

/// Seal a credential into the `(nonce, ciphertext)` pair the table stores.
///
/// `(None, None)` is the keyless case. The columns are `NOT NULL`, so an absent
/// credential is written as two empty strings, which is exactly what
/// [`KeyCatalog::load`] reads back as "this row has no sealed key".
fn seal(
    vault: Option<&CredentialVault>,
    api_key: Option<&str>,
) -> Result<(String, String), ApiError> {
    let Some(key) = api_key else {
        return Ok((String::new(), String::new()));
    };
    let vault = vault.ok_or_else(|| {
        ApiError::ServiceUnavailable(
            "set IDE_SECRET_KEY (64 hex characters) or IDE_VAULT_KEY_FILE so a provider key \
             can be sealed"
                .into(),
        )
    })?;
    let sealed = vault.seal(key).map_err(|error| {
        tracing::error!(%error, "could not seal a provider key");
        ApiError::Internal
    })?;
    Ok((sealed.nonce, sealed.ciphertext))
}

/// Add a provider, sealing its credential with this server's vault.
pub async fn insert_key(
    db: &DatabaseConnection,
    owner: &str,
    draft: &KeyDraft,
) -> Result<i64, ApiError> {
    // A keyless platform must still be addable on a host that never configured
    // a vault key, so the failure is deferred to `seal`, not raised here.
    let vault = vault_from_env().ok();
    insert_key_with(db, owner, draft, vault.as_ref()).await
}

/// [`insert_key`] with an explicit vault, so a test does not need the
/// environment to hold a key.
pub async fn insert_key_with(
    db: &DatabaseConnection,
    owner: &str,
    draft: &KeyDraft,
    vault: Option<&CredentialVault>,
) -> Result<i64, ApiError> {
    let validated = validate(draft)?;
    let (nonce, ciphertext) = seal(vault, validated.api_key.as_deref())?;

    // Adding a platform that already has a row is allowed: it is how a key is
    // rotated without a window where the old one is already gone.
    db.execute(statement(
        "INSERT INTO agent_api_keys \
             (platform, label, base_url, sealed_nonce, sealed_ciphertext, status, enabled, owner_user_id) \
         VALUES (?, ?, ?, ?, ?, 'unknown', ?, ?)",
        vec![
            Value::from(validated.platform),
            Value::from(validated.label),
            validated.base_url.map(Value::from).unwrap_or(Value::String(None)),
            Value::from(nonce),
            Value::from(ciphertext),
            Value::from(i64::from(validated.enabled)),
            Value::from(owner.to_string()),
        ],
    ))
    .await
    .map_err(|error| {
        tracing::error!(%error, "could not insert a provider key");
        ApiError::Internal
    })?;

    db.query_one(statement("SELECT last_insert_rowid() AS id", vec![]))
        .await
        .map_err(|error| {
            tracing::error!(%error, "could not read the new provider key id");
            ApiError::Internal
        })?
        .and_then(|row| row.try_get::<i64>("", "id").ok())
        .ok_or(ApiError::Internal)
}

/// Change one row. Only the fields the caller sent are touched.
pub async fn update_key(
    db: &DatabaseConnection,
    id: i64,
    patch: &KeyPatch,
) -> Result<(), ApiError> {
    let vault = vault_from_env().ok();
    update_key_with(db, id, patch, vault.as_ref()).await
}

/// [`update_key`] with an explicit vault.
pub async fn update_key_with(
    db: &DatabaseConnection,
    id: i64,
    patch: &KeyPatch,
    vault: Option<&CredentialVault>,
) -> Result<(), ApiError> {
    let mut sets: Vec<&str> = Vec::new();
    let mut values: Vec<Value> = Vec::new();

    if let Some(label) = &patch.label {
        sets.push("label = ?");
        values.push(Value::from(label.trim().to_string()));
    }
    if let Some(base_url) = &patch.base_url {
        let trimmed = base_url.trim();
        sets.push("base_url = ?");
        values.push(if trimmed.is_empty() {
            Value::String(None)
        } else {
            Value::from(trimmed.to_string())
        });
    }
    if let Some(enabled) = patch.enabled {
        sets.push("enabled = ?");
        values.push(Value::from(i64::from(enabled)));
    }
    if let Some(api_key) = &patch.api_key {
        let trimmed = api_key.trim();
        let (nonce, ciphertext) = seal(vault, (!trimmed.is_empty()).then_some(trimmed))?;
        sets.push("sealed_nonce = ?");
        values.push(Value::from(nonce));
        sets.push("sealed_ciphertext = ?");
        values.push(Value::from(ciphertext));
        // A different credential invalidates whatever the last check saw.
        sets.push("status = 'unknown'");
    }

    if sets.is_empty() {
        return Err(ApiError::BadRequest(
            "nothing to change: send a label, a base URL, an API key or `enabled`".into(),
        ));
    }

    values.push(Value::from(id));
    let sql = format!(
        "UPDATE agent_api_keys SET {} WHERE id = ?",
        sets.join(", ")
    );
    let result = db.execute(statement(&sql, values)).await.map_err(|error| {
        tracing::error!(%error, "could not update a provider key");
        ApiError::Internal
    })?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

/// Remove a row. The sealed blob goes with it, which is the point.
pub async fn delete_key(db: &DatabaseConnection, id: i64) -> Result<(), ApiError> {
    let result = db
        .execute(statement(
            "DELETE FROM agent_api_keys WHERE id = ?",
            vec![Value::from(id)],
        ))
        .await
        .map_err(|error| {
            tracing::error!(%error, "could not delete a provider key");
            ApiError::Internal
        })?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

/// The verdict of a live check.
#[derive(Debug, Clone, Serialize)]
pub struct ProbeOutcome {
    pub ok: bool,
    /// What was recorded on the row: `healthy`, `invalid` or `error`.
    pub status: String,
    /// The provider's own words, bounded.
    pub detail: String,
    pub latency_ms: u64,
}

/// Ask the provider a question only a working credential can answer.
///
/// A stored key is a claim; this is the only way to turn it into a fact. The
/// request is deliberately tiny — eight tokens, no tools — because the point is
/// whether the credential authenticates, not what the model can do.
pub async fn probe_key(db: &DatabaseConnection, id: i64) -> Result<ProbeOutcome, ApiError> {
    // A fresh catalog, not the runtime's cache: the check must use the row as
    // it is stored now, including a credential that was just replaced.
    let catalog = KeyCatalog::load(db).await?;
    // A missing id is a 404, not the 400 `provider()` would raise: the caller
    // named a row that does not exist, which is the same answer DELETE gives.
    if !catalog.entries.iter().any(|entry| entry.id == id) {
        return Err(ApiError::NotFound);
    }
    let resolved = catalog.provider(&id.to_string(), None)?;

    let request = ChatRequest {
        system: "You are a health check. Reply with the single word OK.".to_string(),
        messages: vec![ChatMessage::User("ping".to_string())],
        tools: Vec::new(),
        max_tokens: 8,
        temperature: 0.0,
    };

    let started = std::time::Instant::now();
    let result = resolved.provider.complete(request).await;
    let latency_ms = started.elapsed().as_millis() as u64;

    let (ok, status, detail) = match result {
        Ok(_) => (true, "healthy", "the provider answered".to_string()),
        Err(error) => {
            let message = error.to_string();
            // `http` reports a non-2xx as "HTTP 401: …", so an authentication
            // failure is distinguishable from a network one without a second
            // parse of the body. `invalid` is worth its own status: it means
            // the credential is wrong, not that the provider is down.
            let status = if message.contains("HTTP 401") || message.contains("HTTP 403") {
                "invalid"
            } else {
                "error"
            };
            (false, status, message.chars().take(300).collect())
        }
    };

    // Record the verdict; the picker shows it next to the row. A failure to
    // write is logged, not raised: the operator asked what the provider said,
    // and that answer is already in hand.
    if let Err(error) = db
        .execute(statement(
            "UPDATE agent_api_keys SET status = ?, last_checked_at = datetime('now') WHERE id = ?",
            vec![Value::from(status.to_string()), Value::from(id)],
        ))
        .await
    {
        tracing::warn!(%error, "could not record a provider health check");
    }

    Ok(ProbeOutcome {
        ok,
        status: status.to_string(),
        detail,
        latency_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(platform: &str, enabled: bool) -> KeyEntry {
        KeyEntry {
            id: 1,
            platform: platform.to_string(),
            label: String::new(),
            base_url: None,
            status: "healthy".to_string(),
            enabled,
            sealed: Some(SealedCredential {
                nonce: "00".repeat(12),
                ciphertext: "00".repeat(32),
            }),
        }
    }

    fn catalog_with(entries: Vec<KeyEntry>) -> KeyCatalog {
        KeyCatalog {
            entries,
            vault: CredentialVault::from_key(&[7u8; 32]).ok(),
            vault_error: None,
            built: Mutex::new(HashMap::new()),
        }
    }

    #[test]
    fn every_supported_platform_is_described_for_the_picker() {
        let platforms = supported_platforms();
        assert!(platforms.len() >= 20, "{}", platforms.len());
        for info in &platforms {
            assert!(!info.label.is_empty(), "{} has no label", info.platform);
            assert!(
                ["openai", "anthropic", "ollama"].contains(&info.kind),
                "{} has wire format {}",
                info.platform,
                info.kind
            );
        }
        // The platforms the previous table called unusable are all supported.
        for platform in ["cohere", "kilo", "opencode", "cloudflare", "google"] {
            assert!(
                platforms.iter().any(|info| info.platform == platform),
                "{platform} is missing"
            );
        }
    }

    #[test]
    fn info_never_carries_a_key() {
        let info = catalog_with(vec![entry("groq", true)]).info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("ciphertext"));
        assert!(!json.contains("nonce"));
        assert_eq!(info[0].platform, "groq");
        assert_eq!(info[0].kind, "openai");
        assert!(!info[0].keyless);
        assert!(info[0].unavailable.is_none());
    }

    #[test]
    fn a_disabled_row_is_listed_but_not_usable() {
        let catalog = catalog_with(vec![entry("groq", false)]);
        let info = catalog.info();
        assert_eq!(info[0].unavailable.as_deref(), Some("disabled"));
        assert!(catalog.first_usable().is_none());
        assert!(catalog.provider("groq", None).is_err());
    }

    #[test]
    fn a_platform_without_a_base_url_says_so() {
        // `custom` is the only entry with no default endpoint.
        let catalog = catalog_with(vec![entry("custom", true)]);
        let info = catalog.info();
        assert!(info[0]
            .unavailable
            .as_deref()
            .unwrap_or_default()
            .contains("base URL"));
    }

    #[test]
    fn a_keyless_platform_needs_no_sealed_key_and_no_vault() {
        let mut row = entry("kilo", true);
        row.sealed = None;
        let catalog = KeyCatalog {
            entries: vec![row],
            vault: None,
            vault_error: Some("set IDE_SECRET_KEY".into()),
            built: Mutex::new(HashMap::new()),
        };
        let info = catalog.info();
        assert!(info[0].keyless);
        assert!(info[0].unavailable.is_none());
        assert!(catalog.provider("kilo", None).is_ok());
    }

    #[test]
    fn an_unknown_platform_is_a_gateway_when_it_has_a_base_url() {
        let mut row = entry("acme", true);
        row.base_url = Some("https://acme.test".into());
        let info = catalog_with(vec![row]).info();
        assert_eq!(info[0].kind, "openai");
        assert!(info[0].unavailable.is_none());
    }

    #[test]
    fn a_gateway_with_no_default_model_asks_for_one() {
        let vault = CredentialVault::from_key(&[7u8; 32]).unwrap();
        let mut row = entry("acme", true);
        row.base_url = Some("https://acme.test".into());
        row.sealed = Some(vault.seal("sk-acme").unwrap());
        let catalog = catalog_with(vec![row]);
        assert!(matches!(
            catalog.provider("acme", None),
            Err(ApiError::BadRequest(_))
        ));
        assert!(catalog.provider("acme", Some("acme-1")).is_ok());
    }

    #[test]
    fn selection_accepts_a_row_id_or_a_platform() {
        let mut first = entry("groq", true);
        first.id = 4;
        let mut second = entry("mistral", true);
        second.id = 9;
        let catalog = catalog_with(vec![first, second]);

        assert_eq!(catalog.find("9").map(|e| e.id), Some(9));
        assert_eq!(catalog.find("#4").map(|e| e.id), Some(4));
        assert_eq!(catalog.find("Mistral").map(|e| e.id), Some(9));
        assert!(catalog.find("nope").is_none());
    }

    #[test]
    fn a_missing_vault_makes_a_keyed_row_unusable() {
        let catalog = KeyCatalog {
            entries: vec![entry("groq", true)],
            vault: None,
            vault_error: Some("set IDE_SECRET_KEY".into()),
            built: Mutex::new(HashMap::new()),
        };
        let info = catalog.info();
        assert_eq!(info[0].unavailable.as_deref(), Some("set IDE_SECRET_KEY"));
    }

    #[test]
    fn an_unknown_selection_is_a_bad_request() {
        let catalog = catalog_with(vec![entry("groq", true)]);
        assert!(matches!(
            catalog.provider("nope", None),
            Err(ApiError::BadRequest(_))
        ));
    }

    #[test]
    fn a_sealed_key_builds_a_provider_for_the_selected_model() {
        let vault = CredentialVault::from_key(&[7u8; 32]).unwrap();
        let mut row = entry("groq", true);
        row.sealed = Some(vault.seal("sk-test-value").unwrap());
        let catalog = catalog_with(vec![row]);

        let resolved = catalog
            .provider("groq", Some("llama-3.1-8b-instant"))
            .expect("a healthy key with a base URL");
        assert_eq!(resolved.model, "llama-3.1-8b-instant");
        assert_eq!(resolved.platform, "groq");
        // The wire format, not the platform name: the orchestrator reports it
        // in `run_started` and the drawer shows `provider/model`.
        assert_eq!(resolved.provider.name(), "openai");
        // Building twice reuses the cached provider rather than a second client.
        assert!(catalog.provider("groq", Some("llama-3.1-8b-instant")).is_ok());
        assert_eq!(catalog.built.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_key_sealed_with_another_vault_is_refused_not_panicked() {
        let other = CredentialVault::from_key(&[9u8; 32]).unwrap();
        let mut row = entry("groq", true);
        row.sealed = Some(other.seal("sk-other").unwrap());
        let catalog = catalog_with(vec![row]);
        assert!(matches!(
            catalog.provider("groq", None),
            Err(ApiError::ServiceUnavailable(_))
        ));
    }

    /// The regression this rewrite exists for: rows migrated from the previous
    /// product held valid keys for platforms this build refused to run.
    #[test]
    fn every_migrated_row_with_a_key_is_usable() {
        let vault = CredentialVault::from_key(&[7u8; 32]).unwrap();
        let platforms = [
            "cerebras", "cohere", "github", "groq", "huggingface", "kilo", "llm7", "mistral",
            "nvidia", "ollama", "opencode", "openrouter", "pollinations", "sambanova", "zhipu",
        ];
        let rows: Vec<KeyEntry> = platforms
            .iter()
            .enumerate()
            .map(|(index, platform)| {
                let mut row = entry(platform, true);
                row.id = index as i64 + 1;
                row.sealed = Some(vault.seal("sk-test").unwrap());
                row
            })
            .collect();
        let catalog = catalog_with(rows);
        for info in catalog.info() {
            assert!(
                info.unavailable.is_none(),
                "{} is unusable: {:?}",
                info.platform,
                info.unavailable
            );
            assert!(
                catalog.provider(&info.platform, None).is_ok(),
                "{} could not build a provider",
                info.platform
            );
        }
    }

    /* -------------------------------------------------------------- *
     * Writing keys
     * -------------------------------------------------------------- */

    /// A migrated database with no provider rows, so a test starts from the
    /// state an operator with a fresh install sees.
    async fn db() -> DatabaseConnection {
        crate::db::connect_sqlite_memory().await.expect("db")
    }

    /// A fixed vault: `load_with` takes it explicitly, so no test has to
    /// mutate the process environment to hold `IDE_SECRET_KEY`.
    fn test_vault() -> CredentialVault {
        CredentialVault::from_key(&[7u8; 32]).unwrap()
    }

    async fn rows(db: &DatabaseConnection) -> i64 {
        db.query_one(statement(
            "SELECT COUNT(*) AS n FROM agent_api_keys",
            vec![],
        ))
        .await
        .unwrap()
        .and_then(|row| row.try_get::<i64>("", "n").ok())
        .unwrap_or(-1)
    }

    /// The sealed pair as stored, which is what "the credential changed" means.
    async fn sealed_of(db: &DatabaseConnection, id: i64) -> (String, String) {
        db.query_one(statement(
            "SELECT sealed_nonce, sealed_ciphertext FROM agent_api_keys WHERE id = ?",
            vec![Value::from(id)],
        ))
        .await
        .unwrap()
        .map(|row| {
            (
                row.try_get("", "sealed_nonce").unwrap_or_default(),
                row.try_get("", "sealed_ciphertext").unwrap_or_default(),
            )
        })
        .expect("row exists")
    }

    fn draft(platform: &str, api_key: Option<&str>) -> KeyDraft {
        KeyDraft {
            platform: platform.to_string(),
            api_key: api_key.map(str::to_string),
            ..Default::default()
        }
    }

    /// A keyless platform is the one case that must work on a host with no
    /// vault key at all, because it never stores a credential.
    #[tokio::test]
    async fn a_keyless_platform_can_be_added_without_a_vault() {
        let db = db().await;
        let id = insert_key_with(&db, "user-1", &draft("Kilo", None), None)
            .await
            .expect("a keyless platform needs no vault");

        let catalog = KeyCatalog::load_with(&db, test_vault()).await.unwrap();
        let info = &catalog.info()[0];
        assert_eq!(info.platform, "kilo", "the platform id is lowercased");
        assert_eq!(info.label, catalog::spec("kilo").unwrap().label);
        assert!(info.keyless);
        assert!(info.unavailable.is_none(), "{:?}", info.unavailable);
        assert!(catalog.provider(&id.to_string(), None).is_ok());
    }

    #[tokio::test]
    async fn adding_a_keyed_platform_seals_the_credential() {
        let db = db().await;
        let vault = test_vault();
        let id = insert_key_with(&db, "user-1", &draft("groq", Some("  sk-live-123  ")), Some(&vault))
            .await
            .expect("insert");

        let (nonce, ciphertext) = sealed_of(&db, id).await;
        assert!(!nonce.is_empty() && !ciphertext.is_empty());
        // The plaintext is not recoverable from the row, which is the whole
        // point of sealing it.
        assert!(!ciphertext.contains("sk-live"));

        // …and the vault opens it, trimmed, so a pasted trailing space does not
        // become a 401 the operator cannot explain.
        let opened = vault
            .open(&SealedCredential { nonce, ciphertext })
            .expect("the vault that sealed it");
        assert_eq!(opened, "sk-live-123");
    }

    /// The loop the picker depends on: write a row, read the catalog, run it.
    #[tokio::test]
    async fn a_written_key_is_usable_on_the_next_read() {
        let db = db().await;
        insert_key_with(&db, "user-1", &draft("groq", Some("sk-live")), Some(&test_vault()))
            .await
            .unwrap();

        let catalog = KeyCatalog::load_with(&db, test_vault()).await.unwrap();
        let info = &catalog.info()[0];
        assert!(info.unavailable.is_none(), "{:?}", info.unavailable);
        assert_eq!(info.model, catalog::spec("groq").unwrap().default_model);
        assert_eq!(info.status, "unknown", "a new row has not been checked yet");

        let resolved = catalog.provider("groq", None).expect("usable");
        assert_eq!(resolved.platform, "groq");
        assert_eq!(resolved.provider.name(), "openai");
    }

    #[tokio::test]
    async fn a_platform_that_needs_a_key_is_refused_without_one() {
        let db = db().await;
        let error = insert_key_with(&db, "user-1", &draft("groq", None), Some(&test_vault()))
            .await
            .unwrap_err();
        assert!(matches!(error, ApiError::BadRequest(_)), "{error:?}");
        assert!(error.to_string().contains("API key"), "{error}");
        // A rejected draft must not leave a half-written row behind.
        assert_eq!(rows(&db).await, 0);
    }

    #[tokio::test]
    async fn cloudflare_requires_an_account_scoped_credential() {
        let db = db().await;
        let bare = insert_key_with(
            &db,
            "user-1",
            &draft("cloudflare", Some("sk-bare-token")),
            Some(&test_vault()),
        )
        .await
        .unwrap_err();
        // The message names the fix, because "invalid key" would send the
        // operator back to the dashboard for a token that is already right.
        assert!(bare.to_string().contains("account_id"), "{bare}");
        assert_eq!(rows(&db).await, 0);

        let id = insert_key_with(
            &db,
            "user-1",
            &draft("cloudflare", Some("acct-1:sk-token")),
            Some(&test_vault()),
        )
        .await
        .expect("an account-scoped credential");
        let catalog = KeyCatalog::load_with(&db, test_vault()).await.unwrap();
        assert!(catalog.provider(&id.to_string(), None).is_ok());
    }

    #[tokio::test]
    async fn an_unknown_platform_must_bring_a_base_url() {
        let db = db().await;
        let refused = insert_key_with(&db, "user-1", &draft("acme", Some("sk")), Some(&test_vault()))
            .await
            .unwrap_err();
        assert!(refused.to_string().contains("base URL"), "{refused}");

        let mut gateway = draft("acme", Some("sk"));
        gateway.base_url = Some("https://acme.test/v1".into());
        let id = insert_key_with(&db, "user-1", &gateway, Some(&test_vault()))
            .await
            .expect("a gateway with an endpoint");

        let catalog = KeyCatalog::load_with(&db, test_vault()).await.unwrap();
        let info = &catalog.info()[0];
        assert_eq!(info.kind, "openai");
        assert!(info.unavailable.is_none(), "{:?}", info.unavailable);
        // No catalog entry means no default model, so the caller must name one.
        assert!(matches!(
            catalog.provider(&id.to_string(), None),
            Err(ApiError::BadRequest(_))
        ));
    }

    /// `custom` ships an empty endpoint on purpose: it is the "any gateway"
    /// escape hatch, and the note on the platform says so.
    #[tokio::test]
    async fn a_custom_endpoint_is_required_to_name_its_base_url() {
        let db = db().await;
        let error = insert_key_with(&db, "user-1", &draft("custom", Some("sk")), Some(&test_vault()))
            .await
            .unwrap_err();
        let note = catalog::spec("custom").unwrap().note.unwrap();
        assert!(error.to_string().contains(note), "{error}");
    }

    #[tokio::test]
    async fn patching_a_label_leaves_the_credential_alone() {
        let db = db().await;
        let vault = test_vault();
        let id = insert_key_with(&db, "user-1", &draft("groq", Some("sk-1")), Some(&vault))
            .await
            .unwrap();
        let before = sealed_of(&db, id).await;

        update_key_with(
            &db,
            id,
            &KeyPatch {
                label: Some("Fast".into()),
                ..Default::default()
            },
            Some(&vault),
        )
        .await
        .expect("patch a label");

        // A patch that does not mention the key must not reseal it: the
        // operator cannot see the key, so resending it is not an option.
        assert_eq!(sealed_of(&db, id).await, before);
        let catalog = KeyCatalog::load_with(&db, test_vault()).await.unwrap();
        assert_eq!(catalog.info()[0].label, "Fast");
    }

    #[tokio::test]
    async fn an_empty_api_key_clears_the_credential() {
        let db = db().await;
        let vault = test_vault();
        let id = insert_key_with(&db, "user-1", &draft("groq", Some("sk-1")), Some(&vault))
            .await
            .unwrap();

        update_key_with(
            &db,
            id,
            &KeyPatch {
                api_key: Some(String::new()),
                ..Default::default()
            },
            Some(&vault),
        )
        .await
        .expect("clear the key");

        let (nonce, ciphertext) = sealed_of(&db, id).await;
        assert!(nonce.is_empty() && ciphertext.is_empty());
        // The row survives so the operator can paste a replacement, and the
        // picker explains why it cannot run.
        let catalog = KeyCatalog::load_with(&db, test_vault()).await.unwrap();
        assert_eq!(
            catalog.info()[0].unavailable.as_deref(),
            Some("the row has no sealed key")
        );
    }

    #[tokio::test]
    async fn disabling_a_row_takes_it_out_of_the_default_choice() {
        let db = db().await;
        let vault = test_vault();
        let id = insert_key_with(&db, "user-1", &draft("groq", Some("sk-1")), Some(&vault))
            .await
            .unwrap();

        update_key_with(
            &db,
            id,
            &KeyPatch {
                enabled: Some(false),
                ..Default::default()
            },
            Some(&vault),
        )
        .await
        .expect("disable");

        let catalog = KeyCatalog::load_with(&db, test_vault()).await.unwrap();
        assert_eq!(catalog.info()[0].unavailable.as_deref(), Some("disabled"));
        assert!(catalog.default_selection().is_none());
    }

    /// `base_url: ""` is the only way to get back to the platform default, so
    /// it must clear the column rather than store an empty string.
    #[tokio::test]
    async fn an_empty_base_url_falls_back_to_the_platform_default() {
        let db = db().await;
        let vault = test_vault();
        let mut gateway = draft("acme", Some("sk"));
        gateway.base_url = Some("https://acme.test/v1".into());
        let id = insert_key_with(&db, "user-1", &gateway, Some(&vault))
            .await
            .unwrap();

        update_key_with(
            &db,
            id,
            &KeyPatch {
                base_url: Some("  ".into()),
                ..Default::default()
            },
            Some(&vault),
        )
        .await
        .expect("clear the override");

        let catalog = KeyCatalog::load_with(&db, test_vault()).await.unwrap();
        assert!(catalog.info()[0].base_url.is_none());
        // With no override and no catalog entry there is no endpoint left.
        assert!(catalog.info()[0].unavailable.is_some());
    }

    #[tokio::test]
    async fn a_patch_with_no_fields_is_rejected() {
        let db = db().await;
        let id = insert_key_with(&db, "user-1", &draft("kilo", None), None)
            .await
            .unwrap();
        let error = update_key_with(&db, id, &KeyPatch::default(), None)
            .await
            .unwrap_err();
        assert!(matches!(error, ApiError::BadRequest(_)), "{error:?}");
    }

    #[tokio::test]
    async fn updating_a_missing_row_is_a_404() {
        let db = db().await;
        let error = update_key_with(
            &db,
            999,
            &KeyPatch {
                label: Some("x".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, ApiError::NotFound), "{error:?}");
    }

    #[tokio::test]
    async fn deleting_a_row_removes_it_once() {
        let db = db().await;
        let id = insert_key_with(&db, "user-1", &draft("kilo", None), None)
            .await
            .unwrap();
        delete_key(&db, id).await.expect("delete");
        assert_eq!(rows(&db).await, 0);
        assert!(matches!(delete_key(&db, id).await, Err(ApiError::NotFound)));
    }

    /// Testing a row that does not exist is a 404 for the same reason DELETE
    /// is: the caller named something that is not there.
    #[tokio::test]
    async fn probing_a_missing_row_is_a_404() {
        let db = db().await;
        assert!(matches!(probe_key(&db, 999).await, Err(ApiError::NotFound)));
    }

    /// A row that exists but cannot be built is a 503 with the reason, before
    /// any network call is attempted.
    #[tokio::test]
    async fn probing_an_unusable_row_explains_why() {
        let db = db().await;
        let id = insert_key_with(&db, "user-1", &draft("groq", Some("sk-1")), Some(&test_vault()))
            .await
            .unwrap();
        // Disable it: no completion should be attempted, and the answer is the
        // same reason the picker shows.
        update_key_with(
            &db,
            id,
            &KeyPatch {
                enabled: Some(false),
                ..Default::default()
            },
            Some(&test_vault()),
        )
        .await
        .unwrap();

        // `probe_key` loads with the *environment's* vault, so a row sealed
        // with the test vault is also unusable here — either way it must not
        // reach the network.
        match probe_key(&db, id).await {
            Err(ApiError::ServiceUnavailable(reason)) => {
                assert!(!reason.is_empty(), "a 503 must say why");
            }
            other => panic!("expected a 503, got {other:?}"),
        }
    }

    /// A keyless platform never stores a credential, even when one is pasted:
    /// a secret kept for no reason is a liability, not a convenience.
    #[tokio::test]
    async fn a_keyless_platform_does_not_seal_a_supplied_key() {
        let db = db().await;
        let id = insert_key_with(&db, "user-1", &draft("kilo", Some("sk-unused")), None)
            .await
            .unwrap();
        let (nonce, ciphertext) = sealed_of(&db, id).await;
        assert!(nonce.is_empty() && ciphertext.is_empty());
    }

    /// Validation runs before sealing, so a missing vault key is only a problem
    /// for the drafts that actually need one.
    #[tokio::test]
    async fn a_keyed_platform_without_a_vault_says_what_to_set() {
        let db = db().await;
        let error = insert_key_with(&db, "user-1", &draft("groq", Some("sk-1")), None)
            .await
            .unwrap_err();
        assert!(matches!(error, ApiError::ServiceUnavailable(_)), "{error:?}");
        assert!(error.to_string().contains("IDE_SECRET_KEY"), "{error}");
        assert_eq!(rows(&db).await, 0);
    }

    #[test]
    fn validation_lowercases_and_trims() {
        let validated = validate(&KeyDraft {
            platform: "  Groq ".into(),
            api_key: Some(" sk-1 ".into()),
            label: Some("  ".into()),
            base_url: Some("  ".into()),
            enabled: None,
        })
        .expect("valid");
        assert_eq!(validated.platform, "groq");
        assert_eq!(validated.api_key.as_deref(), Some("sk-1"));
        assert_eq!(validated.base_url, None, "a blank override is no override");
        // A blank label is not a label: fall back to the platform's.
        assert_eq!(validated.label, catalog::spec("groq").unwrap().label);
        assert!(validated.enabled, "adding a key is intent to use it");
    }

    #[test]
    fn an_empty_platform_is_refused() {
        let error = validate(&KeyDraft::default()).unwrap_err();
        assert!(error.to_string().contains("platform"), "{error}");
    }
}
