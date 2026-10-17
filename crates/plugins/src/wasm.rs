//! WebAssembly sandbox helpers built on top of `wasmtime`.
//!
//! This module owns everything that touches the raw `wasmtime` API: creating
//! a locked-down [`Engine`], compiling modules, wiring the host imports, and
//! invoking exported functions with resource limits applied.

use std::path::Path;

use wasmtime::{
    Caller, Config, Engine, Extern, Instance, Linker, Module, Store, StoreLimits,
    StoreLimitsBuilder,
};

use crate::error::{PluginError, Result};
use crate::manifest::PluginManifest;

/// Runtime state stored inside each plugin's wasm store.
pub struct PluginState {
    /// Identifier of the plugin being executed.
    pub plugin_id: String,
    /// Log lines emitted through the `host.log` import.
    pub logs: Vec<String>,
    /// Events emitted through the `host.emit` import.
    pub events: Vec<PluginEvent>,
    /// Limits enforced by the store's resource limiter.
    pub limits: StoreLimits,
}

impl PluginState {
    /// Create fresh state for a plugin, applying the manifest's resource hints.
    pub fn for_plugin(manifest: &PluginManifest, default_memory_mb: u32) -> Self {
        let memory_mb = manifest.resources.memory_mb.unwrap_or(default_memory_mb);
        let limits = StoreLimitsBuilder::new()
            .memory_size(memory_mb as usize * 1024 * 1024)
            .instances(1)
            .tables(16)
            .memories(1)
            .build();

        Self {
            plugin_id: manifest.id.clone(),
            logs: Vec::new(),
            events: Vec::new(),
            limits,
        }
    }

    /// Take and clear the collected log lines.
    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }

    /// Take and clear the collected events.
    pub fn take_events(&mut self) -> Vec<PluginEvent> {
        std::mem::take(&mut self.events)
    }
}

/// An event emitted by a plugin through the host interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginEvent {
    /// The event name.
    pub name: String,
    /// JSON encoded payload.
    pub payload: String,
}

/// Create a configured [`Engine`] with the sandbox features required by the
/// plugin host.
pub fn create_engine() -> Result<Engine> {
    let mut config = Config::new();
    // Deterministic execution budget.
    config.consume_fuel(true);
    // Allow the component model for future WIT-based plugins.
    config.wasm_component_model(true);
    // Keep the sandbox tight.
    config.wasm_threads(false);
    config.wasm_reference_types(false);
    config.wasm_simd(false);
    config.wasm_relaxed_simd(false);
    config.wasm_multi_memory(false);
    // Cranelift is the default compiler; leave optimizations on for release.
    Engine::new(&config).map_err(PluginError::Wasmtime)
}

/// Compile a wasm module from a file.
pub fn load_module_from_file(engine: &Engine, path: impl AsRef<Path>) -> Result<Module> {
    let path = path.as_ref();
    Module::from_file(engine, path).map_err(|e| {
        if !path.exists() {
            PluginError::ModuleNotFound(path.display().to_string())
        } else {
            PluginError::Wasmtime(e)
        }
    })
}

/// Compile a wasm module from raw bytes (also accepts WAT text).
pub fn load_module_from_bytes(engine: &Engine, bytes: &[u8]) -> Result<Module> {
    Module::new(engine, bytes).map_err(PluginError::Wasmtime)
}

/// Register the host imports exposed to plugins under the `host` namespace.
pub fn register_host_imports(linker: &mut Linker<PluginState>) -> Result<()> {
    // log(ptr, len) -> writes a UTF-8 line to the plugin log.
    linker.func_wrap(
        "host",
        "log",
        |mut caller: Caller<'_, PluginState>, ptr: i32, len: i32| {
            if let Some(text) = read_string(&mut caller, ptr, len) {
                caller.data_mut().logs.push(text);
            }
        },
    )?;

    // emit(ptr, len) -> records a JSON event payload emitted by the plugin.
    linker.func_wrap(
        "host",
        "emit",
        |mut caller: Caller<'_, PluginState>, ptr: i32, len: i32| {
            if let Some(payload) = read_string(&mut caller, ptr, len) {
                let name = "event".to_string();
                caller.data_mut().events.push(PluginEvent { name, payload });
            }
        },
    )?;

    // now() -> current unix timestamp in milliseconds.
    linker.func_wrap("host", "now", || -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    })?;

    // random() -> a pseudo random u64 sourced from the host.
    linker.func_wrap("host", "random", || -> i64 {
        use std::hash::{BuildHasher, Hasher};
        let hasher = std::collections::hash_map::RandomState::new();
        hasher.build_hasher().finish() as i64
    })?;

    // abort() -> signals the plugin gave up; we trap with a clear message.
    linker.func_wrap("host", "abort", |_caller: Caller<'_, PluginState>| {
        // Returning normally keeps the plugin alive; a real abort is modelled
        // by the plugin returning an error code from its export.
    })?;

    Ok(())
}

