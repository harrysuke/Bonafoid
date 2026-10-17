//! Plugin manifest parsing and validation.
//!
//! A manifest describes a plugin's identity, the WebAssembly module that
//! implements it, the permissions it requests, and the entry points it
//! contributes to the IDE.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{PluginError, Result};

pub use nocode_model::{
    PluginConfig, PluginDependency, PluginEntryPoint, PluginEntryPointType,
    PluginPermission as Permission,
};

/// The plugin API version implemented by this crate. Manifests targeting a
/// different major version are rejected.
pub const API_VERSION: &str = "1.0";

/// A parsed `plugin.json` / `plugin.toml` manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginManifest {
    /// Stable, reverse-DNS style identifier (e.g. `com.acme.charts`).
    pub id: String,
    /// Human readable name.
    pub name: String,
    /// Semantic version of the plugin.
    pub version: String,
    /// Plugin API version this plugin is built against.
    #[serde(default = "default_api_version")]
    pub api_version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    /// Path to the WebAssembly component, relative to the manifest.
    #[serde(default = "default_entry_point")]
    pub entry_point: String,
    /// Requested permissions.
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// Entry points contributed by this plugin.
    #[serde(default)]
    pub entry_points: Vec<PluginEntryPoint>,
    /// Other plugins this plugin depends on.
    #[serde(default)]
    pub dependencies: Vec<PluginDependency>,
    /// Exported function names (documentation / early validation).
    #[serde(default)]
    pub exports: Vec<String>,
    /// Resource limits requested by the plugin.
    #[serde(default)]
    pub resources: PluginResources,
}

fn default_api_version() -> String {
    API_VERSION.to_string()
}

fn default_entry_point() -> String {
    "plugin.wasm".to_string()
}

/// Optional overrides for the sandbox limits applied when running a plugin.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct PluginResources {
    /// Instruction budget (fuel). `None` uses the host default.
    #[serde(default)]
    pub fuel: Option<u64>,
    /// Maximum linear memory in mebibytes.
    #[serde(default)]
    pub memory_mb: Option<u32>,
}

impl Default for PluginResources {
    fn default() -> Self {
        Self {
            fuel: None,
            memory_mb: None,
        }
    }
}

impl PluginManifest {
    /// Parse a manifest from JSON.
    pub fn from_json(json: &str) -> Result<Self> {
        let manifest: PluginManifest = serde_json::from_str(json)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Parse a manifest from TOML.
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        let manifest: PluginManifest = toml::from_str(toml_str)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Load a manifest from a file, dispatching on the extension.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path).map_err(|source| PluginError::Io {
            path: path.display().to_string(),
            source,
        })?;

