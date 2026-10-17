//! Process manager for hosted preview backends.
//!
//! Each successful build can be *run*: the generated backend binary is started
//! as a child process on an allocated port, tracked with its PID, and stopped
//! after an idle period so a long-lived IDE server does not accumulate
//! processes.
//!
//! Trust model: the generated code is trusted-host code (the plan defers
//! container isolation to a later milestone), so this manager is deliberately
//! simple — no chroot, no cgroup, no resource limits beyond the idle timeout.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::process::{Child, Command};

/// Lifecycle of one running preview backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Spawned, waiting for the port to accept connections.
    Starting,
    /// Port accepts connections: the app is up.
    Running,
    /// Stopped on request or after idling out.
    Stopped,
    /// The process exited on its own (crash, or a normal shutdown).
    Exited,
    /// Could not be started at all.
    Failed,
}

/// One running preview backend.
#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub build_id: String,
    pub user_id: String,
    pub project_id: String,
    pub port: u16,
    pub pid: Option<u32>,
    pub status: RunStatus,
    /// Exit code when `status` is `Exited`.
    pub exit_code: Option<i32>,
    pub started_at: DateTime<Utc>,
    /// Last proxy traffic; drives idle teardown.
    pub last_used_at: DateTime<Utc>,
    /// How many requests were proxied (diagnostics only).
    pub requests: u64,
}

/// Configuration for the run manager.
#[derive(Debug, Clone)]
pub struct RunConfig {
    /// First port tried; subsequent runs scan upwards and wrap.
    pub base_port: u16,
    /// Hard cap on concurrently running previews.
    pub max_runs: usize,
    /// Stop a preview after this long without traffic.
    pub idle_timeout: Duration,
    /// How long to wait for the spawned app to open its port.
    pub startup_timeout: Duration,
}

impl RunConfig {
    pub fn from_env() -> Self {
        let base_port = std::env::var("IDE_PREVIEW_PORT_BASE")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(8100);
        let max_runs = std::env::var("IDE_PREVIEW_MAX_RUNS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(4);
        let idle_secs = std::env::var("IDE_PREVIEW_IDLE_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(900);
        let startup_secs = std::env::var("IDE_PREVIEW_STARTUP_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(30);
        Self {
            base_port,
            max_runs: max_runs.max(1),
            idle_timeout: Duration::from_secs(idle_secs),
            startup_timeout: Duration::from_secs(startup_secs),
        }
    }
}

struct Process {
    child: Child,
}

/// Tracks running preview backends.
pub struct RunManager {
    records: Mutex<HashMap<String, RunRecord>>,
    processes: Mutex<HashMap<String, Process>>,
    /// Monotonic clock base for idle accounting.
    started: Mutex<HashMap<String, Instant>>,
    config: RunConfig,
    shutting_down: AtomicBool,
}

impl RunManager {
    pub fn new(config: RunConfig) -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            processes: Mutex::new(HashMap::new()),
            started: Mutex::new(HashMap::new()),
            config,
            shutting_down: AtomicBool::new(false),
        }
    }

    pub fn config(&self) -> &RunConfig {
        &self.config
    }

    pub fn get(&self, build_id: &str) -> Option<RunRecord> {
        self.records.lock().unwrap().get(build_id).cloned()
    }

    /// A running preview, if one is alive for this build and user.
    pub fn running_for(&self, build_id: &str, user_id: &str) -> Option<RunRecord> {
        self.get(build_id).filter(|record| {
            record.user_id == user_id
                && matches!(record.status, RunStatus::Starting | RunStatus::Running)
        })
    }

    pub fn list_for_user(&self, user_id: &str) -> Vec<RunRecord> {
        let records = self.records.lock().unwrap();
        let mut list: Vec<RunRecord> = records
            .values()
            .filter(|record| record.user_id == user_id)
            .cloned()
            .collect();
        list.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        list
    }

    /// Record a proxy hit so idle teardown does not kill an active preview.
    pub fn touch(&self, build_id: &str) {
        if let Some(record) = self.records.lock().unwrap().get_mut(build_id) {
            record.last_used_at = Utc::now();
            record.requests += 1;
        }
    }

    /// Find a free TCP port: bind to :0, read the assigned port, release it.
    ///
    /// There is an unavoidable race between releasing the port and the child
    /// binding it; a colliding run then fails to start and is reported as
    /// `Failed`, which the IDE surfaces rather than silently retrying.
    pub async fn allocate_port(&self) -> Option<u16> {
        for _ in 0..64 {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.ok()?;
            let port = listener.local_addr().ok()?.port();
            drop(listener);
            if self.port_in_use(port).await {
                continue;
            }
            return Some(port);
        }
        None
    }

    async fn port_in_use(&self, port: u16) -> bool {
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
    }

    /// Start the generated backend for `build_id`.
    ///
    /// `program`/`args` are passed separately so tests can spawn a trivial
    /// long-lived process instead of a real generated binary. Returns the
    /// record in `Starting` state; call [`RunManager::refresh`] to learn
    /// whether it came up.
    pub async fn start(
        self: &Arc<Self>,
        build_id: &str,
        user_id: &str,
        project_id: &str,
        program: PathBuf,
        args: &[String],
        working_dir: PathBuf,
        env: Vec<(String, String)>,
    ) -> Result<RunRecord, String> {
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err("server is shutting down".into());
        }
        if self
            .records
            .lock()
            .unwrap()
            .values()
            .filter(|record| {
                matches!(record.status, RunStatus::Starting | RunStatus::Running)
            })
            .count()
            >= self.config.max_runs
        {
            return Err(format!(
                "preview limit reached ({} running); stop one first",
                self.config.max_runs
            ));
        }
        // Bare program names (`cmd.exe`, `sh`) are resolved through PATH when
        // spawned, so only probe the filesystem for an explicit path.
        if program.components().count() > 1 && !program.exists() {
            return Err(format!("backend binary {} does not exist", program.display()));
        }

        let port = self
            .allocate_port()
            .await
            .ok_or_else(|| "no free TCP port available".to_string())?;

        let mut command = Command::new(&program);
        command
            .args(args)
            .current_dir(&working_dir)
            .env("PORT", port.to_string())
            .stdin(Stdio::null())
            // Keep the app's logs in the server log: a preview that dies with
            // no output is impossible to diagnose.
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        for (key, value) in env {
            command.env(key, value);
        }

        let child = command
            .spawn()
            .map_err(|error| format!("cannot start backend: {error}"))?;
        let pid = child.id();

        let now = Utc::now();
        let record = RunRecord {
            build_id: build_id.to_string(),
            user_id: user_id.to_string(),
            project_id: project_id.to_string(),
            port,
            pid,
            status: RunStatus::Starting,
            exit_code: None,
            started_at: now,
            last_used_at: now,
            requests: 0,
        };
        self.processes
            .lock()
            .unwrap()
            .insert(build_id.to_string(), Process { child });
        self.started
            .lock()
            .unwrap()
            .insert(build_id.to_string(), Instant::now());
        self.records
            .lock()
            .unwrap()
            .insert(build_id.to_string(), record.clone());
        Ok(record)
    }

