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
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

/// Maximum number of log lines retained per run.
const MAX_RUN_LOG_LINES: usize = 500;

/// How long `stop` waits for a killed child to actually exit before it gives
/// up and reports that the process survived.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// How long a line may sit in the pump's buffer before it is published.
///
/// Batching only on "25 lines or EOF" made the log of a long-running app empty
/// until it exited: a preview that prints a startup banner and then serves
/// forever produced no visible output at all. A short timer keeps the write
/// rate batched while still showing output promptly.
const LOG_FLUSH: Duration = Duration::from_millis(250);

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
    /// The backend's own stdout/stderr, capped at [`MAX_RUN_LOG_LINES`].
    ///
    /// The child used to inherit the server's stdio, so a noisy generated app
    /// flooded the IDE server's log and there was no per-run output at all.
    pub logs: Vec<String>,
}

/// Configuration for the run manager.
#[derive(Debug, Clone)]
pub struct RunConfig {
    /// Hard cap on concurrently running previews.
    pub max_runs: usize,
    /// Stop a preview after this long without traffic.
    pub idle_timeout: Duration,
}

impl RunConfig {
    pub fn from_env() -> Self {
        let max_runs = std::env::var("IDE_PREVIEW_MAX_RUNS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(4);
        let idle_secs = std::env::var("IDE_PREVIEW_IDLE_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(900);
        Self {
            max_runs: max_runs.max(1),
            idle_timeout: Duration::from_secs(idle_secs),
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
    config: RunConfig,
    shutting_down: AtomicBool,
}

/// Everything [`RunManager::start`] needs to spawn a preview backend.
///
/// A struct rather than eight positional arguments: `build_id`, `user_id`
/// and `project_id` are all `&str` and easy to transpose. `program`/`args`
/// are separate so tests can spawn a trivial long-lived process instead of a
/// real generated binary.
pub struct RunRequest<'a> {
    pub build_id: &'a str,
    pub user_id: &'a str,
    pub project_id: &'a str,
    pub program: PathBuf,
    pub args: &'a [String],
    pub working_dir: PathBuf,
    /// Added on top of the inherited allowlist (see [`crate::preview::env`]).
    pub env: Vec<(String, String)>,
}

impl RunManager {
    pub fn new(config: RunConfig) -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            processes: Mutex::new(HashMap::new()),
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
        list.sort_by_key(|record| std::cmp::Reverse(record.started_at));
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

    /// Start the generated backend described by `request`.
    ///
    /// Returns the record in `Starting` state; call [`RunManager::refresh`] to
    /// learn whether it came up.
    pub async fn start(self: &Arc<Self>, request: RunRequest<'_>) -> Result<RunRecord, String> {
        let RunRequest {
            build_id,
            user_id,
            project_id,
            program,
            args,
            working_dir,
            env,
        } = request;
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err("server is shutting down".into());
        }
        // The quota is PER USER. Counting every running preview on the host
        // let one user occupy all of the slots, which is a denial of service
        // against every other user.
        let running_for_user = self
            .records
            .lock()
            .unwrap()
            .values()
            .filter(|record| {
                record.user_id == user_id
                    && matches!(record.status, RunStatus::Starting | RunStatus::Running)
            })
            .count();
        if running_for_user >= self.config.max_runs {
            return Err(format!(
                "preview limit reached ({} running for this account); stop one first",
                self.config.max_runs
            ));
        }
        // Bare program names (`cmd.exe`, `sh`) are resolved through PATH when
        // spawned, so only probe the filesystem for an explicit path.
        if program.components().count() > 1 && !program.exists() {
            return Err(format!(
                "backend binary {} does not exist",
                program.display()
            ));
        }

        let port = self
            .allocate_port()
            .await
            .ok_or_else(|| "no free TCP port available".to_string())?;

        let mut command = Command::new(&program);
        // The child runs generated code: give it a cleared environment, never
        // the IDE server's own (which holds DATABASE_URL, JWT_KEYS_DIR, ...).
        crate::preview::env::isolate(&mut command);
        // Its own process group, so stopping the preview also stops anything
        // the generated app forked (a worker thread that spawns a helper, a
        // dev server wrapper, ...).
        crate::preview::isolate_process_group(&mut command);
        command
            .args(args)
            .current_dir(&working_dir)
            .env("PORT", port.to_string())
            .stdin(Stdio::null())
            // Piped, not inherited: the child's output belongs to the run, not
            // to the IDE server's log.
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (key, value) in env {
            command.env(key, value);
        }

        let mut child = command
            .spawn()
            .map_err(|error| format!("cannot start backend: {error}"))?;
        let pid = child.id();

        // Pump both pipes into the record. The pumps end when the pipes close,
        // which happens when the child exits.
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        if let Some(stdout) = stdout {
            let manager = Arc::clone(self);
            let id = build_id.to_string();
            tokio::spawn(async move { manager.pump(stdout, &id).await });
        }
        if let Some(stderr) = stderr {
            let manager = Arc::clone(self);
            let id = build_id.to_string();
            tokio::spawn(async move { manager.pump(stderr, &id).await });
        }

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
            logs: Vec::new(),
        };
        self.processes
            .lock()
            .unwrap()
            .insert(build_id.to_string(), Process { child });
        self.records
            .lock()
            .unwrap()
            .insert(build_id.to_string(), record.clone());
        Ok(record)
    }

    /// Stream one child pipe into the run's log buffer.
    async fn pump<R>(self: &Arc<Self>, reader: R, build_id: &str)
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let mut lines = BufReader::new(reader).lines();
        let mut pending: Vec<String> = Vec::new();
        loop {
            let next = tokio::select! {
                line = lines.next_line() => line,
                // Only armed when there is something waiting to be published,
                // so an idle child does not wake the task every 250 ms.
                () = tokio::time::sleep(LOG_FLUSH), if !pending.is_empty() => {
                    self.append_logs(build_id, std::mem::take(&mut pending));
                    continue;
                }
            };
            match next {
                Ok(Some(line)) => {
                    tracing::debug!(%build_id, "{line}");
                    pending.push(line);
                    if pending.len() >= 25 {
                        self.append_logs(build_id, std::mem::take(&mut pending));
                    }
                }
                // EOF or a read error: the pipe is gone, so the child is too.
                _ => break,
            }
        }
        if !pending.is_empty() {
            self.append_logs(build_id, pending);
        }
    }

