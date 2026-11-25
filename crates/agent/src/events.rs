//! The SSE event contract between the server and the assistant drawer.
//!
//! `kind()` is the SSE `event:` field; the serialized payload is the `data:`
//! field. Both are part of the wire contract, so changing either is a breaking
//! change for `packages/ide-core/src/stores/agentStore.ts`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::llm::DeltaSink;

/// Token accounting for one run.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Usage {
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub tool_calls: usize,
    pub turns: usize,
}

impl Usage {
    pub fn add(&mut self, tokens_in: u64, tokens_out: u64) {
        self.tokens_in += tokens_in;
        self.tokens_out += tokens_out;
    }

    pub fn total(&self) -> u64 {
        self.tokens_in + self.tokens_out
    }
}

/// One validation finding, as the drawer renders it.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    pub domain: String,
    pub object_id: Option<String>,
    pub message: String,
    pub severity: String,
}

/// Everything the server streams to the client.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Open the drawer timeline.
    RunStarted {
        run_id: String,
        mode: String,
        provider: String,
        model: String,
    },
    /// Pipeline progress.
    StepStarted { step: String, title: String },
    /// Optional narration, when the model says something before calling tools.
    AgentThought { text: String },
    /// Assistant text as it is produced, before the turn is complete.
    ///
    /// Additive: a client that ignores it behaves exactly as before, which is
    /// why this needed no protocol version bump. `seq` is monotonic per run so
    /// a client can drop a duplicate — it is *not* a resume protocol, and a
    /// missing frame is not recoverable.
    AgentDelta {
        text: String,
        step: String,
        seq: u64,
    },
    /// A tool is about to run.
    ToolCall {
        tool: String,
        args: Value,
        step: String,
    },
    /// A tool finished.
    ToolResult {
        tool: String,
        ok: bool,
        summary: String,
        duration_ms: u64,
    },
    /// A whole domain changed. `value` is the domain map, not a JSON Patch.
    ///
    /// `revision` is the **database** revision of the content the client holds.
    /// It does not advance per edit; it advances once, when the run saves. That
    /// keeps it comparable with `project.revision` from the projects table.
    ModelPatch {
        domain: String,
        value: Value,
        revision: u64,
    },
    /// Step 4 result.
    Validation {
        valid: bool,
        issues: Vec<ValidationIssue>,
    },
    /// Full snapshot at milestones and on completion.
    ProjectUpdated { project: Value, revision: u64 },
    /// The save was rejected and the merge found overlapping edits.
    ///
    /// Nothing was written. `objects` names what both sides changed, so the
    /// drawer can say *what* collided instead of "something went wrong".
    Conflict {
        objects: Vec<crate::rebase::Conflict>,
        current_revision: u64,
    },
    /// Terminal event: enables Build & Preview on success.
    RunFinished {
        status: String,
        usage: Usage,
        revision: u64,
        /// True when a stale save was merged and written on the retry.
        rebased: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// A recoverable problem, with a retry affordance.
    Error {
        message: String,
        recoverable: bool,
        kind: String,
    },
}

impl AgentEvent {
    /// The SSE `event:` name. Kept in sync with the serde tag by construction.
    pub fn kind(&self) -> &'static str {
        match self {
            AgentEvent::RunStarted { .. } => "run_started",
            AgentEvent::StepStarted { .. } => "step_started",
            AgentEvent::AgentThought { .. } => "agent_thought",
            AgentEvent::AgentDelta { .. } => "agent_delta",
            AgentEvent::ToolCall { .. } => "tool_call",
            AgentEvent::ToolResult { .. } => "tool_result",
            AgentEvent::ModelPatch { .. } => "model_patch",
            AgentEvent::Validation { .. } => "validation",
            AgentEvent::ProjectUpdated { .. } => "project_updated",
            AgentEvent::Conflict { .. } => "conflict",
            AgentEvent::RunFinished { .. } => "run_finished",
            AgentEvent::Error { .. } => "error",
        }
    }
}

/// Batches token deltas into frames a human can read and a browser can paint.
///
/// One SSE frame per token would push 40–80 frames per second into a channel
/// with a few hundred slots. The channel fills, `send().await` blocks the run
/// loop, and streaming makes runs *slower* than not streaming. Flushing on size
/// or age holds the frame rate near 20/s whatever the model does.
pub struct DeltaCoalescer {
    buffer: String,
    last_flush: Instant,
    /// Shared across the whole run, so `seq` is monotonic per run rather than
    /// per step: a client that drops duplicates must never see it go backwards
    /// when the pipeline moves from one step to the next.
    seq: Arc<AtomicU64>,
    step: String,
}

