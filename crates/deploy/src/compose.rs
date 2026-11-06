//! Docker Compose integration for deployment.
//!
//! Both the web and desktop hosts must agree on image names and build
//! commands, so this module is the single source of truth.

use serde::{Deserialize, Serialize};

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

/// Redacted version for API responses.
#[derive(Debug, Clone, Serialize)]
pub struct RegistryConfigResponse {
    pub namespace: String,
    pub registry: String,
    pub backend_image: String,
    pub frontend_image: String,
}

impl From<&RegistryConfig> for RegistryConfigResponse {
    fn from(config: &RegistryConfig) -> Self {
        Self {
            namespace: config.namespace.clone(),
            registry: config.registry.clone(),
            backend_image: config.backend_image.clone(),
            frontend_image: config.frontend_image.clone(),
        }
    }
}
