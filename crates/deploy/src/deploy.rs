//! Deployment orchestration.
//!
//! 1. Generate the project into a staging directory.
//! 2. Write the model JSON beside it.
//! 3. `git init`, `add`, `commit`.
//! 4. Set the remote and `push`.
//!
//! Pushing IS the trigger: the repository carries the generated workflow.

use crate::{
    compose::ComposeConfig,
    git::{GitError, GitService},
    github::{GitHubClient, GitHubConfig, GitHubError},
    vault::{CredentialVault, SealedCredential, VaultError},
};
use nocode_codegen::project::generate_project;
use nocode_model::Project;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{info, instrument};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum DeployError {
    #[error("codegen failed: {0}")]
    Codegen(#[from] anyhow::Error),
    #[error("git operation failed: {0}")]
    Git(#[from] GitError),
    #[error("GitHub API error: {0}")]
    GitHub(#[from] GitHubError),
    #[error("vault error: {0}")]
    Vault(#[from] VaultError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("deployment already in progress for this commit")]
    Idempotent,
}

/// Configuration for a single deployment.
#[derive(Debug, Clone)]
pub struct DeployConfig {
    pub github: GitHubConfig,
    pub compose: ComposeConfig,
    /// The public API base URL baked into the frontend bundle.
    pub api_base: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeploymentStatus {
    Generating,
    Committing,
    Pushing,
    CiQueued,
    CiRunning,
    CiSucceeded,
    CiFailed,
    Failed,
}

/// Orchestrates deployments.
pub struct DeployService {
    git: GitService,
    vault: CredentialVault,
    staging_root: PathBuf,
}

impl DeployService {
    pub fn new(vault: CredentialVault, staging_root: PathBuf) -> Self {
        Self {
            git: GitService::new(),
            vault,
            staging_root,
        }
    }

    /// Deploy a project.
    ///
    /// The `github_token` is a sealed credential; it is opened briefly for
    /// the push and never persisted or returned.
    #[instrument(skip(self, project, config, sealed_token))]
    pub async fn deploy(
        &self,
        project: &Project,
        config: &DeployConfig,
        sealed_token: &SealedCredential,
    ) -> Result<Deployment, DeployError> {
        let deploy_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        // Open the token briefly for the push
        let token = self.vault.open(sealed_token)?;

        // 1. Stage directory
        let stage = self.staging_root.join(format!("deploy-{}", deploy_id));
        fs::create_dir_all(&stage).await?;

        // 2. Generate project
        info!("generating project {} into {}", project.id.0, stage.display());
        let generated = generate_project(project, &stage).await?;

        // 3. Write model JSON
        let model_json = serde_json::to_string_pretty(project)?;
        fs::write(stage.join("project.json"), model_json).await?;

        // 4. Git init / add / commit
        self.git.init(&stage).await?;
        self.git.add_all(&stage).await?;
        let commit_msg = format!("deploy: generated from project {}", project.id.0);
        self.git.commit(&stage, &commit_msg).await?;
        let commit_sha = self.git.head_sha(&stage).await?;

        // 5. Set remote and push
        let remote_url = format!(
            "https://x-access-token:{}@github.com/{}/{}.git",
            token, config.github.owner, config.github.repo
        );
        self.git.set_remote(&stage, &remote_url).await?;
        self.git.push(&stage, &config.github.branch, &token).await?;

        // Clean up staging directory
        let _ = fs::remove_dir_all(&stage).await;

        Ok(Deployment {
            id: deploy_id,
            project_id: project.id.0.to_string(),
            commit_sha,
            status: DeploymentStatus::Pushing,
            created_at: now,
            updated_at: chrono::Utc::now(),
            github_run_id: None,
            github_run_url: None,
            error_message: None,
        })
    }

    /// Check the CI status for a deployment.
    #[instrument(skip(self, sealed_token))]
    pub async fn check_status(
        &self,
        config: &DeployConfig,
        sealed_token: &SealedCredential,
        deployment: &mut Deployment,
    ) -> Result<(), DeployError> {
        let token = self.vault.open(sealed_token)?;
        let client = GitHubClient::new(token, config.github.clone())?;

        let runs = client.list_workflow_runs().await?;
        if let Some(run) = runs.into_iter().find(|r| r.status != "completed") {
            deployment.github_run_id = Some(run.id);
            deployment.github_run_url = Some(run.html_url.clone());
            deployment.status = match run.status.as_str() {
                "queued" | "waiting" => DeploymentStatus::CiQueued,
                "in_progress" => DeploymentStatus::CiRunning,
                "completed" => match run.conclusion.as_deref() {
                    Some("success") => DeploymentStatus::CiSucceeded,
                    _ => DeploymentStatus::CiFailed,
                },
                _ => DeploymentStatus::CiRunning,
            };
        } else {
            // No active runs — check the most recent completed one for this commit
            let runs = client.list_workflow_runs().await?;
            if let Some(run) = runs.first() {
                deployment.github_run_id = Some(run.id);
                deployment.github_run_url = Some(run.html_url.clone());
                deployment.status = match run.conclusion.as_deref() {
                    Some("success") => DeploymentStatus::CiSucceeded,
                    _ => DeploymentStatus::CiFailed,
                };
            }
        }
        deployment.updated_at = chrono::Utc::now();
        Ok(())
    }
}
