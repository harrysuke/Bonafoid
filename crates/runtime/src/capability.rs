//! Capabilities: what a node intends to touch.
//!
//! Phase 35 only needs the *vocabulary* — the executor trait reports the
//! capabilities a node will exercise so the canvas can show them and the
//! journal can record them. Phase 36 adds the policy engine that intersects
//! them with `bonafoid.json` and drives the approval flow.
//!
//! A capability is a `kind:action[:target]` string, matching the manifest in
//! the plan (`fs:read`, `shell:exec:cargo`, `net:http:api.github.com`). Keeping
//! it a string rather than an enum means a plugin can introduce a capability
//! without a change here, and the manifest stays readable by a human.

use std::fmt;

use serde::{Deserialize, Serialize};

/// One thing a node is allowed to do.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capability(String);

impl Capability {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The capability family, i.e. everything before the second colon:
    /// `shell:exec:cargo` → `shell:exec`.
    pub fn action(&self) -> &str {
        match self.0.match_indices(':').nth(1) {
            Some((index, _)) => &self.0[..index],
            None => &self.0,
        }
    }

    /// Whether this capability is covered by `granted`.
    ///
    /// A grant may be narrower or wider than the request:
    /// `shell:exec` covers `shell:exec:cargo`, and `shell:exec:git` covers only
    /// itself. Compared segment by segment so `fs:read` never accidentally
    /// covers `fs:read-secret`.
    pub fn is_covered_by(&self, granted: &Capability) -> bool {
        let mut have = granted.0.split(':');
        let mut want = self.0.split(':');
        loop {
            match (have.next(), want.next()) {
                (None, _) => return true,
                (Some(_), None) => return false,
                (Some(a), Some(b)) if a == b => continue,
                _ => return false,
            }
        }
    }

    pub fn fs_read(path: impl AsRef<str>) -> Self {
        Self(format!("fs:read:{}", path.as_ref()))
    }

    pub fn fs_write(path: impl AsRef<str>) -> Self {
        Self(format!("fs:write:{}", path.as_ref()))
    }

    pub fn shell_exec(program: impl AsRef<str>) -> Self {
        Self(format!("shell:exec:{}", program.as_ref()))
    }

    pub fn net_http(host: impl AsRef<str>) -> Self {
        Self(format!("net:http:{}", host.as_ref()))
    }

    pub fn net_llm() -> Self {
        Self("net:llm".to_string())
    }

    pub fn git_write() -> Self {
        Self("git:write".to_string())
    }

    pub fn memory_read() -> Self {
        Self("memory:read".to_string())
    }

    pub fn memory_write() -> Self {
        Self("memory:write".to_string())
    }

    pub fn db_sqlite(path: impl AsRef<str>) -> Self {
        Self(format!("db:sqlite:{}", path.as_ref()))
    }

    /// Calling another workflow in the project.
    pub fn flow_call(workflow_id: impl AsRef<str>) -> Self {
        Self(format!("flow:call:{}", workflow_id.as_ref()))
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Capability {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exact_grant_covers_only_itself() {
        let granted = Capability::new("shell:exec:git");
        assert!(Capability::new("shell:exec:git").is_covered_by(&granted));
        assert!(!Capability::new("shell:exec:cargo").is_covered_by(&granted));
    }

    #[test]
    fn a_family_grant_covers_its_members() {
        let granted = Capability::new("shell:exec");
        assert!(Capability::new("shell:exec:cargo").is_covered_by(&granted));
        assert!(Capability::new("shell:exec:ssh").is_covered_by(&granted));
        assert!(!Capability::new("net:http:x").is_covered_by(&granted));
    }

    #[test]
    fn a_narrower_grant_never_covers_a_wider_request() {
        let granted = Capability::new("fs:read:src/a.rs");
        assert!(!Capability::new("fs:read").is_covered_by(&granted));
    }

    #[test]
    fn a_similar_prefix_is_not_a_match() {
        // Segment comparison, not `starts_with`: `fs:read-secret` must not be
        // covered by `fs:read`.
        let granted = Capability::new("fs:read");
        assert!(!Capability::new("fs:read-secret").is_covered_by(&granted));
    }

    #[test]
    fn action_is_the_first_two_segments() {
        assert_eq!(Capability::new("shell:exec:cargo").action(), "shell:exec");
        assert_eq!(Capability::new("net:llm").action(), "net:llm");
    }

    #[test]
    fn constructors_use_the_manifest_spelling() {
        assert_eq!(Capability::shell_exec("cargo").as_str(), "shell:exec:cargo");
        assert_eq!(Capability::net_llm().as_str(), "net:llm");
        assert_eq!(Capability::git_write().as_str(), "git:write");
    }
}
