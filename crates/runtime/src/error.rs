//! Errors, and the codes a caller branches on.
//!
//! A node failure carries a machine-readable [`NodeErrorCode`] rather than only
//! prose, for the same reason `McpError::StaleRevision` does in M5: the canvas
//! and the compiled CLI need to decide what to do — retry, ask a human, or give
//! up — without pattern-matching on an English sentence.

use std::fmt;

use thiserror::Error;

/// Why a node failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeErrorCode {
    /// The node's payload is unusable. Always a bug in the graph, never
    /// transient.
    InvalidConfig,
    /// A capability was not granted, or a path left the workspace.
    PermissionDenied,
    /// The node's own `timeout_ms` elapsed.
    Timeout,
    /// The run was cancelled while the node was in flight.
    Cancelled,
    /// A child process exited non-zero.
    NonzeroExit,
    /// A remote service asked us to slow down.
    RateLimited,
    /// A structured-output node produced something that did not validate.
    InvalidOutput,
    /// A human rejected an approval.
    Rejected,
    /// A referenced object (workflow, file, secret) does not exist.
    NotFound,
    /// The filesystem or network failed in a way the node did not classify.
    Io,
    /// A defect in the runtime or an executor.
    Internal,
}

impl NodeErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidConfig => "invalid_config",
            Self::PermissionDenied => "permission_denied",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::NonzeroExit => "nonzero_exit",
            Self::RateLimited => "rate_limited",
            Self::InvalidOutput => "invalid_output",
            Self::Rejected => "rejected",
            Self::NotFound => "not_found",
            Self::Io => "io",
            Self::Internal => "internal",
        }
    }

    /// Whether retrying this failure could plausibly succeed.
    ///
    /// A permission error is never retryable — it will fail identically — and
    /// neither is a bad config. `nonzero_exit` is deliberately *not* here: it
    /// is retryable only when the executor opts in via
    /// [`NodeError::with_retryable`], because a failing `cargo test` should not
    /// be run three times by default.
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Timeout | Self::RateLimited | Self::Io)
    }
}

impl fmt::Display for NodeErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A node failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeError {
    pub code: NodeErrorCode,
    pub message: String,
    /// Whether a retry may help. Defaults from the code; an executor can
    /// override it (`nonzero_exit` with a known-flaky command).
    pub retryable: bool,
    /// Extra context kept out of `message` — stderr, a validation report.
    /// Journalled, so it must not contain a secret.
    pub detail: Option<String>,
}

impl NodeError {
    pub fn new(code: NodeErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: code.retryable(),
            detail: None,
        }
    }

    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::new(NodeErrorCode::InvalidConfig, message)
    }

    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::new(NodeErrorCode::PermissionDenied, message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(NodeErrorCode::Timeout, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(NodeErrorCode::NotFound, message)
    }

    pub fn cancelled() -> Self {
        Self::new(NodeErrorCode::Cancelled, "the run was cancelled")
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(NodeErrorCode::Internal, message)
    }

    /// Make this failure retryable (or not), overriding the code's default.
    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for NodeError {}

/// A failure of the journal itself.
#[derive(Debug, Error)]
pub enum JournalError {
    #[error("journal io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("journal record is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// A line that parsed as JSON but not as a record. Reported with its line
    /// number: a hand-edited journal is the usual cause, and the position is
    /// the only useful thing to say about it.
    #[error("journal is corrupt at line {line}: {detail}")]
    Corrupt { line: usize, detail: String },

    /// The run has already been finished, so it cannot be resumed.
    #[error("run `{0}` is already finished and cannot be resumed")]
    AlreadyFinished(String),

    /// A run id that already has a journal. Starting it again would destroy the
    /// record of a run that may have had side effects.
    #[error("run `{0}` already has a journal; use a new id or resume it")]
    AlreadyStarted(String),

    /// The run can still be resumed, so its journal is the only record of work
    /// that may have happened. Refused rather than deleted.
    #[error("run `{0}` can still be resumed; finish or cancel it first")]
    StillResumable(String),

    /// A journal with no `run_started` record — truncated or the wrong file.
    #[error("journal has no run_started record")]
    NotStarted,
}

/// Why a run stopped.
#[derive(Debug, Error)]
pub enum RunError {
    #[error("invalid graph: {0}")]
    InvalidGraph(String),

    #[error("node `{node_name}` failed: {error}")]
    Node {
        node: String,
        node_name: String,
        error: NodeError,
    },

    #[error("run budget exceeded: {limit} ({detail})")]
    BudgetExceeded {
        /// `steps`, `wall_clock`, `tokens`, `cost_usd`, `tool_calls`.
        limit: &'static str,
        detail: String,
    },

    #[error("the run was cancelled")]
    Cancelled,

    /// A node has an intent record but no outcome, and the runtime will not
    /// guess whether its side effect happened.
    #[error(
        "node `{node_name}` needs attention: it started but did not finish, and \
         it is not marked idempotent, so re-running it could repeat a side effect"
    )]
    NeedsAttention {
        node: String,
        node_name: String,
    },

    #[error("sub-workflow nesting exceeded {0} levels")]
    SubflowDepth(usize),

    #[error(transparent)]
    Journal(#[from] JournalError),
}
