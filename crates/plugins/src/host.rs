//! Plugin host: installs, validates, and executes plugins inside the wasm
//! sandbox.
//!
//! The host keeps an in-memory registry of installed plugins. Each plugin's
//! manifest is validated against a host policy before it can be installed or
//! executed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use wasmtime::{Engine, Module, Store};

use crate::error::{PluginError, Result};
use crate::manifest::{Permission, PluginManifest};
use crate::wasm::{self, PluginState};

/// Policy applied to all plugins hosted by a [`PluginHost`].
#[derive(Debug, Clone)]
pub struct HostPolicy {
    /// Permissions the host is willing to grant. Empty means "grant nothing".
    pub allowed_permissions: Vec<Permission>,
    /// Default instruction budget per invocation.
    pub fuel: u64,
    /// Default maximum linear memory per plugin, in MiB.
    pub memory_mb: u32,
    /// Whether plugins requesting network access are permitted to load.
    pub allow_network: bool,
    /// Whether plugin code execution is permitted at all.
    pub allow_execution: bool,
}

impl Default for HostPolicy {
    fn default() -> Self {
        Self {
            allowed_permissions: vec![
                Permission::ReadForms,
                Permission::ReadEntities,
                Permission::ReadPages,
                Permission::ReadThemes,
            ],
            fuel: 50_000_000,
            memory_mb: 64,
            allow_network: false,
            allow_execution: true,
        }
    }
}

impl HostPolicy {
    /// Whether the policy grants the given permission.
    pub fn allows(&self, permission: &Permission) -> bool {
        if matches!(permission, Permission::AccessNetwork) && !self.allow_network {
            return false;
        }
        if matches!(permission, Permission::ExecuteCode) && !self.allow_execution {
            return false;
        }
        self.allowed_permissions.contains(permission)
    }
}

/// A plugin installed in the host.
pub struct InstalledPlugin {
    /// Declared manifest.
    pub manifest: PluginManifest,
    /// Absolute path to the wasm module on disk (if loaded from disk).
    pub module_path: Option<PathBuf>,
    /// Compiled module, cached for repeated instantiation.
    module: Module,
    /// Whether the plugin is currently enabled.
    pub enabled: bool,
}

impl InstalledPlugin {
    /// The plugin identifier.
    pub fn id(&self) -> &str {
        &self.manifest.id
    }
}

/// Result of invoking a plugin export.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionOutcome {
    /// Log lines produced during execution.
    pub logs: Vec<String>,
    /// Events emitted during execution.
    pub events: Vec<wasm::PluginEvent>,
    /// The `i32` result, when the invoked export returns one.
    pub result: Option<i32>,
    /// Fuel consumed, when metering is available.
    pub fuel_used: u64,
}

/// Owns the wasmtime engine and the set of installed plugins.
pub struct PluginHost {
    engine: Engine,
    policy: HostPolicy,
    plugins: Arc<Mutex<IndexMap<String, InstalledPlugin>>>,
}

impl PluginHost {
    /// Create a host with the default policy.
    pub fn new() -> Result<Self> {
        Self::with_policy(HostPolicy::default())
    }

    /// Create a host with a custom policy.
    pub fn with_policy(policy: HostPolicy) -> Result<Self> {
        let engine = wasm::create_engine()?;
        Ok(Self {
            engine,
            policy,
            plugins: Arc::new(Mutex::new(IndexMap::new())),
        })
    }

    /// The active host policy.
    pub fn policy(&self) -> &HostPolicy {
        &self.policy
    }

    /// The underlying wasmtime engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Validate a manifest against the host policy.
    pub fn check_policy(&self, manifest: &PluginManifest) -> Result<()> {
        for permission in &manifest.permissions {
            if !self.policy.allows(permission) {
                return Err(PluginError::PermissionDenied {
                    plugin: manifest.id.clone(),
                    permission: permission.clone(),
                });
            }
        }
        Ok(())
    }

