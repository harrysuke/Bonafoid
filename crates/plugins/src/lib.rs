//! No-Code IDE plugin system.
//!
//! Plugins are WebAssembly modules (executed with `wasmtime`) described by a
//! [`manifest::PluginManifest`]. The [`host::PluginHost`] owns the sandbox
//! engine and the registry of installed plugins.

pub mod error;
pub mod host;
pub mod manifest;
pub mod wasm;

pub use error::{PluginError, Result};
pub use host::{ExecutionOutcome, HostPolicy, InstalledPlugin, PluginHost};
pub use manifest::{Permission, PluginManifest, PluginResources, API_VERSION};
pub use wasm::{PluginEvent, PluginState};

/// Compile a WebAssembly module from bytes using the host's engine.
///
/// A consumer that receives a plugin over the network needs this: the
/// manifest can be parsed from JSON, but the module has to be compiled
/// before it can be installed.
pub use wasm::load_module_from_bytes;
