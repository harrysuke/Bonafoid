//! The interpolation grammar, and the redactor that keeps secrets out of the
//! journal.
//!
//! Deliberately small (§7.6):
//!
//! ```text
//! ${input.<key>}              node input port
//! ${vars.<name>}              run variables
//! ${nodes.<node_id>.<path>}   a previous node's output, dotted path
//! ${env.<NAME>}               environment variable (allowlisted)
//! ${secret:<NAME>}            resolved at execution, never journalled
//! ${now} ${run_id} ${workspace}
//! ```
//!
//! No arithmetic, no function calls, no conditionals. Logic belongs in a
//! gateway or a shell node; a template language with branching becomes a
//! programming language with no debugger.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde_json::Value;

use crate::context::SecretResolver;
use crate::error::{NodeError, NodeErrorCode};

/// The values a template can read.
#[derive(Clone)]
pub struct Scope {
    pub input: IndexMap<String, Value>,
    pub vars: IndexMap<String, Value>,
    /// Node id (as a string) to that node's output object, in completion
    /// order — `IndexMap` rather than `HashMap` so a merged run output and a
    /// gateway condition see a deterministic precedence.
    pub nodes: IndexMap<String, Value>,
    pub now: DateTime<Utc>,
    pub run_id: String,
    pub workspace: PathBuf,
    /// Environment variables `${env.NAME}` may read. Empty denies all.
    pub env_allowlist: Vec<String>,
    pub secrets: Arc<dyn SecretResolver>,
}

impl Scope {
    pub fn new(
        run_id: impl Into<String>,
        workspace: impl Into<PathBuf>,
        input: IndexMap<String, Value>,
        secrets: Arc<dyn SecretResolver>,
    ) -> Self {
        Self {
            input,
            vars: IndexMap::new(),
            nodes: IndexMap::new(),
            now: Utc::now(),
            run_id: run_id.into(),
            workspace: workspace.into(),
            env_allowlist: Vec::new(),
            secrets,
        }
    }

    pub fn set_node_output(&mut self, node_id: &str, output: Value) {
        self.nodes.insert(node_id.to_string(), output);
    }

    /// The variable map a gateway condition is evaluated against.
    ///
    /// `input` and `vars` first, then every completed node's *top-level* keys,
    /// so an output of `{"approved": true}` makes `approved` visible to a
    /// condition. This is how the BPMN simulator accumulates variables, so a
    /// condition behaves the same in a preview and a real run. A later node
    /// wins over an earlier one with the same key, and `vars` wins over both.
    pub fn condition_vars(&self) -> IndexMap<String, Value> {
        let mut merged = IndexMap::new();
        merged.extend(self.input.clone());
        for output in self.nodes.values() {
            if let Value::Object(map) = output {
                for (key, value) in map {
                    merged.insert(key.clone(), value.clone());
                }
            }
        }
        merged.extend(self.vars.clone());
        merged
    }
}

impl std::fmt::Debug for Scope {
    /// Hand-written because `secrets` is a trait object, and because a derived
    /// `Debug` would be a place secret values could reach a log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scope")
            .field("input", &self.input)
            .field("vars", &self.vars)
            .field("nodes", &self.nodes.keys().collect::<Vec<_>>())
            .field("now", &self.now)
            .field("run_id", &self.run_id)
            .field("workspace", &self.workspace)
            .field("env_allowlist", &self.env_allowlist)
            .finish_non_exhaustive()
    }
}

