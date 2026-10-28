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
use uuid::Uuid;

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
        config: &PreviewConfig,
    ) -> Self {
        let now = Utc::now();
        let workspace = config.root.join(&id);
        // The binary's exact path is only known once cargo has built it
        // (`set_backend_binary` fills in `backend_binary`), but the directory
        // it will land in is known up front: the user's own cargo target dir.
        // Reporting an empty path until the build succeeded made the record
        // point at nothing, and an earlier version pointed at
        // `root.parent()/.cargo-target`, which is not where builds go.
        let backend_bin_dir = config.cargo_target_dir_for(&user_id).join("debug");
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
    /// Pass trunk's --offline flag (hosts without internet).
    pub trunk_offline: bool,
    /// `DATABASE_URL` handed to preview backends of non-SQLite projects.
    ///
    /// Never defaults to the project's own connection string: a preview runs
    /// generated code, and that code must not be able to read or destroy
    /// production data. Empty means "non-SQLite previews are refused".
    pub preview_database_url: Option<String>,
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

    /// The cargo target directory for one user's builds.
    ///
    /// PER USER, NOT PER PROJECT, and not shared. The generated crate is
    /// named after the project slug and the binary lands at
    /// `<target>/debug/<slug>`, but a slug is unique only WITHIN a user
    /// (the projects table has a unique index on (user_id, slug)). With one
    /// shared directory, two users with a project named "Blog" would write
    /// to the same path, and the second build would overwrite the first
    /// user's binary — which the first user could then be made to run.
    ///
    /// Per user rather than per project, so a user's own projects still
    /// share the compiled dependencies and the incremental build stays fast.
    pub fn cargo_target_dir_for(&self, user_id: &str) -> PathBuf {
        per_user_dir(&self.cargo_target_dir, user_id)
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
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes"
                )
            })
            .unwrap_or(false);
        let preview_database_url = std::env::var("IDE_PREVIEW_DATABASE_URL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Self {
            root,
            api_base,
            timeout: Duration::from_secs(timeout_secs),
            max_parallel: max_parallel.max(1),
            cargo_target_dir,
            trunk_offline,
            preview_database_url,
        }
        .absolutize()
    }
}

/// A directory under `base` that belongs to one user.
///
/// The user id is hashed rather than used directly: it comes from a JWT
/// claim, and a path is built from it. Hashing also means the id cannot
/// contain a separator, a `..`, or anything else that would escape
/// `base`.
pub fn per_user_dir(base: &std::path::Path, user_id: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(user_id.as_bytes());
    let name: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    base.join(name)
}

/// Is `candidate` equal to `root` or inside it?
///
/// Lexical only: components are resolved against each other without touching
/// the filesystem, so it works for a path that does not exist yet and cannot
/// be fooled by a `..` component. Symlinks are deliberately not followed —
/// `IDE_PREVIEW_ROOT` is operator configuration, and canonicalising it on
/// Windows yields `\\?\` paths that break the generated manifests.
///
/// Used before any recursive delete, because `remove_dir_all` on a path that
/// escaped the root would delete arbitrary directories.
pub fn is_within(root: &std::path::Path, candidate: &std::path::Path) -> bool {
    fn normalise(path: &std::path::Path) -> Vec<std::ffi::OsString> {
        let mut parts: Vec<std::ffi::OsString> = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    parts.pop();
                }
                other => parts.push(other.as_os_str().to_os_string()),
            }
        }
        parts
    }

    let root = normalise(root);
    let candidate = normalise(candidate);
    candidate.len() >= root.len() && candidate.iter().zip(root.iter()).all(|(a, b)| a == b)
}

/// Registry of preview builds plus the concurrency gate.
#[derive(Debug)]
pub struct BuildManager {
    inner: Mutex<HashMap<String, BuildRecord>>,
    /// Ids whose worker should stop at the next checkpoint.
    ///
    /// Entries are removed by the worker itself when it finishes
    /// ([`BuildManager::forget_cancellation`]). Cancelling a build that was
    /// never started would otherwise leave its id here forever.
    cancelled: Mutex<std::collections::HashSet<String>>,
    /// One semaphore per user, created on first use.
    ///
    /// A single process-wide semaphore let one user occupy every build slot,
    /// which is a denial of service against everyone else on the host.
    slots: Mutex<HashMap<String, Arc<Semaphore>>>,
    config: PreviewConfig,
}