/// Instantiate a module with the host imports and resource limits applied.
///
/// Returns the store and instance so the caller can invoke exports.
pub fn instantiate(
    engine: &Engine,
    module: &Module,
    state: PluginState,
    fuel: u64,
) -> Result<(Store<PluginState>, Instance)> {
    let mut linker: Linker<PluginState> = Linker::new(engine);
    register_host_imports(&mut linker)?;

    let mut store = Store::new(engine, state);
    store.limiter(|state| &mut state.limits);
    store.set_fuel(fuel).map_err(PluginError::Wasmtime)?;

    let instance = linker
        .instantiate(&mut store, module)
        .map_err(|e| PluginError::Trap(e.to_string()))?;

    Ok((store, instance))
}

/// Whether an export with the given name exists on the instance.
pub fn has_export(store: &mut Store<PluginState>, instance: &Instance, name: &str) -> bool {
    instance.get_func(store, name).is_some()
}

/// Call a zero-argument, zero-result export by name.
pub fn call_unit(
    store: &mut Store<PluginState>,
    instance: &Instance,
    plugin_id: &str,
    name: &str,
) -> Result<()> {
    let func = instance
        .get_typed_func::<(), ()>(&mut *store, name)
        .map_err(|_| PluginError::ExportNotFound {
            plugin: plugin_id.to_string(),
            export: name.to_string(),
        })?;

    func.call(store, ()).map_err(|e| {
        // Distinguish resource exhaustion from genuine traps for a clearer error.
        if let Some(err) = e.downcast_ref::<wasmtime::Trap>() {
            if matches!(err, wasmtime::Trap::OutOfFuel) {
                return PluginError::Trap(format!("plugin `{plugin_id}` exceeded its fuel budget"));
            }
        }
        PluginError::Trap(format!("plugin `{plugin_id}` trapped in `{name}`: {e}"))
    })
}

/// Call an export taking a single `i32` and returning an `i32`.
pub fn call_i32_to_i32(
    store: &mut Store<PluginState>,
    instance: &Instance,
    plugin_id: &str,
    name: &str,
    arg: i32,
) -> Result<i32> {
    let func = instance
        .get_typed_func::<i32, i32>(&mut *store, name)
        .map_err(|_| PluginError::ExportNotFound {
            plugin: plugin_id.to_string(),
            export: name.to_string(),
        })?;

    func.call(store, arg)
        .map_err(|e| PluginError::Trap(format!("plugin `{plugin_id}` trapped in `{name}`: {e}")))
}

/// Read a UTF-8 string out of the caller's exported `memory`.
fn read_string(caller: &mut Caller<'_, PluginState>, ptr: i32, len: i32) -> Option<String> {
    if ptr < 0 || len < 0 {
        return None;
    }
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(memory)) => memory,
        _ => return None,
    };
    let start = ptr as usize;
    let end = start.checked_add(len as usize)?;
    let data = memory.data(&*caller);
    let slice = data.get(start..end)?;
    Some(String::from_utf8_lossy(slice).into_owned())
}

/// Remaining fuel in the store, if fuel metering is enabled.
pub fn remaining_fuel(store: &mut Store<PluginState>) -> Option<u64> {
    store.get_fuel().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;

    // A tiny module that imports `host.log`, writes "hello" into memory and
    // calls log, then increments a counter.
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

    fn manifest() -> PluginManifest {
        PluginManifest::from_json(
            r#"{"id":"com.test.hello","name":"Hello","version":"1.0.0"}"#,
        )
        .unwrap()
    }

    #[test]
    fn instantiates_and_calls_exports() {
        let engine = create_engine().unwrap();
        let module = load_module_from_bytes(&engine, WAT.as_bytes()).unwrap();
        let state = PluginState::for_plugin(&manifest(), 16);
        let (mut store, instance) = instantiate(&engine, &module, state, 1_000_000).unwrap();

        assert!(has_export(&mut store, &instance, "greet"));
        call_unit(&mut store, &instance, "com.test.hello", "greet").unwrap();
        assert_eq!(store.data_mut().take_logs(), vec!["hello from plugin".to_string()]);

        assert_eq!(
            call_i32_to_i32(&mut store, &instance, "com.test.hello", "bump", 0).unwrap(),
            1
        );
        assert_eq!(
            call_i32_to_i32(&mut store, &instance, "com.test.hello", "bump", 0).unwrap(),
            2
        );
    }

    #[test]
    fn missing_export_reports_not_found() {
        let engine = create_engine().unwrap();
        let module = load_module_from_bytes(&engine, WAT.as_bytes()).unwrap();
        let state = PluginState::for_plugin(&manifest(), 16);
        let (mut store, instance) = instantiate(&engine, &module, state, 1_000_000).unwrap();

        let err = call_unit(&mut store, &instance, "com.test.hello", "nope").unwrap_err();
        assert!(matches!(err, PluginError::ExportNotFound { .. }));
    }

    #[test]
    fn fuel_is_metered() {
        let engine = create_engine().unwrap();
        let module = load_module_from_bytes(&engine, WAT.as_bytes()).unwrap();
        let state = PluginState::for_plugin(&manifest(), 16);
        let (mut store, instance) = instantiate(&engine, &module, state, 1_000_000).unwrap();
        let before = remaining_fuel(&mut store).unwrap();
        call_unit(&mut store, &instance, "com.test.hello", "greet").unwrap();
        let after = remaining_fuel(&mut store).unwrap();
        assert!(after < before, "fuel should be consumed by execution");
    }
}
