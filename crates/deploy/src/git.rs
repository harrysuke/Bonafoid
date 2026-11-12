//! Git operations for deployment.
//!
//! # Credential discipline
//!
//! The personal access token reaches git ONLY through the environment. An
//! inline `credential.helper` is injected with `GIT_CONFIG_*` env vars and
//! reads `$GITHUB_TOKEN` when git asks for credentials. This means the token
//! is never:
//!   - on a command line (visible in `ps`),
//!   - in a remote URL (persisted to `.git/config`),
//!   - written to a temporary askpass script,
//!   - echoed into a log line.
//!
//! Every stderr/stdout that leaves this module is passed through [`redact`].

use crate::{redact, valid_branch};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tracing::{debug, instrument, warn};

/// Injected as `credential.helper`. The `$GITHUB_TOKEN` is expanded by the
/// shell git spawns, not by us, so the value stored in the environment is a
/// recipe rather than a credential-bearing command line.
const CREDENTIAL_HELPER: &str =
    "!f() { echo username=x-access-token; echo password=$GITHUB_TOKEN; }; f";

/// Fixed identity + date make a generated commit content-addressed: the same
/// tree, parent and message always produce the same SHA. That is what makes
/// deployment idempotency testable.
const AUTHOR_NAME: &str = "No-Code IDE";
const AUTHOR_EMAIL: &str = "deploy@nocode-ide.local";
const AUTHOR_DATE: &str = "2000-01-01T00:00:00+0000";

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git {operation} failed: {message}")]
    Command {
        operation: &'static str,
        message: String,
    },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("remote URL must not contain credentials")]
    CredentialInUrl,
    #[error("invalid branch name")]
    InvalidBranch,
    #[error("invalid git object id")]
    InvalidObjectId,
    #[error("git produced an empty {0}")]
    EmptyOutput(&'static str),
}

/// A thin async wrapper around git operations.
pub struct GitService;

impl GitService {
    pub fn new() -> Self {
        Self
    }

    /// The exact argv used for a push. Exposed so tests can assert that no
    /// token can ever appear on a command line. Branch and commit are
    /// validated here too, so a caller cannot skip the checks above this
    /// layer and smuggle a git option into the refspec.
    pub fn push_args(branch: &str, commit_sha: &str) -> Result<Vec<String>, GitError> {
        check_branch(branch)?;
        check_oid(commit_sha)?;
        Ok(vec![
            "push".to_string(),
            "origin".to_string(),
            // An explicit refspec with no '+' prefix means git performs a
            // fast-forward check. There is deliberately no --force anywhere.
            format!("{commit_sha}:refs/heads/{branch}"),
        ])
    }

    /// Initialize a git repository in `dir`.
    #[instrument(skip(self))]
    pub async fn init(&self, dir: &Path) -> Result<(), GitError> {
        run(dir, &["init", "--quiet"], None, "init").await?;
        debug!(?dir, "git init");
        Ok(())
    }

    /// Stage everything in the generated tree (including deletions).
    #[instrument(skip(self))]
    pub async fn add_all(&self, dir: &Path) -> Result<(), GitError> {
        run(dir, &["add", "-A"], None, "add").await?;
        Ok(())
    }

    /// Write the staged tree and return its object id.
    #[instrument(skip(self))]
    pub async fn write_tree(&self, dir: &Path) -> Result<String, GitError> {
        let sha = run(dir, &["write-tree"], None, "write-tree").await?;
        if sha.is_empty() {
            return Err(GitError::EmptyOutput("tree id"));
        }
        Ok(sha)
    }

    /// Create a commit object for `tree`, optionally parented on `parent`.
    ///
    /// Uses `commit-tree` rather than `commit` so a repeat deploy can build a
    /// child of the current remote tip without checking files out over the
    /// freshly generated working tree.
    #[instrument(skip(self, message))]
    pub async fn commit_tree(
        &self,
        dir: &Path,
        tree: &str,
        parent: Option<&str>,
        message: &str,
    ) -> Result<String, GitError> {
        let mut args = vec!["commit-tree", tree];
        if let Some(p) = parent {
            args.push("-p");
            args.push(p);
        }
        args.push("-m");
        args.push(message);
        let sha = run_deterministic(dir, &args, "commit-tree").await?;
        if sha.is_empty() {
            return Err(GitError::EmptyOutput("commit id"));
        }
        Ok(sha)
    }

    /// Set the origin remote. Credential-bearing URLs are rejected outright.
    #[instrument(skip(self))]
    pub async fn set_remote(&self, dir: &Path, url: &str) -> Result<(), GitError> {
        if !remote_url_is_clean(url) {
            return Err(GitError::CredentialInUrl);
        }
        // Idempotent. `remote add` can write the origin section before a
        // transient Windows file-lock failure, so a retried call answers
        // "already exists"; in that case update the URL instead of failing.
        // Silently ignoring a failed `remote remove` used to hide exactly
        // that state, so there is no remove step here.
        match run(dir, &["remote", "add", "origin", url], None, "remote").await {
            Ok(_) => {}
            Err(GitError::Command { message, .. }) if message.contains("already exists") => {
                run(dir, &["remote", "set-url", "origin", url], None, "remote").await?;
            }
            Err(error) => return Err(error),
        }
        debug!(?url, "git remote origin set (clean URL)");
        Ok(())
    }

