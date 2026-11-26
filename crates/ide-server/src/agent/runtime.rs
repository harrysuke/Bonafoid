//! Resolved agent settings for this process, and the provider catalog.
//!
//! Two sources, in priority order:
//!
//! 1. the environment (`AGENT_PROVIDER` / `AGENT_API_KEY`), which is what a
//!    container deployment sets and what every test uses;
//! 2. `agent_api_keys`, which is what the drawer's picker lists.
//!
//! The environment stays first because it is the one setting an operator can
//! change without touching the database, and because a host with no database
//! (the desktop shell, a unit test) must still be able to run a provider.
//!
//! The runtime is also the seam that makes the endpoint testable. A test
//! injects a scripted [`LlmProvider`]; production reads the environment and the
//! key table. The handler cannot tell the difference, so the SSE path, the
//! audit row and the session lifecycle are all exercised by tests.

use std::sync::Arc;

use nocode_agent::{AgentConfig, ChatOptions, Limits, LlmProvider};
use sea_orm::DatabaseConnection;

use crate::error::ApiError;

use super::keys::{KeyCatalog, ProviderInfo, Resolved};

/// The provider chosen by the environment, if one is.
struct EnvProvider {
    provider: Arc<dyn LlmProvider>,
    /// The platform id (`groq`, `anthropic`, …).
    platform: String,
    /// The wire format, which several platforms share.
    kind: String,
    model: String,
}

/// Whether the assistant can run, and with which settings.
pub struct AgentRuntime {
    env: Option<EnvProvider>,
    /// Present when this host has a database to read keys from.
    db: Option<DatabaseConnection>,
    /// Loaded on first use: a server whose operator never opens the assistant
    /// should not pay for the query, and the table may not exist yet at boot.
    ///
    /// A `RwLock` rather than a `OnceCell` because the key-management routes
    /// write rows: adding a provider must not require a restart, so the slot
    /// has to be droppable. The read path is unchanged — one query per process
    /// until something is written.
    catalog: tokio::sync::RwLock<Option<Arc<KeyCatalog>>>,
    chat: ChatOptions,
    limits: Limits,
    /// Why the assistant is off, for the 503 body. `None` when it is on.
    disabled_reason: Option<String>,
}

impl AgentRuntime {
    /// Read the environment. A missing provider is a normal state: the IDE
    /// works, the drawer is hidden, and the endpoint answers 503 with the fix.
    pub fn from_env() -> Self {
        Self::build(None)
    }

    /// Read the environment, and fall back to `agent_api_keys` for providers.
    ///
    /// A deployment with no `AGENT_*` variables is not "disabled" any more: it
    /// is a server whose keys live in the database, which is the point of the
    /// picker. It is only disabled when neither source has anything.
    pub fn from_env_and_db(db: DatabaseConnection) -> Self {
        Self::build(Some(db))
    }

    fn build(db: Option<DatabaseConnection>) -> Self {
        // Read before `db` moves into the struct.
        let no_db = db.is_none();
        let config = match AgentConfig::from_env() {
            Ok(config) => config,
            Err(error) => {
                tracing::info!(reason = %error, "no AGENT_* provider; using stored keys if any");
                let defaults = AgentConfig::defaults();
                return Self {
                    env: None,
                    db,
                    catalog: tokio::sync::RwLock::new(None),
                    chat: ChatOptions {
                        max_tokens: defaults.max_tokens,
                        temperature: defaults.temperature,
                        stream_deltas: defaults.stream_deltas,
                    },
                    limits: defaults.limits,
                    disabled_reason: no_db.then(|| error.to_string()),
                };
            }
        };
        // `build_provider` can only fail on a programming error (an unknown
        // provider kind). Turning that into "disabled with a reason" keeps the
        // failure at startup rather than on the user's first request.
        let provider = match nocode_agent::build_provider(&config) {
            Ok(provider) => provider,
            Err(error) => {
                tracing::warn!(reason = %error, "provider could not be built");
                return Self {
                    env: None,
                    db,
                    catalog: tokio::sync::RwLock::new(None),
                    chat: ChatOptions {
                        max_tokens: config.max_tokens,
                        temperature: config.temperature,
                        stream_deltas: config.stream_deltas,
                    },
                    limits: config.limits,
                    disabled_reason: no_db.then(|| error.to_string()),
                };
            }
        };
        Self {
            env: Some(EnvProvider {
                provider,
                // The catalog platform, not the wire format: a deployment that
                // sets `AGENT_PROVIDER=groq` must report `groq`, not `openai`.
                platform: config.platform.clone(),
                kind: config.kind.as_str().to_string(),
                model: config.model.clone(),
            }),
            db,
            catalog: tokio::sync::RwLock::new(None),
            chat: ChatOptions {
                max_tokens: config.max_tokens,
                temperature: config.temperature,
                stream_deltas: config.stream_deltas,
            },
            limits: config.limits,
            disabled_reason: None,
        }
    }

    /// An always-on runtime around a caller-supplied provider (tests).
    pub fn with_provider(provider: Arc<dyn LlmProvider>, limits: Limits) -> Self {
        Self {
            env: Some(EnvProvider {
                platform: provider.name().to_string(),
                kind: provider.name().to_string(),
                model: provider.model().to_string(),
                provider,
            }),
            db: None,
            catalog: tokio::sync::RwLock::new(None),
            chat: ChatOptions::default(),
            limits,
            disabled_reason: None,
        }
    }

