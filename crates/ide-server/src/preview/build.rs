//! The build worker: materialize a project, then compile it.
//!
//! Everything here runs inside a spawned Tokio task; the HTTP layer only ever
//! reads [`BuildRecord`] snapshots. Commands are executed with an explicit
//! timeout and their output is streamed into the build's log buffer.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use nocode_codegen::generate_project;
use nocode_model::Project;

use super::manager::{BuildManager, BuildRecord, BuildState, BuildTarget, PreviewCapabilities};

/// How often partial output is flushed into the build record.
const LOG_FLUSH_INTERVAL: usize = 25;

/// Report whether a command exists by running it with `--version`.
pub async fn detect_tool(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Probe the host toolchain so the IDE can explain a missing `trunk`.
pub async fn capabilities(manager: &BuildManager) -> PreviewCapabilities {
    let config = manager.config();
    PreviewCapabilities {
        cargo: detect_tool("cargo").await,
        trunk: detect_tool("trunk").await,
        root: config.root.display().to_string(),
        max_parallel: config.max_parallel,
        timeout_secs: config.timeout.as_secs(),
    }
}

/// Run one build to completion, updating `manager`'s record as it goes.
///
/// Cancellation is observed between phases and on timeout (dropping the
/// future kills the child process because it was spawned `kill_on_drop`).
pub async fn run_build(
    manager: Arc<BuildManager>,
    build_id: String,
    project: Project,
    target: BuildTarget,
) {
    let permit = manager.slots().acquire_owned().await.ok();
    if permit.is_none() {
        manager.finish_err(&build_id, "build queue is closed".into());
        return;
    }
    let _permit = permit.expect("permit was just acquired");

    let record = match manager.get(&build_id) {
        Some(record) => record,
        None => return,
    };
    let timeout = manager.config().timeout;

    let result =
        tokio::time::timeout(timeout, drive(manager.clone(), &record, project, target)).await;

    match result {
        Ok(Ok(())) => manager.finish_ok(&build_id),
        Ok(Err(error)) => manager.finish_err(&build_id, error),
        Err(_) => {
            manager.append_logs(
                &build_id,
                vec![format!(
                    "build timed out after {}s",
                    manager.config().timeout.as_secs()
                )],
            );
            manager.finish_err(&build_id, "build timed out".into());
        }
    }
}

async fn drive(
    manager: Arc<BuildManager>,
    record: &BuildRecord,
    project: Project,
    target: BuildTarget,
) -> Result<(), String> {
    let build_id = record.id.clone();
    let config = manager.config().clone();

    if manager.is_cancelled(&build_id) {
        manager.finish_cancelled(&build_id);
        return Ok(());
    }

    // 1. Materialize the generated project.
    manager.advance(&build_id, BuildState::Generating);
    manager.append_logs(
        &build_id,
        vec![format!(
            "generating project into {}",
            record.workspace.display()
        )],
    );
    if let Some(parent) = record.workspace.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("cannot create workspace root: {error}"))?;
    }
    generate_project(&project, &record.workspace)
        .await
        .map_err(|error| format!("code generation failed: {error}"))?;
    manager.append_logs(
        &build_id,
        vec![format!(
            "generated {} files",
            count_files(&record.workspace).await
        )],
    );

    // 2. Backend: cargo build with a shared target directory so incremental
    //    rebuilds across preview sessions stay fast.
    if target.wants_backend() {
        if manager.is_cancelled(&build_id) {
            manager.finish_cancelled(&build_id);
            return Ok(());
        }
        if !detect_tool("cargo").await {
            return Err("cargo is not available on the server host".into());
        }
        manager.advance(&build_id, BuildState::BuildingBackend);
        manager.append_logs(&build_id, vec!["building backend (cargo build)…".into()]);
        let manifest = record.workspace.join("backend").join("Cargo.toml");
        // cargo runs the generated crate's build scripts, which is arbitrary
        // code: isolate it from the server environment like a preview run.
        let mut cargo = Command::new("cargo");
        crate::preview::env::isolate(&mut cargo);
        cargo
            .arg("build")
            .arg("--manifest-path")
            .arg(&manifest)
            .env("CARGO_TARGET_DIR", &config.cargo_target_dir);
        run_command(
            Arc::clone(&manager),
            &build_id,
            &mut cargo,
            record.workspace.join("backend"),
        )
        .await?;

        // The generated crate is named after the project slug and the binary
        // lands in the shared cargo target dir, not in the workspace.
        let binary = config
            .cargo_target_dir
            .join("debug")
            .join(generated_binary_name(&project));
        if !binary.exists() {
            return Err(format!(
                "cargo reported success but {} is missing",
                binary.display()
            ));
        }
        manager.set_backend_binary(&build_id, binary);
    }

    // 3. Frontend: trunk build with API_BASE baked into the bundle.
    if target.wants_frontend() {
        if manager.is_cancelled(&build_id) {
            manager.finish_cancelled(&build_id);
            return Ok(());
        }
        if !detect_tool("trunk").await {
            return Err(
                "trunk is not available on the server host (install: cargo install trunk)".into(),
            );
        }
        manager.advance(&build_id, BuildState::BuildingFrontend);
        manager.append_logs(&build_id, vec!["building frontend (trunk build)…".into()]);
        let mut trunk = Command::new("trunk");
        crate::preview::env::isolate(&mut trunk);
        trunk
            .arg("build")
            .arg("--release")
            .current_dir(record.workspace.join("frontend"))
            // The generated frontend calls `{API_BASE}/api/v1/{entity}`, so
            // API_BASE has to be the build's own preview URL, including the
            // build id. Without it the bundle called /preview/api/v1/...,
            // which the proxy read as a build called "api".
            .env("API_BASE", format!("{}/{}", config.api_base, build_id));
        // Trunk downloads wasm-bindgen on first use; hosts without internet
        // (or users who pre-seeded the cache) can force offline mode.
        if config.trunk_offline {
            trunk.arg("--offline");
        }
        run_command(
            Arc::clone(&manager),
            &build_id,
            &mut trunk,
            record.workspace.join("frontend"),
        )
        .await?;
    }

    if target.wants_frontend() && !record.dist_dir.exists() {
        return Err(format!(
            "frontend build produced no {} directory",
            record.dist_dir.display()
        ));
    }
    manager.append_logs(&build_id, vec!["build finished".into()]);
    Ok(())
}

