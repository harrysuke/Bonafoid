//! Docker Compose integration for deployment.
//!
//! Both the web and desktop hosts must agree on image names and build
//! commands, so this module is the single source of truth. It also checks
//! the *generated* files before a push, because a workflow that pushes an
//! image name the compose file never builds fails only minutes later on a
//! GitHub runner.

use crate::valid_branch;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The exact trigger block emitted by `codegen::generate_deploy_workflow`.
/// Patching fails closed if that shape ever changes, rather than silently
/// pushing to a branch CI will never build.
const MAIN_TRIGGER: &str = "  push:\n    branches: [main]\n";

#[derive(Debug, thiserror::Error)]
pub enum ComposeError {
    #[error("generated deployment file is missing: {0}")]
    MissingFile(String),
    #[error("generated deployment files disagree: {0}")]
    Mismatch(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid deploy branch")]
    InvalidBranch,
    #[error("generated workflow trigger has an unexpected shape; refusing to patch")]
    UnexpectedTrigger,
}

/// Docker Hub / registry configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub namespace: String,
    pub registry: String,
    pub backend_image: String,
    pub frontend_image: String,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            namespace: String::new(),
            registry: "docker.io".to_string(),
            backend_image: String::new(),
            frontend_image: String::new(),
        }
    }
}

impl RegistryConfig {
    /// Fill empty image names from the project slug, the same names codegen
    /// writes into the compose file.
    pub fn with_slug(mut self, slug: &str) -> Self {
        if self.backend_image.is_empty() {
            self.backend_image = format!("{slug}-backend");
        }
        if self.frontend_image.is_empty() {
            self.frontend_image = format!("{slug}-frontend");
        }
        self
    }

    pub fn validate(&self) -> Result<(), ComposeError> {
        if self.namespace.is_empty()
            || !crate::valid_repo_component(&self.namespace)
            || !crate::valid_repo_component(&self.registry)
            || !crate::valid_repo_component(&self.backend_image)
            || !crate::valid_repo_component(&self.frontend_image)
        {
            return Err(ComposeError::Mismatch(
                "registry, namespace and image names must be plain identifiers".into(),
            ));
        }
        Ok(())
    }
}

/// Full compose configuration assembled from project + user inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeConfig {
    pub project_slug: String,
    pub registry: RegistryConfig,
    pub api_base: String,
}

impl ComposeConfig {
    /// The fully-qualified backend image name for `docker push`.
    pub fn backend_image_fqn(&self) -> String {
        format!(
            "{}/{}/{}",
            self.registry.registry, self.registry.namespace, self.registry.backend_image
        )
    }

    /// The fully-qualified frontend image name for `docker push`.
    pub fn frontend_image_fqn(&self) -> String {
        format!(
            "{}/{}/{}",
            self.registry.registry, self.registry.namespace, self.registry.frontend_image
        )
    }

    /// Local image names as they appear in the generated compose file.
    pub fn backend_image_local(&self) -> String {
        format!("{}-backend", self.project_slug)
    }

    pub fn frontend_image_local(&self) -> String {
        format!("{}-frontend", self.project_slug)
    }
}

/// Replace the generated workflow's `main` trigger with the user's chosen
/// branch. The branch is validated first, so no YAML/shell injection is
/// possible through the substitution.
pub fn patch_workflow_branch(workflow: &str, branch: &str) -> Result<String, ComposeError> {
    if !valid_branch(branch) {
        return Err(ComposeError::InvalidBranch);
    }
    if !workflow.contains(MAIN_TRIGGER) {
        return Err(ComposeError::UnexpectedTrigger);
    }
    Ok(workflow.replacen(
        MAIN_TRIGGER,
        &format!("  push:\n    branches: ['{branch}']\n"),
        1,
    ))
}

/// Verify that the staged generated tree agrees with the image names and
/// commands this service will rely on. Call AFTER the workflow branch patch.
pub fn verify_generated(stage: &Path, config: &ComposeConfig) -> Result<(), ComposeError> {
    let compose = read_required(stage, "docker/docker-compose.yml")?;
    let workflow = read_required(stage, ".github/workflows/deploy.yml")?;

    let backend = config.backend_image_local();
    let frontend = config.frontend_image_local();

    require(
        &compose,
        &format!("image: {backend}"),
        "compose backend image name",
    )?;
    require(
        &compose,
        &format!("image: {frontend}"),
        "compose frontend image name",
    )?;
    require(
        &workflow,
        &format!("BACKEND_IMAGE: {backend}"),
        "workflow BACKEND_IMAGE",
    )?;
    require(
        &workflow,
        &format!("FRONTEND_IMAGE: {frontend}"),
        "workflow FRONTEND_IMAGE",
    )?;
    require(
        &workflow,
        "docker compose -f docker/docker-compose.yml build backend",
        "workflow backend build command",
    )?;
    // The tag steps are what make `docker push <FQN>` possible at all: the
    // compose file only builds locally-named images. The namespace is a
    // GitHub secret, so the FQN cannot appear literally in the workflow —
    // verify the tag/push template instead.
    require(
        &workflow,
        "docker tag ${{ env.BACKEND_IMAGE }} ${{ env.REGISTRY }}/${{ secrets.DOCKERHUB_USERNAME }}/${{ env.BACKEND_IMAGE }}:latest",
        "workflow backend tag step",
    )?;
    require(
        &workflow,
        "docker tag ${{ env.FRONTEND_IMAGE }} ${{ env.REGISTRY }}/${{ secrets.DOCKERHUB_USERNAME }}/${{ env.FRONTEND_IMAGE }}:latest",
        "workflow frontend tag step",
    )?;
    require(
        &workflow,
        "docker push ${{ env.REGISTRY }}/${{ secrets.DOCKERHUB_USERNAME }}/${{ env.BACKEND_IMAGE }}:latest",
        "workflow backend push step",
    )?;
    require(
        &workflow,
        "docker push ${{ env.REGISTRY }}/${{ secrets.DOCKERHUB_USERNAME }}/${{ env.FRONTEND_IMAGE }}:latest",
        "workflow frontend push step",
    )?;
    // The config still has to name the same local images codegen emits.
    if config.registry.backend_image != backend || config.registry.frontend_image != frontend {
        return Err(ComposeError::Mismatch(
            "configured image names do not match the generated compose file".into(),
        ));
    }
    Ok(())
}