    /// Install a plugin from a compiled module with an already-parsed manifest.
    pub fn install(
        &self,
        manifest: PluginManifest,
        module: Module,
        module_path: Option<PathBuf>,
    ) -> Result<()> {
        manifest.validate()?;
        self.check_policy(&manifest)?;

        let mut plugins = self.plugins.lock().unwrap();
        if plugins.contains_key(&manifest.id) {
            return Err(PluginError::AlreadyInstalled(manifest.id));
        }
        let id = manifest.id.clone();
        plugins.insert(
            id,
            InstalledPlugin {
                manifest,
                module_path,
                module,
                enabled: true,
            },
        );
        Ok(())
    }

    /// Install a plugin from a manifest file path, resolving the wasm module
    /// relative to the manifest.
    pub fn install_from_manifest_file(&self, manifest_path: impl AsRef<Path>) -> Result<String> {
        let manifest_path = manifest_path.as_ref();
        let manifest = PluginManifest::from_file(manifest_path)?;
        let wasm_path = manifest.resolve_wasm_path(manifest_path);
        let module = wasm::load_module_from_file(&self.engine, &wasm_path)?;
        let id = manifest.id.clone();
        self.install(manifest, module, Some(wasm_path))?;
        Ok(id)
    }

    /// Scan a directory for plugin manifests (`plugin.json` / `plugin.toml`)
    /// and install each one. Returns the ids that were installed successfully
    /// along with any errors.
    pub fn load_from_dir(&self, dir: impl AsRef<Path>) -> Result<(Vec<String>, Vec<PluginError>)> {
        let dir = dir.as_ref();
        let mut installed = Vec::new();
        let mut errors = Vec::new();

        let entries = std::fs::read_dir(dir).map_err(|source| PluginError::Io {
            path: dir.display().to_string(),
            source,
        })?;

        for entry in entries.flatten() {
            let path = entry.path();
            let is_manifest = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == "plugin.json" || n == "plugin.toml")
                .unwrap_or(false);
            if !is_manifest {
                continue;
            }
            match self.install_from_manifest_file(&path) {
                Ok(id) => installed.push(id),
                Err(e) => errors.push(e),
            }
        }