/// Expand every `${…}` in `template`.
pub fn interpolate(template: &str, scope: &Scope) -> Result<String, NodeError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            return Err(NodeError::invalid_config(format!(
                "unterminated `${{` in `{template}`"
            )));
        };
        let expression = &after[..end];
        out.push_str(&resolve(expression, scope)?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Interpolate every string inside a JSON value, leaving the shape alone.
///
/// Used for node configs: a `shell` node's `args` array is interpolated
/// element by element rather than as one joined string, so a value containing
/// a space cannot split into two arguments.
pub fn interpolate_value(value: &Value, scope: &Scope) -> Result<Value, NodeError> {
    Ok(match value {
        Value::String(text) => Value::String(interpolate(text, scope)?),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| interpolate_value(item, scope))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (key, item) in map {
                out.insert(key.clone(), interpolate_value(item, scope)?);
            }
            Value::Object(out)
        }
        other => other.clone(),
    })
}

fn resolve(expression: &str, scope: &Scope) -> Result<String, NodeError> {
    Ok(render(&resolve_value(expression, scope)?))
}

/// Resolve one `${…}` expression to a value, keeping its type.
///
/// [`interpolate`] renders everything to a string because it is filling in a
/// template. An edge binding is not filling in a template — `{"files":
/// "nodes.list.files"}` must hand the target node the array itself, not its
/// JSON text — so it needs the value.
pub fn resolve_value(expression: &str, scope: &Scope) -> Result<Value, NodeError> {
    let expression = expression.trim();

    match expression {
        "now" => return Ok(Value::String(scope.now.to_rfc3339())),
        "run_id" => return Ok(Value::String(scope.run_id.clone())),
        "workspace" => return Ok(Value::String(scope.workspace.display().to_string())),
        _ => {}
    }

    if let Some(name) = expression.strip_prefix("secret:") {
        return scope
            .secrets
            .resolve(name.trim())
            .map(Value::String)
            .ok_or_else(|| missing(format!("secret `{name}` is not available")));
    }

    let (root, path) = match expression.split_once('.') {
        Some((root, path)) => (root, path),
        None => (expression, ""),
    };

    match root {
        "input" => lookup_value(&scope.input, path, "input"),
        "vars" => lookup_value(&scope.vars, path, "vars"),
        "env" => {
            let name = path;
            if !scope.env_allowlist.iter().any(|allowed| allowed == name) {
                return Err(NodeError::permission_denied(format!(
                    "`env.{name}` is not in the run's environment allowlist"
                )));
            }
            std::env::var(name)
                .map(Value::String)
                .map_err(|_| missing(format!("environment variable `{name}` is not set")))
        }
        "nodes" => {
            let (node_id, node_path) = match path.split_once('.') {
                Some((id, rest)) => (id, rest),
                None => (path, ""),
            };
            let output = scope
                .nodes
                .get(node_id)
                .ok_or_else(|| missing(format!("node `{node_id}` has not run yet")))?;
            walk(output, node_path).cloned()
        }
        other => Err(NodeError::invalid_config(format!(
            "unknown interpolation root `{other}` in `${{{expression}}}`"
        ))),
    }
}

/// Resolve an expression that may be written either way.
///
/// Two spellings exist because two documents describe the same thing: the
/// model's own doc comments write a bare path (`{"files": "input.files"}`),
/// while §7.6 teaches `${…}`. A bare path resolves to its *value*, keeping the
/// type — an array stays an array. A `${…}` template is interpolated and the
/// result is a string, because that is what filling in a template produces.
///
/// Used for port bindings on an edge and for a subflow's input and output
/// mappings, which are the three places a port is wired by name.
pub fn resolve_expression(expression: &str, scope: &Scope) -> Result<Value, NodeError> {
    let trimmed = expression.trim();
    if trimmed.contains("${") {
        interpolate_value(&Value::String(trimmed.to_string()), scope)
    } else {
        resolve_value(trimmed, scope)
    }
}

fn lookup_value(
    map: &IndexMap<String, Value>,
    path: &str,
    root: &str,
) -> Result<Value, NodeError> {
    let value = map
        .get(path)
        .ok_or_else(|| missing(format!("`{root}.{path}` is not set")))?;
    Ok(value.clone())
}

