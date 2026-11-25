//! In-process agent that builds a whole application from one prompt.
//!
//! This crate is the "agentic" half of Milestone 5. It owns no HTTP server and
//! no database: it drives [`nocode_mcp::McpToolExecutor`] — the same tool
//! surface `POST /mcp` exposes — and streams [`AgentEvent`]s to whoever is
//! listening. The IDE server turns those events into SSE; tests turn them into
//! a `Vec`.
//!
//! # Shape of a run
//!
//! ```text
//! prompt ──► Runner ──► provider.stream()  (one turn, deltas to the drawer)
//!               │              │
//!               │              └─► tool calls
//!               │                     │
//!               │                     ├─► executor.call()  ──► model changes
//!               │                     ├─► diff(before, after) ──► model_patch
//!               │                     └─► tool_result
//!               │
//!               ├─► integrity::report() ──► validation
//!               └─► project_save        ──► project_updated / run_finished
//! ```
//!
//! # Why the loop lives here and not in the HTTP handler
//!
//! Three properties are only testable if the loop is a plain function:
//! bounded turns, per-step tool allowlists, and "never save an invalid
//! project". The SSE handler is then a thin adapter: it owns a
//! `tokio::sync::mpsc` channel, spawns [`run`], and forwards events until the
//! sender drops.
//!
//! # Security posture
//!
//! - The model never sees an API key; providers hold it.
//! - The model never calls `project_save`; no step's allowlist contains it.
//! - Tool output and project content are labelled untrusted data in every
//!   system prompt, because a project can contain text that reads like an
//!   instruction.
//! - A tool that is not in the current step's allowlist is refused with an
//!   error result rather than executed.

#![forbid(unsafe_code)]

pub mod config;
pub mod context;
pub mod diff;
pub mod error;
pub mod events;
pub mod llm;
pub mod orchestrator;
pub mod prompts;
pub mod rebase;

pub use config::{AgentConfig, Limits, ProviderKind};
pub use error::AgentError;
pub use events::{AgentEvent, DeltaCoalescer, EventDeltaSink, Usage, ValidationIssue};
pub use llm::{build as build_provider, DeltaSink, LlmProvider, NullSink};
pub use orchestrator::{run, ChatOptions, Mode, RunContext, RunReport, RunRequest, RunStatus};
pub use rebase::{Conflict, Rebase};