impl DeltaCoalescer {
    /// Flush threshold in characters — roughly one short line of text.
    pub const MAX_CHARS: usize = 48;

    /// Flush threshold in time. 40 ms caps the frame rate at about 25/s.
    pub const MAX_DELAY: Duration = Duration::from_millis(40);

    pub fn new(step: impl Into<String>) -> Self {
        Self::with_counter(step, Arc::new(AtomicU64::new(0)))
    }

    /// Share a run's sequence counter with this coalescer.
    pub fn with_counter(step: impl Into<String>, seq: Arc<AtomicU64>) -> Self {
        Self {
            buffer: String::new(),
            last_flush: Instant::now(),
            seq,
            step: step.into(),
        }
    }

    /// Add a fragment, returning a frame when the buffer is worth sending.
    pub fn push(&mut self, delta: &str) -> Option<AgentEvent> {
        self.buffer.push_str(delta);
        if self.buffer.chars().count() >= Self::MAX_CHARS
            || self.last_flush.elapsed() >= Self::MAX_DELAY
        {
            return self.flush();
        }
        None
    }

    /// Send whatever is buffered, if anything.
    pub fn flush(&mut self) -> Option<AgentEvent> {
        if self.buffer.is_empty() {
            return None;
        }
        let text = std::mem::take(&mut self.buffer);
        self.last_flush = Instant::now();
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        Some(AgentEvent::AgentDelta {
            text,
            step: self.step.clone(),
            seq,
        })
    }
}

/// The bridge between a provider's deltas and the run's event channel.
///
/// Deltas are *best effort*: a full channel means the client is reading slowly,
/// and dropping a frame costs nothing because the complete text still arrives
/// as `agent_thought` when the turn ends. Real events keep using `emit`, which
/// awaits and therefore applies backpressure.
pub struct EventDeltaSink {
    events: mpsc::Sender<AgentEvent>,
    coalescer: DeltaCoalescer,
    enabled: bool,
}

impl EventDeltaSink {
    pub fn new(events: mpsc::Sender<AgentEvent>, step: impl Into<String>, enabled: bool) -> Self {
        Self::with_counter(events, step, enabled, Arc::new(AtomicU64::new(0)))
    }

    /// Share the run's sequence counter, so `seq` keeps increasing across steps.
    pub fn with_counter(
        events: mpsc::Sender<AgentEvent>,
        step: impl Into<String>,
        enabled: bool,
        seq: Arc<AtomicU64>,
    ) -> Self {
        Self {
            events,
            coalescer: DeltaCoalescer::with_counter(step, seq),
            enabled,
        }
    }

    /// Send whatever is still buffered.
    ///
    /// Called before any non-delta event so the timeline can never show text
    /// *after* the tool call that followed it.
    pub fn finish(&mut self) {
        if let Some(event) = self.coalescer.flush() {
            self.send(event);
        }
    }

    fn send(&self, event: AgentEvent) {
        // A closed channel is already reported by `closed()`; a full one means
        // the reader is behind. Neither is worth failing a run over.
        let _ = self.events.try_send(event);
    }
}

impl DeltaSink for EventDeltaSink {
    fn text(&mut self, delta: &str) {
        if !self.enabled {
            return;
        }
        if let Some(event) = self.coalescer.push(delta) {
            self.send(event);
        }
    }