    /// An explicitly off runtime. Used by tests and by a host that wants to
    /// disable the assistant even though the environment is configured.
    pub fn disabled(reason: String) -> Self {
        Self {
            env: None,
            db: None,
            catalog: tokio::sync::RwLock::new(None),
            chat: ChatOptions::default(),
            limits: Limits::default(),
            disabled_reason: Some(reason),
        }
    }

    /// The key catalog, loaded once. `None` when this host has no database.
    async fn catalog(&self) -> Option<Arc<KeyCatalog>> {
        // Fast path: a clone of the `Arc`, so a picker open costs no lock
        // contention and no query.
        if let Some(catalog) = self.catalog.read().await.clone() {
            return Some(catalog);
        }
        let db = self.db.as_ref()?;

        // Slow path, under the write lock: a burst of picker opens must not
        // each run the same query.
        let mut slot = self.catalog.write().await;
        if let Some(catalog) = slot.clone() {
            return Some(catalog);
        }
        match KeyCatalog::load(db).await {
            Ok(catalog) => {
                let catalog = Arc::new(catalog);
                *slot = Some(catalog.clone());
                Some(catalog)
            }
            Err(error) => {
                tracing::warn!(?error, "could not load the provider key catalog");
                None
            }
        }
    }

    /// Forget the cached catalog so the next read sees a key that was just
    /// written.
    ///
    /// The reload is deferred to the next reader on purpose: a create/update/
    /// delete only has to make the *next* answer correct, and doing the query
    /// here would make the write wait for a read nobody asked for.
    pub async fn invalidate_catalog(&self) {
        *self.catalog.write().await = None;
    }

    /// What the drawer's picker shows, plus which entry a bare run uses.
    pub async fn catalog_view(&self) -> CatalogView {
        let mut providers = Vec::new();
        if let Some(env) = &self.env {
            providers.push(ProviderInfo {
                // A reserved token: no row can be `env`.
                id: "env".to_string(),
                platform: env.platform.clone(),
                label: "Server default".to_string(),
                kind: env.kind.clone(),
                model: env.model.clone(),
                models: vec![env.model.clone()],
                status: "healthy".to_string(),
                enabled: true,
                base_url: None,
                // A key configured through the environment is by definition
                // already satisfied.
                keyless: false,
                unavailable: None,
            });
        }

        let keys = self.catalog().await;
        if let Some(catalog) = &keys {
            providers.extend(catalog.info());
        }

        let default = if self.env.is_some() {
            Some("env".to_string())
        } else {
            keys.as_ref().and_then(|catalog| catalog.default_selection())
        };

        let usable = providers.iter().any(|p| p.unavailable.is_none());
        let reason = if usable {
            None
        } else if let Some(catalog) = &keys {
            if catalog.is_empty() {
                Some(
                    "no provider keys are configured: add a row to agent_api_keys, or set \
                     AGENT_PROVIDER and AGENT_API_KEY"
                        .to_string(),
                )
            } else {
                // Every row exists but none can run: the first reason is the
                // most useful one, because it is usually shared (no vault key).
                providers
                    .iter()
                    .find_map(|p| p.unavailable.clone())
                    .or_else(|| Some("no provider can run".to_string()))
            }
        } else {
            self.disabled_reason.clone().or_else(|| {
                Some("this host has no provider configured".to_string())
            })
        };

        CatalogView {
            providers,
            default,
            reason,
        }
    }

    /// The provider for one run.
    ///
    /// `selection` is a catalog id (`"6"`, `"env"`) or a platform name. `None`
    /// means "whatever the operator made the default", which is the environment
    /// when it is configured and the first usable key otherwise.
    pub async fn provider_for(
        &self,
        selection: Option<&str>,
        model: Option<&str>,
    ) -> Result<Resolved, ApiError> {
        let selection = selection.map(str::trim).filter(|s| !s.is_empty());

        // A named provider that is not the environment's.
        if let Some(selection) = selection {
            if selection != "env" {
                let catalog = self.catalog().await.ok_or_else(|| {
                    ApiError::ServiceUnavailable(
                        "this host has no provider catalog; set AGENT_PROVIDER and AGENT_API_KEY"
                            .into(),
                    )
                })?;
                return catalog.provider(selection, model);
            }
        }

        if let Some(env) = &self.env {
            // The model is fixed by the environment: a per-run override would
            // need a second provider built from a key the runtime never sees.
            return Ok(Resolved {
                provider: env.provider.clone(),
                platform: env.platform.clone(),
                label: "Server default".to_string(),
                model: env.model.clone(),
            });
        }

        let catalog = self.catalog().await.ok_or_else(|| {
            ApiError::ServiceUnavailable(format!(
                "the assistant is not configured: {}",
                self.disabled_reason
                    .as_deref()
                    .unwrap_or("set AGENT_PROVIDER and AGENT_API_KEY")
            ))
        })?;
        let selection = catalog.default_selection().ok_or_else(|| {
            ApiError::ServiceUnavailable(
                "the assistant is not configured: no enabled provider key in agent_api_keys"
                    .to_string(),
            )
        })?;
        catalog.provider(&selection, model)
    }

    pub fn chat(&self) -> ChatOptions {
        self.chat
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }
}

/// The picker's payload.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CatalogView {
    pub providers: Vec<ProviderInfo>,
    /// Selection a request that names none will use.
    pub default: Option<String>,
    /// Why nothing can run, when nothing can. The drawer's empty state.
    pub reason: Option<String>,
}