    /// Poll a starting preview: promotes `Starting` to `Running` once the port
    /// answers, or records the failure when the process died.
    pub async fn refresh(&self, build_id: &str) -> Option<RunRecord> {
        let port = {
            let records = self.records.lock().unwrap();
            records.get(build_id).map(|record| record.port)
        }?;

        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            if let Some(record) = self.records.lock().unwrap().get_mut(build_id) {
                if record.status == RunStatus::Starting {
                    record.status = RunStatus::Running;
                }
            }
        } else if let Some(status) = self.poll_exit(build_id).await {
            // Read the exit code before locking again: holding a std Mutex
            // guard across an await would make the future non-Send.
            let exit_code = self.take_exit_code(build_id).await;
            let mut records = self.records.lock().unwrap();
            if let Some(record) = records.get_mut(build_id) {
                record.status = status;
                record.exit_code = exit_code;
            }
        }

        self.get(build_id)
    }

    /// `Some(status)` when the child process is no longer running.
    async fn poll_exit(&self, build_id: &str) -> Option<RunStatus> {
        let mut processes = self.processes.lock().unwrap();
        let process = processes.get_mut(build_id)?;
        match process.child.try_wait() {
            Ok(Some(status)) => Some(match status.code() {
                Some(0) => RunStatus::Exited,
                _ => RunStatus::Failed,
            }),
            Ok(None) => None,
            Err(_) => Some(RunStatus::Failed),
        }
    }

    async fn take_exit_code(&self, build_id: &str) -> Option<i32> {
        let mut processes = self.processes.lock().unwrap();
        let process = processes.get_mut(build_id)?;
        process.child.try_wait().ok().flatten().and_then(|s| s.code())
    }

    /// Stop every running preview of one project.
    ///
    /// Called before a build: `cargo` cannot relink a binary that is still
    /// executing, so rebuilding implies restarting the app. Returns the build
    /// ids that were stopped.
    pub async fn stop_for_project(&self, project_id: &str) -> Vec<String> {
        let ids: Vec<String> = {
            let records = self.records.lock().unwrap();
            records
                .iter()
                .filter(|(_, record)| {
                    record.project_id == project_id
                        && matches!(record.status, RunStatus::Starting | RunStatus::Running)
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in &ids {
            tracing::info!(build_id = %id, project = %project_id, "stopping preview before rebuild");
            self.stop(id).await;
        }
        ids
    }

    /// Stop a preview and drop its record.
    pub async fn stop(&self, build_id: &str) -> bool {
        let existed = self.records.lock().unwrap().remove(build_id).is_some();
        self.started.lock().unwrap().remove(build_id);
        let process = { self.processes.lock().unwrap().remove(build_id) };
        if let Some(mut process) = process {
            let _ = process.child.start_kill();
            // Give the OS a moment to release the port before we forget it.
            let _ = tokio::time::timeout(Duration::from_secs(5), process.child.wait()).await;
        }
        existed
    }

    /// Stop everything (called during graceful shutdown).
    pub async fn stop_all(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        let ids: Vec<String> = self.records.lock().unwrap().keys().cloned().collect();
        for id in ids {
            self.stop(&id).await;
        }
    }

    /// Stop previews that have not served traffic for `idle_timeout`.
    ///
    /// Returns the ids that were reaped.
    pub async fn reap_idle(&self) -> Vec<String> {
        let idle = self.config.idle_timeout;
        let mut reaped = Vec::new();
        let candidates: Vec<String> = {
            let records = self.records.lock().unwrap();
            let now = Utc::now();
            records
                .iter()
                .filter(|(_, record)| {
                    matches!(record.status, RunStatus::Starting | RunStatus::Running)
                        && (now - record.last_used_at).to_std().unwrap_or_default() > idle
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in candidates {
            tracing::info!(build_id = %id, "stopping idle preview");
            self.stop(&id).await;
            reaped.push(id);
        }
        reaped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> Arc<RunManager> {
        Arc::new(RunManager::new(RunConfig {
            base_port: 0,
            max_runs: 2,
            idle_timeout: Duration::from_secs(60),
            startup_timeout: Duration::from_secs(2),
        }))
    }

    #[tokio::test]
    async fn allocates_distinct_free_ports() {
        let manager = manager();
        let first = manager.allocate_port().await.expect("a free port");
        let second = manager.allocate_port().await.expect("another free port");
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn start_reports_a_missing_binary() {
        let manager = manager();

        // An explicit path gets the friendly pre-check.
        let error = manager
            .start(
                "b1",
                "u1",
                "p1",
                std::env::current_dir()
                    .unwrap()
                    .join("definitely-missing-binary"),
                &[],
                std::env::temp_dir(),
                Vec::new(),
            )
            .await
            .expect_err("missing binary must fail");
        assert!(error.contains("does not exist"), "unexpected: {error}");
        assert!(manager.get("b1").is_none());

        // A bare name is resolved through PATH, so the spawn error surfaces.
        let error = manager
            .start(
                "b1b",
                "u1",
                "p1",
                PathBuf::from("definitely-missing-binary"),
                &[],
                std::env::temp_dir(),
                Vec::new(),
            )
            .await
            .expect_err("missing binary must fail");
        assert!(error.contains("cannot start backend"), "unexpected: {error}");
        assert!(manager.get("b1b").is_none());
    }

    #[tokio::test]
    async fn start_then_stop_returns_the_port_to_the_pool() {
        let manager = manager();
        let (program, args) = sleeper();
        let record = manager
            .start("b2", "u1", "p1", program, &args, std::env::temp_dir(), Vec::new())
            .await
            .expect("start");
        assert!(matches!(record.status, RunStatus::Starting));
        assert_eq!(record.user_id, "u1");

        assert!(manager.running_for("b2", "u1").is_some());
        // Ownership is enforced: another user must not see it.
        assert!(manager.running_for("b2", "other").is_none());

        assert!(manager.stop("b2").await);
        assert!(manager.get("b2").is_none());
        assert!(!manager.stop("b2").await);
    }

    #[tokio::test]
    async fn touch_keeps_a_preview_alive_and_counts_requests() {
        let manager = manager();
        let (program, args) = sleeper();
        let _ = manager
            .start("b3", "u1", "p1", program, &args, std::env::temp_dir(), Vec::new())
            .await;
        manager.touch("b3");
        manager.touch("b3");
        let record = manager.get("b3").expect("record");
        assert_eq!(record.requests, 2);
    }

    #[tokio::test]
    async fn idle_previews_are_reaped() {
        let manager = Arc::new(RunManager::new(RunConfig {
            base_port: 0,
            max_runs: 4,
            // Zero means "already idle".
            idle_timeout: Duration::from_secs(0),
            startup_timeout: Duration::from_secs(2),
        }));
        let (program, args) = sleeper();
        let _ = manager
            .start("b4", "u1", "p1", program, &args, std::env::temp_dir(), Vec::new())
            .await;
        let reaped = manager.reap_idle().await;
        assert_eq!(reaped, vec!["b4".to_string()]);
        assert!(manager.get("b4").is_none());
    }

    /// A trivial long-lived process, used instead of a real generated binary.
    fn sleeper() -> (PathBuf, Vec<String>) {
        #[cfg(windows)]
        {
            (
                PathBuf::from("cmd.exe"),
                vec![
                    "/C".to_string(),
                    "ping -n 60 127.0.0.1 > nul".to_string(),
                ],
            )
        }
        #[cfg(not(windows))]
        {
            (
                PathBuf::from("sh"),
                vec!["-c".to_string(), "sleep 60".to_string()],
            )
        }
    }
}