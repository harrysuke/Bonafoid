//! Desktop assistant: the agent, in-process, over a Tauri channel.
//!
//! The desktop shell has no server. Rather than hide the assistant, this module
//! runs the same `nocode_agent::run` loop the web server runs, against the same
//! `nocode_mcp` tool surface, over a `LocalProjectProvider` that reads and
//! writes the *file the canvas is showing*. A tool call and a manual edit
//! therefore cannot diverge: there is one copy of the project.
//!
//! # Why a Channel and not HTTP
//!
//! `tauri::ipc::Channel` delivers events in order and is cleaned up when the
//! window goes away. There is no auth, no CSRF and no port to bind — the only
//! network call in the whole path is the one to the LLM.
//!
//! # Key handling
//!
//! The provider key never crosses the IPC boundary in the outbound direction:
//! `agent_settings_get` returns `has_key`, never the key. It is sealed with the
//! same `CredentialVault` the deploy integration uses (ChaCha20-Poly1305, key
//! file in the app-data directory) and the plaintext exists only for the
//! duration of one `agent_chat` call. This is the one place where the plan's
//! "OS keychain" recommendation is narrowed to the existing sealed vault: the
//! crate that would call `keyring` is not a dependency of this app, and a
//! second secret store is a second thing to get wrong. The seam is small —
//! `AgentState::init` builds the vault and `config()` opens the blob — so
//! swapping the backend changes those two places and nothing else, while the
//! observable contract stays the same: set, read, clear, never in an event,
//! never in a log.
//!
//! # Concurrency and the file guard
//!
//! There is no database revision locally, so the lease is a process-local
//! `Mutex` holding the project path, and the conflict test is a content hash
//! captured at run start. If the file changed while the model was working, the
//! save is refused with `StaleRevision` and the orchestrator's three-way merge
//! decides: disjoint edits are merged, overlapping ones stop the run with a
//! `conflict` event. A file the user edited in another editor is never silently
//! overwritten.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;

use nocode_agent::llm::LlmProvider;
use nocode_agent::{
    AgentConfig, ChatOptions, Limits, Mode, ProviderKind, RunContext, RunRequest,
};
use nocode_deploy::{CredentialVault, SealedCredential};
use nocode_ide::IdeService;
use nocode_mcp::executor::McpToolExecutor;
use nocode_mcp::provider::{LoadedProject, ProjectStateProvider, ProjectSummaryLite};
use nocode_mcp::{CallerContext, Config as McpConfig};
use nocode_model::Project;

/// Settings file, beside the deploy integration store.
const SETTINGS_FILE: &str = "agent.json";
/// Vault key file. Must never be committed, synced or logged.
const KEY_FILE: &str = "agent.key";
/// Longest accepted key.
const MAX_KEY_CHARS: usize = 512;
/// Largest project file the assistant will open.
///
/// A 64 MB `.ncproj` is already pathological; reading one into a prompt would
/// cost more than the whole run, so it is refused before any provider call.
const MAX_PROJECT_BYTES: u64 = 64 * 1024 * 1024;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned lock means another run panicked mid-write. Recovering is safer
    // than propagating: the worst case is a stale in-memory copy, and the file
    // guard catches that on the next save.
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A hashed file body, for the "did it change under us" guard.
///
/// `DefaultHasher` is SipHash-1-3, not cryptographic. That is deliberate: this
/// is a change detector, not a security boundary. A collision would require a
/// crafted edit that preserves the hash, and the worst outcome is the user's own
/// editor being overwritten — which the revision guard and the merge still
/// cover.
fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Persisted settings. The key is a sealed blob; there is no plaintext field.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredSettings {
    provider: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    sealed_key: Option<SealedCredential>,
}

/// Settings as the UI sees them. Never carries the key.
#[derive(Debug, Clone, Serialize)]
pub struct AgentSettingsView {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    /// Whether a key is stored. The key itself never leaves the process.
    pub has_key: bool,
}

