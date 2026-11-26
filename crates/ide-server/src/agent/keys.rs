//! Provider keys: the assistant's model catalog.
//!
//! Before this module the assistant could drive exactly one model, chosen by
//! `AGENT_PROVIDER` / `AGENT_API_KEY` at boot. That is a fine default and a bad
//! product: the drawer could not offer a choice, and changing models meant
//! restarting the server. `agent_api_keys` (migration 0012) holds one row per
//! provider, sealed with the same [`CredentialVault`] the deploy service uses,
//! so the drawer can list what is available and a run can pick one.
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

use nocode_agent::llm::LlmProvider;
use nocode_agent::{AgentConfig, ProviderKind};
use nocode_deploy::vault::{CredentialVault, SealedCredential};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value};
use serde::Serialize;

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
    /// Wire format: `openai`, `anthropic`, `ollama`, or `unsupported`.
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
    /// Why this provider cannot run, when it cannot. `None` means usable.
    pub unavailable: Option<String>,
}

/// A provider resolved for one run.
pub struct Resolved {
    pub provider: Arc<dyn LlmProvider>,
    pub platform: String,
    pub label: String,
    pub model: String,
}

/// How to talk to one platform.
struct PlatformSpec {
    platform: &'static str,
    label: &'static str,
    /// `None` when the platform's API is not one of the three wire formats.
    kind: Option<ProviderKind>,
    /// Default endpoint. `None` means the row must carry one (Cloudflare needs
    /// an account id in the path; an unknown gateway has no default at all).
    base_url: Option<&'static str>,
    default_model: &'static str,
    models: &'static [&'static str],
    /// Shown when `kind` is `None` or `base_url` is missing.
    note: Option<&'static str>,
}