    /// Return the remote tip of `branch`, or `None` if the branch does not
    /// exist. Reads only; never changes the remote.
    #[instrument(skip(self, token))]
    pub async fn ls_remote_head(
        &self,
        dir: &Path,
        branch: &str,
        token: &str,
    ) -> Result<Option<String>, GitError> {
        check_branch(branch)?;
        let args = ["ls-remote", "--heads", "origin", branch];
        let output = run(dir, &args, Some(token), "ls-remote").await?;
        Ok(output
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().next())
            .filter(|sha| !sha.is_empty())
            .map(str::to_string))
    }

    /// The tree id of a remote commit, used to detect an unchanged re-deploy.
    #[instrument(skip(self))]
    pub async fn tree_of(&self, dir: &Path, commit: &str) -> Result<String, GitError> {
        check_oid(commit)?;
        let expr = format!("{commit}^{{tree}}");
        let sha = run(dir, &["rev-parse", &expr], None, "rev-parse").await?;
        Ok(sha)
    }

    /// Fetch a remote branch so its objects are available locally.
    #[instrument(skip(self, token))]
    pub async fn fetch(&self, dir: &Path, branch: &str, token: &str) -> Result<(), GitError> {
        check_branch(branch)?;
        let args = ["fetch", "--quiet", "origin", branch];
        run(dir, &args, Some(token), "fetch").await?;
        Ok(())
    }

    /// Push an existing commit object to `branch`. Fast-forward only, no
    /// force, no upstream tracking. The token never enters argv.
    #[instrument(skip(self, token))]
    pub async fn push(
        &self,
        dir: &Path,
        branch: &str,
        commit_sha: &str,
        token: &str,
    ) -> Result<String, GitError> {
        let owned = Self::push_args(branch, commit_sha)?;
        let args: Vec<&str> = owned.iter().map(String::as_str).collect();
        let output = run(dir, &args, Some(token), "push").await?;
        Ok(output)
    }

    /// Full SHA of HEAD (used only for local verification).
    #[instrument(skip(self))]
    pub async fn head_sha(&self, dir: &Path) -> Result<String, GitError> {
        let sha = run(dir, &["rev-parse", "HEAD"], None, "rev-parse").await?;
        if sha.is_empty() {
            return Err(GitError::EmptyOutput("HEAD"));
        }
        Ok(sha)
    }
}

impl Default for GitService {
    fn default() -> Self {
        Self::new()
    }
}

/// Add the environment-only credential helper and deterministic identity to
/// a command.
fn apply_auth(cmd: &mut Command, token: &str) {
    cmd.env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "credential.helper")
        .env("GIT_CONFIG_VALUE_0", CREDENTIAL_HELPER)
        .env("GITHUB_TOKEN", token)
        .env("GIT_TERMINAL_PROMPT", "0");
}

/// A remote is safe to persist in `.git/config` only when it has no userinfo
/// and is either a plain local path (tests use a bare repo) or plain HTTPS.
fn remote_url_is_clean(url: &str) -> bool {
    !url.is_empty()
        // A leading '-' would be parsed by git as an option rather than a
        // URL, both by `remote add` and by `remote set-url`.
        && !url.starts_with('-')
        && !url.contains('@')
        && !url.contains("x-access-token")
        && (!url.contains("://") || url.starts_with("https://"))
}

fn check_branch(branch: &str) -> Result<(), GitError> {
    if valid_branch(branch) {
        Ok(())
    } else {
        Err(GitError::InvalidBranch)
    }
}

/// Object ids coming back from git (or from a remote) are only ever used to
/// build revision expressions, so require a plain hex sha before doing so.
fn check_oid(oid: &str) -> Result<(), GitError> {
    if (40..=64).contains(&oid.len()) && oid.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(GitError::InvalidObjectId)
    }
}

fn apply_identity(cmd: &mut Command) {
    cmd.env("GIT_AUTHOR_NAME", AUTHOR_NAME)
        .env("GIT_AUTHOR_EMAIL", AUTHOR_EMAIL)
        .env("GIT_AUTHOR_DATE", AUTHOR_DATE)
        .env("GIT_COMMITTER_NAME", AUTHOR_NAME)
        .env("GIT_COMMITTER_EMAIL", AUTHOR_EMAIL)
        .env("GIT_COMMITTER_DATE", AUTHOR_DATE);
}

/// Signatures of a transient Windows sharing violation. `git init` writes
/// `.git/config` immediately after the directory is created, and a scanner or
/// indexer briefly holding a handle makes that write fail with "Permission
/// denied" even though nothing is actually wrong.
const TRANSIENT_LOCK_SIGNATURES: &[&str] = &[
    "Permission denied",
    "Access is denied",
    "being used by another process",
    "os error 5",
];

