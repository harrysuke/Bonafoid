//! Desktop integration service.
//!
//! The Tauri host's counterpart to `ide-server::integrations`. It deliberately
//! shares `nocode-deploy` with the server, so credential encryption, git
//! pushing and CI status have exactly one implementation in the product.
//!
//! # Secret handling
//! A GitHub PAT arrives over the Tauri IPC channel, is moved straight into a
//! `Zeroizing` buffer, sealed with the local vault key, and only the sealed
//! blob is written to disk. No command returns a token, and every error
//! string is passed through `safe_error` before it crosses the IPC boundary.
//! The vault key lives in its own file (`vault.key`); it must never be
//! synced, logged or committed.
//!
//! # Status honesty
//! As on the server, a successful push is `pushed`, not "deployed". Only a
//! workflow run for this exact commit can promote it to `ci_succeeded`.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use nocode_deploy::{
    safe_error, valid_repo_component, ComposeConfig, CredentialVault, DeployConfig, DeployError,
    DeployOptions, DeployService, Deployment, DeploymentStatus, GitHubClient, GitHubConfig,
    GitHubConfigResponse, RegistryConfig, RegistryConfigResponse, SealedCredential,
};
use nocode_ide::IdeService;
use nocode_model::Project;
use serde::{Deserialize, Serialize};
use tauri::State;
use zeroize::Zeroizing;

const KEY_FILE: &str = "vault.key";
const STORE_FILE: &str = "integrations.json";
const DEFAULT_API_BASE: &str = "http://localhost:8081";
const MAX_TOKEN_CHARS: usize = 512;
const MAX_DEPLOYMENTS: usize = 100;

fn default_branch() -> String {
    "main".to_string()
}

fn default_registry() -> String {
    "docker.io".to_string()
}

/// Scrub anything that could carry a token before it reaches JS.
fn clean(error: impl std::fmt::Display) -> String {
    safe_error(&error)
}

fn bounded_token(token: &str) -> Result<Zeroizing<String>, String> {
    let owned = Zeroizing::new(token.trim().to_string());
    if owned.is_empty() || owned.chars().count() > MAX_TOKEN_CHARS {
        return Err("a token is required and must be at most 512 characters".into());
    }
    Ok(owned)
}

/// Everything the desktop persists. All secret-bearing fields are sealed
/// blobs; there is no plaintext token field anywhere in this struct.
///
/// `#[serde(default)]` keeps older files readable when a field is added.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct DeployData {
    github: Option<GitHubConfig>,
    github_sealed: Option<SealedCredential>,
    github_verified: bool,
    registry: Option<RegistryConfig>,
    registry_sealed: Option<SealedCredential>,
    registry_verified: bool,
    /// `(project_id, branch)` pairs this IDE has actually pushed to. An
    /// existing remote branch is refused unless it appears here, so a deploy
    /// can never fast-forward over someone else's work.
    pushed_branches: Vec<(String, String)>,
    /// Newest first.
    deployments: Vec<Deployment>,
}

impl DeployData {
    fn require_ready(&self) -> Result<(), String> {
        if self.github.is_none() || self.github_sealed.is_none() || !self.github_verified {
            return Err("configure and verify the GitHub integration first".into());
        }
        if self.registry.is_none() || !self.registry_verified {
            return Err("configure the Docker Hub integration first".into());
        }
        Ok(())
    }

    fn config(&self, api_base: String) -> Result<DeployConfig, String> {
        Ok(DeployConfig {
            github: self
                .github
                .clone()
                .ok_or("configure the GitHub integration first")?,
            compose: ComposeConfig {
                project_slug: String::new(),
                registry: self.registry.clone().unwrap_or_default(),
                api_base: api_base.clone(),
            },
            api_base,
        })
    }
}

/// Managed Tauri state. `service` is `None` when the local vault could not be
/// opened; the rest of the IDE stays usable and every deploy command returns
/// the stored, redacted reason.
pub struct DeployState {
    data: Mutex<DeployData>,
    service: Option<DeployService>,
    store_path: PathBuf,
    setup_error: Option<String>,
}

impl DeployState {
    /// Build the state from the app-data directory. Called from `setup`,
    /// before the webview can invoke a command.
    pub fn init(app_dir: &Path) -> Self {
        let _ = std::fs::create_dir_all(app_dir);
        let store_path = app_dir.join(STORE_FILE);
        let data = read_store(&store_path);
        let (service, setup_error) = match CredentialVault::from_key_file(&app_dir.join(KEY_FILE)) {
            Ok(vault) => (
                Some(DeployService::new(vault, app_dir.join("staging"))),
                None,
            ),
            Err(error) => (
                None,
                Some(format!(
                    "deployments are unavailable: the local credential vault could not be opened ({})",
                    clean(error)
                )),
            ),
        };
        Self {
            data: Mutex::new(data),
            service,
            store_path,
            setup_error,
        }
    }

