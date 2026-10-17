//! Error types for the plugin system.

use thiserror::Error;

/// Result alias used throughout the plugin crate.
pub type Result<T> = std::result::Result<T, PluginError>;

/// Errors that can occur while loading, validating, or executing plugins.
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin manifest error: {0}")]
    Manifest(String),

    #[error("plugin validation failed: {0}")]
    Validation(String),

    #[error("plugin `{0}` is not installed")]
    NotFound(String),

    #[error("plugin `{0}` already installed")]
    AlreadyInstalled(String),

    #[error("plugin `{id}` requires missing dependency `{dep}`")]
    MissingDependency { id: String, dep: String },

    #[error("plugin `{id}` requires version `{required}` of dependency `{dep}`")]
    DependencyVersion {
        id: String,
        dep: String,
        required: String,
    },

    #[error("plugin `{plugin}` lacks required permission `{permission:?}`")]
    PermissionDenied {
        plugin: String,
        permission: crate::manifest::Permission,
    },

    #[error("wasm module for plugin `{0}` not found")]
    ModuleNotFound(String),

    #[error("wasmtime error: {0}")]
    Wasmtime(#[from] wasmtime::Error),

    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("plugin `{0}` trapped or exceeded its resource limits")]
    Trap(String),

    #[error("export `{export}` not found on plugin `{plugin}`")]
    ExportNotFound { plugin: String, export: String },
}

impl From<std::io::Error> for PluginError {
    fn from(source: std::io::Error) -> Self {
        PluginError::Io {
            path: String::new(),
            source,
        }
    }
}