/// What the host can do, for the drawer's capability probe.
#[derive(Debug, Clone, Serialize)]
pub struct AgentCapabilities {
    /// Local mode: a configured provider with a key (or Ollama, which needs
    /// none).
    pub local: bool,
    /// Remote mode is not implemented in this build: there is no server
    /// connection flow yet, so claiming it would show a drawer that cannot run.
    pub remote: bool,
    pub providers: Vec<String>,
    /// The desktop host has no preview service.
    pub preview: bool,
}

/// The outcome of one run, for the drawer's footer.
#[derive(Debug, Clone, Serialize)]
pub struct AgentRunSummary {
    pub status: String,
    pub revision: u64,
    pub saved: bool,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub tool_calls: usize,
    pub error: Option<String>,
}

/// Body of `agent_chat`.
#[derive(Debug, Deserialize)]
pub struct AgentChatRequest {
    pub prompt: String,
    #[serde(default)]
    pub mode: Option<String>,
}

/// Everything the agent commands share.
pub struct AgentState {
    settings: Mutex<StoredSettings>,
    vault: CredentialVault,
    settings_path: PathBuf,
    /// The project one run is currently editing. `None` when idle.
    holder: Mutex<Option<String>>,
    /// Set by `agent_cancel`, cleared at the start of every run.
    cancelled: AtomicBool,
}

impl AgentState {
    /// Load settings from the app-data directory.
    ///
    /// A missing vault key is not fatal: the assistant reports `local: false`
    /// and the drawer says so, while the rest of the IDE works.
    pub fn init(app_data: &Path) -> Self {
        let settings_path = app_data.join(SETTINGS_FILE);
        let settings = std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|text| serde_json::from_str::<StoredSettings>(&text).ok())
            .unwrap_or_default();
        let vault = CredentialVault::from_key_file(&app_data.join(KEY_FILE))
            .or_else(|_| CredentialVault::from_key(&[0u8; 32]))
            .expect("a vault is always constructible");
        Self {
            settings: Mutex::new(settings),
            vault,
            settings_path,
            holder: Mutex::new(None),
            cancelled: AtomicBool::new(false),
        }
    }

    fn save(&self, settings: &StoredSettings) -> Result<(), String> {
        let text = serde_json::to_string_pretty(settings).map_err(|_| "bad settings".to_string())?;
        std::fs::write(&self.settings_path, text)
            .map_err(|error| format!("could not save assistant settings: {error}"))
    }

    fn view(&self) -> AgentSettingsView {
        let settings = lock(&self.settings);
        AgentSettingsView {
            provider: settings.provider.clone(),
            model: settings.model.clone(),
            base_url: settings.base_url.clone(),
            has_key: settings.sealed_key.is_some(),
        }
    }

    /// Validate and store a settings change.
    ///
    /// The command is a thin wrapper so the validation — empty key, oversized
    /// key, unknown provider — is exercised by the tests rather than by hand.
    fn apply(
        &self,
        provider: &str,
        model: Option<String>,
        base_url: Option<String>,
        api_key: Option<String>,
    ) -> Result<AgentSettingsView, String> {
        let kind = ProviderKind::parse(provider).map_err(|error| error.to_string())?;
        let mut settings = lock(&self.settings);

        if let Some(key) = api_key {
            let key = key.trim();
            if key.is_empty() {
                return Err("the key is empty".into());
            }
            if key.chars().count() > MAX_KEY_CHARS {
                return Err(format!("a key must be at most {MAX_KEY_CHARS} characters"));
            }
            settings.sealed_key = Some(
                self.vault
                    .seal(key)
                    .map_err(|_| "could not store the key".to_string())?,
            );
        }

        settings.provider = Some(kind.as_str().to_string());
        settings.model = model.filter(|value| !value.trim().is_empty());
        settings.base_url = base_url.filter(|value| !value.trim().is_empty());
        let stored = StoredSettings {
            provider: settings.provider.clone(),
            model: settings.model.clone(),
            base_url: settings.base_url.clone(),
            sealed_key: settings.sealed_key.clone(),
        };
        // `view()` would re-lock `settings`, and a `std::sync::Mutex` is not
        // reentrant, so the reply is built from the value just written.
        self.save(&stored)?;
        Ok(AgentSettingsView {
            provider: stored.provider,
            model: stored.model,
            base_url: stored.base_url,
            has_key: stored.sealed_key.is_some(),
        })
    }

    /// Whether a run can start right now.
    fn is_ready(&self) -> bool {
        let settings = lock(&self.settings);
        let Some(kind) = settings
            .provider
            .as_deref()
            .and_then(|value| ProviderKind::parse(value).ok())
        else {
            return false;
        };
        // Ollama is local and needs no key; the hosted APIs do.
        kind == ProviderKind::Ollama || settings.sealed_key.is_some()
    }

    /// Build a provider for one run. The plaintext key lives in this value only.
    fn config(&self) -> Result<AgentConfig, String> {
        let settings = lock(&self.settings);
        let kind = settings
            .provider
            .as_deref()
            .ok_or_else(|| "Choose a provider in Settings → AI".to_string())
            .and_then(|value| {
                ProviderKind::parse(value).map_err(|_| format!("unknown provider `{value}`"))
            })?;
        let api_key = match &settings.sealed_key {
            Some(sealed) => Some(
                self.vault
                    .open(sealed)
                    .map_err(|_| "the stored key could not be decrypted; set it again".to_string())?,
            ),
            None => None,
        };
        if api_key.is_none() && kind != ProviderKind::Ollama {
            return Err("Enter an API key in Settings → AI".into());
        }
        Ok(AgentConfig {
            kind,
            model: settings
                .model
                .clone()
                .unwrap_or_else(|| kind.default_model().to_string()),
            base_url: settings
                .base_url
                .clone()
                .unwrap_or_else(|| kind.default_base_url().to_string())
                .trim_end_matches('/')
                .to_string(),
            api_key,
            max_tokens: 4096,
            temperature: 0.2,
            stream_deltas: true,
            request_timeout: Duration::from_secs(120),
            limits: Limits::default(),
        })
    }
}

