//! Deployment and integration service for the No-Code IDE.
//!
//! Handles GitHub and Docker Hub connections, credential encryption,
//! generated code deployment, and CI/CD orchestration.

pub mod compose;
pub mod deploy;
pub mod git;
pub mod github;
pub mod vault;

pub use compose::ComposeConfig;
pub use deploy::{DeployConfig, DeployService};
pub use git::GitService;
pub use github::{GitHubClient, GitHubConfig};
pub use vault::{CredentialVault, VaultError};