/// Execute a command, streaming stdout+stderr into the build log.
///
/// Returns `Err` with the exit status when the command fails.
async fn run_command(
    manager: Arc<BuildManager>,
    build_id: &str,
    command: &mut Command,
    working_dir: PathBuf,
) -> Result<(), String> {
    let mut child = command
        .current_dir(&working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("cannot start {:?}: {error}", command.as_std().get_program()))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // stdout and stderr are different concrete types, so each pump gets its
    // own task rather than sharing a vector of heterogeneous futures.
    let mut tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    let for_stdout = Arc::clone(&manager);
    let for_stderr = Arc::clone(&manager);
    if let Some(stdout) = stdout {
        tasks.push(tokio::spawn(pump(stdout, for_stdout, build_id.to_string())));
    }
    if let Some(stderr) = stderr {
        tasks.push(tokio::spawn(pump(stderr, for_stderr, build_id.to_string())));
    }

    let status = child
        .wait()
        .await
        .map_err(|error| format!("command failed: {error}"))?;
    for task in tasks {
        let _ = task.await;
    }

    if status.success() {
        Ok(())
    } else {
        let code = status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".to_string());
        Err(format!(
            "command {:?} exited with {code}",
            command.as_std().get_program()
        ))
    }
}

/// Stream one pipe into the build log, returning when it closes.
async fn pump<R>(reader: R, manager: Arc<BuildManager>, build_id: String)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(reader).lines();
    let mut pending: Vec<String> = Vec::new();
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::info!(%build_id, "{line}");
        pending.push(line);
        if pending.len() >= LOG_FLUSH_INTERVAL {
            manager.append_logs(&build_id, std::mem::take(&mut pending));
        }
    }
    if !pending.is_empty() {
        manager.append_logs(&build_id, pending);
    }
}

/// Name of the binary produced for a generated backend.
///
/// The codegen names the crate after the project slug, so the executable is
/// `<slug>` (plus `.exe` on Windows).
pub fn generated_binary_name(project: &Project) -> String {
    let exe = if cfg!(windows) { ".exe" } else { "" };
    format!("{}{exe}", project.slug)
}

/// Count generated files (one level deep is enough for the log line).
async fn count_files(dir: &Path) -> usize {
    let mut count = 0usize;
    if let Ok(mut entries) = tokio::fs::read_dir(dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry
                .file_type()
                .await
                .map(|kind| kind.is_dir())
                .unwrap_or(false)
            {
                if let Ok(mut children) = tokio::fs::read_dir(entry.path()).await {
                    while children.next_entry().await.ok().flatten().is_some() {
                        count += 1;
                    }
                }
            } else {
                count += 1;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::manager::PreviewConfig;
    use std::time::Duration;

    fn manager() -> Arc<BuildManager> {
        let root = std::env::temp_dir().join("nocode-preview-worker-test");
        Arc::new(BuildManager::new(PreviewConfig {
            root,
            api_base: "http://127.0.0.1/preview".into(),
            timeout: Duration::from_secs(30),
            max_parallel: 1,
            cargo_target_dir: std::env::temp_dir().join("nocode-preview-worker-target"),
            trunk_offline: false,
            preview_database_url: None,
        }))
    }

    #[tokio::test]
    async fn detects_a_missing_tool_without_panicking() {
        // `definitely-not-a-real-tool` must report false rather than error out.
        assert!(!detect_tool("definitely-not-a-real-tool").await);
    }

    #[tokio::test]
    async fn command_failure_is_reported_with_the_program_name() {
        let manager = manager();
        manager.enqueue("t1".into(), "p1".into(), "u1".into(), BuildTarget::Backend);
        let mut command = Command::new("definitely-not-a-real-tool");
        let error = run_command(
            Arc::clone(&manager),
            "t1",
            &mut command,
            std::env::temp_dir(),
        )
        .await
        .expect_err("spawning a missing binary must fail");
        assert!(error.contains("cannot start"), "unexpected error: {error}");
    }

    #[tokio::test]
    async fn command_output_is_streamed_into_the_record() {
        let manager = manager();
        manager.enqueue("t2".into(), "p1".into(), "u1".into(), BuildTarget::Backend);
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd");
            command.args(["/C", "echo hello-from-build"]);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", "echo hello-from-build"]);
            command
        };
        run_command(
            Arc::clone(&manager),
            "t2",
            &mut command,
            std::env::temp_dir(),
        )
        .await
        .expect("command should succeed");
        let record = manager.get("t2").expect("record");
        assert!(
            record
                .logs
                .iter()
                .any(|line| line.contains("hello-from-build")),
            "logs: {:?}",
            record.logs
        );
    }
}