fn walk<'a>(value: &'a Value, path: &str) -> Result<&'a Value, NodeError> {
    if path.is_empty() {
        return Ok(value);
    }
    let mut current = value;
    for segment in path.split('.') {
        current = match current {
            Value::Object(map) => map
                .get(segment)
                .ok_or_else(|| missing(format!("`{segment}` is not in the node's output")))?,
            Value::Array(items) => {
                let index: usize = segment
                    .parse()
                    .map_err(|_| missing(format!("`{segment}` is not an array index")))?;
                items
                    .get(index)
                    .ok_or_else(|| missing(format!("index `{segment}` is out of range")))?
            }
            _ => return Err(missing(format!("`{segment}` is not reachable"))),
        };
    }
    Ok(current)
}

/// A value as it appears in a string: strings bare, everything else as JSON.
fn render(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn missing(detail: String) -> NodeError {
    NodeError::new(NodeErrorCode::InvalidConfig, detail)
}

/// Removes secret values from anything on its way to the journal.
///
/// Built from the *config* of the node about to run: every `${secret:NAME}` it
/// mentions is resolved once, and the resulting values are scrubbed from the
/// output and from any error detail. A value shorter than
/// [`Redactor::MIN_REDACT_LEN`] is not scrubbed — replacing every `"a"` in a
/// document would corrupt it, and a one-character secret is not a secret.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    values: Vec<String>,
}

impl Redactor {
    /// Below this length, scrubbing does more damage than the leak it prevents.
    pub const MIN_REDACT_LEN: usize = 4;

    pub fn new() -> Self {
        Self::default()
    }

