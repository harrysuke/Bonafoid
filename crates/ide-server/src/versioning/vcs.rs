//! Pure version-control primitives used by the versioning API.
//!
//! The schema carries a `UNIQUE INDEX ON project_revisions(parent_id)`, so the
//! commit graph is a **chain**, not a DAG: a revision can have at most one
//! child. The HTTP layer therefore always appends to the project's newest
//! revision and moves only the branch pointer being operated on. Branches are
//! cheap pointers into that chain, and a merge advances a pointer (fast
//! forward) or appends one new revision (explicit conflict policy). Nothing in
//! this module touches the database, which keeps the algorithms testable in
//! isolation.

use std::collections::HashSet;

use serde::Serialize;
use serde_json::{Map, Value};

/// Maximum length (in characters) of a version-control branch name.
pub const MAX_BRANCH_LEN: usize = 64;
/// Upper bound on ancestor traversal; a corrupt cycle must not hang a request.
const MAX_ANCESTOR_STEPS: usize = 100_000;

/// One line of a deterministic, line-based diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    /// `add`, `remove` or `context`.
    pub kind: &'static str,
    /// The line text, without a trailing newline.
    pub text: String,
}

impl DiffLine {
    fn add(text: String) -> Self {
        Self { kind: "add", text }
    }

    fn remove(text: String) -> Self {
        Self {
            kind: "remove",
            text,
        }
    }

    fn context(text: String) -> Self {
        Self {
            kind: "context",
            text,
        }
    }
}

/// A proper longest-common-subsequence diff.
///
/// Ties are resolved deterministically: when skipping a line from `a` and a
/// line from `b` would leave the same-length subsequence, the line from `a`
/// is emitted as `remove` first. Given the same inputs this always returns
/// the same sequence, which is what the `/diff` endpoint promises.
pub fn lcs_diff(a_lines: &[String], b_lines: &[String]) -> Vec<DiffLine> {
    let n = a_lines.len();
    let m = b_lines.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a_lines[i] == b_lines[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut lines = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a_lines[i] == b_lines[j] {
            lines.push(DiffLine::context(b_lines[j].clone()));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            lines.push(DiffLine::remove(a_lines[i].clone()));
            i += 1;
        } else {
            lines.push(DiffLine::add(b_lines[j].clone()));
            j += 1;
        }
    }
    while i < n {
        lines.push(DiffLine::remove(a_lines[i].clone()));
        i += 1;
    }
    while j < m {
        lines.push(DiffLine::add(b_lines[j].clone()));
        j += 1;
    }
    lines
}