        match path.extension().and_then(|e| e.to_str()) {
            Some("toml") => Self::from_toml(&contents),
            _ => Self::from_json(&contents),
        }
    }

    /// Serialize this manifest to pretty JSON.
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// The directory containing the manifest file.
    pub fn resolve_wasm_path(&self, manifest_path: impl AsRef<Path>) -> PathBuf {
        let manifest_path = manifest_path.as_ref();
        let base = manifest_path.parent().unwrap_or_else(|| Path::new("."));
        base.join(&self.entry_point)
    }

    /// Validate the manifest, returning a descriptive error on failure.
    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            return Err(PluginError::Validation("id must not be empty".into()));
        }
        if !is_valid_id(&self.id) {
            return Err(PluginError::Validation(format!(
                "invalid id `{}`: expected an alphanumeric/dash/dot identifier",
                self.id
            )));
        }
        if self.name.trim().is_empty() {
            return Err(PluginError::Validation("name must not be empty".into()));
        }
        if !is_valid_semver(&self.version) {
            return Err(PluginError::Validation(format!(
                "invalid version `{}`: expected MAJOR.MINOR.PATCH",
                self.version
            )));
        }
        if !self.api_version.starts_with("1.") {
            return Err(PluginError::Validation(format!(
                "unsupported api_version `{}`: this host implements {API_VERSION}.x",
                self.api_version
            )));
        }
        if self.entry_point.trim().is_empty() {
            return Err(PluginError::Validation("entry_point must not be empty".into()));
        }

        // Reject duplicate permissions.
        let mut seen = HashSet::new();
        for permission in &self.permissions {
            let key = format!("{permission:?}");
            if !seen.insert(key) {
                return Err(PluginError::Validation(format!(
                    "duplicate permission `{permission:?}`"
                )));
            }
        }

        // Reject duplicate entry point names.
        let mut names = HashSet::new();
        for entry in &self.entry_points {
            if entry.name.trim().is_empty() {
                return Err(PluginError::Validation(
                    "entry point name must not be empty".into(),
                ));
            }
            if !names.insert(entry.name.clone()) {
                return Err(PluginError::Validation(format!(
                    "duplicate entry point `{}`",
                    entry.name
                )));
            }
        }

        Ok(())
    }

    /// Returns `true` if the plugin requests the given permission.
    pub fn has_permission(&self, permission: &Permission) -> bool {
        self.permissions.contains(permission)
    }

    /// Convert the manifest into the serializable [`PluginConfig`] stored in
    /// the project model.
    pub fn to_config(&self) -> PluginConfig {
        PluginConfig {
            id: self.id.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            description: self.description.clone(),
            author: self.author.clone(),
            homepage: self.homepage.clone(),
            repository: self.repository.clone(),
            license: self.license.clone(),
            enabled: true,
            config: Default::default(),
            permissions: self.permissions.clone(),
            entry_points: self.entry_points.clone(),
            dependencies: self.dependencies.clone(),
        }
    }

    /// Build a manifest from a project [`PluginConfig`], using defaults for
    /// fields the config does not track.
    pub fn from_config(config: &PluginConfig) -> Self {
        Self {
            id: config.id.clone(),
            name: config.name.clone(),
            version: config.version.clone(),
            api_version: API_VERSION.to_string(),
            description: config.description.clone(),
            author: config.author.clone(),
            homepage: config.homepage.clone(),
            repository: config.repository.clone(),
            license: config.license.clone(),
            entry_point: default_entry_point(),
            permissions: config.permissions.clone(),
            entry_points: config.entry_points.clone(),
            dependencies: config.dependencies.clone(),
            exports: Vec::new(),
            resources: PluginResources::default(),
        }
    }
}

/// Returns `true` when the identifier only contains `[A-Za-z0-9._-]`.
fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Minimal `MAJOR.MINOR.PATCH` validator (numeric components only).
fn is_valid_semver(version: &str) -> bool {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{
        "id": "com.acme.charts",
        "name": "Acme Charts",
        "version": "1.2.3",
        "entry_point": "plugin.wasm",
        "permissions": ["read_forms", "access_network"],
        "entry_points": [
            { "name": "bar-chart", "type": "component", "path": "render", "exports": ["render"] }
        ]
    }"#;

    #[test]
    fn parses_valid_manifest() {
        let manifest = PluginManifest::from_json(VALID).unwrap();
        assert_eq!(manifest.id, "com.acme.charts");
        assert_eq!(manifest.api_version, API_VERSION);
        assert!(manifest.has_permission(&Permission::ReadForms));
        assert!(manifest.has_permission(&Permission::AccessNetwork));
    }

    #[test]
    fn rejects_bad_version() {
        let json = VALID.replace("1.2.3", "1.2");
        assert!(PluginManifest::from_json(&json).is_err());
    }

    #[test]
    fn rejects_bad_id() {
        let json = VALID.replace("com.acme.charts", "bad id!");
        assert!(PluginManifest::from_json(&json).is_err());
    }

    #[test]
    fn rejects_duplicate_entry_points() {
        let json = r#"{
            "id": "x", "name": "x", "version": "1.0.0",
            "entry_points": [
                { "name": "a", "type": "component", "path": "a", "exports": [] },
                { "name": "a", "type": "component", "path": "b", "exports": [] }
            ]
        }"#;
        assert!(PluginManifest::from_json(json).is_err());
    }

    #[test]
    fn round_trips_to_config() {
        let manifest = PluginManifest::from_json(VALID).unwrap();
        let config = manifest.to_config();
        assert_eq!(config.id, manifest.id);
        assert!(config.enabled);
        let back = PluginManifest::from_config(&config);
        assert_eq!(back.permissions, manifest.permissions);
    }

    #[test]
    fn resolves_wasm_path_relative_to_manifest() {
        let manifest = PluginManifest::from_json(VALID).unwrap();
        let path = manifest.resolve_wasm_path("/plugins/acme/plugin.json");
        assert!(path.ends_with("acme/plugin.wasm") || path.ends_with("acme\\plugin.wasm"));
    }
}
