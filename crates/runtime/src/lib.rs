//! The agentic workflow runtime.
//!
//! §7 of the plan in one sentence: the same graph the canvas draws and the BPMN
//! simulator previews is walked here by an async token scheduler that writes a
//! journal before every side effect and after every result.
//!
//! The pieces, in the order a run touches them:
//!
//! | Module | Question it answers |
//! |---|---|
//! | [`limits`] | May this run keep going? |
//! | [`context`] | What is outside the graph — secrets, approvals, subflows, cancellation? |
//! | [`interpolate`] | What does `${nodes.x.y}` mean right now? |
//! | [`executor`] | How does one node kind do its work? |
//! | [`executors`] | The two kinds that need no outside world: `approval`, `subflow`. |
//! | [`runner`] | Who runs next, and what gets written down? |
//! | [`journal`] | What survives a crash? |
//! | [`session`] | Where does a host start, and where does it resume? |
//! | [`error`] | What went wrong, in words a node author can act on? |
//! | [`capability`] | Is this node allowed to touch that? |
//! | [`time`] | What time is it, and can a test decide? |
//!
//! # Why the journal comes before the effect
//!
//! §7.3 splits every node into an *intent* record (written before the side
//! effect, fsynced) and an *outcome* record (written after). A crash between
//! them leaves an intent with no outcome, which is the journal's way of saying
//! "something may have happened here and I do not know what". That is the only
//! honest answer available, and §7.4 turns it into a decision: re-run if the
//! node is idempotent, stop and ask a human if it is not.
//!
//! Recording the outcome first would be cheaper and would let the runtime lie.
//!
//! # Why suspension writes nothing
//!
//! There is no `run_suspended` record. A run is resumable exactly when it has
//! no `run_finished` record, so "not done" has one spelling instead of two, and
//! a process that dies mid-flight is indistinguishable from one that suspended
//! on purpose. Both are handled by the same code path, which means the crash
//! path is exercised every time an approval is.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use nocode_runtime::{
//!     ExecutorRegistry, GraphRunner, JsonlJournal, RunContext, RunLimits, SystemClock,
//! };
//!
//! # async fn run(graph: &nocode_model::Workflow) -> Result<(), Box<dyn std::error::Error>> {
//! let journal = JsonlJournal::create(".bonafoid/runs/r1.jsonl")?;
//! let runner = GraphRunner::new(
//!     Arc::new(ExecutorRegistry::new()),
//!     Arc::new(journal),
//!     RunLimits::default(),
//!     Arc::new(SystemClock),
//! );
//! let cx = RunContext::new("r1", "/path/to/workspace");
//! let outcome = runner.run(graph, Default::default(), &cx).await?;
//! if outcome.is_suspended() {
//!     // The journal has no `run_finished`; `runner.resume(..)` continues it.
//! }
//! # Ok(())
//! # }
//! ```

// No crate-level `missing_docs`: the rest of the workspace does not set it, and
// the invariants that actually bite here are documented where they are
// enforced (the journal protocol in `journal`, the resume table in `runner`).

pub mod capability;
pub mod context;
pub mod error;
pub mod executor;
pub mod executors;
pub mod interpolate;
pub mod journal;
pub mod limits;
pub mod runner;
pub mod session;
pub mod time;

// The flat surface. Most callers want `GraphRunner` and a journal and nothing
// else; module paths stay available for the ones that want to be precise.
pub use capability::Capability;
pub use context::{
    AlwaysApproves, ApprovalDecision, ApprovalGate, MapSecrets, NoApprovals, NoSecrets, NoSubflows,
    RunContext, RunInput, SecretResolver, SubflowResolver,
};
pub use error::{JournalError, NodeError, NodeErrorCode, RunError};
pub use executor::{
    agent_config, ExecutorRegistry, NodeContext, NodeExecutor, NodeOutcome, NodeOutput, Usage,
    WaitState,
};
pub use executors::{ApprovalExecutor, SubflowExecutor};
pub use interpolate::{interpolate, interpolate_value, resolve_expression, Redactor, Scope};
pub use journal::{
    graph_hash, Journal, JournalEvent, JournalRecord, JsonlJournal, MemoryJournal, NodeState,
    NodeStatus, RunJournal, RunStatus,
};
pub use limits::{Budget, Limit, RunLimits};
pub use runner::{GraphRunner, RunOutcome, DEFAULT_MAX_OUTPUT_BYTES, MAX_SUBFLOW_DEPTH};
pub use session::{
    RunOutcomeKind, RunPaths, RunPhase, RunReport, RunSession, RunSummary,
};
pub use time::{parse_duration, Clock, ManualClock, SystemClock};

#[cfg(test)]
mod tests {
    /// The crate is only useful if the two halves of the seam agree: an
    /// executor registers under the kind its payload declares, and the runner
    /// looks it up by the same kind. A test that names both spellings is the
    /// cheapest guard against a palette rename breaking dispatch silently.
    #[test]
    fn every_agent_kind_has_a_stable_name() {
        use nocode_model::AgentNodeKind;
        for kind in AgentNodeKind::ALL {
            let name = kind.as_str();
            assert_eq!(
                AgentNodeKind::parse(name),
                Some(kind),
                "`{name}` does not round-trip through parse"
            );
            assert!(!name.is_empty());
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "`{name}` is not kebab-case; the palette and the MCP catalogue both key on it"
            );
        }
    }
}