/// Branch names are `^[a-z0-9-]{1,64}$`: safe for URLs, SQL parameters and log
/// lines, with no separators that could confuse route matching.
pub fn validate_branch(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_BRANCH_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Whether `ancestor` is reachable from `descendant` by following parents.
///
/// `parent_of` is a lookup closure so this stays database-agnostic and easy to
/// test. A revision is its own ancestor. Corrupt cycles return `false`.
pub fn is_ancestor<F>(ancestor: &str, descendant: &str, parent_of: F) -> bool
where
    F: Fn(&str) -> Option<String>,
{
    if ancestor == descendant {
        return true;
    }
    let mut current = descendant.to_string();
    let mut seen = HashSet::new();
    for _ in 0..MAX_ANCESTOR_STEPS {
        if !seen.insert(current.clone()) {
            return false;
        }
        let Some(parent) = parent_of(&current) else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        current = parent;
    }
    false
}

/// Resolve an explicit merge conflict policy to the winning JSON payload.
///
/// `fast_forward` does not resolve a conflict — the caller must first verify
/// ancestry — but it maps to the source state, which is what the target sees
/// once its pointer is advanced. Unknown strategies are rejected so no
/// implicit last-write-wins can sneak in.
pub fn merge_strategy_wins(strategy: &str, ours: &Value, theirs: &Value) -> Result<Value, String> {
    match strategy {
        "fast_forward" | "theirs" => Ok(theirs.clone()),
        "ours" => Ok(ours.clone()),
        other => Err(format!("unsupported merge strategy `{other}`")),
    }
}

/// Config keys whose values are credentials, case-insensitively.
const SECRET_KEY_MARKERS: &[&str] = &[
    "secret",
    "token",
    "password",
    "passwd",
    "api_key",
    "apikey",
    "access_key",
    "private_key",
    "credential",
    "authorization",
];

fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SECRET_KEY_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

/// Recursively remove secret material from an environment config.
///
/// Object keys that look like credentials are replaced with the shared
/// `***REDACTED***` marker and their children are never traversed. Remaining
/// strings pass through `nocode_deploy::redact` so an inline PAT in an
/// otherwise innocent field is masked too. The result is safe to store in
/// `config_json` and safe to return in an API response.
pub fn redact_env_config(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, entry) in map {
                if is_secret_key(key) {
                    out.insert(
                        key.clone(),
                        Value::String(nocode_deploy::REDACTED.to_string()),
                    );
                } else {
                    out.insert(key.clone(), redact_env_config(entry));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_env_config).collect()),
        Value::String(text) => Value::String(nocode_deploy::redact(text)),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lines(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn lcs_diff_marks_context_add_and_remove() {
        let a = lines(&["one", "two", "three"]);
        let b = lines(&["one", "TWO", "three", "four"]);
        let diff = lcs_diff(&a, &b);
        assert_eq!(diff[0], DiffLine::context("one".to_string()));
        assert_eq!(diff[1], DiffLine::remove("two".to_string()));
        assert_eq!(diff[2], DiffLine::add("TWO".to_string()));
        assert_eq!(diff[3], DiffLine::context("three".to_string()));
        assert_eq!(diff[4], DiffLine::add("four".to_string()));
    }

    #[test]
    fn lcs_diff_is_deterministic_and_handles_empty_sides() {
        let a = lines(&["a", "b", "a"]);
        let b = lines(&["a", "c", "a"]);
        assert_eq!(lcs_diff(&a, &b), lcs_diff(&a, &b));
        assert_eq!(lcs_diff(&[], &b).len(), b.len());
        assert!(lcs_diff(&[], &b).iter().all(|line| line.kind == "add"));
        assert!(lcs_diff(&a, &[]).iter().all(|line| line.kind == "remove"));
        assert!(lcs_diff(&[], &[]).is_empty());
    }

    #[test]
    fn branch_validation_accepts_only_lowercase_slugs() {
        assert!(validate_branch("main"));
        assert!(validate_branch("feature-123"));
        assert!(!validate_branch(""));
        assert!(!validate_branch("Main"));
        assert!(!validate_branch("feature_x"));
        assert!(!validate_branch("has space"));
        assert!(!validate_branch(&"a".repeat(MAX_BRANCH_LEN + 1)));
        assert!(validate_branch(&"a".repeat(MAX_BRANCH_LEN)));
    }

    #[test]
    fn ancestor_traversal_follows_parents_and_stops_on_cycles() {
        let mut parents = std::collections::HashMap::new();
        parents.insert("r2".to_string(), Some("r1".to_string()));
        parents.insert("r1".to_string(), Some("r0".to_string()));
        parents.insert("r0".to_string(), None);
        let lookup = |id: &str| parents.get(id).cloned().flatten();
        assert!(is_ancestor("r0", "r2", lookup));
        assert!(is_ancestor("r2", "r2", lookup));
        assert!(!is_ancestor("r2", "r0", lookup));
        assert!(!is_ancestor("missing", "r2", lookup));

        let mut cyclic = std::collections::HashMap::new();
        cyclic.insert("a".to_string(), Some("b".to_string()));
        cyclic.insert("b".to_string(), Some("a".to_string()));
        assert!(!is_ancestor("c", "a", |id| cyclic
            .get(id)
            .cloned()
            .flatten()));
    }

    #[test]
    fn merge_policy_is_explicit() {
        let ours = json!({"color": "blue"});
        let theirs = json!({"color": "red"});
        assert_eq!(
            merge_strategy_wins("theirs", &ours, &theirs).unwrap(),
            theirs
        );
        assert_eq!(merge_strategy_wins("ours", &ours, &theirs).unwrap(), ours);
        assert_eq!(
            merge_strategy_wins("fast_forward", &ours, &theirs).unwrap(),
            theirs
        );
        assert!(merge_strategy_wins("last-write-wins", &ours, &theirs).is_err());
    }

    #[test]
    fn redaction_removes_nested_secrets_and_inline_tokens() {
        let config = json!({
            "region": "eu-west-1",
            "nested": {"api_key": "ghp_supersecret", "size": 2},
            "url": "https://x-access-token:ghp_inline@github.com/acme/app.git",
            "items": [{"password": "hunter2"}, "plain"],
        });
        let redacted = redact_env_config(&config);
        let rendered = redacted.to_string();
        assert!(!rendered.contains("supersecret"));
        assert!(!rendered.contains("hunter2"));
        assert!(!rendered.contains("ghp_inline"));
        assert_eq!(
            redacted["nested"]["api_key"],
            json!(nocode_deploy::REDACTED)
        );
        assert_eq!(redacted["nested"]["size"], json!(2));
        assert_eq!(redacted["items"][1], json!("plain"));
    }
}
