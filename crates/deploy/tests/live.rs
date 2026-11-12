//! Opt-in live GitHub integration tests.
//!
//! Every test here is `#[ignore]`d. Nothing runs in CI, and no credential is
//! ever supplied by this repository. To run one, **export** the variables in
//! your own shell first — do not inline them on the cargo command line, where
//! they would land in shell history and in `ps` output:
//!
//! ```text
//!   export NOCODE_LIVE_GITHUB_PAT=<your fine-grained PAT>
//!   export NOCODE_LIVE_GITHUB_OWNER=<owner>
//!   export NOCODE_LIVE_GITHUB_REPO=<repo>
//!
//!   cargo test -p nocode-deploy --test live live_github_api_accepts_the_pat -- --ignored --nocapture
//! ```
//!
//! The push test additionally needs a **dedicated scratch branch** and a
//! Docker Hub namespace, and it really does push generated code to that
//! branch:
//!
//! ```text
//!   export NOCODE_LIVE_DEPLOY_BRANCH=nocode-ide-live-check
//!   export NOCODE_LIVE_DOCKERHUB_NAMESPACE=<namespace>
//!
//!   cargo test -p nocode-deploy --test live live_deploy_pushes_a_dedicated_branch -- --ignored --nocapture
//! ```
//!
//! # What is asserted
//! * The PAT really authenticates against `api.github.com` and can read the
//!   repository and its deploy workflow runs.
//! * A real `git push` succeeds with no credential in the remote URL, no
//!   `--force`, and returns `pushed` — never "deployed".
//! * Re-deploying an unchanged project is recognised as idempotent against
//!   the real remote instead of creating a second commit.
//! * No surfaced error string ever contains the PAT.
//!
//! CI on the scratch branch will normally fail unless the repository has the
//! `DOCKERHUB_USERNAME` / `DOCKERHUB_TOKEN` secrets. That is expected: these
//! tests verify the push, not the image build.

use nocode_deploy::{
    safe_error, valid_branch, ComposeConfig, CredentialVault, DeployConfig, DeployError,
    DeployOptions, DeployService, DeploymentStatus, GitHubClient, GitHubConfig, RegistryConfig,
    SealedCredential,
};
use nocode_model::Project;
use zeroize::Zeroizing;

const PAT: &str = "NOCODE_LIVE_GITHUB_PAT";
const OWNER: &str = "NOCODE_LIVE_GITHUB_OWNER";
const REPO: &str = "NOCODE_LIVE_GITHUB_REPO";
const BRANCH: &str = "NOCODE_LIVE_DEPLOY_BRANCH";
const NAMESPACE: &str = "NOCODE_LIVE_DOCKERHUB_NAMESPACE";

const HOW_TO: &str =
    "export NOCODE_LIVE_* in your shell (never inline a token on the cargo command line)";

/// Read a normal setting without ever echoing its value.
fn required(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => panic!("missing {name}: {HOW_TO}"),
    }
}

/// Read the PAT into memory that is wiped on drop.
fn secret(name: &str) -> Zeroizing<String> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Zeroizing::new(value.trim().to_string()),
        _ => panic!("missing {name}: {HOW_TO}"),
    }
}

fn live_config(branch: Option<String>) -> GitHubConfig {
    let config = GitHubConfig {
        owner: required(OWNER),
        repo: required(REPO),
        branch: branch.unwrap_or_else(|| "main".to_string()),
    };
    assert!(
        config.validate().is_ok(),
        "invalid owner/repo/branch in NOCODE_LIVE_*"
    );
    config
}

/// Panic with a redacted reason. No untrusted error reaches the console
/// without passing through `safe_error` first.
fn ok<T, E: std::fmt::Display>(result: Result<T, E>, what: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{what} failed: {}", safe_error(&error)),
    }
}

fn assert_no_token(token: &str, text: &str, label: &str) {
    assert!(!text.contains(token), "{label} leaked the live credential");
}

/// Read-only: prove the credential works against the real GitHub API.
#[tokio::test]
#[ignore = "requires a real GitHub PAT; export NOCODE_LIVE_* in your shell"]
async fn live_github_api_accepts_the_pat() {
    let token = secret(PAT);
    let config = live_config(None);

    let client = ok(
        GitHubClient::new(token.as_str().to_string(), config.clone()),
        "building the GitHub client",
    );
    let repo = ok(client.verify_repo().await, "verifying the repository");
    assert_eq!(
        repo.full_name.to_ascii_lowercase(),
        format!("{}/{}", config.owner, config.repo).to_ascii_lowercase(),
        "the PAT authenticated against a different repository"
    );
    assert_no_token(token.as_str(), &repo.html_url, "repository URL");

    let runs = ok(
        client.list_workflow_runs().await,
        "listing deploy workflow runs",
    );
    println!(
        "live: verified {}/{} — {} recent deploy workflow run(s)",
        config.owner,
        config.repo,
        runs.len()
    );
}

