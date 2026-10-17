//! Build records and the in-memory registry that tracks preview builds.
//!
//! A build is: materialize the generated project on disk, compile the Rust
//! backend, then compile the Leptos frontend with Trunk. The work happens in a
//! background task so the HTTP request that starts it returns immediately; the
//! IDE polls [`BuildRecord::state`] and streams [`BuildRecord::logs`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

/// Maximum number of log lines retained per build (oldest are dropped).
const MAX_LOG_LINES: usize = 2000;

/// Lifecycle of one preview build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildState {
    /// Accepted, waiting for a build slot.
    Queued,
    /// Writing the generated project to the workspace.
    Generating,
    /// `cargo build` for the backend.
    BuildingBackend,
    /// `trunk build` for the frontend.
    BuildingFrontend,
    /// Both toolchains succeeded; `dist/` is servable.
    Ready,
    /// Terminated by the client before completion.
    Cancelled,
    /// Something went wrong; see `error` and the logs.
    Failed,
}

impl BuildState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Ready | Self::Failed | Self::Cancelled)
    }
}

/// Which artifacts a build should produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildTarget {
    /// Backend + frontend (the default; what the preview needs).
    All,
    /// Backend only.
    Backend,
    /// Frontend only.
    Frontend,
}

impl BuildTarget {
    pub fn wants_backend(self) -> bool {
        matches!(self, Self::All | Self::Backend)
    }

    pub fn wants_frontend(self) -> bool {
        matches!(self, Self::All | Self::Frontend)
    }
}

/// One preview build as observed by the IDE.
#[derive(Debug, Clone, Serialize)]
pub struct BuildRecord {
    pub id: String,
    pub project_id: String,
    pub user_id: String,
    pub target: BuildTarget,
    pub state: BuildState,
    /// Human-readable failure reason when `state` is `Failed`.
    pub error: Option<String>,
    /// Absolute workspace directory holding the generated project.
    pub workspace: PathBuf,
    /// Directory holding the compiled frontend (`frontend/dist`).
    pub dist_dir: PathBuf,
    /// Directory the generated backend binary was built into (shared cargo
    /// target dir, not the workspace, so rebuilds stay incremental).
    pub backend_bin_dir: PathBuf,
    /// Absolute path of the produced backend binary; set once the backend
    /// build succeeds, because the crate name follows the project slug.
    pub backend_binary: Option<PathBuf>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub logs: Vec<String>,
}

impl BuildRecord {
    fn new(
        id: String,
        project_id: String,
        user_id: String,
        target: BuildTarget,
        root: &std::path::Path,
    ) -> Self {
        let now = Utc::now();
        let workspace = root.join(&id);
        let backend_bin_dir = root
            .parent()
            .map(|parent| parent.join(".cargo-target").join("debug"))
            .unwrap_or_else(|| workspace.join("backend").join("target").join("debug"));
        Self {
            id,
            project_id,
            user_id,
            target,
            state: BuildState::Queued,
            error: None,
            dist_dir: workspace.join("frontend").join("dist"),
            backend_bin_dir,
            backend_binary: None,
            workspace,
            created_at: now,
            updated_at: now,
            logs: Vec::new(),
        }
    }

    /// Append log lines, keeping the most recent [`MAX_LOG_LINES`].
    fn push_logs(&mut self, lines: Vec<String>) {
        for line in lines {
            self.logs.push(line);
        }
        while self.logs.len() > MAX_LOG_LINES {
            self.logs.remove(0);
        }
        self.updated_at = Utc::now();
    }
}

/// Toolchain availability on the host, surfaced to the IDE so the preview
/// panel can explain *why* a build cannot run instead of failing opaquely.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewCapabilities {
    pub cargo: bool,
    pub trunk: bool,
    pub root: String,
    pub max_parallel: usize,
    pub timeout_secs: u64,
}

/// Configuration for the preview subsystem (env-driven).
#[derive(Debug, Clone)]
pub struct PreviewConfig {
    /// Parent directory for per-build workspaces.
    pub root: PathBuf,
    /// `API_BASE` baked into the generated frontend bundle.
    pub api_base: String,
    /// Hard limit for a single build.
    pub timeout: Duration,
    /// Concurrent builds allowed across all users.
    pub max_parallel: usize,
    /// Shared cargo target directory; keeps rebuilds fast.
    pub cargo_target_dir: PathBuf,
    /// Pass 	runk build --offline (hosts without internet).
    pub trunk_offline: bool,
}

