//! Per-run context: the seams the host fills in.
//!
//! Every capability the runtime cannot own lives behind a trait here, so the
//! IDE adapter, the compiled CLI and a test can each supply their own:
//!
//! | seam | IDE | CLI | test |
//! |---|---|---|---|
//! | [`SecretResolver`] | `CredentialVault` | env, then keychain | a map |
//! | [`ApprovalGate`] | a row the canvas writes | a terminal prompt | a queue |
//! | [`SubflowResolver`] | the open project | the bundled project | a map |
//!
//! None of them is optional in the type system, because a run that silently
//! lacks one would fail at the worst possible moment — after a side effect.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use nocode_model::Workflow;

/// What a run starts with.
#[derive(Debug, Clone, Default)]
pub struct RunInput {
    /// Values bound to `${input.*}`.
    pub variables: IndexMap<String, Value>,
}

impl RunInput {
    pub fn new(variables: IndexMap<String, Value>) -> Self {
        Self { variables }
    }

    pub fn empty() -> Self {
        Self::default()
    }
}

/// Where a `${secret:NAME}` comes from.
pub trait SecretResolver: Send + Sync {
    /// `None` means "not available", which fails the node. It never means
    /// "empty string": a blank credential is a misconfiguration that should
    /// surface, not a valid value.
    fn resolve(&self, name: &str) -> Option<String>;
}

/// A resolver that has no secrets. The safe default.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSecrets;

impl SecretResolver for NoSecrets {
    fn resolve(&self, _name: &str) -> Option<String> {
        None
    }
}

/// A resolver over a fixed map, for tests and for a CLI reading env vars.
#[derive(Debug, Default, Clone)]
pub struct MapSecrets(HashMap<String, String>);

impl MapSecrets {
    pub fn new(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        Self(pairs.into_iter().collect())
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0.insert(name.into(), value.into());
    }
}

impl SecretResolver for MapSecrets {
    fn resolve(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

/// A human's answer to an approval card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approve { comment: Option<String> },
    Reject { comment: Option<String> },
}

impl ApprovalDecision {
    pub fn approved() -> Self {
        Self::Approve { comment: None }
    }

    pub fn rejected() -> Self {
        Self::Reject { comment: None }
    }
}

/// Where an approval answer comes from.
///
/// `poll` is non-blocking on purpose: a run that suspends must be able to exit
/// the process (§7.1), so the runtime asks once, gets `None`, and journals a
/// `waiting` outcome. The answer is picked up by the *next* attempt, whether
/// that is a resume seconds later or a resume tomorrow.
pub trait ApprovalGate: Send + Sync {
    fn poll(&self, run_id: &str, node_id: &str) -> Option<ApprovalDecision>;
}

/// Nobody has answered. The default for a headless host.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoApprovals;

impl ApprovalGate for NoApprovals {
    fn poll(&self, _run_id: &str, _node_id: &str) -> Option<ApprovalDecision> {
        None
    }
}

/// Everybody said yes. For tests and for `--yes` in CI.
#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysApproves;

impl ApprovalGate for AlwaysApproves {
    fn poll(&self, _run_id: &str, _node_id: &str) -> Option<ApprovalDecision> {
        Some(ApprovalDecision::approved())
    }
}

/// Where a `subflow` node's target workflow comes from.
pub trait SubflowResolver: Send + Sync {
    fn resolve(&self, workflow_id: &str) -> Option<Workflow>;
}

/// No subflows available. A `subflow` node fails with `not_found`.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSubflows;

impl SubflowResolver for NoSubflows {
    fn resolve(&self, _workflow_id: &str) -> Option<Workflow> {
        None
    }
}

/// Everything a run needs that the runtime does not own.
#[derive(Clone)]
pub struct RunContext {
    pub run_id: String,
    /// The directory `${workspace}` resolves to, and the jail a future `file`
    /// executor will enforce. Phase 35 only needs the value.
    pub workspace: PathBuf,
    /// The run's cancel tree. Every executor gets a child of this.
    pub cancel: CancellationToken,
    pub secrets: Arc<dyn SecretResolver>,
    pub approvals: Arc<dyn ApprovalGate>,
    pub subflows: Arc<dyn SubflowResolver>,
    /// Environment variables `${env.NAME}` may read. Empty denies all, which is
    /// the right default for a graph that arrived as a file.
    pub env_allowlist: Vec<String>,
    /// Nesting level, 0 for the outermost run.
    pub depth: usize,
}

impl RunContext {
    pub fn new(run_id: impl Into<String>, workspace: impl Into<PathBuf>) -> Self {
        Self {
            run_id: run_id.into(),
            workspace: workspace.into(),
            cancel: CancellationToken::new(),
            secrets: Arc::new(NoSecrets),
            approvals: Arc::new(NoApprovals),
            subflows: Arc::new(NoSubflows),
            env_allowlist: Vec::new(),
            depth: 0,
        }
    }

    pub fn with_secrets(mut self, secrets: Arc<dyn SecretResolver>) -> Self {
        self.secrets = secrets;
        self
    }

    pub fn with_approvals(mut self, approvals: Arc<dyn ApprovalGate>) -> Self {
        self.approvals = approvals;
        self
    }

    pub fn with_subflows(mut self, subflows: Arc<dyn SubflowResolver>) -> Self {
        self.subflows = subflows;
        self
    }

    pub fn with_env_allowlist(mut self, names: impl IntoIterator<Item = String>) -> Self {
        self.env_allowlist = names.into_iter().collect();
        self
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// A nested context for a subflow run: same seams, a fresh cancel token
    /// that is still cancelled when the parent is, and `depth + 1`.
    pub fn nested(&self, run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            workspace: self.workspace.clone(),
            cancel: self.cancel.child_token(),
            secrets: self.secrets.clone(),
            approvals: self.approvals.clone(),
            subflows: self.subflows.clone(),
            env_allowlist: self.env_allowlist.clone(),
            depth: self.depth + 1,
        }
    }
}

impl std::fmt::Debug for RunContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The seams are trait objects with no useful Debug, and a resolver
        // could hold a credential, so only the plain fields are printed.
        f.debug_struct("RunContext")
            .field("run_id", &self.run_id)
            .field("workspace", &self.workspace)
            .field("depth", &self.depth)
            .field("env_allowlist", &self.env_allowlist)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_context_defaults_to_denying_secrets_and_approvals() {
        let cx = RunContext::new("r1", "/tmp");
        assert!(cx.secrets.resolve("ANYTHING").is_none());
        assert!(cx.approvals.poll("r1", "n1").is_none());
        assert!(cx.subflows.resolve("w1").is_none());
        assert!(cx.env_allowlist.is_empty(), "env reads are opt-in");
    }

    #[test]
    fn a_nested_context_inherits_seams_and_bumps_depth() {
        let mut secrets = MapSecrets::default();
        secrets.insert("TOKEN", "s3cret");
        let parent = RunContext::new("r1", "/tmp").with_secrets(Arc::new(secrets));

        let child = parent.nested("r2");
        assert_eq!(child.depth, 1);
        assert_eq!(child.run_id, "r2");
        assert_eq!(
            child.secrets.resolve("TOKEN").as_deref(),
            Some("s3cret"),
            "a subflow sees the same secrets"
        );
        assert!(!child.cancel.is_cancelled());
        parent.cancel.cancel();
        assert!(
            child.cancel.is_cancelled(),
            "cancelling the parent must stop the nested run"
        );
    }

    #[test]
    fn always_approves_is_a_real_gate() {
        let gate = AlwaysApproves;
        assert_eq!(gate.poll("r", "n"), Some(ApprovalDecision::approved()));
    }
}
