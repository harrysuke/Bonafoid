//! Deployment orchestration.
//!
//! 1. Generate the project into a staging directory.
//! 2. Write the model JSON beside it and align the CI trigger with the branch.
//! 3. `git init`, `add`, create a commit (parented on the remote tip when the
//!    branch already belongs to this project).
//! 4. `push` with an explicit, non-forced refspec. Pushing IS the trigger:
//!    the repository carries the generated workflow.
//!
//! # Status honesty
//!
//! A successful push is [`DeploymentStatus::Pushed`], NOT "deployed". The
//! image build happens minutes later on a GitHub runner. Only a workflow run
//! whose `head_sha` equals this deployment's commit may move it to
//! `CiSucceeded`.

use crate::compose::{patch_workflow_branch, verify_generated, ComposeConfig, ComposeError};
use crate::git::{GitError, GitService};
use crate::github::{GitHubClient, GitHubConfig, GitHubError, WorkflowRun, GITHUB_API_BASE};
use crate::vault::{CredentialVault, SealedCredential, VaultError};
use crate::{redact, valid_branch};
use nocode_codegen::project::generate_project;
use nocode_model::Project;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{instrument, warn};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum DeployError {
    #[error("codegen failed: {0}")]
    Codegen(#[from] anyhow::Error),
    #[error("model serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("git operation failed: {0}")]
    Git(#[from] GitError),
    #[error("GitHub API error: {0}")]
    GitHub(#[from] GitHubError),
    #[error("vault error: {0}")]
    Vault(#[from] VaultError),
    #[error("generated deployment files are inconsistent: {0}")]
    Compose(#[from] ComposeError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("deployment already in progress for this commit")]
    Idempotent,
    #[error("branch `{0}` already exists and was not created by a deployment of this project")]
    BranchExists(String),
    #[error("invalid deployment configuration: {0}")]
    Config(String),
    #[error("staging path escaped the staging root")]
    StagingOutsideRoot,
}

/// Configuration for a single deployment.
#[derive(Debug, Clone)]
pub struct DeployConfig {
    pub github: GitHubConfig,
    pub compose: ComposeConfig,
    /// The public API base URL baked into the frontend bundle.
    pub api_base: String,
}

/// Caller-supplied facts the service cannot know on its own.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeployOptions {
    /// True when this project has already pushed to this branch through the
    /// IDE. Without it, an existing remote branch is refused rather than
    /// overwritten — even by a fast-forward push.
    pub branch_created_by_deploy: bool,
}

/// A deployment record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Deployment {
    pub id: Uuid,
    pub project_id: String,
    pub commit_sha: String,
    pub status: DeploymentStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub github_run_id: Option<u64>,
    pub github_run_url: Option<String>,
    pub error_message: Option<String>,
}

impl Deployment {
    /// The app is deployed only when CI actually finished successfully.
    pub fn deployed(&self) -> bool {
        self.status == DeploymentStatus::CiSucceeded
    }

    /// A coarse, UI-friendly description of where the CI leg is.
    pub fn ci_state(&self) -> &'static str {
        match self.status {
            DeploymentStatus::CiQueued => "queued",
            DeploymentStatus::CiRunning => "running",
            DeploymentStatus::CiSucceeded => "succeeded",
            DeploymentStatus::CiFailed | DeploymentStatus::Failed => "failed",
            _ => "not_started",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Generating,
    Committing,
    Pushing,
    /// Push succeeded; the CI outcome is not known yet.
    Pushed,
    CiQueued,
    CiRunning,
    CiSucceeded,
    CiFailed,
    Failed,
}

impl DeploymentStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::CiSucceeded | Self::CiFailed | Self::Failed)
    }
}

/// Orchestrates deployments.
pub struct DeployService {
    git: GitService,
    vault: CredentialVault,
    staging_root: PathBuf,
    github_base_url: String,
    /// Overrides the git remote. Production leaves this `None` and pushes to
    /// the clean GitHub URL; local integration tests point it at a bare repo
    /// so a real push can be verified without any credential or network.
    git_remote_override: Option<String>,
}

impl DeployService {
    pub fn new(vault: CredentialVault, staging_root: PathBuf) -> Self {
        Self {
            git: GitService::new(),
            vault,
            staging_root,
            github_base_url: GITHUB_API_BASE.to_string(),
            git_remote_override: None,
        }
    }

    /// Override the GitHub API base URL. Tests point this at a local server;
    /// production keeps the default `https://api.github.com`.
    pub fn with_github_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.github_base_url = base_url.into();
        self
    }

    /// Override the git remote URL (local bare repositories in tests).
    ///
    /// This never accepts a credential-bearing URL: [`GitService::set_remote`]
    /// applies the same check it applies to the production GitHub URL.
    pub fn with_git_remote(mut self, url: impl Into<String>) -> Self {
        self.git_remote_override = Some(url.into());
        self
    }

    pub fn vault(&self) -> &CredentialVault {
        &self.vault
    }

    /// The GitHub REST base URL this service talks to. Handlers use it to
    /// build a verification client with the same injection point as
    /// `check_status`, so tests can point every GitHub call at one fake.
    pub fn github_base_url(&self) -> &str {
        &self.github_base_url
    }

    /// Deploy a project.
    ///
    /// The `github_token` is a sealed credential; it is opened briefly for
    /// the push and never persisted or returned.
    #[instrument(skip_all)]
    pub async fn deploy(
        &self,
        project: &Project,
        config: &DeployConfig,
        sealed_token: &SealedCredential,
        options: DeployOptions,
    ) -> Result<Deployment, DeployError> {
        config.github.validate()?;
        if !valid_branch(&config.github.branch) {
            return Err(DeployError::Config("invalid branch name".into()));
        }
        if config.api_base.is_empty() || config.api_base.chars().any(char::is_whitespace) {
            return Err(DeployError::Config("invalid API base URL".into()));
        }

        // The image names come from the project itself, not from client input,
        // so the generated compose file and the workflow cannot disagree.
        let mut effective = config.clone();
        effective.compose.project_slug = project.slug.clone();
        effective.compose.registry = effective.compose.registry.clone().with_slug(&project.slug);
        effective.compose.registry.validate()?;

        let token = Zeroizing::new(self.vault.open(sealed_token)?);
        let deploy_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let stage = self.staging_root.join(format!("deploy-{deploy_id}"));
        self.ensure_within(&stage)?;
        fs::create_dir_all(&stage).await?;
        // Cleanup on every path, including an early error or a dropped future.
        let _guard = StageGuard(stage.clone());

        // 1. Generate the tree (codegen writes .github/workflows/deploy.yml).
        generate_project(project, &stage).await?;

        // 2. Carry the model alongside the code, as the roadmap requires.
        let model_json = serde_json::to_string_pretty(project)?;
        fs::write(stage.join("project.json"), model_json).await?;

        // 3. The generated workflow triggers on `main`. Make it trigger on the
        //    branch we are about to push, or CI would never run.
        let workflow_path = stage.join(".github/workflows/deploy.yml");
        let workflow = fs::read_to_string(&workflow_path).await?;
        fs::write(
            &workflow_path,
            patch_workflow_branch(&workflow, &effective.github.branch)?,
        )
        .await?;

        // 4. Fail locally rather than minutes later on a GitHub runner.
        verify_generated(&stage, &effective.compose)?;

        // 5. Commit + push. No credentials in the URL, no --force, ever.
        let remote = self
            .git_remote_override
            .clone()
            .unwrap_or_else(|| effective.github.remote_url());
        self.git.init(&stage).await?;
        self.git.set_remote(&stage, &remote).await?;
        self.git.add_all(&stage).await?;
        let tree = self.git.write_tree(&stage).await?;

        let remote_tip = self
            .git
            .ls_remote_head(&stage, &effective.github.branch, token.as_str())
            .await?;
        let parent = match remote_tip {
            Some(tip) => {
                if !options.branch_created_by_deploy {
                    return Err(DeployError::BranchExists(effective.github.branch.clone()));
                }
                self.git
                    .fetch(&stage, &effective.github.branch, token.as_str())
                    .await?;
                if self.git.tree_of(&stage, &tip).await? == tree {
                    // Same generated tree: no new commit, no new push.
                    return Err(DeployError::Idempotent);
                }
                Some(tip)
            }
            None => None,
        };

        let commit_msg = format!("deploy: generated from project {}", project.id.0);
        let commit_sha = self
            .git
            .commit_tree(&stage, &tree, parent.as_deref(), &commit_msg)
            .await?;
        self.git
            .push(
                &stage,
                &effective.github.branch,
                &commit_sha,
                token.as_str(),
            )
            .await?;

        Ok(Deployment {
            id: deploy_id,
            project_id: project.id.0.to_string(),
            commit_sha,
            // Honest: the push worked, the image build has not even started.
            status: DeploymentStatus::Pushed,
            created_at: now,
            updated_at: chrono::Utc::now(),
            github_run_id: None,
            github_run_url: None,
            error_message: None,
        })
    }

    /// Refresh a deployment from GitHub. Only in-flight records are polled,
    /// and only a run for THIS commit can move the status.
    #[instrument(skip_all)]
    pub async fn check_status(
        &self,
        config: &DeployConfig,
        sealed_token: &SealedCredential,
        deployment: &mut Deployment,
    ) -> Result<(), DeployError> {
        if deployment.status.is_terminal() {
            return Ok(());
        }
        let token = Zeroizing::new(self.vault.open(sealed_token)?);
        let client = GitHubClient::with_base_url(
            token.to_string(),
            config.github.clone(),
            self.github_base_url.clone(),
        )?;
        let runs = client.list_workflow_runs().await?;
        if let Some((status, run_id, run_url)) =
            classify_run(&runs, &deployment.commit_sha, &config.github.branch)
        {
            deployment.status = status;
            deployment.github_run_id = Some(run_id);
            deployment.github_run_url = Some(run_url);
        }
        deployment.updated_at = chrono::Utc::now();
        Ok(())
    }

    fn ensure_within(&self, path: &Path) -> Result<(), DeployError> {
        if path.starts_with(&self.staging_root) && path != self.staging_root {
            Ok(())
        } else {
            Err(DeployError::StagingOutsideRoot)
        }
    }
}