    /// Collect the secrets a node's config refers to.
    pub fn for_config(config: &Value, secrets: &dyn SecretResolver) -> Self {
        let mut values = Vec::new();
        collect_secret_names(config, &mut |name| {
            if let Some(value) = secrets.resolve(name) {
                if value.len() >= Self::MIN_REDACT_LEN && !values.contains(&value) {
                    values.push(value);
                }
            }
        });
        Self { values }
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn redact_str(&self, text: &str) -> String {
        let mut out = text.to_string();
        for value in &self.values {
            if out.contains(value.as_str()) {
                out = out.replace(value.as_str(), "[redacted]");
            }
        }
        out
    }

    /// Scrub every string in a JSON value, in place.
    pub fn redact_value(&self, value: &mut Value) {
        if self.values.is_empty() {
            return;
        }
        match value {
            Value::String(text) => {
                let scrubbed = self.redact_str(text);
                if scrubbed != *text {
                    *text = scrubbed;
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| self.redact_value(item)),
            Value::Object(map) => map.values_mut().for_each(|item| self.redact_value(item)),
            _ => {}
        }
    }
}

/// Find every `${secret:NAME}` inside a JSON value.
fn collect_secret_names(value: &Value, on_name: &mut impl FnMut(&str)) {
    match value {
        Value::String(text) => {
            let mut rest = text.as_str();
            while let Some(start) = rest.find("${secret:") {
                let after = &rest[start + "${secret:".len()..];
                match after.find('}') {
                    Some(end) => {
                        on_name(after[..end].trim());
                        rest = &after[end + 1..];
                    }
                    None => break,
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_secret_names(item, on_name)),
        Value::Object(map) => map
            .values()
            .for_each(|item| collect_secret_names(item, on_name)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::MapSecrets;
    use serde_json::json;

    fn scope() -> Scope {
        let mut secrets = MapSecrets::default();
        secrets.insert("TOKEN", "hunter2-long");
        let mut input = IndexMap::new();
        input.insert("diff".to_string(), json!("src/main.rs | 3 +"));
        let mut scope = Scope::new("run-1", "/repo", input, Arc::new(secrets));
        scope.vars.insert("target".to_string(), json!("main"));
        scope.set_node_output(
            "3f1c1a2e-0000-4000-8000-000000000001",
            json!({ "files": ["a.rs", "b.rs"], "count": 2 }),
        );
        scope
    }

    #[test]
    fn the_documented_forms_resolve() {
        let scope = scope();
        assert_eq!(interpolate("${input.diff}", &scope).unwrap(), "src/main.rs | 3 +");
        assert_eq!(interpolate("${vars.target}", &scope).unwrap(), "main");
        assert_eq!(interpolate("${run_id}", &scope).unwrap(), "run-1");
        assert_eq!(interpolate("${workspace}", &scope).unwrap(), "/repo");
        assert_eq!(
            interpolate("${nodes.3f1c1a2e-0000-4000-8000-000000000001.files.1}", &scope).unwrap(),
            "b.rs"
        );
        assert_eq!(
            interpolate("${nodes.3f1c1a2e-0000-4000-8000-000000000001.count}", &scope).unwrap(),
            "2"
        );
    }

    #[test]
    fn text_around_placeholders_is_preserved() {
        let scope = scope();
        assert_eq!(
            interpolate("diff of ${vars.target}: ${input.diff}!", &scope).unwrap(),
            "diff of main: src/main.rs | 3 +!"
        );
    }

    #[test]
    fn a_missing_value_is_an_error_not_an_empty_string() {
        // An empty substitution turns a prompt into a subtly wrong prompt,
        // which is worse than a failed node.
        let scope = scope();
        let error = interpolate("${vars.nope}", &scope).unwrap_err();
        assert_eq!(error.code, NodeErrorCode::InvalidConfig);
        assert!(error.message.contains("vars.nope"), "{}", error.message);
    }

    #[test]
    fn env_reads_are_allowlisted() {
        let scope = scope();
        let error = interpolate("${env.PATH}", &scope).unwrap_err();
        assert_eq!(error.code, NodeErrorCode::PermissionDenied);
    }

    #[test]
    fn an_unterminated_placeholder_is_rejected() {
        let scope = scope();
        assert!(interpolate("${vars.target", &scope).is_err());
    }

    #[test]
    fn an_unknown_root_is_rejected() {
        let scope = scope();
        let error = interpolate("${env2.X}", &scope).unwrap_err();
        assert_eq!(error.code, NodeErrorCode::InvalidConfig);
    }

    #[test]
    fn arrays_interpolate_element_by_element() {
        // Joined-then-split would let a value with a space become two args.
        let scope = scope();
        let value = json!(["diff", "${vars.target}", "${input.diff}"]);
        let out = interpolate_value(&value, &scope).unwrap();
        assert_eq!(out, json!(["diff", "main", "src/main.rs | 3 +"]));
    }

    #[test]
    fn the_redactor_scrubs_config_secrets_from_an_output() {
        let mut secrets = MapSecrets::default();
        secrets.insert("TOKEN", "hunter2-long");
        let config = json!({ "args": ["--token", "${secret:TOKEN}"] });
        let redactor = Redactor::for_config(&config, &secrets);
        assert!(!redactor.is_empty());

        let mut output = json!({ "stdout": "sent hunter2-long to the server" });
        redactor.redact_value(&mut output);
        assert_eq!(output["stdout"], "sent [redacted] to the server");
    }

    #[test]
    fn a_short_secret_is_not_scrubbed() {
        // Replacing every "ab" in a document would corrupt it for no benefit.
        let mut secrets = MapSecrets::default();
        secrets.insert("PIN", "ab");
        let redactor = Redactor::for_config(&json!("${secret:PIN}"), &secrets);
        assert!(redactor.is_empty());
    }

    #[test]
    fn the_redactor_finds_secrets_at_any_depth() {
        let mut secrets = MapSecrets::default();
        secrets.insert("DEEP", "deep-secret-value");
        let config = json!({ "a": [{ "b": ["x", "${secret:DEEP}"] }] });
        let redactor = Redactor::for_config(&config, &secrets);
        assert_eq!(redactor.redact_str("deep-secret-value!"), "[redacted]!");
    }
}