    fn closed(&self) -> bool {
        self.events.is_closed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_sse_name_matches_the_payload_tag() {
        let event = AgentEvent::ToolCall {
            tool: "entity_create".into(),
            args: json!({}),
            step: "data".into(),
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["type"], "tool_call");
        assert_eq!(event.kind(), "tool_call");
    }

    #[test]
    fn every_event_kind_is_snake_case() {
        let events = [
            AgentEvent::RunStarted {
                run_id: "r".into(),
                mode: "pipeline".into(),
                provider: "anthropic".into(),
                model: "m".into(),
            },
            AgentEvent::StepStarted {
                step: "data".into(),
                title: "Data".into(),
            },
            AgentEvent::AgentThought { text: "t".into() },
            AgentEvent::AgentDelta {
                text: "t".into(),
                step: "data".into(),
                seq: 1,
            },
            AgentEvent::ToolResult {
                tool: "t".into(),
                ok: true,
                summary: "s".into(),
                duration_ms: 1,
            },
            AgentEvent::ModelPatch {
                domain: "entities".into(),
                value: json!([]),
                revision: 1,
            },
            AgentEvent::Validation {
                valid: true,
                issues: vec![],
            },
            AgentEvent::ProjectUpdated {
                project: json!({}),
                revision: 1,
            },
            AgentEvent::Conflict {
                objects: vec![crate::rebase::Conflict {
                    domain: "entities".into(),
                    object_id: "e1".into(),
                }],
                current_revision: 3,
            },
            AgentEvent::RunFinished {
                status: "succeeded".into(),
                usage: Usage::default(),
                revision: 1,
                rebased: false,
                error: None,
            },
            AgentEvent::Error {
                message: "m".into(),
                recoverable: false,
                kind: "internal".into(),
            },
        ];
        for event in events {
            let kind = event.kind();
            assert!(
                kind.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{kind}"
            );
            assert_eq!(serde_json::to_value(&event).unwrap()["type"], kind);
        }
    }

    /// Pull the text out of a delta frame.
    fn text_of(event: Option<AgentEvent>) -> String {
        match event {
            Some(AgentEvent::AgentDelta { text, .. }) => text,
            other => panic!("expected a delta, got {other:?}"),
        }
    }

    #[test]
    fn one_hundred_and_twenty_deltas_become_at_most_four_frames() {
        // The gate from the plan: 120 single-character deltas, no more than
        // four frames, and the concatenation is exactly what the model wrote.
        let mut coalescer = DeltaCoalescer::new("data");
        let mut frames = Vec::new();
        let mut seqs = Vec::new();
        let mut joined = String::new();

        for index in 0..120 {
            let delta = if index % 10 == 0 { "a" } else { "b" };
            joined.push_str(delta);
            if let Some(event) = coalescer.push(delta) {
                if let AgentEvent::AgentDelta { seq, .. } = &event {
                    seqs.push(*seq);
                }
                frames.push(text_of(Some(event)));
            }
        }
        if let Some(event) = coalescer.flush() {
            if let AgentEvent::AgentDelta { seq, .. } = &event {
                seqs.push(*seq);
            }
            frames.push(text_of(Some(event)));
        }

        assert!(frames.len() <= 4, "{} frames", frames.len());
        assert_eq!(frames.concat(), joined);
        // Monotonic, so a client can drop a duplicate.
        assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]), "{seqs:?}");
    }

    #[test]
    fn a_flush_never_sends_an_empty_frame() {
        let mut coalescer = DeltaCoalescer::new("data");
        assert!(coalescer.flush().is_none());
    }

    #[test]
    fn seq_keeps_rising_across_steps() {
        // One counter per run: the pipeline creates a new sink per step, and a
        // client that drops duplicates must not see the number reset.
        let seq = Arc::new(AtomicU64::new(0));
        let mut data = DeltaCoalescer::with_counter("data", seq.clone());
        let mut ui = DeltaCoalescer::with_counter("ui", seq.clone());

        let fill = "x".repeat(DeltaCoalescer::MAX_CHARS);
        let first = data.push(&fill).expect("a full buffer flushes");
        let second = ui.push(&fill).expect("a full buffer flushes");

        let seq_of = |event: AgentEvent| match event {
            AgentEvent::AgentDelta { seq, .. } => seq,
            other => panic!("expected a delta, got {other:?}"),
        };
        assert_eq!(seq_of(first), 1);
        assert_eq!(seq_of(second), 2);
        assert_eq!(seq.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_long_fragment_flushes_immediately() {
        let mut coalescer = DeltaCoalescer::new("data");
        let text = "x".repeat(DeltaCoalescer::MAX_CHARS);
        assert_eq!(text_of(coalescer.push(&text)), text);
        // Nothing left over.
        assert!(coalescer.flush().is_none());
    }

    #[tokio::test]
    async fn a_closed_channel_is_reported_to_the_provider() {
        let (tx, rx) = mpsc::channel(4);
        let sink = EventDeltaSink::new(tx, "data", true);
        assert!(!sink.closed());
        drop(rx);
        assert!(sink.closed(), "a dropped reader must stop the provider");
    }

    #[tokio::test]
    async fn a_disabled_sink_emits_nothing() {
        let (tx, mut rx) = mpsc::channel(4);
        let mut sink = EventDeltaSink::new(tx, "data", false);
        sink.text("ignored");
        sink.finish();
        assert!(rx.try_recv().is_err());
    }
}