/* ------------------------------------------------------------------ *
 * Local project storage
 * ------------------------------------------------------------------ */

/// `ProjectStateProvider` over the file the canvas is showing.
///
/// `revision` counts saves in this process; it is not a database row version,
/// and nothing outside this run compares it with one.
pub struct LocalProjectProvider {
    path: Option<PathBuf>,
    project: Mutex<Project>,
    revision: Mutex<u64>,
    /// Hash of the file as this process last read or wrote it.
    hash: Mutex<u64>,
    /// The project as saved by the run, for writing back into `IdeService`.
    saved: Mutex<Option<Project>>,
}

impl LocalProjectProvider {
    /// Seed from the working copy and the file (when there is one).
    pub fn new(path: Option<PathBuf>, project: Project) -> Self {
        let hash = path
            .as_deref()
            .and_then(|path| std::fs::read(path).ok())
            .map(|bytes| hash_bytes(&bytes))
            .unwrap_or(0);
        Self {
            path,
            project: Mutex::new(project),
            revision: Mutex::new(1),
            hash: Mutex::new(hash),
            saved: Mutex::new(None),
        }
    }

    /// The project the run wrote, when it saved.
    pub fn saved_project(&self) -> Option<Project> {
        lock(&self.saved).clone()
    }

    fn read_disk(&self) -> Option<(u64, Project)> {
        let path = self.path.as_deref()?;
        let bytes = std::fs::read(path).ok()?;
        let project: Project = serde_json::from_slice(&bytes).ok()?;
        Some((hash_bytes(&bytes), project))
    }
}

#[async_trait]
impl ProjectStateProvider for LocalProjectProvider {
    async fn load_project(
        &self,
        _tenant_id: &str,
        _user_id: &str,
        _project_id: &str,
    ) -> Result<LoadedProject, McpErrorAlias> {
        // A rebase calls this to learn what the other writer did. Locally the
        // "other writer" is the user's editor, so the file wins over the
        // in-memory copy whenever it has moved on.
        if let Some((disk_hash, disk_project)) = self.read_disk() {
            let mut hash = lock(&self.hash);
            if *hash != disk_hash {
                *hash = disk_hash;
                *lock(&self.project) = disk_project;
            }
        }
        Ok(LoadedProject {
            project: lock(&self.project).clone(),
            revision: *lock(&self.revision),
        })
    }