    fn append_logs(&self, build_id: &str, lines: Vec<String>) {
        let mut records = self.records.lock().unwrap();
        if let Some(record) = records.get_mut(build_id) {
            record.logs.extend(lines);
            while record.logs.len() > MAX_RUN_LOG_LINES {
                record.logs.remove(0);
            }
        }
    }

    /// Poll a starting preview: promotes `Starting` to `Running` once the port
    /// answers, or records the failure when the process died.
    pub async fn refresh(&self, build_id: &str) -> Option<RunRecord> {
        let port = {
            let records = self.records.lock().unwrap();
            records.get(build_id).map(|record| record.port)
        }?;

        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
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
        process
            .child
            .try_wait()
            .ok()
            .flatten()
            .and_then(|s| s.code())
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
    ///
    /// Returns whether a running preview was stopped. The process is killed
    /// and WAITED FOR before the record (and therefore the port) is released:
    /// the previous version removed the record first and discarded the wait
    /// result, so a process that survived the kill looked stopped and its port
    /// was handed to the next run.
    pub async fn stop(&self, build_id: &str) -> bool {
        let process = { self.processes.lock().unwrap().remove(build_id) };
        let Some(mut process) = process else {
            // Nothing tracked: drop any leftover record so the id does not
            // linger and keep counting against the quota.
            return self.records.lock().unwrap().remove(build_id).is_some();
        };

        let _ = process.child.start_kill();
        let gone = match tokio::time::timeout(STOP_GRACE, process.child.wait()).await {
            // Reaped: the process is definitely gone.
            Ok(Ok(_)) => true,
            // The handle is broken, so nothing can be waited on again; treat
            // the process as gone rather than tracking it forever.
            Ok(Err(_)) => true,
            // Still running after the grace period.
            Err(_) => false,
        };

        if !gone {
            tracing::warn!(
                build_id,
                pid = process.child.id(),
                "preview process survived stop(); keeping it tracked"
            );
            self.processes
                .lock()
                .unwrap()
                .insert(build_id.to_string(), process);
            if let Some(record) = self.records.lock().unwrap().get_mut(build_id) {
                record.status = RunStatus::Failed;
            }
            return false;
        }

        self.records.lock().unwrap().remove(build_id).is_some()
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
    /// Also detects crashes first. A preview that died on its own used to stay
    /// `Running` until somebody listed runs, so it kept consuming a per-user
    /// slot for up to a full idle timeout — long after it could serve anything.
    ///
    /// Returns the ids that were reaped.
    pub async fn reap_idle(&self) -> Vec<String> {
        // Refresh first so a dead child is marked failed and drops out of the
        // candidate set below.
        let active: Vec<String> = {
            let records = self.records.lock().unwrap();
            records
                .iter()
                .filter(|(_, record)| {
                    matches!(record.status, RunStatus::Starting | RunStatus::Running)
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in active {
            self.refresh(&id).await;
        }

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
            max_runs: 2,
            idle_timeout: Duration::from_secs(60),
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
            .start(RunRequest {
                build_id: "b1",
                user_id: "u1",
                project_id: "p1",
                program: std::env::current_dir()
                    .unwrap()
                    .join("definitely-missing-binary"),
                args: &[],
                working_dir: std::env::temp_dir(),
                env: Vec::new(),
            })
            .await
            .expect_err("missing binary must fail");
        assert!(error.contains("does not exist"), "unexpected: {error}");
        assert!(manager.get("b1").is_none());

        // A bare name is resolved through PATH, so the spawn error surfaces.
        let error = manager
            .start(RunRequest {
                build_id: "b1b",
                user_id: "u1",
                project_id: "p1",
                program: PathBuf::from("definitely-missing-binary"),
                args: &[],
                working_dir: std::env::temp_dir(),
                env: Vec::new(),
            })
            .await
            .expect_err("missing binary must fail");
        assert!(
            error.contains("cannot start backend"),
            "unexpected: {error}"
        );
        assert!(manager.get("b1b").is_none());
    }

    #[tokio::test]
    async fn start_then_stop_returns_the_port_to_the_pool() {
        let manager = manager();
        let (program, args) = sleeper();
        let record = manager
            .start(RunRequest {
                build_id: "b2",
                user_id: "u1",
                project_id: "p1",
                program: program.clone(),
                args: &args,
                working_dir: std::env::temp_dir(),
                env: Vec::new(),
            })
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
            .start(RunRequest {
                build_id: "b3",
                user_id: "u1",
                project_id: "p1",
                program: program.clone(),
                args: &args,
                working_dir: std::env::temp_dir(),
                env: Vec::new(),
            })
            .await;
        manager.touch("b3");
        manager.touch("b3");
        let record = manager.get("b3").expect("record");
        assert_eq!(record.requests, 2);
    }

    #[tokio::test]
    async fn idle_previews_are_reaped() {
        let manager = Arc::new(RunManager::new(RunConfig {
            max_runs: 4,
            // Zero means "already idle".
            idle_timeout: Duration::from_secs(0),
        }));
        let (program, args) = sleeper();
        let _ = manager
            .start(RunRequest {
                build_id: "b4",
                user_id: "u1",
                project_id: "p1",
                program: program.clone(),
                args: &args,
                working_dir: std::env::temp_dir(),
                env: Vec::new(),
            })
            .await;
        let reaped = manager.reap_idle().await;
        assert_eq!(reaped, vec!["b4".to_string()]);
        assert!(manager.get("b4").is_none());
    }

    /// The run quota is per user: one account cannot occupy every slot.
    #[tokio::test]
    async fn the_run_quota_is_per_user() {
        let manager = Arc::new(RunManager::new(RunConfig {
            max_runs: 1,
            idle_timeout: Duration::from_secs(600),
        }));
        let (program, args) = sleeper();
        manager
            .start(RunRequest {
                build_id: "quota-1",
                user_id: "alice",
                project_id: "p1",
                program: program.clone(),
                args: &args,
                working_dir: std::env::temp_dir(),
                env: Vec::new(),
            })
            .await
            .expect("alice's first preview fits");

        let error = manager
            .start(RunRequest {
                build_id: "quota-2",
                user_id: "alice",
                project_id: "p2",
                program: program.clone(),
                args: &args,
                working_dir: std::env::temp_dir(),
                env: Vec::new(),
            })
            .await
            .expect_err("alice's second preview must be refused");
        assert!(error.contains("preview limit reached"), "{error}");

        // Bob is unaffected by Alice's usage.
        manager
            .start(RunRequest {
                build_id: "quota-3",
                user_id: "bob",
                project_id: "p3",
                program,
                args: &args,
                working_dir: std::env::temp_dir(),
                env: Vec::new(),
            })
            .await
            .expect("bob has his own quota");

        manager.stop_all().await;
    }

    /// A preview's own output is captured, not dumped into the server log.
    #[tokio::test]
    async fn child_output_is_captured_in_the_run_log() {
        let manager = Arc::new(RunManager::new(RunConfig {
            max_runs: 2,
            idle_timeout: Duration::from_secs(600),
        }));
        #[cfg(windows)]
        let (program, args) = (
            PathBuf::from("cmd.exe"),
            vec![
                "/C".to_string(),
                "echo hello-from-preview & ping -n 5 127.0.0.1 > nul".to_string(),
            ],
        );
        #[cfg(not(windows))]
        let (program, args) = (
            PathBuf::from("sh"),
            vec![
                "-c".to_string(),
                "echo hello-from-preview; sleep 5".to_string(),
            ],
        );

        manager
            .start(RunRequest {
                build_id: "logs-1",
                user_id: "u1",
                project_id: "p1",
                program,
                args: &args,
                working_dir: std::env::temp_dir(),
                env: Vec::new(),
            })
            .await
            .expect("start");

        let mut seen = false;
        for _ in 0..50 {
            if manager
                .get("logs-1")
                .map(|record| {
                    record
                        .logs
                        .iter()
                        .any(|line| line.contains("hello-from-preview"))
                })
                .unwrap_or(false)
            {
                seen = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(seen, "the child's output never reached the run record");
        manager.stop_all().await;
    }

    /// A preview that died is noticed by the reaper, so it stops holding a
    /// quota slot.
    #[tokio::test]
    async fn the_reaper_notices_a_crashed_preview() {
        let manager = Arc::new(RunManager::new(RunConfig {
            max_runs: 1,
            // Never idle: only the crash detection can free this record.
            idle_timeout: Duration::from_secs(600),
        }));
        #[cfg(windows)]
        let (program, args) = (
            PathBuf::from("cmd.exe"),
            vec!["/C".to_string(), "exit 3".to_string()],
        );
        #[cfg(not(windows))]
        let (program, args) = (
            PathBuf::from("sh"),
            vec!["-c".to_string(), "exit 3".to_string()],
        );

        manager
            .start(RunRequest {
                build_id: "crash-1",
                user_id: "u1",
                project_id: "p1",
                program,
                args: &args,
                working_dir: std::env::temp_dir(),
                env: Vec::new(),
            })
            .await
            .expect("start");

        let reaped = manager.reap_idle().await;
        assert!(
            reaped.is_empty(),
            "a crashed preview is not idle, it is dead: {reaped:?}"
        );
        let record = manager.get("crash-1").expect("record is kept for the IDE");
        assert!(
            matches!(record.status, RunStatus::Failed | RunStatus::Exited),
            "unexpected status {:?}",
            record.status
        );

        // The slot is free again.
        let (program, args) = sleeper();
        manager
            .start(RunRequest {
                build_id: "crash-2",
                user_id: "u1",
                project_id: "p2",
                program,
                args: &args,
                working_dir: std::env::temp_dir(),
                env: Vec::new(),
            })
            .await
            .expect("a crashed preview must not keep the slot");
        manager.stop_all().await;
    }

    /// A trivial long-lived process, used instead of a real generated binary.
    fn sleeper() -> (PathBuf, Vec<String>) {
        #[cfg(windows)]
        {
            (
                PathBuf::from("cmd.exe"),
                vec!["/C".to_string(), "ping -n 60 127.0.0.1 > nul".to_string()],
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