        Ok((installed, errors))
    }

    /// Remove an installed plugin.
    pub fn uninstall(&self, id: &str) -> Result<()> {
        let mut plugins = self.plugins.lock().unwrap();
        plugins
            .shift_remove(id)
            .map(|_| ())
            .ok_or_else(|| PluginError::NotFound(id.to_string()))
    }

    /// Enable or disable a plugin.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let mut plugins = self.plugins.lock().unwrap();
        let plugin = plugins
            .get_mut(id)
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;
        plugin.enabled = enabled;
        Ok(())
    }

    /// List installed plugin ids in insertion order.
    pub fn list(&self) -> Vec<String> {
        self.plugins.lock().unwrap().keys().cloned().collect()
    }

    /// The manifest for an installed plugin.
    pub fn manifest(&self, id: &str) -> Option<PluginManifest> {
        self.plugins
            .lock()
            .unwrap()
            .get(id)
            .map(|p| p.manifest.clone())
    }

    /// Resolve dependency plugins, returning an error if any are missing or
    /// version-mismatched.
    pub fn check_dependencies(&self, manifest: &PluginManifest) -> Result<()> {
        let plugins = self.plugins.lock().unwrap();
        for dep in &manifest.dependencies {
            let Some(existing) = plugins.get(&dep.id) else {
                if dep.optional {
                    continue;
                }
                return Err(PluginError::MissingDependency {
                    id: manifest.id.clone(),
                    dep: dep.id.clone(),
                });
            };
            if !version_matches(&existing.manifest.version, &dep.version) {
                return Err(PluginError::DependencyVersion {
                    id: manifest.id.clone(),
                    dep: dep.id.clone(),
                    required: dep.version.clone(),
                });
            }
        }
        Ok(())
    }

    /// Invoke a zero-argument export on a plugin and collect its side effects.
    pub fn call_export(&self, id: &str, export: &str) -> Result<ExecutionOutcome> {
        let (module, manifest, enabled) = {
            let plugins = self.plugins.lock().unwrap();
            let plugin = plugins
                .get(id)
                .ok_or_else(|| PluginError::NotFound(id.to_string()))?;
            (
                plugin.module.clone(),
                plugin.manifest.clone(),
                plugin.enabled,
            )
        };

        if !enabled {
            return Err(PluginError::Validation(format!(
                "plugin `{id}` is disabled"
            )));
        }

        let fuel = manifest.resources.fuel.unwrap_or(self.policy.fuel);
        let state = PluginState::for_plugin(&manifest, self.policy.memory_mb);
        let (mut store, instance) = wasm::instantiate(&self.engine, &module, state, fuel)?;

        let before = wasm::remaining_fuel(&mut store).unwrap_or(fuel);
        wasm::call_unit(&mut store, &instance, id, export)?;
        let after = wasm::remaining_fuel(&mut store).unwrap_or(before);

        let state = store.data_mut();
        Ok(ExecutionOutcome {
            logs: state.take_logs(),
            events: state.take_events(),
            result: None,
            fuel_used: before.saturating_sub(after),
        })
    }

    /// Invoke an export taking a single `i32` and returning an `i32`.
    pub fn call_i32(&self, id: &str, export: &str, arg: i32) -> Result<ExecutionOutcome> {
        let (module, manifest, enabled) = {
            let plugins = self.plugins.lock().unwrap();
            let plugin = plugins
                .get(id)
                .ok_or_else(|| PluginError::NotFound(id.to_string()))?;
            (
                plugin.module.clone(),
                plugin.manifest.clone(),
                plugin.enabled,
            )
        };

        if !enabled {
            return Err(PluginError::Validation(format!(
                "plugin `{id}` is disabled"
            )));
        }

        let fuel = manifest.resources.fuel.unwrap_or(self.policy.fuel);
        let state = PluginState::for_plugin(&manifest, self.policy.memory_mb);
        let (mut store, instance) = wasm::instantiate(&self.engine, &module, state, fuel)?;

        let before = wasm::remaining_fuel(&mut store).unwrap_or(fuel);
        let result = wasm::call_i32_to_i32(&mut store, &instance, id, export, arg)?;
        let after = wasm::remaining_fuel(&mut store).unwrap_or(before);

        let state = store.data_mut();
        Ok(ExecutionOutcome {
            logs: state.take_logs(),
            events: state.take_events(),
            result: Some(result),
            fuel_used: before.saturating_sub(after),
        })
    }

    /// Aggregate entry points across all enabled plugins, grouped by type.
    pub fn entry_points_by_type(&self) -> HashMap<String, Vec<(String, String)>> {
        let plugins = self.plugins.lock().unwrap();
        let mut map: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for plugin in plugins.values() {
            if !plugin.enabled {
                continue;
            }
            for entry in &plugin.manifest.entry_points {
                map.entry(format!("{:?}", entry.r#type).to_lowercase())
                    .or_default()
                    .push((plugin.manifest.id.clone(), entry.name.clone()));
            }
        }
        map
    }
}

/// A permissive module-level store type alias used by consumers that want to
/// manage their own store lifecycle.
pub type PluginStore = Store<PluginState>;

/// Compile a plugin module without installing it (useful for validation).
pub fn compile_module(engine: &Engine, path: impl AsRef<Path>) -> Result<Module> {
    wasm::load_module_from_file(engine, path)
}

/// Returns `true` when `have` satisfies the `want` constraint.
///
/// Supports exact versions (`1.2.3`) and caret requirements (`^1.2.3`,
/// `1.2` -> same major, `1` -> same major).
fn version_matches(have: &str, want: &str) -> bool {
    let want = want.trim().trim_start_matches('^');
    if want == "*" {
        return true;
    }
    let have_parts = parse_version(have);
    let want_parts = parse_version(want);
    match (have_parts, want_parts) {
        (Some(h), Some(w)) => {
            if w.len() == 1 {
                h[0] == w[0]
            } else if w.len() == 2 {
                h[0] == w[0] && h[1] == w[1]
            } else {
                h == w
            }
        }
        _ => have == want,
    }
}

fn parse_version(version: &str) -> Option<Vec<u64>> {
    let core = version.split(['-', '+']).next()?;
    core.split('.')
        .map(|p| p.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;

    const WAT: &str = r#"
        (module
            (import "host" "log" (func $log (param i32 i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "hello from plugin")
            (global $counter (mut i32) (i32.const 0))
            (func (export "greet")
                (call $log (i32.const 0) (i32.const 17)))
            (func (export "bump") (param i32) (result i32)
                (global.set $counter (i32.add (global.get $counter) (i32.const 1)))
                (global.get $counter))
        )
    "#;

    fn manifest(id: &str) -> PluginManifest {
        PluginManifest::from_json(&format!(
            r#"{{"id":"{id}","name":"T","version":"1.0.0","permissions":["read_forms"]}}"#
        ))
        .unwrap()
    }

    fn host_and_module() -> (PluginHost, Module) {
        let host = PluginHost::new().unwrap();
        let module = wasm::load_module_from_bytes(host.engine(), WAT.as_bytes()).unwrap();
        (host, module)
    }

    #[test]
    fn installs_and_runs() {
        let (host, module) = host_and_module();
        host.install(manifest("com.test.hello"), module, None)
            .unwrap();

        assert_eq!(host.list(), vec!["com.test.hello".to_string()]);
        let outcome = host.call_export("com.test.hello", "greet").unwrap();
        assert_eq!(outcome.logs, vec!["hello from plugin".to_string()]);
    }

    #[test]
    fn runs_i32_export_and_meters_fuel() {
        let (host, module) = host_and_module();
        host.install(manifest("com.test.hello"), module, None)
            .unwrap();
        let outcome = host.call_i32("com.test.hello", "bump", 0).unwrap();
        assert_eq!(outcome.result, Some(1));
        assert!(outcome.fuel_used > 0);
    }

    #[test]
    fn rejects_disallowed_permission() {
        let (host, module) = host_and_module();
        let m = PluginManifest::from_json(
            r#"{"id":"com.test.net","name":"N","version":"1.0.0","permissions":["access_network"]}"#,
        )
        .unwrap();
        let err = host.install(m, module, None).unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied { .. }));
    }

    #[test]
    fn rejects_duplicate_install() {
        let (host, module) = host_and_module();
        host.install(manifest("com.test.hello"), module.clone(), None)
            .unwrap();
        let err = host
            .install(manifest("com.test.hello"), module, None)
            .unwrap_err();
        assert!(matches!(err, PluginError::AlreadyInstalled(_)));
    }

    #[test]
    fn disable_blocks_execution() {
        let (host, module) = host_and_module();
        host.install(manifest("com.test.hello"), module, None)
            .unwrap();
        host.set_enabled("com.test.hello", false).unwrap();
        assert!(host.call_export("com.test.hello", "greet").is_err());
    }

    #[test]
    fn uninstall_removes_plugin() {
        let (host, module) = host_and_module();
        host.install(manifest("com.test.hello"), module, None)
            .unwrap();
        host.uninstall("com.test.hello").unwrap();
        assert!(host.list().is_empty());
        assert!(host.uninstall("com.test.hello").is_err());
    }

    #[test]
    fn version_matching() {
        assert!(version_matches("1.2.3", "^1.2.3"));
        assert!(version_matches("1.2.9", "1.2"));
        assert!(version_matches("1.9.0", "1"));
        assert!(!version_matches("2.0.0", "1"));
        assert!(!version_matches("1.3.0", "1.2"));
    }

    #[test]
    fn detects_missing_dependency() {
        let (host, module) = host_and_module();
        let m = PluginManifest::from_json(
            r#"{"id":"com.test.a","name":"A","version":"1.0.0",
                "dependencies":[{"id":"com.test.b","version":"1.0.0","optional":false}]}"#,
        )
        .unwrap();
        host.install(m, module, None).unwrap();
        let a = host.manifest("com.test.a").unwrap();
        let err = host.check_dependencies(&a).unwrap_err();
        assert!(matches!(err, PluginError::MissingDependency { .. }));
    }
}