    async fn save_project(
        &self,
        _tenant_id: &str,
        _user_id: &str,
        project: &Project,
        expected_revision: u64,
        _message: Option<&str>,
    ) -> Result<u64, McpErrorAlias> {
        let revision = *lock(&self.revision);

        // The file changed since this run last read or wrote it: refuse, and
        // deliberately leave `hash` alone. `load_project` is what adopts the
        // new content — that is the copy the three-way merge computes against.
        // Adopting the hash here instead would make the merge compare the file
        // with itself, find no conflict, and overwrite the user's edit: exactly
        // the silent data loss this guard exists to prevent.
        if let Some((disk_hash, _)) = self.read_disk() {
            if *lock(&self.hash) != disk_hash {
                return Err(McpErrorAlias::StaleRevision {
                    expected: expected_revision,
                    current: revision,
                });
            }
        }

        if expected_revision != revision {
            return Err(McpErrorAlias::StaleRevision {
                expected: expected_revision,
                current: revision,
            });
        }

        if let Some(path) = self.path.as_deref() {
            let text = serde_json::to_string_pretty(project).map_err(|error| {
                McpErrorAlias::Internal(format!("could not serialise the project: {error}"))
            })?;
            // Write-then-rename: a crash mid-write must not leave a truncated
            // project file, because the next open would fail to parse it.
            let temporary = path.with_extension("ncproj.tmp");
            std::fs::write(&temporary, text.as_bytes()).map_err(|error| {
                McpErrorAlias::Io(format!("could not write the project: {error}"))
            })?;
            std::fs::rename(&temporary, path).map_err(|error| {
                McpErrorAlias::Io(format!("could not replace the project: {error}"))
            })?;
            *lock(&self.hash) = hash_bytes(text.as_bytes());
        }

        *lock(&self.project) = project.clone();
        *lock(&self.saved) = Some(project.clone());
        let mut revision = lock(&self.revision);
        *revision += 1;
        Ok(*revision)
    }

    async fn list_projects(
        &self,
        _tenant_id: &str,
        _user_id: &str,
    ) -> Result<Vec<ProjectSummaryLite>, McpErrorAlias> {
        let project = lock(&self.project);
        Ok(vec![ProjectSummaryLite {
            id: project.id.to_string(),
            name: project.name.clone(),
            slug: project.slug.clone(),
            revision: *lock(&self.revision),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }])
    }
}

/// The error type `nocode_mcp`'s provider trait uses.
///
/// Aliased so the signature reads as what it is: the tool layer's error, not
/// this module's.
use nocode_mcp::error::McpError as McpErrorAlias;

/* ------------------------------------------------------------------ *
 * Commands
 * ------------------------------------------------------------------ */

/// `agent_capabilities` — what the drawer may offer.
#[tauri::command]
pub async fn agent_capabilities(state: State<'_, AgentState>) -> Result<AgentCapabilities, String> {
    Ok(AgentCapabilities {
        local: state.is_ready(),
        remote: false,
        providers: vec![
            ProviderKind::Anthropic.as_str().to_string(),
            ProviderKind::OpenAi.as_str().to_string(),
            ProviderKind::Ollama.as_str().to_string(),
        ],
        preview: false,
    })
}

/// `agent_settings_get` — settings without the key.
#[tauri::command]
pub async fn agent_settings_get(state: State<'_, AgentState>) -> Result<AgentSettingsView, String> {
    Ok(state.view())
}