/// Real push to a dedicated branch, plus real idempotency against GitHub.
#[tokio::test]
#[ignore = "pushes generated code to NOCODE_LIVE_DEPLOY_BRANCH; opt-in only"]
async fn live_deploy_pushes_a_dedicated_branch() {
    let token = secret(PAT);
    let branch = required(BRANCH);
    assert!(valid_branch(&branch), "invalid deploy branch: {branch}");
    assert!(
        !matches!(
            branch.to_ascii_lowercase().as_str(),
            "main" | "master" | "develop" | "release" | "trunk"
        ),
        "refusing to push to the protected branch `{branch}`; use a dedicated scratch branch"
    );
    let namespace = required(NAMESPACE);
    let config = live_config(Some(branch.clone()));

    // Confirm access before generating or pushing anything.
    let client = ok(
        GitHubClient::new(token.as_str().to_string(), config.clone()),
        "building the GitHub client",
    );
    ok(client.verify_repo().await, "verifying the repository");

    let dir = tempfile::tempdir().expect("temporary directory");
    let vault = ok(
        CredentialVault::from_key_file(&dir.path().join("vault.key")),
        "opening an ephemeral vault",
    );
    let sealed: SealedCredential = ok(vault.seal(token.as_str()), "sealing the PAT");
    let service = DeployService::new(vault, dir.path().join("staging"));

    let project = Project::new("No-Code IDE Live Deploy Check");
    let api_base = "https://api.example.com".to_string();
    let deploy_config = DeployConfig {
        github: config.clone(),
        compose: ComposeConfig {
            project_slug: project.slug.clone(),
            registry: RegistryConfig {
                namespace,
                registry: "docker.io".into(),
                backend_image: String::new(),
                frontend_image: String::new(),
            },
            api_base: api_base.clone(),
        },
        api_base,
    };

    // First run: the branch must not exist yet. If a previous run of this
    // same test created it, the branch genuinely belongs to the IDE and a
    // fast-forward child commit is allowed.
    let first = match service
        .deploy(
            &project,
            &deploy_config,
            &sealed,
            DeployOptions {
                branch_created_by_deploy: false,
            },
        )
        .await
    {
        Ok(deployment) => deployment,
        Err(DeployError::BranchExists(_)) => ok(
            service
                .deploy(
                    &project,
                    &deploy_config,
                    &sealed,
                    DeployOptions {
                        branch_created_by_deploy: true,
                    },
                )
                .await,
            "re-deploying to the branch this test created earlier",
        ),
        Err(error) => {
            let safe = safe_error(&error);
            assert_no_token(token.as_str(), &safe, "deploy error");
            panic!("live deploy failed: {safe}");
        }
    };

    assert_eq!(
        first.status,
        DeploymentStatus::Pushed,
        "push must be honest"
    );
    assert!(!first.deployed(), "a push is not a deployment");
    assert_eq!(first.commit_sha.len(), 40, "expected a full git object id");

    // The same project produces the same tree, so a repeat deploy must be
    // detected against the real remote without a second commit.
    let second = service
        .deploy(
            &project,
            &deploy_config,
            &sealed,
            DeployOptions {
                branch_created_by_deploy: true,
            },
        )
        .await;
    assert!(
        matches!(second, Err(DeployError::Idempotent)),
        "an unchanged re-deploy should be idempotent, got {second:?}"
    );

    // Polling must not invent a CI outcome. Any attached run has to be for
    // this exact commit (checked inside the deploy crate).
    let mut record = first.clone();
    ok(
        service
            .check_status(&deploy_config, &sealed, &mut record)
            .await,
        "refreshing CI status",
    );
    if record.github_run_id.is_some() {
        assert_ne!(
            record.status,
            DeploymentStatus::Pushed,
            "a matching CI run must change the status"
        );
    }

    println!(
        "live: pushed {} to {}/{} on `{}` (status: {:?}, ci: {})",
        first.commit_sha,
        config.owner,
        config.repo,
        branch,
        record.status,
        record.ci_state()
    );
}