impl BuildManager {
    pub fn new(config: PreviewConfig) -> Self {
        let config = config.absolutize();
        Self {
            inner: Mutex::new(HashMap::new()),
            cancelled: Mutex::new(std::collections::HashSet::new()),
            slots: Mutex::new(HashMap::new()),
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

    /// Drop a build's cancellation flag. Called by the worker when it stops
    /// observing it, which is the only moment the flag is guaranteed dead.
    pub fn forget_cancellation(&self, id: &str) {
        self.cancelled.lock().unwrap().remove(id);
    }

    /// Number of cancellation flags currently held (tests + diagnostics).
    pub fn pending_cancellations(&self) -> usize {
        self.cancelled.lock().unwrap().len()
    }

    pub fn config(&self) -> &PreviewConfig {
        &self.config
    }

    /// The build semaphore belonging to one user.
    ///
    /// Per user rather than global: `max_parallel` is a per-user quota, so one
    /// user queueing builds cannot starve another user's.
    pub fn slots_for(&self, user_id: &str) -> Arc<Semaphore> {
        let mut slots = self.slots.lock().unwrap();
        slots
            .entry(user_id.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(self.config.max_parallel)))
            .clone()
    }

    /// Register a new build in the `Queued` state and return a snapshot.
    ///
    /// `id` becomes a path component (`root/<id>`), so it is validated as a
    /// UUID first. `enqueue` is public and the id used to be joined into a
    /// path unvalidated: on Windows `root.join("C:\\evil")` REPLACES the root
    /// entirely, and `..` traverses out of it. Every production caller already
    /// generates a UUID; this makes the public API unable to do otherwise.
    pub fn enqueue(
        &self,
        id: String,
        project_id: String,
        user_id: String,
        target: BuildTarget,
    ) -> Result<BuildRecord, String> {
        if Uuid::parse_str(&id).is_err() {
            return Err(format!("invalid build id {id:?}: expected a UUID"));
        }
        let record = BuildRecord::new(id, project_id, user_id, target, &self.config);
        let snapshot = record.clone();
        self.inner.lock().unwrap().insert(record.id.clone(), record);
        Ok(snapshot)
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
        records.sort_by_key(|record| std::cmp::Reverse(record.created_at));
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
            preview_database_url: None,
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
        let id = Uuid::new_v4().to_string();
        let record = manager
            .enqueue(id.clone(), "p1".into(), "u1".into(), BuildTarget::All)
            .expect("a UUID build id is accepted");
        assert_eq!(record.state, BuildState::Queued);
        assert_eq!(record.workspace.file_name().unwrap(), id.as_str());
        assert_eq!(record.dist_dir.file_name().unwrap(), "dist");

        assert!(manager.get_owned(&id, "u1").is_some());
        assert!(manager.get_owned(&id, "other").is_none());
        assert_eq!(manager.list_for_user("u1").len(), 1);
        assert!(manager.list_for_user("nobody").is_empty());
    }

    /// A build id becomes a path component, so anything that is not a UUID
    /// must be refused before it can reach `Path::join`.
    ///
    /// On Windows `root.join("C:\\evil")` replaces the root entirely, and
    /// `..` walks out of it. `enqueue` is public, so this has to hold for
    /// callers other than the one HTTP handler that generates a UUID.
    #[test]
    fn a_build_id_that_is_not_a_uuid_is_refused() {
        let manager = manager();
        for bad in [
            "b1",
            "..",
            "../escape",
            r"C:\Windows\Temp\evil",
            "00000000-0000-0000-0000-00000000000",
            "",
        ] {
            let error = manager
                .enqueue(bad.into(), "p1".into(), "u1".into(), BuildTarget::All)
                .expect_err("a non-UUID build id must be refused");
            assert!(error.contains("invalid build id"), "{bad:?}: {error}");
            assert!(
                manager.list_for_user("u1").is_empty(),
                "{bad:?} must not create a record"
            );
        }
    }

    #[test]
    fn state_transitions_record_failure_reason() {
        let manager = manager();
        let id = Uuid::new_v4().to_string();
        manager
            .enqueue(id.clone(), "p1".into(), "u1".into(), BuildTarget::Backend)
            .expect("enqueue");
        manager.advance(&id, BuildState::BuildingBackend);
        assert_eq!(manager.get(&id).unwrap().state, BuildState::BuildingBackend);

        manager.finish_err(&id, "cargo failed".into());
        let record = manager.get(&id).unwrap();
        assert_eq!(record.state, BuildState::Failed);
        assert!(record.state.is_terminal());
        assert_eq!(record.error.as_deref(), Some("cargo failed"));

        manager.finish_ok(&id);
        assert!(manager.get(&id).unwrap().state.is_terminal());
    }

    #[test]
    fn logs_are_capped_and_ordered() {
        let manager = manager();
        let id = Uuid::new_v4().to_string();
        manager
            .enqueue(id.clone(), "p1".into(), "u1".into(), BuildTarget::All)
            .expect("enqueue");
        let total = MAX_LOG_LINES + 25;
        manager.append_logs(
            &id,
            (0..total).map(|index| format!("line {index}")).collect(),
        );
        let record = manager.get(&id).unwrap();
        assert_eq!(record.logs.len(), MAX_LOG_LINES);
        // The oldest lines were dropped, the newest survive in order.
        assert_eq!(record.logs.last().unwrap(), &format!("line {}", total - 1));
        assert_eq!(record.logs.first().unwrap(), "line 25");
    }

    /// The cancellation flag is dropped by the worker when it stops looking.
    ///
    /// Without this the set grows for the lifetime of the process: every
    /// cancelled build left its id behind, and nothing ever removed it.
    #[test]
    fn cancellation_flags_are_released_when_the_worker_stops() {
        let manager = manager();
        manager.mark_cancelled("gone");
        assert_eq!(manager.pending_cancellations(), 1);
        manager.forget_cancellation("gone");
        assert_eq!(manager.pending_cancellations(), 0);
        assert!(!manager.is_cancelled("gone"));
    }

    /// Build slots are a per-user quota, not a process-wide one.
    #[test]
    fn build_slots_are_per_user() {
        let manager = manager();
        let alice = manager.slots_for("alice");
        let bob = manager.slots_for("bob");
        assert_eq!(alice.available_permits(), 2);
        assert_eq!(bob.available_permits(), 2);

        let _held = alice.try_acquire().expect("alice has a free slot");
        assert_eq!(alice.available_permits(), 1);
        // Alice taking a slot must not consume Bob's.
        assert_eq!(bob.available_permits(), 2);
        // The same user gets the same semaphore back.
        assert!(Arc::ptr_eq(&alice, &manager.slots_for("alice")));
    }

    /// Containment is lexical, so it works on paths that do not exist and
    /// cannot be defeated by `..`.
    #[test]
    fn containment_rejects_escapes() {
        let root = PathBuf::from("preview-workspaces");
        assert!(is_within(&root, &root.join("abc")));
        assert!(is_within(&root, &root));
        assert!(is_within(&root, &root.join("abc").join("frontend")));
        assert!(!is_within(&root, &root.join("..").join("elsewhere")));
        assert!(!is_within(
            &root,
            &root.join("abc").join("..").join("..").join("x")
        ));
        assert!(!is_within(&root, &PathBuf::from("other-root")));
        // An absolute escape attempt resolves to a different root.
        #[cfg(windows)]
        assert!(!is_within(&root, &PathBuf::from(r"C:\Windows\Temp")));
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
            preview_database_url: None,
        };
        assert_eq!(already_absolute.absolutize().root, cwd);

        let relative = PreviewConfig {
            root: PathBuf::from("preview-workspaces"),
            api_base: String::new(),
            timeout: Duration::from_secs(1),
            max_parallel: 1,
            cargo_target_dir: PathBuf::from("preview-workspaces/.cargo-target"),
            trunk_offline: false,
            preview_database_url: None,
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

    /// Two users must never share a cargo target directory.
    ///
    /// The generated crate is named after the project slug and the binary
    /// lands at <target>/debug/<slug>, but a slug is unique only WITHIN a
    /// user. A shared target directory would let one user's build
    /// overwrite another user's binary, and the first user could then be
    /// made to run it.
    #[test]
    fn the_cargo_target_dir_is_per_user() {
        let config = PreviewConfig {
            root: PathBuf::from("preview-workspaces"),
            api_base: String::new(),
            timeout: Duration::from_secs(1),
            max_parallel: 1,
            cargo_target_dir: PathBuf::from("preview-workspaces/.cargo-target"),
            trunk_offline: false,
            preview_database_url: None,
        };

        let alice = config.cargo_target_dir_for("alice");
        let bob = config.cargo_target_dir_for("bob");
        assert_ne!(alice, bob, "two users share a cargo target directory");
        assert!(alice.starts_with(&config.cargo_target_dir));
        assert!(bob.starts_with(&config.cargo_target_dir));
    }

    /// The same user always gets the same directory, so their own projects
    /// still share the compiled dependencies.
    #[test]
    fn the_cargo_target_dir_is_stable_per_user() {
        let config = PreviewConfig {
            root: PathBuf::from("preview-workspaces"),
            api_base: String::new(),
            timeout: Duration::from_secs(1),
            max_parallel: 1,
            cargo_target_dir: PathBuf::from("preview-workspaces/.cargo-target"),
            trunk_offline: false,
            preview_database_url: None,
        };
        assert_eq!(
            config.cargo_target_dir_for("alice"),
            config.cargo_target_dir_for("alice")
        );
    }

    /// The directory name is derived from a hash, so no user id can put a
    /// separator, a `..` or anything else into the path.
    #[test]
    fn a_user_id_cannot_escape_the_target_dir() {
        let base = PathBuf::from("preview-workspaces/.cargo-target");
        for user_id in ["../../etc", "a/b", "..", "a\\b", ""] {
            let dir = per_user_dir(&base, user_id);
            assert!(
                dir.starts_with(&base),
                "{user_id:?} escaped the target dir: {}",
                dir.display()
            );
            assert_eq!(
                dir.parent(),
                Some(base.as_path()),
                "{user_id:?} produced more than one path component"
            );
        }
    }
}