const GIT_ATTEMPTS: usize = 3;

fn is_transient_lock(message: &str) -> bool {
    TRANSIENT_LOCK_SIGNATURES
        .iter()
        .any(|signature| message.contains(signature))
}

/// `push` is deliberately excluded: it is the only operation that mutates a
/// remote, so an ambiguous failure must never be replayed. Every other
/// operation here is local and idempotent.
fn may_retry(operation: &'static str) -> bool {
    operation != "push"
}

/// Run git, retrying only a bounded, local, transient file-lock failure.
/// The retry log deliberately omits stderr — child-process output is
/// redacted before it is returned, but never logged.
async fn execute(
    dir: &Path,
    args: &[&str],
    token: Option<&str>,
    operation: &'static str,
    deterministic: bool,
) -> Result<String, GitError> {
    let mut attempt = 1;
    loop {
        let mut cmd = Command::new("git");
        cmd.args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(t) = token {
            apply_auth(&mut cmd, t);
        }
        if deterministic {
            apply_identity(&mut cmd);
        }
        let output = cmd.output().await?;
        if output.status.success() {
            return Ok(redact(String::from_utf8_lossy(&output.stdout).trim()));
        }
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if attempt < GIT_ATTEMPTS && may_retry(operation) && is_transient_lock(&stderr) {
            warn!(operation, attempt, "transient git file lock; retrying");
            tokio::time::sleep(Duration::from_millis(50 * attempt as u64)).await;
            attempt += 1;
            continue;
        }
        return Err(GitError::Command {
            operation,
            message: redact(&stderr),
        });
    }
}

async fn run(
    dir: &Path,
    args: &[&str],
    token: Option<&str>,
    operation: &'static str,
) -> Result<String, GitError> {
    execute(dir, args, token, operation, false).await
}

async fn run_deterministic(
    dir: &Path,
    args: &[&str],
    operation: &'static str,
) -> Result<String, GitError> {
    execute(dir, args, None, operation, true).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn push_argv_has_no_credentials_and_no_force() {
        let args = GitService::push_args("main", SHA).unwrap();
        let joined = args.join(" ");
        assert!(!joined.contains("--force"));
        assert!(!joined.contains("+"));
        assert!(!joined.contains("ghp_"));
        assert_eq!(args[2], format!("{SHA}:refs/heads/main"));
    }

    #[test]
    fn credential_helper_value_has_no_literal_token() {
        assert!(CREDENTIAL_HELPER.contains("$GITHUB_TOKEN"));
        assert!(!CREDENTIAL_HELPER.contains("ghp_"));
    }

    #[test]
    fn push_refuses_an_invalid_branch() {
        assert!(matches!(
            GitService::push_args("-u", SHA),
            Err(GitError::InvalidBranch)
        ));
        assert!(matches!(
            GitService::push_args("main;echo pwned", SHA),
            Err(GitError::InvalidBranch)
        ));
        assert!(matches!(
            GitService::push_args("main", "-p;echo pwned"),
            Err(GitError::InvalidObjectId)
        ));
    }

    #[test]
    fn remote_url_must_never_carry_a_credential() {
        for url in [
            "https://x-access-token:ghp_secret@github.com/acme/app.git",
            "https://ghp_secret@github.com/acme/app.git",
            "http://github.com/acme/app.git",
            "ssh://git@github.com/acme/app.git",
            "",
            "--upload-pack=evil",
        ] {
            assert!(!remote_url_is_clean(url), "{url} must be rejected");
        }
        assert!(remote_url_is_clean("https://github.com/acme/app.git"));
        // Tests push to a local bare repository instead of GitHub.
        assert!(remote_url_is_clean("D:/tmp/bare.git"));
    }

    #[tokio::test]
    async fn setting_a_remote_twice_is_idempotent() {
        let dir = tempfile::tempdir().expect("temp dir");
        let git = GitService::new();
        git.init(dir.path()).await.expect("git init");
        let url = "https://github.com/acme/app.git";
        git.set_remote(dir.path(), url).await.expect("first set");
        // A retried or repeated deploy must converge, not report
        // "remote origin already exists".
        git.set_remote(dir.path(), url).await.expect("second set");
        let config = std::fs::read_to_string(dir.path().join(".git/config")).expect("config");
        assert_eq!(config.matches("remote \"origin\"").count(), 1);
        assert!(config.contains(url));
    }

    #[test]
    fn only_transient_local_locks_are_retried() {
        assert!(is_transient_lock(
            "error: could not write config file .git/config: Permission denied"
        ));
        assert!(is_transient_lock("fatal: Access is denied. (os error 5)"));
        assert!(!is_transient_lock("fatal: not a git repository"));
        assert!(!is_transient_lock("remote: Permission to acme/app denied"));
        // A remote mutation is never replayed on an ambiguous failure.
        assert!(!may_retry("push"));
        assert!(may_retry("init"));
        assert!(may_retry("add"));
        assert!(may_retry("commit-tree"));
    }
}