/// Pick the newest run that belongs to exactly this commit and branch.
///
/// Returns `None` when no such run exists, which means "keep waiting" — never
/// "guess from the most recent run on the repo".
pub fn classify_run(
    runs: &[WorkflowRun],
    commit_sha: &str,
    branch: &str,
) -> Option<(DeploymentStatus, u64, String)> {
    let run = runs
        .iter()
        .filter(|r| r.head_sha == commit_sha)
        .filter(|r| r.head_branch.is_empty() || r.head_branch == branch)
        .max_by_key(|r| r.id)?;

    let status = if run.status == "completed" {
        match run.conclusion.as_deref() {
            Some("success") => DeploymentStatus::CiSucceeded,
            _ => DeploymentStatus::CiFailed,
        }
    } else {
        match run.status.as_str() {
            "queued" | "waiting" | "pending" | "requested" => DeploymentStatus::CiQueued,
            _ => DeploymentStatus::CiRunning,
        }
    };
    Some((status, run.id, run.html_url.clone()))
}

/// Removes the staging directory on every exit path. Uses the synchronous
/// API because `Drop` cannot await; git commands have all been awaited by
/// the time this runs.
struct StageGuard(PathBuf);

impl Drop for StageGuard {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            warn!(path = %self.0.display(), %error, "could not clean deploy staging directory");
        }
    }
}