/// The platforms this build knows how to reach.
///
/// Everything here speaks the OpenAI chat-completions shape except Anthropic
/// and Ollama. A row whose platform is missing from this table still appears in
/// the picker if it carries a `base_url`: it is treated as OpenAI-compatible,
/// because that is what every gateway added since this table was written will
/// be.
const PLATFORMS: &[PlatformSpec] = &[
    PlatformSpec {
        platform: "openai",
        label: "OpenAI",
        kind: Some(ProviderKind::OpenAi),
        base_url: Some("https://api.openai.com"),
        default_model: "gpt-4o-mini",
        models: &["gpt-4o-mini", "gpt-4o", "gpt-4.1-mini"],
        note: None,
    },
    PlatformSpec {
        platform: "anthropic",
        label: "Anthropic",
        kind: Some(ProviderKind::Anthropic),
        base_url: Some("https://api.anthropic.com"),
        default_model: "claude-sonnet-4-5",
        models: &["claude-sonnet-4-5", "claude-opus-4-1", "claude-haiku-4-5"],
        note: None,
    },
    PlatformSpec {
        platform: "groq",
        label: "Groq",
        kind: Some(ProviderKind::OpenAi),
        base_url: Some("https://api.groq.com/openai"),
        default_model: "llama-3.3-70b-versatile",
        models: &[
            "llama-3.3-70b-versatile",
            "llama-3.1-8b-instant",
            "openai/gpt-oss-120b",
        ],
        note: None,
    },
    PlatformSpec {
        platform: "cerebras",
        label: "Cerebras",
        kind: Some(ProviderKind::OpenAi),
        base_url: Some("https://api.cerebras.ai"),
        default_model: "llama-3.3-70b",
        models: &["llama-3.3-70b", "llama3.1-8b", "qwen-3-32b"],
        note: None,
    },
    PlatformSpec {
        platform: "sambanova",
        label: "SambaNova",
        kind: Some(ProviderKind::OpenAi),
        base_url: Some("https://api.sambanova.ai"),
        default_model: "Meta-Llama-3.3-70B-Instruct",
        models: &[
            "Meta-Llama-3.3-70B-Instruct",
            "Meta-Llama-3.1-8B-Instruct",
        ],
        note: None,
    },
    PlatformSpec {
        platform: "nvidia",
        label: "NVIDIA NIM",
        kind: Some(ProviderKind::OpenAi),
        base_url: Some("https://integrate.api.nvidia.com"),
        default_model: "meta/llama-3.3-70b-instruct",
        models: &[
            "meta/llama-3.3-70b-instruct",
            "nvidia/llama-3.1-nemotron-70b-instruct",
        ],
        note: None,
    },
    PlatformSpec {
        platform: "mistral",
        label: "Mistral",
        kind: Some(ProviderKind::OpenAi),
        base_url: Some("https://api.mistral.ai"),
        default_model: "mistral-small-latest",
        models: &["mistral-small-latest", "mistral-large-latest", "open-mistral-nemo"],
        note: None,
    },
    PlatformSpec {
        platform: "openrouter",
        label: "OpenRouter",
        kind: Some(ProviderKind::OpenAi),
        base_url: Some("https://openrouter.ai/api"),
        default_model: "openai/gpt-4o-mini",
        models: &[
            "openai/gpt-4o-mini",
            "anthropic/claude-sonnet-4",
            "google/gemini-2.5-flash",
            "meta-llama/llama-3.3-70b-instruct",
        ],
        note: None,
    },
    PlatformSpec {
        platform: "github",
        label: "GitHub Models",
        kind: Some(ProviderKind::OpenAi),
        // A full endpoint, not a base: GitHub's path is `/inference/chat/...`
        // with no version segment, so the `{base}/v1/...` rule does not fit.
        base_url: Some("https://models.github.ai/inference/chat/completions"),
        default_model: "openai/gpt-4o-mini",
        models: &["openai/gpt-4o-mini", "openai/gpt-4o"],
        note: None,
    },
    PlatformSpec {
        platform: "zhipu",
        label: "Zhipu (GLM)",
        kind: Some(ProviderKind::OpenAi),
        base_url: Some("https://open.bigmodel.cn/api/paas/v4"),
        default_model: "glm-4-plus",
        models: &["glm-4-plus", "glm-4-flash", "glm-4.5"],
        note: None,
    },
    PlatformSpec {
        platform: "ollama",
        label: "Ollama (local)",
        kind: Some(ProviderKind::Ollama),
        base_url: Some("http://127.0.0.1:11434"),
        default_model: "llama3.1",
        models: &["llama3.1", "qwen2.5-coder", "mistral"],
        note: None,
    },
    PlatformSpec {
        platform: "huggingface",
        label: "Hugging Face",
        kind: Some(ProviderKind::OpenAi),
        base_url: Some("https://router.huggingface.co/v1"),
        default_model: "meta-llama/Llama-3.3-70B-Instruct",
        models: &[
            "meta-llama/Llama-3.3-70B-Instruct",
            "Qwen/Qwen2.5-72B-Instruct",
        ],
        note: None,
    },
    PlatformSpec {
        platform: "pollinations",
        label: "Pollinations",
        kind: Some(ProviderKind::OpenAi),
        base_url: Some("https://gen.pollinations.ai/v1"),
        default_model: "openai",
        models: &["openai", "mistral"],
        note: None,
    },
    PlatformSpec {
        platform: "llm7",
        label: "LLM7",
        kind: Some(ProviderKind::OpenAi),
        base_url: Some("https://api.llm7.io/v1"),
        default_model: "gpt-4o-mini",
        models: &["gpt-4o-mini"],
        note: None,
    },
    PlatformSpec {
        platform: "cohere",
        label: "Cohere",
        kind: None,
        base_url: None,
        default_model: "",
        models: &[],
        note: Some("Cohere's chat API is not OpenAI-compatible; no adapter yet"),
    },
    PlatformSpec {
        platform: "cloudflare",
        label: "Cloudflare Workers AI",
        kind: Some(ProviderKind::OpenAi),
        base_url: None,
        default_model: "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
        models: &["@cf/meta/llama-3.3-70b-fast"],
        note: Some("needs a base URL with the account id on its key row"),
    },
    PlatformSpec {
        platform: "kilo",
        label: "Kilo",
        kind: Some(ProviderKind::OpenAi),
        base_url: None,
        default_model: "gpt-4o-mini",
        models: &["gpt-4o-mini"],
        note: Some("needs a base URL on its key row"),
    },
    PlatformSpec {
        platform: "opencode",
        label: "OpenCode",
        kind: Some(ProviderKind::OpenAi),
        base_url: None,
        default_model: "gpt-4o-mini",
        models: &["gpt-4o-mini"],
        note: Some("needs a base URL on its key row"),
    },
];

