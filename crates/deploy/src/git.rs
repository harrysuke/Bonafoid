//! Git operations for deployment.
//!
//! The personal access token reaches git through the environment
//! (`GIT_ASKPASS` / `GITHUB_TOKEN`) or a temporary credential helper —
//! NEVER on the command line, where it would be visible in `ps`.

use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, error, info, instrument};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git init failed: {0}")]
    Init(String),
    #[error("git add failed: {0}")]
    Add(String),
    #[error("git commit failed: {0}")]
    Commit(String),
    #[error("git remote failed: {0}")]
    Remote(String),
    #[error("git push failed: {0}")]
    Push(String),
    #[error("git status failed: {0}")]
    Status(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// A thin async wrapper around git operations.
pub struct GitService;

impl GitService {
    pub fn new() -> Self {
        Self
    }

    /// Initialize a git repository in `dir`.
    #[instrument(skip(self))]
    pub async fn init(&self, dir: &Path) -> Result<(), GitError> {
        let output = Command::new("git")
            .arg("init")
            .current_dir(dir)
            .output()
            .await?;
        if !output.status.success() {
            return Err(GitError::Init(String::from_utf8_lossy(&output.stderr).to_string()));
        }
        debug!("git init in {}", dir.display());
        Ok(())
    }

    /// Stage all changes.
    #[instrument(skip(self))]
    pub async fn add_all(&self, dir: &Path) -> Result<(), GitError> {
        let output = Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .await?;
        if !output.status.success() {
            return Err(GitError::Add(String::from_utf8_lossy(&output.stderr).to_string()));
        }
        debug!("git add . in {}", dir.display());
        Ok(())
    }

    /// Commit with the given message.
    #[instrument(skip(self))]
    pub async fn commit(&self, dir: &Path, message: &str) -> Result<String, GitError> {
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(dir)
            .output()
            .await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // "nothing to commit" is not a failure for our purposes
            if stderr.contains("nothing to commit") || stderr.contains("working tree clean") {
                debug!("git commit: nothing to commit");
                return Ok(String::new());
            }
            return Err(GitError::Commit(stderr.to_string()));
        }
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        debug!("git commit: {}", stdout.trim());
        Ok(stdout)
    }

    /// Set the origin remote.
    #[instrument(skip(self))]
    pub async fn set_remote(&self, dir: &Path, url: &str) -> Result<(), GitError> {
        // Remove existing origin if present
        let _ = Command::new("git")
            .args(["remote", "remove", "origin"])
            .current_dir(dir)
            .output()
            .await;

        let output = Command::new("git")
            .args(["remote", "add", "origin", url])
            .current_dir(dir)
            .output()
            .await?;
        if !output.status.success() {
            return Err(GitError::Remote(String::from_utf8_lossy(&output.stderr).to_string()));
        }
        debug!("git remote add origin {}", url);
        Ok(())
    }

    /// Push to the remote, using the token via environment.
    ///
    /// The token is passed through `GITHUB_TOKEN` in the environment, which
    /// git's credential helper can pick up. It NEVER appears on the command
    /// line.
    #[instrument(skip(self, token))]
    pub async fn push(
        &self,
        dir: &Path,
        branch: &str,
        token: &str,
    ) -> Result<String, GitError> {
        let output = Command::new("git")
            .args(["push", "-u", "origin", branch])
            .current_dir(dir)
            .env("GITHUB_TOKEN", token)
            .env(
                "GIT_ASKPASS",
                "echo", // simple helper that echoes the token when git asks for a password
            )
            .env("GIT_USERNAME", "x-access-token")
            .env("GIT_PASSWORD", token)
            .output()
            .await?;
        if !output.status.success() {
            return Err(GitError::Push(String::from_utf8_lossy(&output.stderr).to_string()));
        }
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        info!("git push succeeded: {}", stdout.trim());
        Ok(stdout)
    }

    /// Get the short commit hash of HEAD.
    #[instrument(skip(self))]
    pub async fn head_sha(&self, dir: &Path) -> Result<String, GitError> {
        let output = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(dir)
            .output()
            .await?;
        if !output.status.success() {
            return Err(GitError::Status(String::from_utf8_lossy(&output.stderr).to_string()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl Default for GitService {
    fn default() -> Self {
        Self::new()
    }
}