fn read_required(stage: &Path, relative: &str) -> Result<String, ComposeError> {
    std::fs::read_to_string(stage.join(relative))
        .map_err(|_| ComposeError::MissingFile(relative.to_string()))
}

fn require(haystack: &str, needle: &str, label: &str) -> Result<(), ComposeError> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(ComposeError::Mismatch(format!(
            "{label} does not contain `{needle}`"
        )))
    }
}

/// Redacted version for API responses.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RegistryConfigResponse {
    pub configured: bool,
    pub namespace: String,
    pub registry: String,
    pub backend_image: String,
    pub frontend_image: String,
    pub verified: bool,
}

impl From<&RegistryConfig> for RegistryConfigResponse {
    fn from(config: &RegistryConfig) -> Self {
        Self {
            configured: true,
            namespace: config.namespace.clone(),
            registry: config.registry.clone(),
            backend_image: config.backend_image.clone(),
            frontend_image: config.frontend_image.clone(),
            verified: false,
        }
    }
}

impl RegistryConfigResponse {
    pub fn verified(mut self) -> Self {
        self.verified = true;
        self
    }

    /// A response for a user who has not configured a registry yet.
    pub fn absent() -> Self {
        Self {
            registry: "docker.io".to_string(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ComposeConfig {
        ComposeConfig {
            project_slug: "my-app".into(),
            registry: RegistryConfig {
                namespace: "acme".into(),
                ..RegistryConfig::default()
            }
            .with_slug("my-app"),
            api_base: "https://api.example.com".into(),
        }
    }

    #[test]
    fn fqn_and_local_names_agree_with_codegen() {
        let c = config();
        assert_eq!(c.backend_image_local(), "my-app-backend");
        assert_eq!(c.frontend_image_local(), "my-app-frontend");
        assert_eq!(c.backend_image_fqn(), "docker.io/acme/my-app-backend");
    }

    #[test]
    fn patch_replaces_only_the_trigger() {
        let workflow = format!("name: Deploy\n\non:\n{MAIN_TRIGGER}  workflow_dispatch:\n");
        let patched = patch_workflow_branch(&workflow, "release/2").unwrap();
        assert!(patched.contains("branches: ['release/2']"));
        assert!(patched.contains("workflow_dispatch:"));
        assert!(!patched.contains("branches: [main]"));
    }

    /// The verifier is only useful if it agrees with what codegen actually
    /// emits. Generate a real project tree and check the whole contract,
    /// including the workflow branch patch, against it.
    #[tokio::test]
    async fn generated_tree_satisfies_the_deploy_contract() {
        let project = nocode_model::Project::new("Contract Check");
        let stage = tempfile::tempdir().expect("temp staging dir");
        nocode_codegen::project::generate_project(&project, stage.path())
            .await
            .expect("generate project");

        let workflow_path = stage.path().join(".github/workflows/deploy.yml");
        let workflow = std::fs::read_to_string(&workflow_path).expect("deploy workflow");
        std::fs::write(
            &workflow_path,
            patch_workflow_branch(&workflow, "release/2").expect("patch trigger"),
        )
        .expect("write patched workflow");

        let config = ComposeConfig {
            project_slug: project.slug.clone(),
            registry: RegistryConfig {
                namespace: "acme".into(),
                ..RegistryConfig::default()
            }
            .with_slug(&project.slug),
            api_base: "https://api.example.com".into(),
        };
        verify_generated(stage.path(), &config).expect("generated files must agree");

        // A configuration that disagrees with the generated tree must be
        // refused locally, not on a GitHub runner minutes later.
        let mut wrong = config.clone();
        wrong.registry.backend_image = "some-other-backend".into();
        assert!(verify_generated(stage.path(), &wrong).is_err());
    }

    #[test]
    fn patch_fails_closed_on_injection_and_unknown_shape() {
        assert!(matches!(
            patch_workflow_branch("on:\n  push:\n    branches: [main]\n", "main'; echo pwned"),
            Err(ComposeError::InvalidBranch)
        ));
        assert!(matches!(
            patch_workflow_branch("on:\n  push:\n    branches: ['**']\n", "main"),
            Err(ComposeError::UnexpectedTrigger)
        ));
    }
}
