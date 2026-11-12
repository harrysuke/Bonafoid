//! Local-only integration tests: a real `git push` into a bare repository.
//!
//! No network and no credential is involved. These prove the deploy service
//! can actually produce a commit, push it without force, keep the token out
//! of `.git/config`, and detect an unchanged re-deploy.

use nocode_deploy::github::GitHubConfig;
use nocode_deploy::{
    ComposeConfig, CredentialVault, DeployConfig, DeployError, DeployOptions, DeployService,
    DeploymentStatus, RegistryConfig,
};
use nocode_model::Project;
use std::path::Path;
use std::process::Command;

const BRANCH: &str = "deploy-test";

fn git(args: &[&str], cwd: Option<&Path>) -> String {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let output = command.output().expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn bare_repo(root: &Path) -> String {
    let path = root.join("remote.git");
    let mut command = Command::new("git");
    command.args(["init", "--bare", "--quiet"]).arg(&path);
    let output = command.output().expect("git init --bare");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    path.to_string_lossy().replace('\\', "/")
}

fn config() -> DeployConfig {
    DeployConfig {
        github: GitHubConfig {
            owner: "acme".into(),
            repo: "app".into(),
            branch: BRANCH.into(),
        },
        compose: ComposeConfig {
            project_slug: "local-app".into(),
            registry: RegistryConfig {
                namespace: "acme".into(),
                registry: "docker.io".into(),
                backend_image: "local-app-backend".into(),
                frontend_image: "local-app-frontend".into(),
            },
            api_base: "http://localhost:8081".into(),
        },
        api_base: "http://localhost:8081".into(),
    }
}

#[tokio::test]
async fn pushes_to_a_local_bare_repo_without_leaking_the_token_and_is_idempotent() {
    let dir = tempfile::tempdir().expect("temp dir");
    let remote = bare_repo(dir.path());
    let vault = CredentialVault::from_key(&[3u8; 32]).expect("vault");
    let sealed = vault.seal("ghp_localTestTokenNeverLeaked").expect("seal");
    let service = DeployService::new(vault, dir.path().join("staging"))
        .with_git_remote(&remote)
        .with_github_base_url("http://127.0.0.1:1");
    let project = Project::new("Local App");
    let config = config();

    let first = service
        .deploy(&project, &config, &sealed, DeployOptions::default())
        .await
        .expect("first deploy");
    assert_eq!(first.status, DeploymentStatus::Pushed);
    assert!(!first.deployed());
    assert_eq!(first.commit_sha.len(), 40);

    // The push really landed on the configured branch in the bare repo.
    let remote_tip = git(
        &["rev-parse", &format!("refs/heads/{BRANCH}")],
        Some(Path::new(&remote)),
    );
    assert_eq!(remote_tip, first.commit_sha);

    // No credential anywhere in the bare repository's config or refs.
    let config_file = std::fs::read_to_string(Path::new(&remote).join("config")).unwrap();
    assert!(!config_file.contains("ghp_"));
    assert!(!config_file.contains("x-access-token"));

    // A second deploy of the identical generated tree must be refused as
    // idempotent rather than pushing a redundant commit.
    let second = service
        .deploy(
            &project,
            &config,
            &sealed,
            DeployOptions {
                branch_created_by_deploy: true,
            },
        )
        .await;
    assert!(
        matches!(second, Err(DeployError::Idempotent)),
        "expected Idempotent, got {second:?}"
    );

    let after = git(
        &["rev-parse", &format!("refs/heads/{BRANCH}")],
        Some(Path::new(&remote)),
    );
    assert_eq!(
        after, remote_tip,
        "idempotent deploy must not move the branch"
    );
}

#[tokio::test]
async fn an_existing_branch_that_was_not_created_here_is_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let remote = bare_repo(dir.path());
    let vault = CredentialVault::from_key(&[4u8; 32]).expect("vault");
    let sealed = vault.seal("ghp_localTestTokenNeverLeaked").expect("seal");
    let service = DeployService::new(vault, dir.path().join("staging"))
        .with_git_remote(&remote)
        .with_github_base_url("http://127.0.0.1:1");
    let project = Project::new("Local App");
    let config = config();

    // First deploy establishes the branch and its deployment record.
    service
        .deploy(&project, &config, &sealed, DeployOptions::default())
        .await
        .expect("first deploy");

    // Pretend the record was lost: the remote branch now looks foreign.
    let error = service
        .deploy(&project, &config, &sealed, DeployOptions::default())
        .await
        .expect_err("an unowned remote branch must be refused");
    assert!(
        matches!(error, DeployError::BranchExists(ref b) if b == BRANCH),
        "unexpected error: {error:?}"
    );
}