fn spec_for(platform: &str) -> Option<&'static PlatformSpec> {
    PLATFORMS
        .iter()
        .find(|spec| spec.platform.eq_ignore_ascii_case(platform))
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
    fn unavailable(&self, entry: &KeyEntry) -> Option<String> {
        let spec = spec_for(&entry.platform);
        if let Some(spec) = spec {
            if spec.kind.is_none() {
                return spec.note.map(str::to_string);
            }
        }
        let base_url = entry
            .base_url
            .clone()
            .or_else(|| spec.and_then(|spec| spec.base_url.map(str::to_string)));
        if base_url.is_none() {
            return Some(
                spec.and_then(|spec| spec.note)
                    .unwrap_or("set a base URL on its key row")
                    .to_string(),
            );
        }
        if !entry.enabled {
            return Some("disabled".to_string());
        }
        if self.vault.is_none() {
            return self.vault_error.clone().or_else(|| {
                Some("the server cannot open stored keys".to_string())
            });
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
                    .and_then(|spec| spec.kind)
                    .map(|kind| kind.as_str().to_string())
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
        let kind = spec
            .and_then(|spec| spec.kind)
            // Unknown platform with a base URL: treat it as a gateway.
            .unwrap_or(ProviderKind::OpenAi);
        let base_url = entry
            .base_url
            .clone()
            .or_else(|| spec.and_then(|spec| spec.base_url.map(str::to_string)))
            .ok_or_else(|| {
                ApiError::ServiceUnavailable(format!(
                    "the assistant cannot use {}: no base URL",
                    entry.platform
                ))
            })?;
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

        let vault = self
            .vault
            .as_ref()
            .ok_or_else(|| ApiError::ServiceUnavailable("the server cannot open stored keys".into()))?;
        let sealed = entry
            .sealed
            .as_ref()
            .ok_or_else(|| ApiError::ServiceUnavailable("the key row has no sealed key".into()))?;
        let api_key = vault.open(sealed).map_err(|error| {
            // The error is about the vault, never the key: safe to log.
            tracing::warn!(%error, platform = %entry.platform, "could not open a provider key");
            ApiError::ServiceUnavailable(format!(
                "the key for {} could not be decrypted; check IDE_SECRET_KEY / IDE_VAULT_KEY_FILE",
                entry.platform
            ))
        })?;

        let config = AgentConfig::for_provider(kind, model.clone(), base_url, Some(api_key));
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

    fn catalog(entries: Vec<KeyEntry>) -> KeyCatalog {
        KeyCatalog {
            entries,
            vault: CredentialVault::from_key(&[7u8; 32]).ok(),
            vault_error: None,
            built: Mutex::new(HashMap::new()),
        }
    }

    #[test]
    fn every_known_platform_has_a_default_model() {
        for spec in PLATFORMS {
            if spec.kind.is_some() && spec.base_url.is_some() {
                assert!(
                    !spec.default_model.is_empty(),
                    "{} has no default model",
                    spec.platform
                );
            }
        }
    }

    #[test]
    fn info_never_carries_a_key() {
        let info = catalog(vec![entry("groq", true)]).info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("ciphertext"));
        assert!(!json.contains("nonce"));
        assert_eq!(info[0].platform, "groq");
        assert_eq!(info[0].kind, "openai");
        assert!(info[0].unavailable.is_none());
    }

    #[test]
    fn a_disabled_row_is_listed_but_not_usable() {
        let catalog = catalog(vec![entry("groq", false)]);
        let info = catalog.info();
        assert_eq!(info[0].unavailable.as_deref(), Some("disabled"));
        assert!(catalog.first_usable().is_none());
        assert!(catalog.provider("groq", None).is_err());
    }

    #[test]
    fn a_platform_without_a_base_url_says_so() {
        let catalog = catalog(vec![entry("opencode", true)]);
        let info = catalog.info();
        assert!(info[0]
            .unavailable
            .as_deref()
            .unwrap_or_default()
            .contains("base URL"));
    }

    #[test]
    fn an_unknown_platform_is_a_gateway_when_it_has_a_base_url() {
        let mut row = entry("acme", true);
        row.base_url = Some("https://acme.test".into());
        let info = catalog(vec![row]).info();
        assert_eq!(info[0].kind, "openai");
        assert!(info[0].unavailable.is_none());
    }

    #[test]
    fn a_gateway_with_no_default_model_asks_for_one() {
        let vault = CredentialVault::from_key(&[7u8; 32]).unwrap();
        let mut row = entry("acme", true);
        row.base_url = Some("https://acme.test".into());
        row.sealed = Some(vault.seal("sk-acme").unwrap());
        let catalog = catalog(vec![row]);
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
        let catalog = catalog(vec![first, second]);

        assert_eq!(catalog.find("9").map(|e| e.id), Some(9));
        assert_eq!(catalog.find("#4").map(|e| e.id), Some(4));
        assert_eq!(catalog.find("Mistral").map(|e| e.id), Some(9));
        assert!(catalog.find("nope").is_none());
    }

    #[test]
    fn a_missing_vault_makes_every_row_unusable() {
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
        let catalog = catalog(vec![entry("groq", true)]);
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
        let catalog = catalog(vec![row]);

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
        let catalog = catalog(vec![row]);
        assert!(matches!(
            catalog.provider("groq", None),
            Err(ApiError::ServiceUnavailable(_))
        ));
    }
}