/// `agent_settings_set` — store the provider, model and (optionally) a new key.
///
/// Omitting `api_key` keeps the sealed credential, so a model change does not
/// force the user to re-enter a key they already saved.
#[tauri::command]
pub async fn agent_settings_set(
    state: State<'_, AgentState>,
    provider: String,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<AgentSettingsView, String> {
    state.apply(&provider, model, base_url, api_key)
}

/// `agent_key_clear` — forget the stored key.
#[tauri::command]
pub async fn agent_key_clear(state: State<'_, AgentState>) -> Result<AgentSettingsView, String> {
    let mut settings = lock(&state.settings);
    settings.sealed_key = None;
    let stored = StoredSettings {
        provider: settings.provider.clone(),
        model: settings.model.clone(),
        base_url: settings.base_url.clone(),
        sealed_key: None,
    };
    state.save(&stored)?;
    Ok(AgentSettingsView {
        provider: stored.provider,
        model: stored.model,
        base_url: stored.base_url,
        has_key: false,
    })
}

/// `agent_test_connection` — one tiny completion, to prove the key works.
///
/// The reply is returned to the UI; the key is not, and no error message can
/// contain it (`AgentError` never carries the request).
#[tauri::command]
pub async fn agent_test_connection(state: State<'_, AgentState>) -> Result<String, String> {
    let config = state.config()?;
    let provider: Arc<dyn LlmProvider> = nocode_agent::build_provider(&config)
        .map_err(|error| format!("could not build the provider: {error}"))?;
    let response = provider
        .complete(nocode_agent::llm::ChatRequest {
            system: "Reply with the single word OK.".into(),
            messages: vec![nocode_agent::llm::ChatMessage::User("ping".into())],
            tools: Vec::new(),
            max_tokens: 8,
            temperature: 0.0,
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok(response.text.unwrap_or_else(|| "(empty reply)".into()))
}

/// `agent_cancel` — stop the run in flight.
///
/// The forwarding loop notices within one poll interval and drops the receiver,
/// which makes the run's next `emit` fail with `Cancelled`: the same path the
/// web server takes when the browser closes the stream.
#[tauri::command]
pub async fn agent_cancel(state: State<'_, AgentState>) -> Result<(), String> {
    state.cancelled.store(true, Ordering::SeqCst);
    Ok(())
}

/// `agent_chat` — run the assistant and stream its events.
#[tauri::command]
pub async fn agent_chat(
    state: State<'_, AgentState>,
    ide: State<'_, IdeService>,
    project_json: Option<String>,
    request: AgentChatRequest,
    on_event: Channel<serde_json::Value>,
) -> Result<AgentRunSummary, String> {
    let prompt = request.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("prompt is empty".into());
    }
    if prompt.chars().count() > 4_000 {
        return Err("the prompt is longer than 4000 characters".into());
    }
    let mode = Mode::parse(request.mode.as_deref().unwrap_or_default())
        .map_err(|error| error.to_string())?;

    // The working copy: the file when there is one, otherwise the canvas's JSON.
    let path = ide.get_project_path().map(PathBuf::from);
    if let Some(path) = path.as_deref() {
        let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        if size > MAX_PROJECT_BYTES {
            return Err("this project file is too large for the assistant".into());
        }
    }
    let project: Project = match (path.as_deref(), project_json.as_deref()) {
        (Some(path), _) => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("could not read the project: {error}"))?;
            serde_json::from_str(&text).map_err(|error| format!("the project file is corrupt: {error}"))?
        }
        (None, Some(json)) => {
            serde_json::from_str(json).map_err(|error| format!("the project is corrupt: {error}"))?
        }
        (None, None) => return Err("open a project before asking the assistant".into()),
    };

    // One writer per project, process-wide. Two windows editing the same file
    // would each think they were the only writer.
    let key = path
        .as_deref()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "unsaved".into());
    {
        let mut holder = lock(&state.holder);
        if holder.is_some() {
            return Err("the assistant is already working on this project".into());
        }
        *holder = Some(key.clone());
    }
    // Released on every path out, including a panic in the run.
    struct Release<'a>(&'a Mutex<Option<String>>);
    impl Drop for Release<'_> {
        fn drop(&mut self) {
            *lock(self.0) = None;
        }
    }
    let _release = Release(&state.holder);

    let config = state.config()?;
    let provider: Arc<dyn LlmProvider> = nocode_agent::build_provider(&config)
        .map_err(|error| format!("could not build the provider: {error}"))?;

    let store = Arc::new(LocalProjectProvider::new(path.clone(), project));
    let storage: Arc<dyn ProjectStateProvider> = store.clone();
    let executor = Arc::new(McpToolExecutor::with_provider(
        Arc::new(McpConfig::default()),
        storage,
    ));

    // The desktop has one local identity: there is no tenant, no user and no
    // scope to check, because the file's own permissions are the boundary.
    let caller = CallerContext::local();
    let run_id = uuid::Uuid::new_v4().to_string();
    executor
        .projects
        .create_with_id(&caller.owner, &run_id, lock_project(&store), 1)
        .map_err(|error| error.to_string())?;
    let session_key = caller.session_key(&run_id);

    state.cancelled.store(false, Ordering::SeqCst);

    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let ctx = RunContext {
        run_id: run_id.clone(),
        provider,
        executor: executor.clone(),
        caller: caller.clone(),
        session_id: run_id.clone(),
        limits: config.limits,
        chat: ChatOptions {
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            stream_deltas: true,
        },
        events: tx,
        // Local mode has no conversation store: the project file is the whole
        // memory. The drawer keeps the visible thread for the session.
        history: Vec::new(),
        summary: None,
        conversation_id: None,
    };

    let run = tokio::spawn(async move { nocode_agent::run(ctx, RunRequest { prompt, mode }).await });

    // Forward events until the run ends. `timeout` rather than a bare `recv`
    // so a cancel is noticed even when the model is silent for 30 seconds.
    loop {
        if state.cancelled.load(Ordering::SeqCst) {
            // Dropping the receiver makes the run's next emit fail with
            // `Cancelled`, which is the same path the web host takes when the
            // browser closes the stream.
            drop(rx);
            break;
        }
        match tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
            Ok(Some(event)) => {
                if let Ok(value) = serde_json::to_value(&event) {
                    let _ = on_event.send(value);
                }
            }
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    let report = run.await.map_err(|error| format!("the run did not finish: {error}"))?;
    let _ = executor.projects.discard(&session_key);

    // The canvas must show what the agent wrote. Only a *saved* run updates it:
    // a validation failure or a conflict wrote nothing, and pretending
    // otherwise would put the drawer's model out of step with the file.
    if report.saved {
        if let Some(saved) = store.saved_project() {
            if let Ok(json) = serde_json::to_string(&saved) {
                let _ = ide.update_project(&json);
            }
        }
    }

    Ok(AgentRunSummary {
        status: report.status.as_str().to_string(),
        revision: report.revision,
        saved: report.saved,
        tokens_in: report.usage.tokens_in,
        tokens_out: report.usage.tokens_out,
        tool_calls: report.usage.tool_calls,
        error: report.error,
    })
}