impl PreviewConfig {
    /// Make `root` absolute. Build commands set a `current_dir` to a workspace
    /// subdirectory, so relative roots would break every manifest path handed
    /// to cargo/trunk.
    ///
    /// Windows `canonicalize` returns verbatim (`\\?\D:\...`) paths, which are
    /// awkward in logs and break naive string handling in the IDE, so an
    /// already-absolute root is only prefixed with the CWD when relative.
    pub fn absolutize(mut self) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        if !self.root.is_absolute() {
            self.root = cwd.join(&self.root);
        }
        if !self.cargo_target_dir.is_absolute() {
            self.cargo_target_dir = cwd.join(&self.cargo_target_dir);
        }
        self
    }

    pub fn from_env() -> Self {
        let root = std::env::var("IDE_PREVIEW_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("preview-workspaces"));
        let timeout_secs = std::env::var("IDE_PREVIEW_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(900);
        let max_parallel = std::env::var("IDE_PREVIEW_PARALLEL")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(2);
        let api_base = std::env::var("IDE_PREVIEW_API_BASE").unwrap_or_else(|_| {
            format!(
                "http://{}/preview",
                std::env::var("PREVIEW_PUBLIC_ORIGIN")
                    .unwrap_or_else(|_| "127.0.0.1:8081".to_string())
            )
        });
        let cargo_target_dir = root.join(".cargo-target");
        let trunk_offline = std::env::var("IDE_PREVIEW_TRUNK_OFFLINE")
            .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        Self {
            root,
            api_base,
            timeout: Duration::from_secs(timeout_secs),
            max_parallel: max_parallel.max(1),
            cargo_target_dir,
            trunk_offline,
        }
        .absolutize()
    }
}

/// Registry of preview builds plus the concurrency gate.
#[derive(Debug)]
pub struct BuildManager {
    inner: Mutex<HashMap<String, BuildRecord>>,
    cancelled: Mutex<std::collections::HashSet<String>>,
    slots: Arc<Semaphore>,
    config: PreviewConfig,
}

impl BuildManager {
    pub fn new(config: PreviewConfig) -> Self {
        let config = config.absolutize();
        let slots = Arc::new(Semaphore::new(config.max_parallel));
        Self {
            inner: Mutex::new(HashMap::new()),
            cancelled: Mutex::new(std::collections::HashSet::new()),
            slots,
            config,
        }
    }

    /// Flag a build as cancelled. The worker observes this between phases and
    /// at the log-flush points, so an in-flight compiler process finishes
    /// before the workspace is dropped.
    pub fn mark_cancelled(&self, id: &str) {
        self.cancelled.lock().unwrap().insert(id.to_string());
    }

    pub fn is_cancelled(&self, id: &str) -> bool {
        self.cancelled.lock().unwrap().contains(id)
    }

    pub fn config(&self) -> &PreviewConfig {
        &self.config
    }

    pub fn slots(&self) -> Arc<Semaphore> {
        self.slots.clone()
    }

    /// Register a new build in the `Queued` state and return a snapshot.
    pub fn enqueue(
        &self,
        id: String,
        project_id: String,
        user_id: String,
        target: BuildTarget,
    ) -> BuildRecord {
        let record = BuildRecord::new(id, project_id, user_id, target, &self.config.root);
        let snapshot = record.clone();
        self.inner.lock().unwrap().insert(record.id.clone(), record);
        snapshot
    }

    pub fn get(&self, id: &str) -> Option<BuildRecord> {
        self.inner.lock().unwrap().get(id).cloned()
    }

    /// Fetch a build owned by `user_id` (scoping prevents cross-user access).
    pub fn get_owned(&self, id: &str, user_id: &str) -> Option<BuildRecord> {
        self.get(id).filter(|record| record.user_id == user_id)
    }

    /// All builds for one user, newest first.
    pub fn list_for_user(&self, user_id: &str) -> Vec<BuildRecord> {
        let inner = self.inner.lock().unwrap();
        let mut records: Vec<BuildRecord> = inner
            .values()
            .filter(|record| record.user_id == user_id)
            .cloned()
            .collect();
        records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        records
    }

    /// Forget terminal builds (called after cleanup).
    pub fn remove(&self, id: &str) -> Option<BuildRecord> {
        self.inner.lock().unwrap().remove(id)
    }

    /// Record where the backend binary landed (after a successful cargo build).
    pub fn set_backend_binary(&self, id: &str, binary: PathBuf) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(record) = inner.get_mut(id) {
            record.backend_bin_dir = binary.parent().unwrap_or(&binary).to_path_buf();
            record.backend_binary = Some(binary);
            record.updated_at = Utc::now();
        }
    }

    pub fn advance(&self, id: &str, state: BuildState) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(record) = inner.get_mut(id) {
            record.state = state;
            record.updated_at = Utc::now();
        }
    }

    pub fn append_logs(&self, id: &str, lines: Vec<String>) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(record) = inner.get_mut(id) {
            record.push_logs(lines);
        }
    }

    pub fn finish_ok(&self, id: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(record) = inner.get_mut(id) {
            record.state = BuildState::Ready;
            record.error = None;
            record.updated_at = Utc::now();
        }
    }

    pub fn finish_err(&self, id: &str, error: String) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(record) = inner.get_mut(id) {
            record.state = BuildState::Failed;
            record.error = Some(error);
            record.updated_at = Utc::now();
        }
    }

    pub fn finish_cancelled(&self, id: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(record) = inner.get_mut(id) {
            record.state = BuildState::Cancelled;
            record.updated_at = Utc::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> BuildManager {
        let root = std::env::temp_dir().join("nocode-preview-test");
        let cargo_target_dir = root.join(".cargo-target");
        BuildManager::new(PreviewConfig {
            root,
            api_base: "http://127.0.0.1/preview".into(),
            timeout: Duration::from_secs(1),
            max_parallel: 2,
            cargo_target_dir,
            trunk_offline: false,
        })
    }

    #[test]
    fn cancellation_flag_is_observable() {
        let manager = manager();
        assert!(!manager.is_cancelled("b9"));
        manager.mark_cancelled("b9");
        assert!(manager.is_cancelled("b9"));
        assert!(!manager.is_cancelled("b10"));
    }

    #[test]
    fn enqueue_starts_queued_and_scoped_to_owner() {
        let manager = manager();
        let record = manager.enqueue("b1".into(), "p1".into(), "u1".into(), BuildTarget::All);
        assert_eq!(record.state, BuildState::Queued);
        assert_eq!(record.workspace.file_name().unwrap(), "b1");
        assert_eq!(record.dist_dir.file_name().unwrap(), "dist");

        assert!(manager.get_owned("b1", "u1").is_some());
        assert!(manager.get_owned("b1", "other").is_none());
        assert_eq!(manager.list_for_user("u1").len(), 1);
        assert!(manager.list_for_user("nobody").is_empty());
    }

    #[test]
    fn state_transitions_record_failure_reason() {
        let manager = manager();
        manager.enqueue("b2".into(), "p1".into(), "u1".into(), BuildTarget::Backend);
        manager.advance("b2", BuildState::BuildingBackend);
        assert_eq!(manager.get("b2").unwrap().state, BuildState::BuildingBackend);

        manager.finish_err("b2", "cargo failed".into());
        let record = manager.get("b2").unwrap();
        assert_eq!(record.state, BuildState::Failed);
        assert!(record.state.is_terminal());
        assert_eq!(record.error.as_deref(), Some("cargo failed"));

        manager.finish_ok("b2");
        assert!(manager.get("b2").unwrap().state.is_terminal());
    }

    #[test]
    fn logs_are_capped_and_ordered() {
        let manager = manager();
        manager.enqueue("b3".into(), "p1".into(), "u1".into(), BuildTarget::All);
        let total = MAX_LOG_LINES + 25;
        manager.append_logs("b3", (0..total).map(|index| format!("line {index}")).collect());
        let record = manager.get("b3").unwrap();
        assert_eq!(record.logs.len(), MAX_LOG_LINES);
        // The oldest lines were dropped, the newest survive in order.
        assert_eq!(record.logs.last().unwrap(), &format!("line {}", total - 1));
        assert_eq!(record.logs.first().unwrap(), "line 25");
    }

    #[test]
    fn target_selects_artifacts() {
        assert!(BuildTarget::All.wants_backend() && BuildTarget::All.wants_frontend());
        assert!(BuildTarget::Backend.wants_backend());
        assert!(!BuildTarget::Backend.wants_frontend());
        assert!(BuildTarget::Frontend.wants_frontend());
        assert!(!BuildTarget::Frontend.wants_backend());
    }

    #[test]
    fn absolutize_keeps_absolute_roots_and_fixes_relative_ones() {
        let cwd = std::env::current_dir().unwrap();
        let already_absolute = PreviewConfig {
            root: cwd.clone(),
            api_base: String::new(),
            timeout: Duration::from_secs(1),
            max_parallel: 1,
            cargo_target_dir: cwd.join("target-cache"),
            trunk_offline: false,
        };
        assert_eq!(already_absolute.absolutize().root, cwd);

        let relative = PreviewConfig {
            root: PathBuf::from("preview-workspaces"),
            api_base: String::new(),
            timeout: Duration::from_secs(1),
            max_parallel: 1,
            cargo_target_dir: PathBuf::from("preview-workspaces/.cargo-target"),
            trunk_offline: false,
        }
        .absolutize();
        assert!(relative.root.is_absolute());
        assert!(relative.cargo_target_dir.is_absolute());
        // No verbatim Windows prefixes leaking into the API surface.
        assert!(
            !relative.root.to_string_lossy().starts_with(r"\\?\"),
            "unexpected verbatim path: {}",
            relative.root.display()
        );
    }
}