    fn data(&self) -> Result<MutexGuard<'_, DeployData>, String> {
        self.data
            .lock()
            .map_err(|_| "deployment state is unavailable".to_string())
    }

    fn service(&self) -> Result<&DeployService, String> {
        self.service.as_ref().ok_or_else(|| {
            self.setup_error
                .clone()
                .unwrap_or_else(|| "deployments are unavailable".to_string())
        })
    }

    /// Atomic write: a crash mid-write must not leave a half-parsed config.
    /// Only sealed blobs are written, but the file is still user-scoped.
    fn persist(&self) -> Result<(), String> {
        let json = {
            let data = self.data()?;
            serde_json::to_vec_pretty(&*data).map_err(clean)?
        };
        let tmp = self.store_path.with_extension("json.tmp");
        std::fs::write(&tmp, &json).map_err(clean)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &self.store_path).map_err(clean)?;
        Ok(())
    }
}

fn read_store(path: &Path) -> DeployData {
    // A missing or unreadable store is not fatal: the user simply has to
    // configure the integrations again. Never abort app startup over it.
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// What the UI gets for one deployment. `deployed` and `ci_state` are derived
/// from the deploy crate's record, so the desktop cannot disagree with CI.
#[derive(Debug, Serialize)]
pub struct DeploymentView {
    pub id: String,
    pub project_id: String,
    pub commit_sha: String,
    pub status: DeploymentStatus,
    pub deployed: bool,
    pub ci_state: &'static str,
    pub github_run_id: Option<u64>,
    pub github_run_url: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&Deployment> for DeploymentView {
    fn from(deployment: &Deployment) -> Self {
        Self {
            id: deployment.id.to_string(),
            project_id: deployment.project_id.clone(),
            commit_sha: deployment.commit_sha.clone(),
            status: deployment.status,
            deployed: deployment.deployed(),
            ci_state: deployment.ci_state(),
            github_run_id: deployment.github_run_id,
            github_run_url: deployment.github_run_url.clone(),
            error_message: deployment.error_message.clone(),
            created_at: deployment.created_at.to_rfc3339(),
            updated_at: deployment.updated_at.to_rfc3339(),
        }
    }
}

fn current_project(ide: &IdeService) -> Result<Project, String> {
    let json = ide
        .get_current_project()?
        .ok_or_else(|| "open a project before deploying".to_string())?;
    serde_json::from_str(&json).map_err(|e| format!("the current project is invalid: {e}"))
}

/// `GET`-equivalent: redacted GitHub config, never a token.
#[tauri::command]
pub async fn github_get_config(
    state: State<'_, DeployState>,
) -> Result<GitHubConfigResponse, String> {
    let data = state.data()?;
    let Some(config) = data.github.as_ref() else {
        return Ok(GitHubConfigResponse::absent());
    };
    let mut response = GitHubConfigResponse::from(config);
    if data.github_verified {
        response = response.verified();
    }
    Ok(response)
}

/// Validate, seal, verify against GitHub, then persist.
#[tauri::command]
pub async fn github_set_config(
    owner: String,
    repo: String,
    branch: Option<String>,
    token: String,
    state: State<'_, DeployState>,
) -> Result<GitHubConfigResponse, String> {
    let service = state.service()?;
    let token = bounded_token(&token)?;
    let config = GitHubConfig {
        owner,
        repo,
        branch: branch.unwrap_or_else(default_branch),
    };
    config.validate().map_err(clean)?;

    let sealed = service.vault().seal(token.as_str()).map_err(clean)?;

    // Verify before persisting: a token with no access to the repo must not
    // leave a "verified" row behind.
    let client = GitHubClient::with_base_url(
        token.as_str().to_string(),
        config.clone(),
        service.github_base_url(),
    )
    .map_err(clean)?;
    client.verify_repo().await.map_err(clean)?;

    {
        let mut data = state.data()?;
        data.github = Some(config.clone());
        data.github_sealed = Some(sealed);
        data.github_verified = true;
    }
    state.persist()?;
    Ok(GitHubConfigResponse::from(&config).verified())
}

/// `GET`-equivalent: redacted registry config, never a token.
#[tauri::command]
pub async fn dockerhub_get_config(
    state: State<'_, DeployState>,
) -> Result<RegistryConfigResponse, String> {
    let data = state.data()?;
    let Some(config) = data.registry.as_ref() else {
        return Ok(RegistryConfigResponse::absent());
    };
    let mut response = RegistryConfigResponse::from(config);
    if data.registry_verified {
        response = response.verified();
    }
    Ok(response)
}

/// Store-only configuration. Omit `token` on update to keep the sealed one.
#[tauri::command]
pub async fn dockerhub_set_config(
    namespace: String,
    registry: Option<String>,
    token: Option<String>,
    state: State<'_, DeployState>,
) -> Result<RegistryConfigResponse, String> {
    let service = state.service()?;
    let config = RegistryConfig {
        namespace,
        registry: registry.unwrap_or_else(default_registry),
        // Derived from the project slug at deploy time, never from input.
        backend_image: String::new(),
        frontend_image: String::new(),
    };
    if !valid_repo_component(&config.namespace) || !valid_repo_component(&config.registry) {
        return Err("namespace and registry must be plain identifiers".into());
    }

    let sealed = match token {
        Some(raw) if !raw.trim().is_empty() => {
            let owned = bounded_token(&raw)?;
            Some(service.vault().seal(owned.as_str()).map_err(clean)?)
        }
        _ => state.data()?.registry_sealed.clone(),
    };
    let sealed = sealed
        .ok_or_else(|| "a Docker Hub token is required on first configuration".to_string())?;

    {
        let mut data = state.data()?;
        data.registry = Some(config.clone());
        data.registry_sealed = Some(sealed);
        data.registry_verified = true;
    }
    state.persist()?;
    Ok(RegistryConfigResponse::from(&config).verified())
}

/// Generate, commit and push the currently open project.
#[tauri::command]
pub async fn deploy_project(
    api_base: Option<String>,
    state: State<'_, DeployState>,
    ide: State<'_, IdeService>,
) -> Result<DeploymentView, String> {
    let service = state.service()?;
    let project = current_project(&ide)?;
    let project_id = project.id.0.to_string();
    let api_base = api_base.unwrap_or_else(|| DEFAULT_API_BASE.to_string());
    // 15.4: this value is baked into the generated frontend at build time.
    // Refuse anything that could smuggle a credential into a generated file
    // or change the meaning of a template.
    if !nocode_deploy::valid_api_base(&api_base) {
        return Err(
            "the public API base URL must be a plain absolute http(s) URL".to_string(),
        );
    }

    // Scoped so no MutexGuard is held across an await.
    let (config, sealed, branch_created) = {
        let data = state.data()?;
        data.require_ready()?;
        let config = data.config(api_base)?;
        let sealed = data
            .github_sealed
            .clone()
            .ok_or_else(|| "the GitHub credential is missing".to_string())?;
        let branch_created = data
            .pushed_branches
            .contains(&(project_id.clone(), config.github.branch.clone()));
        (config, sealed, branch_created)
    };
    let branch = config.github.branch.clone();

    let deployment = match service
        .deploy(
            &project,
            &config,
            &sealed,
            DeployOptions {
                branch_created_by_deploy: branch_created,
            },
        )
        .await
    {
        Ok(deployment) => deployment,
        Err(DeployError::Idempotent) => {
            // The remote already has exactly this generated tree.
            let data = state.data()?;
            return data
                .deployments
                .iter()
                .find(|d| d.project_id == project_id)
                .map(DeploymentView::from)
                .ok_or_else(|| "this generated commit is already deployed".to_string());
        }
        Err(error) => return Err(clean(error)),
    };

    {
        let mut data = state.data()?;
        let pair = (deployment.project_id.clone(), branch);
        if !data.pushed_branches.contains(&pair) {
            data.pushed_branches.push(pair);
        }
        data.deployments.insert(0, deployment.clone());
        data.deployments.truncate(MAX_DEPLOYMENTS);
    }
    state.persist()?;
    Ok(DeploymentView::from(&deployment))
}

/// Deployment history for the current desktop user, newest first.
#[tauri::command]
pub async fn list_deployments(
    state: State<'_, DeployState>,
) -> Result<Vec<DeploymentView>, String> {
    let data = state.data()?;
    Ok(data.deployments.iter().map(DeploymentView::from).collect())
}

/// Poll GitHub for one deployment. Only a run for this exact commit counts.
#[tauri::command]
pub async fn refresh_deployment(
    id: String,
    state: State<'_, DeployState>,
) -> Result<DeploymentView, String> {
    let service = state.service()?;
    let (config, sealed, mut deployment) = {
        let data = state.data()?;
        let deployment = data
            .deployments
            .iter()
            .find(|d| d.id.to_string() == id)
            .cloned()
            .ok_or_else(|| "unknown deployment".to_string())?;
        let config = data.config(DEFAULT_API_BASE.to_string())?;
        let sealed = data
            .github_sealed
            .clone()
            .ok_or_else(|| "the GitHub credential is missing".to_string())?;
        (config, sealed, deployment)
    };

    service
        .check_status(&config, &sealed, &mut deployment)
        .await
        .map_err(clean)?;

    {
        let mut data = state.data()?;
        if let Some(slot) = data.deployments.iter_mut().find(|d| d.id == deployment.id) {
            *slot = deployment.clone();
        }
    }
    state.persist()?;
    Ok(DeploymentView::from(&deployment))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PAT: &str = "ghp_thisIsAFakeTokenForTestsOnly123";

    fn sample_deployment(status: DeploymentStatus) -> Deployment {
        let now = chrono::Utc::now();
        Deployment {
            id: uuid::Uuid::new_v4(),
            project_id: "project-1".into(),
            commit_sha: "0123456789abcdef0123456789abcdef01234567".into(),
            status,
            created_at: now,
            updated_at: now,
            github_run_id: None,
            github_run_url: None,
            error_message: None,
        }
    }

    #[test]
    fn a_push_is_not_reported_as_deployed() {
        let view = DeploymentView::from(&sample_deployment(DeploymentStatus::Pushed));
        assert!(!view.deployed);
        assert_eq!(view.ci_state, "not_started");
        assert_eq!(view.status, DeploymentStatus::Pushed);
        let succeeded = DeploymentView::from(&sample_deployment(DeploymentStatus::CiSucceeded));
        assert!(succeeded.deployed);
        assert_eq!(succeeded.ci_state, "succeeded");
    }

    #[test]
    fn token_bounds_reject_empty_and_oversized_input() {
        assert!(bounded_token("").is_err());
        assert!(bounded_token("   ").is_err());
        assert!(bounded_token(&"x".repeat(MAX_TOKEN_CHARS + 1)).is_err());
        assert_eq!(bounded_token(TEST_PAT).unwrap().as_str(), TEST_PAT);
    }

    #[test]
    fn the_key_file_is_created_once_and_reused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = DeployState::init(dir.path());
        assert!(first.service().is_ok(), "{:?}", first.setup_error);
        let key = std::fs::read_to_string(dir.path().join(KEY_FILE)).expect("key file");
        assert_eq!(key.len(), 64, "key is a 32-byte hex string");

        let second = DeployState::init(dir.path());
        let key_again = std::fs::read_to_string(dir.path().join(KEY_FILE)).expect("key file");
        assert_eq!(key, key_again, "a restart must reuse the same master key");
        assert!(second.service().is_ok());
    }

    #[test]
    fn persisted_store_contains_sealed_blobs_but_never_plaintext() {
        let dir = tempfile::tempdir().expect("temp dir");
        let state = DeployState::init(dir.path());
        let sealed = state
            .service()
            .expect("service")
            .vault()
            .seal(TEST_PAT)
            .expect("seal");
        {
            let mut data = state.data().expect("data");
            data.github = Some(GitHubConfig {
                owner: "acme".into(),
                repo: "app".into(),
                branch: "main".into(),
            });
            data.github_sealed = Some(sealed);
            data.github_verified = true;
            data.deployments
                .push(sample_deployment(DeploymentStatus::Pushed));
        }
        state.persist().expect("persist");

        let raw = std::fs::read_to_string(dir.path().join(STORE_FILE)).expect("store");
        assert!(!raw.contains("ghp_"), "plaintext token written to disk");
        assert!(raw.contains("ciphertext"));
        assert!(raw.contains("pushed"));

        // The state reloads from the same file with its history intact.
        let reloaded = DeployState::init(dir.path());
        let data = reloaded.data().expect("data");
        assert_eq!(data.deployments.len(), 1);
        assert!(data.github_verified);
    }

    #[test]
    fn a_corrupt_store_degrades_to_an_unconfigured_state() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join(STORE_FILE), b"not json").expect("write");
        let state = DeployState::init(dir.path());
        assert!(state.service().is_ok(), "the vault still works");
        assert!(state.data().expect("data").github.is_none());
    }
}