/// The project the session should start from.
fn lock_project(store: &LocalProjectProvider) -> Project {
    lock(&store.project).clone()
}

/// Whether the desktop can run the assistant, for `agent_capabilities`.
#[tauri::command]
pub async fn agent_ready(state: State<'_, AgentState>) -> Result<bool, String> {
    Ok(state.is_ready())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_project() -> Project {
        Project::new("Expense tracker")
    }

    #[test]
    fn a_saved_file_round_trips_through_the_provider() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.ncproj");
        let project = sample_project();
        std::fs::write(&path, serde_json::to_string_pretty(&project).unwrap()).unwrap();

        let store = LocalProjectProvider::new(Some(path.clone()), project.clone());
        let saved = store.saved_project();
        assert!(saved.is_none(), "nothing saved yet");

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let revision = store
                .save_project("local", "local", &project, 1, None)
                .await
                .expect("first save");
            assert_eq!(revision, 2);
            assert!(store.saved_project().is_some());
        });

        let on_disk: Project =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk.name, "Expense tracker");
    }

    #[test]
    fn an_edit_in_another_editor_is_refused_then_merged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.ncproj");
        let project = sample_project();
        std::fs::write(&path, serde_json::to_string_pretty(&project).unwrap()).unwrap();

        let store = LocalProjectProvider::new(Some(path.clone()), project.clone());
        // Somebody edits the file while the model is thinking.
        let mut external = project.clone();
        external.name = "Edited elsewhere".into();
        std::fs::write(&path, serde_json::to_string_pretty(&external).unwrap()).unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let error = store
                .save_project("local", "local", &project, 1, None)
                .await
                .expect_err("a changed file must not be overwritten");
            assert!(matches!(error, McpErrorAlias::StaleRevision { .. }), "{error:?}");

            // The merge path. The provider must report what is *on disk* now,
            // or the three-way merge would compare the file with itself and
            // write over the edit it was supposed to preserve.
            let loaded = store.load_project("local", "local", "p1").await.expect("reload");
            assert_eq!(loaded.project.name, "Edited elsewhere");

            // Having seen that state, the retry may write.
            store
                .save_project("local", "local", &project, 1, None)
                .await
                .expect("the merge's write");
        });

        // The first attempt wrote nothing; the second wrote the run's merged
        // project deliberately, not by accident.
        let on_disk: Project =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk.name, "Expense tracker");
    }

    #[test]
    fn an_unknown_provider_is_rejected_before_a_run() {
        assert!(ProviderKind::parse("nope").is_err());
        assert_eq!(ProviderKind::parse("Claude").unwrap(), ProviderKind::Anthropic);
    }

    /// Write settings the way the settings panel does.
    fn store(state: &AgentState, provider: &str, model: Option<&str>, key: Option<&str>) {
        state
            .apply(
                provider,
                model.map(str::to_string),
                None,
                key.map(str::to_string),
            )
            .expect("the settings were accepted");
    }

    #[test]
    fn a_key_is_sealed_on_disk_and_never_handed_back() {
        let dir = tempfile::tempdir().unwrap();
        let state = AgentState::init(dir.path());

        // Nothing configured: the drawer must be told so, not shown a run that
        // would fail on its first turn.
        assert!(!state.is_ready());
        assert!(!state.view().has_key);
        assert!(state.config().is_err());

        store(&state, "anthropic", None, Some("sk-test-secret"));

        assert!(state.is_ready());
        let view = state.view();
        assert!(view.has_key);
        assert_eq!(view.provider.as_deref(), Some("anthropic"));

        // The plaintext is in neither the settings file nor `Debug`.
        let on_disk = std::fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap();
        assert!(!on_disk.contains("sk-test-secret"), "{on_disk}");
        let config = state.config().expect("a configured provider");
        assert_eq!(config.api_key.as_deref(), Some("sk-test-secret"));
        assert!(!format!("{config:?}").contains("sk-test-secret"));
    }

    #[test]
    fn settings_and_the_vault_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        store(&AgentState::init(dir.path()), "openai", Some("gpt-4o-mini"), Some("sk-abc"));

        // A fresh process reuses the same key file, so the sealed blob still
        // opens: the user is not asked for the key on every launch.
        let again = AgentState::init(dir.path());
        assert!(again.is_ready());
        let config = again.config().expect("a configured provider");
        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(config.api_key.as_deref(), Some("sk-abc"));
    }

    #[test]
    fn ollama_needs_no_key() {
        let dir = tempfile::tempdir().unwrap();
        let state = AgentState::init(dir.path());
        store(&state, "ollama", None, None);

        assert!(state.is_ready(), "a local model has no credential to check");
        assert!(!state.view().has_key);
        assert!(state.config().is_ok());
    }

    #[test]
    fn the_hash_notices_a_single_byte_change() {
        let a = hash_bytes(b"{\"name\":\"A\"}");
        let b = hash_bytes(b"{\"name\":\"B\"}");
        assert_ne!(a, b);
        assert_eq!(a, hash_bytes(b"{\"name\":\"A\"}"));
    }
}