/// Errors from child processes can echo a URL containing a token. Belt and
/// braces: scrub any error string before it reaches an API response.
pub fn safe_error(error: &impl std::fmt::Display) -> String {
    redact(&error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        id: u64,
        sha: &str,
        branch: &str,
        status: &str,
        conclusion: Option<&str>,
    ) -> WorkflowRun {
        WorkflowRun {
            id,
            status: status.into(),
            conclusion: conclusion.map(str::to_string),
            html_url: format!("https://github.com/acme/app/actions/runs/{id}"),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            head_sha: sha.into(),
            head_branch: branch.into(),
        }
    }

    #[test]
    fn only_this_commits_run_counts() {
        let runs = vec![
            run(1, "other-sha", "main", "completed", Some("success")),
            run(2, "our-sha", "main", "in_progress", None),
        ];
        let (status, id, _) = classify_run(&runs, "our-sha", "main").unwrap();
        assert_eq!(status, DeploymentStatus::CiRunning);
        assert_eq!(id, 2);
        assert!(classify_run(&runs[..1], "our-sha", "main").is_none());
    }

    #[test]
    fn branch_mismatch_is_ignored() {
        let runs = vec![run(1, "our-sha", "feature", "completed", Some("success"))];
        assert!(classify_run(&runs, "our-sha", "main").is_none());
    }

    #[test]
    fn completed_run_uses_conclusion() {
        let success = vec![run(1, "sha", "main", "completed", Some("success"))];
        assert_eq!(
            classify_run(&success, "sha", "main").unwrap().0,
            DeploymentStatus::CiSucceeded
        );
        let failure = vec![run(1, "sha", "main", "completed", Some("failure"))];
        assert_eq!(
            classify_run(&failure, "sha", "main").unwrap().0,
            DeploymentStatus::CiFailed
        );
    }

    #[test]
    fn pushed_is_not_deployed() {
        let deployment = Deployment {
            id: Uuid::new_v4(),
            project_id: "p".into(),
            commit_sha: "sha".into(),
            status: DeploymentStatus::Pushed,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            github_run_id: None,
            github_run_url: None,
            error_message: None,
        };
        assert!(!deployment.deployed());
        assert_eq!(deployment.ci_state(), "not_started");
    }

    #[test]
    fn safe_error_masks_a_leaked_token() {
        let err = "fatal: unable to access 'https://x-access-token:ghp_leaked@github.com/'";
        assert!(!safe_error(&err).contains("leaked"));
    }
}
