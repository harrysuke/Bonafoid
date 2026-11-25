//! The run loop: bounded turns, per-step tool allowlists, streamed patches.
//!
//! One `Runner` drives either a single generalist loop (`Mode::Auto`) or the
//! four-step specialist pipeline (`Mode::Pipeline`). Both share the same turn
//! machinery, so limits, diffing and event ordering cannot diverge.
//!
//! The runner never trusts the model with `project_save`: saving happens here,
//! after validation, exactly once.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use nocode_mcp::executor::McpToolExecutor;
use nocode_mcp::integrity;
use nocode_mcp::CallerContext;

use crate::config::Limits;
use crate::context;
use crate::diff;
use crate::error::AgentError;
use crate::events::{AgentEvent, EventDeltaSink, Usage, ValidationIssue};
use crate::llm::{ChatMessage, ChatRequest, LlmProvider, ToolSchema};
use crate::prompts::{self, Step};
use crate::rebase::{self, Rebase};

/// How the run is driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// One loop with every tool. Best for a small edit.
    Auto,
    /// Data -> Process -> UI -> Verify.
    Pipeline,
}

impl Mode {
    pub fn parse(value: &str) -> Result<Self, AgentError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "pipeline" | "build" => Ok(Mode::Pipeline),
            "auto" | "edit" => Ok(Mode::Auto),
            other => Err(AgentError::Internal(format!(
                "unknown mode `{other}`; expected pipeline or auto"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Pipeline => "pipeline",
        }
    }
}

/// What the user asked for.
#[derive(Debug, Clone)]
pub struct RunRequest {
    pub prompt: String,
    pub mode: Mode,
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    /// Model built, validation passed, revision saved.
    Succeeded,
    /// The model finished but integrity errors remain; nothing was saved.
    ValidationFailed,
    /// Another writer changed the same objects; nothing was saved.
    Conflict,
    /// A provider, limit or transport error ended the run.
    Failed,
    /// The client went away.
    Cancelled,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Succeeded => "succeeded",
            RunStatus::ValidationFailed => "validation_failed",
            RunStatus::Conflict => "conflict",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
        }
    }
}

/// The outcome handed back to the HTTP handler and the audit row.
#[derive(Debug, Clone)]
pub struct RunReport {
    pub run_id: String,
    pub status: RunStatus,
    pub usage: Usage,
    pub revision: u64,
    pub issues: Vec<ValidationIssue>,
    pub saved: bool,
    /// True when a stale save was merged with the current row and written.
    pub rebased: bool,
    pub error: Option<String>,
    /// What this run said, in order, ready to be appended to the conversation.
    ///
    /// Only the *raw* user prompt is recorded, never the augmented prompt with
    /// the project card: the card is re-read from the project on every run, and
    /// storing four copies of it per pipeline run would both bloat the table and
    /// invite a later run to trust a stale card.
    pub transcript: Vec<ChatMessage>,
}

/// Token budget, temperature and streaming for one completion.
#[derive(Debug, Clone, Copy)]
pub struct ChatOptions {
    pub max_tokens: u32,
    pub temperature: f32,
    /// Whether to emit `agent_delta` frames while the model writes.
    ///
    /// A display switch, not a control-flow one: with it off, runs behave
    /// exactly as they did before streaming existed. It lives here because it
    /// is per-run tuning, not per-request identity.
    pub stream_deltas: bool,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self {
            max_tokens: 4096,
            temperature: 0.2,
            stream_deltas: true,
        }
    }
}

/// Everything a run needs. Bundled so the entry point stays under clippy's
/// argument-count limit and callers cannot transpose two `&str`s.
pub struct RunContext {
    /// Correlation id, chosen by the caller.
    ///
    /// The caller owns it so the `run_started` event the browser receives and
    /// the audit row the server writes carry the *same* id. Generating it here
    /// would make the two impossible to join.
    pub run_id: String,
    pub provider: Arc<dyn LlmProvider>,
    pub executor: Arc<McpToolExecutor>,
    pub caller: CallerContext,
    pub session_id: String,
    pub limits: Limits,
    pub chat: ChatOptions,
    pub events: mpsc::Sender<AgentEvent>,
    /// Earlier turns of this conversation, oldest first.
    ///
    /// Injected into the **first** step only: pipeline steps 2–4 already share
    /// the live session and the rebuilt project card, so repeating the
    /// transcript there would multiply the prompt four times for no new
    /// information (plan §15.3.2).
    ///
    /// The transcript supplies *intent*; the session supplies *truth*. Nothing
    /// here is ever used to reconstruct project state.
    pub history: Vec<ChatMessage>,
    /// A compaction summary of turns that are no longer in `history`.
    pub summary: Option<String>,
    /// The conversation this run belongs to, echoed in `run_started`.
    ///
    /// `None` for a host with no conversation store (the desktop agent), which
    /// keeps the event contract additive.
    pub conversation_id: Option<String>,
}

/// Run one agent request to completion.
///
/// Never returns `Err`: every failure is folded into a [`RunReport`] and
/// streamed as an `error` / `run_finished` event, because the HTTP handler has
/// already committed to a 200 SSE response by the time this is called.
pub async fn run(ctx: RunContext, request: RunRequest) -> RunReport {
    let run_id = ctx.run_id.clone();
    let mode = request.mode;
    let provider_name = ctx.provider.name().to_string();
    let model = ctx.provider.model().to_string();

    // The raw prompt is the first transcript row, recorded before anything can
    // fail: a run that dies on turn one still leaves the user's question in the
    // conversation, which is what a follow-up needs to resolve.
    let transcript = vec![ChatMessage::User(request.prompt.clone())];

    let mut runner = Runner {
        provider: ctx.provider,
        executor: ctx.executor,
        caller: ctx.caller,
        session_id: ctx.session_id,
        limits: ctx.limits,
        chat: ctx.chat,
        events: ctx.events,
        started: Instant::now(),
        usage: Usage::default(),
        prompt: request.prompt,
        mode,
        status: RunStatus::Failed,
        issues: Vec::new(),
        saved: false,
        rebased: false,
        revision: 0,
        base: None,
        delta_seq: Arc::new(AtomicU64::new(0)),
        error: None,
        history: ctx.history,
        summary: ctx.summary,
        history_injected: false,
        transcript,
    };

    let _ = runner
        .emit(AgentEvent::RunStarted {
            run_id: run_id.clone(),
            mode: mode.as_str().to_string(),
            provider: provider_name,
            model,
            conversation_id: ctx.conversation_id,
        })
        .await;

    if let Err(error) = runner.execute().await {
        runner.fail(&error).await;
    }

    let status = runner.status;
    let _ = runner
        .emit(AgentEvent::RunFinished {
            status: status.as_str().to_string(),
            usage: runner.usage,
            revision: runner.revision,
            rebased: runner.rebased,
            error: runner.error.clone(),
        })
        .await;

    RunReport {
        run_id,
        status,
        usage: runner.usage,
        revision: runner.revision,
        issues: runner.issues,
        saved: runner.saved,
        rebased: runner.rebased,
        error: runner.error,
        transcript: runner.transcript,
    }
}

struct Runner {
    provider: Arc<dyn LlmProvider>,
    executor: Arc<McpToolExecutor>,
    caller: CallerContext,
    session_id: String,
    limits: Limits,
    chat: ChatOptions,
    events: mpsc::Sender<AgentEvent>,
    started: Instant,
    usage: Usage,
    prompt: String,
    mode: Mode,
    status: RunStatus,
    issues: Vec<ValidationIssue>,
    saved: bool,
    rebased: bool,
    revision: u64,
    /// The project as this run loaded it. The base of the three-way merge, and
    /// the only snapshot that makes a rebase meaningful: it is what "the run's
    /// changes" are measured against.
    base: Option<nocode_model::Project>,
    /// Monotonic counter for `agent_delta.seq`, shared by every step of this
    /// run so the number never goes backwards mid-build.
    delta_seq: Arc<AtomicU64>,
    error: Option<String>,
    /// Earlier turns, injected once (see `RunContext::history`).
    history: Vec<ChatMessage>,
    /// Compaction summary, folded into the system prompt.
    summary: Option<String>,
    /// Whether `history` has been handed to a step yet.
    history_injected: bool,
    /// What this run said, for the conversation store.
    transcript: Vec<ChatMessage>,
}

impl Runner {
    async fn execute(&mut self) -> Result<(), AgentError> {
        // Captured before any edit, so a later rebase can tell which objects
        // this run touched.
        self.base = self.snapshot().ok();

        match self.mode {
            Mode::Pipeline => self.pipeline().await?,
            Mode::Auto => self.auto_loop().await?,
        }
        self.finish().await
    }

    /* -------------------------------------------------------------- *
     * Modes
     * -------------------------------------------------------------- */

    async fn auto_loop(&mut self) -> Result<(), AgentError> {
        self.emit(AgentEvent::StepStarted {
            step: "auto".into(),
            title: "Editing the project".into(),
        })
        .await?;
        let card = context::render(&self.snapshot()?);
        let prompt = format!(
            "The user asked for:\n{}\n\nCurrent project (real ids — use these):\n{card}",
            self.prompt
        );
        let system = format!(
            "{}\n\n{}",
            prompts::GUARDRAILS,
            prompts::step_system(Step::Data)
        );
        self.run_step(Step::Data, &system, &prompt, Step::all_tools())
            .await
    }

    async fn pipeline(&mut self) -> Result<(), AgentError> {
        for step in [Step::Data, Step::Process, Step::Ui] {
            self.emit(AgentEvent::StepStarted {
                step: step.as_str().into(),
                title: step.title().into(),
            })
            .await?;

            // The card is rebuilt from the live session between steps: this is
            // what stops step 3 inventing entity ids.
            let card = context::render(&self.snapshot()?);
            let prompt = prompts::step_prompt(step, &self.prompt, &card);
            let system = format!("{}\n\n{}", prompts::GUARDRAILS, prompts::step_system(step));
            self.run_step(step, &system, &prompt, step.allowed_tools())
                .await?;

            self.emit_snapshot().await?;
        }

        // A quiet check: `finish()` emits the `validation` event the client
        // sees. Emitting here too would put two on the wire for one run.
        let (valid, issues) = self.validation()?;
        self.issues = issues;

        if !valid {
            self.emit(AgentEvent::StepStarted {
                step: Step::Verify.as_str().into(),
                title: Step::Verify.title().into(),
            })
            .await?;
            let card = context::render(&self.snapshot()?);
            let prompt = format!(
                "{}\n\n{}",
                prompts::step_prompt(Step::Verify, &self.prompt, &card),
                prompts::repair_prompt(&self.issues)
            );
            let system = format!(
                "{}\n\n{}",
                prompts::GUARDRAILS,
                prompts::step_system(Step::Verify)
            );
            self.run_step(Step::Verify, &system, &prompt, Step::Verify.allowed_tools())
                .await?;
            self.emit_snapshot().await?;
        }
        Ok(())
    }

    /// Validate, save when clean, and settle the run status.
    async fn finish(&mut self) -> Result<(), AgentError> {
        let (valid, issues) = self.validation()?;
        self.issues = issues;
        self.emit(AgentEvent::Validation {
            valid,
            issues: self.issues.clone(),
        })
        .await?;

        if !valid {
            self.status = RunStatus::ValidationFailed;
            self.error = Some(format!(
                "{} integrity error(s) remain; the project was not saved",
                self.issues
                    .iter()
                    .filter(|issue| issue.severity == "error")
                    .count()
            ));
            return Ok(());
        }

        self.save().await?;
        // `save` sets the status itself when it ends in a conflict; a run that
        // wrote the row is a success.
        if self.saved {
            self.status = RunStatus::Succeeded;
        }
        Ok(())
    }

    /* -------------------------------------------------------------- *
     * Turns
     * -------------------------------------------------------------- */

    /// The message list a step starts from.
    ///
    /// History is handed to the **first** step only. Steps 2–4 already see the
    /// live session and a freshly rebuilt project card, so repeating the
    /// transcript would quadruple the prompt and add nothing — and, worse, would
    /// let an old turn's "the threshold is 500" compete with a card that says
    /// 750. The project wins; the prompt says so.
    fn seed_messages(&mut self, prompt: &str) -> Vec<ChatMessage> {
        let mut messages = Vec::with_capacity(self.history.len() + 2);
        if !self.history_injected && !self.history.is_empty() {
            messages.extend(self.history.iter().cloned());
            self.history_injected = true;
        }
        messages.push(ChatMessage::User(prompt.to_string()));
        messages
    }

    /// The step's system prompt, with any compaction summary appended.
    fn system_with_summary(&self, system: &str) -> String {
        match &self.summary {
            Some(summary) if !summary.trim().is_empty() => format!(
                "{system}\n\n{}\n{summary}",
                "Earlier in this conversation (a summary; the project below is authoritative \
                 wherever the two disagree):"
            ),
            _ => system.to_string(),
        }
    }

    async fn run_step(
        &mut self,
        step: Step,
        system: &str,
        prompt: &str,
        allow: &[&str],
    ) -> Result<(), AgentError> {
        // The summary belongs in the system prompt, not in the message list:
        // it is standing background, not something the user said.
        let system = self.system_with_summary(system);

        let tools: Vec<ToolSchema> = self
            .executor
            .list_tools_filtered(allow)
            .into_iter()
            .map(|tool| {
                // Project tools act on the session this run was started for, so
                // `session_id` is not the model's to supply: it is injected on
                // the way out and hidden here, so the prompt and the schema
                // agree and a hallucinated id cannot reach the executor.
                let input_schema = if project_scoped(&tool.name) {
                    hide_session_argument(tool.input_schema)
                } else {
                    tool.input_schema
                };
                ToolSchema {
                    name: tool.name,
                    description: tool.description,
                    input_schema,
                }
            })
            .collect();

        let mut messages = self.seed_messages(prompt);
        for _turn in 0..self.limits.max_turns {
            self.check_budget()?;

            // Text reaches the drawer while it is being written. The sink is
            // finished before anything else is emitted, so the timeline can
            // never show narration *after* the tool call it introduced.
            let mut sink = EventDeltaSink::with_counter(
                self.events.clone(),
                step.as_str(),
                self.chat.stream_deltas,
                self.delta_seq.clone(),
            );
            let response = self
                .provider
                .stream(
                    ChatRequest {
                        system: system.to_string(),
                        messages: messages.clone(),
                        tools: tools.clone(),
                        max_tokens: self.chat.max_tokens,
                        temperature: self.chat.temperature,
                    },
                    &mut sink,
                )
                .await;
            sink.finish();
            let response = response?;

            self.usage
                .add(response.usage.tokens_in, response.usage.tokens_out);
            self.usage.turns += 1;

            if let Some(text) = response.text.clone().filter(|text| !text.trim().is_empty()) {
                self.emit(AgentEvent::AgentThought { text }).await?;
            }

            // Record the turn even when it carries no tool calls. The final
            // answer is the part of a run a follow-up most needs ("now make the
            // threshold 1000" has to see what was said about the threshold), and
            // returning before the push dropped exactly that message.
            let assistant = ChatMessage::Assistant {
                text: response.text.clone(),
                tool_calls: response.tool_calls.clone(),
            };
            self.transcript.push(assistant.clone());

            if response.tool_calls.is_empty() {
                return Ok(());
            }

            messages.push(assistant);

            for call in response.tool_calls {
                if !allow.contains(&call.name.as_str()) {
                    // The model saw a filtered tool list, so this is either a
                    // hallucination or an attempt to escape the allowlist.
                    // Report it as a failed call rather than dropping it: the
                    // drawer should show the attempt and the refusal.
                    let message = format!("tool `{}` is not allowed in this step", call.name);
                    self.emit(AgentEvent::ToolCall {
                        tool: call.name.clone(),
                        args: call.arguments.clone(),
                        step: step.as_str().to_string(),
                    })
                    .await?;
                    self.emit(AgentEvent::ToolResult {
                        tool: call.name.clone(),
                        ok: false,
                        summary: message.clone(),
                        duration_ms: 0,
                    })
                    .await?;
                    let refused = ChatMessage::ToolResult {
                        call_id: call.id.clone(),
                        tool: call.name.clone(),
                        content: format!("error: {message}"),
                    };
                    self.transcript.push(refused.clone());
                    messages.push(refused);
                    continue;
                }
                let (ok, content) = self.call_tool(&call.name, call.arguments, step).await?;
                let result = ChatMessage::ToolResult {
                    call_id: call.id,
                    tool: call.name,
                    content: if ok {
                        content
                    } else {
                        format!("error: {content}")
                    },
                };
                // Stored as `{ok, summary, ids}` when it is large: the next run
                // needs ids and success, and can re-read the rest from the
                // session (plan §15.3.1).
                self.transcript
                    .push(crate::memory::compact_tool_result(&result));
                messages.push(result);
            }
        }
        Err(AgentError::Limit("max_turns"))
    }

    /// Execute one tool call and stream its events.
    ///
    /// A tool failure is **not** an `Err`: it is returned as content so the
    /// model can read the message and correct itself.
    async fn call_tool(
        &mut self,
        name: &str,
        arguments: Value,
        step: Step,
    ) -> Result<(bool, String), AgentError> {
        self.usage.tool_calls += 1;
        if self.usage.tool_calls > self.limits.max_tool_calls {
            return Err(AgentError::Limit("max_tool_calls"));
        }

        let arguments = self.scoped_arguments(name, arguments);

        self.emit(AgentEvent::ToolCall {
            tool: name.to_string(),
            args: arguments.clone(),
            step: step.as_str().to_string(),
        })
        .await?;

        let before = self.snapshot()?;
        let started = Instant::now();
        let result = self.executor.call(&self.caller, name, arguments).await;
        let elapsed = started.elapsed().as_millis() as u64;

        let (ok, content) = match result {
            Ok(value) => {
                let ok = !value
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                (ok, envelope_text(&value))
            }
            Err(error) => (false, error.to_string()),
        };

        // Patch before reporting the result, so the canvas is already correct
        // when the timeline row turns green.
        if let Ok(after) = self.snapshot() {
            let revision = self.database_revision().unwrap_or(0);
            for patch in diff::diff(&before, &after) {
                self.emit(AgentEvent::ModelPatch {
                    domain: patch.domain.to_string(),
                    value: patch.value,
                    revision,
                })
                .await?;
            }
        }

        self.emit(AgentEvent::ToolResult {
            tool: name.to_string(),
            ok,
            summary: truncate(&content, 300),
            duration_ms: elapsed,
        })
        .await?;

        Ok((ok, content))
    }

    /* -------------------------------------------------------------- *
     * Persistence and validation
     * -------------------------------------------------------------- */

    async fn save(&mut self) -> Result<(), AgentError> {
        let message = format!("AI: {}", headline(&self.prompt));
        match self.write_project(&message, None).await {
            Ok(revision) => self.commit(revision).await,
            // Somebody else saved while the model was working. Merge rather
            // than throw the run away; the merge either writes both sides or
            // reports the objects that truly collide.
            Err(SaveOutcome::Stale) => self.rebase_and_save(&message).await,
            Err(SaveOutcome::Fatal(error)) => {
                self.error = Some(error.to_string());
                Err(error)
            }
        }
    }

    /// Write the session to storage at `expected` (or the session's own guard).
    async fn write_project(
        &mut self,
        message: &str,
        expected: Option<u64>,
    ) -> Result<u64, SaveOutcome> {
        let mut arguments = json!({ "session_id": self.session_id, "message": message });
        if let Some(expected) = expected {
            arguments["expected_revision"] = json!(expected);
        }

        match self
            .executor
            .call(&self.caller, "project_save", arguments)
            .await
        {
            Ok(value) if !is_error(&value) => {
                // The tool result carries the revision the store assigned. That
                // is the only authoritative number: the session's own counters
                // are edit counts, not row versions.
                Ok(saved_revision(&value)
                    .or_else(|| self.database_revision().ok())
                    .unwrap_or(0))
            }
            Ok(value) if envelope_code(&value) == Some("stale_revision") => Err(SaveOutcome::Stale),
            Ok(value) => {
                let detail = envelope_text(&value);
                Err(SaveOutcome::Fatal(AgentError::Tool {
                    tool: "project_save".into(),
                    message: detail,
                }))
            }
            // `project_save` reports a revision conflict as a *transport* error
            // when the store raises it rather than the tool envelope returning
            // `isError`. Both spellings mean the same thing to this loop: the
            // row moved on, so merge instead of failing the run. Treating only
            // the envelope form as stale turned every concurrent save into a
            // failed run — the exact data-loss bug §15.2 exists to remove.
            Err(nocode_mcp::error::McpError::StaleRevision { .. }) => Err(SaveOutcome::Stale),
            Err(error) => Err(SaveOutcome::Fatal(error.into())),
        }
    }

    /// Merge this run's changes onto the current row and save once.
    ///
    /// Deliberately capped at one retry: a second `stale_revision` after a
    /// successful merge means a third writer is active, and looping would just
    /// spend tokens racing them.
    async fn rebase_and_save(&mut self, message: &str) -> Result<(), AgentError> {
        let Some(base) = self.base.clone() else {
            let error = AgentError::Session("no snapshot to merge against".into());
            self.error = Some(error.to_string());
            return Err(error);
        };
        let theirs = self.snapshot()?;
        let project_id = theirs.id.to_string();

        let provider = self
            .executor
            .provider()
            .map_err(|error| AgentError::Session(error.to_string()))?
            .clone();
        let current = provider
            .load_project(&self.caller.tenant_id, &self.caller.user_id, &project_id)
            .await
            .map_err(|error| AgentError::Session(error.to_string()))?;

        match rebase::rebase(&base, &theirs, &current.project)? {
            Rebase::Conflicts(objects) => {
                self.emit(AgentEvent::Conflict {
                    objects: objects.clone(),
                    current_revision: current.revision,
                })
                .await?;
                self.status = RunStatus::Conflict;
                self.error = Some(format!(
                    "{} object(s) changed in another session; nothing was saved",
                    objects.len()
                ));
                Ok(())
            }
            Rebase::Clean(merged) => {
                let key = self.caller.session_key(&self.session_id);
                self.executor.projects.replace(&key, *merged)?;

                // The merge can produce a project neither side validated (a
                // delete here plus a relation there), so validate again.
                let (valid, issues) = self.validation()?;
                self.issues = issues;
                if !valid {
                    self.emit(AgentEvent::Validation {
                        valid,
                        issues: self.issues.clone(),
                    })
                    .await?;
                    self.status = RunStatus::ValidationFailed;
                    self.error =
                        Some("the merged project failed validation; nothing was saved".into());
                    return Ok(());
                }

                match self.write_project(message, Some(current.revision)).await {
                    Ok(revision) => {
                        self.rebased = true;
                        self.commit(revision).await
                    }
                    Err(SaveOutcome::Stale) => {
                        self.status = RunStatus::Conflict;
                        self.error = Some(
                            "the project changed again during the merge; nothing was saved".into(),
                        );
                        Ok(())
                    }
                    Err(SaveOutcome::Fatal(error)) => {
                        self.error = Some(error.to_string());
                        Err(error)
                    }
                }
            }
        }
    }

    /// Record a successful write and publish the new snapshot.
    async fn commit(&mut self, revision: u64) -> Result<(), AgentError> {
        self.saved = true;
        self.revision = revision;
        self.emit_snapshot().await
    }

    /// Validate the session once and return both the verdict and the findings.
    fn validation(&self) -> Result<(bool, Vec<ValidationIssue>), AgentError> {
        let project = self.snapshot()?;
        let issues = integrity::report(&project);
        let valid = integrity::is_valid(&issues);
        Ok((
            valid,
            issues
                .into_iter()
                .map(|issue| ValidationIssue {
                    domain: issue.domain,
                    object_id: issue.object_id,
                    message: issue.message,
                    severity: issue.severity.as_str().to_string(),
                })
                .collect(),
        ))
    }

    async fn emit_snapshot(&mut self) -> Result<(), AgentError> {
        let project = self.snapshot()?;
        let value = serde_json::to_value(&project)?;
        let revision = self.database_revision().unwrap_or(0);
        self.emit(AgentEvent::ProjectUpdated {
            project: value,
            revision,
        })
        .await
    }

    /* -------------------------------------------------------------- *
     * Plumbing
     * -------------------------------------------------------------- */

    fn snapshot(&self) -> Result<nocode_model::Project, AgentError> {
        let key = self.caller.session_key(&self.session_id);
        self.executor
            .projects
            .snapshot(&key)
            .map_err(|error| AgentError::Session(error.to_string()))
    }

    /// Tool arguments with the server-owned session id injected.
    ///
    /// The model cannot choose a session: any value it supplies is replaced
    /// here. That is both a usability fix (every project tool needs the id, and
    /// the model was never told it) and a safety one — a session id that
    /// arrived in tool output or project content cannot redirect an edit into
    /// someone else's session.
    fn scoped_arguments(&self, tool: &str, arguments: Value) -> Value {
        if !project_scoped(tool) {
            return arguments;
        }
        let mut object = match arguments {
            Value::Object(map) => map,
            // A non-object is a malformed call. The session id still goes in,
            // so the error the model reads is about its own arguments rather
            // than a missing session.
            _ => serde_json::Map::new(),
        };
        object.insert(
            "session_id".to_string(),
            Value::String(self.session_id.clone()),
        );
        Value::Object(object)
    }

    /// The database revision of the content the client now holds.
    ///
    /// Deliberately not the session's edit counter: `revision` appears in
    /// `model_patch`, `project_updated` and `run_finished`, and the client
    /// compares all three against `project.revision` from the projects table.
    /// It therefore must not jump to an edit count that no row ever had.
    /// During a run it stays at the revision the session was loaded from, and
    /// moves once, when the run saves.
    fn database_revision(&self) -> Result<u64, AgentError> {
        let key = self.caller.session_key(&self.session_id);
        self.executor
            .projects
            .with_read(&key, |session| Ok(session.saved_revision))
            .map_err(|error| AgentError::Session(error.to_string()))
    }

    fn check_budget(&self) -> Result<(), AgentError> {
        if self.started.elapsed() > self.limits.max_wall_clock {
            return Err(AgentError::Limit("max_wall_clock"));
        }
        if self.usage.total() > self.limits.max_tokens {
            return Err(AgentError::Limit("max_tokens"));
        }
        Ok(())
    }

    async fn emit(&self, event: AgentEvent) -> Result<(), AgentError> {
        self.events
            .send(event)
            .await
            .map_err(|_| AgentError::Cancelled)
    }

    async fn fail(&mut self, error: &AgentError) {
        self.status = if matches!(error, AgentError::Cancelled) {
            RunStatus::Cancelled
        } else {
            RunStatus::Failed
        };
        self.error = Some(error.to_string());
        let _ = self
            .emit(AgentEvent::Error {
                message: error.to_string(),
                recoverable: error.is_recoverable(),
                kind: error.kind().to_string(),
            })
            .await;
    }
}

/// Whether a tool operates on the project session rather than a workflow one.
///
/// The scope table in `nocode_mcp` already enumerates exactly the tools that
/// read or mutate a project; reusing it keeps the two lists from drifting.
fn project_scoped(tool: &str) -> bool {
    matches!(tool, "project_open" | "project_get")
        || nocode_mcp::transport::http::is_project_write(tool)
}

/// Remove the server-owned `session_id` argument from a tool schema.
///
/// The model is never told the session id, so advertising it in the schema
/// would invite a guess. Only the root-level `properties` are touched, which is
/// where `schemars` puts a struct's fields.
fn hide_session_argument(mut schema: Value) -> Value {
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.remove("session_id");
    }
    if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|name| name.as_str() != Some("session_id"));
    }
    schema
}

/// Extract the text of an MCP tool-result envelope.
fn envelope_text(value: &Value) -> String {
    value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|blocks| blocks.first())
        .and_then(|block| block.get("text"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

/// Whether a tool result is flagged as an error.
fn is_error(value: &Value) -> bool {
    value
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The machine-readable failure kind of a tool result, when there is one.
fn envelope_code(value: &Value) -> Option<&str> {
    value.get("code").and_then(Value::as_str)
}

/// Why a write did not happen. Staleness is recoverable; everything else is not.
enum SaveOutcome {
    Stale,
    Fatal(AgentError),
}

/// The `revision` field of a `project_save` result, which is JSON inside the
/// envelope's text block.
fn saved_revision(envelope: &Value) -> Option<u64> {
    let text = envelope_text(envelope);
    serde_json::from_str::<Value>(&text)
        .ok()?
        .get("revision")?
        .as_u64()
}

fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut out: String = value.chars().take(limit).collect();
    out.push('…');
    out
}

/// A one-line description of the request for the revision message.
fn headline(prompt: &str) -> String {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("agent run");
    truncate(line, 120)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_parse_with_sensible_defaults() {
        assert_eq!(Mode::parse("").unwrap(), Mode::Pipeline);
        assert_eq!(Mode::parse("auto").unwrap(), Mode::Auto);
        assert!(Mode::parse("chaos").is_err());
    }

    #[test]
    fn statuses_are_snake_case() {
        for status in [
            RunStatus::Succeeded,
            RunStatus::ValidationFailed,
            RunStatus::Conflict,
            RunStatus::Failed,
            RunStatus::Cancelled,
        ] {
            assert!(status
                .as_str()
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }

    #[test]
    fn a_stale_save_is_recognised_by_its_code_not_its_prose() {
        let stale = json!({
            "content": [{ "type": "text", "text": "stale revision: expected 3, current 4" }],
            "isError": true,
            "code": "stale_revision"
        });
        assert!(is_error(&stale));
        assert_eq!(envelope_code(&stale), Some("stale_revision"));

        // A different failure must not be mistaken for one worth merging.
        let missing = json!({
            "content": [{ "text": "not found: entity e1" }],
            "isError": true,
            "code": "not_found"
        });
        assert_eq!(envelope_code(&missing), Some("not_found"));
        assert!(!is_error(&json!({ "content": [], "isError": false })));
    }

    #[test]
    fn envelopes_are_unwrapped() {
        let value = json!({ "content": [{ "type": "text", "text": "hello" }], "isError": false });
        assert_eq!(envelope_text(&value), "hello");
    }

    #[test]
    fn the_saved_revision_is_read_from_the_envelope() {
        let value = json!({
            "content": [{
                "type": "text",
                "text": "{\"saved\":true,\"revision\":7,\"project_id\":\"p\"}"
            }],
            "isError": false
        });
        assert_eq!(saved_revision(&value), Some(7));
        // A result that is not JSON has no revision, which is not an error.
        assert_eq!(
            saved_revision(&json!({ "content": [{ "text": "not json" }] })),
            None
        );
    }

    #[test]
    fn headlines_stay_on_one_line() {
        assert_eq!(
            headline("Build an expense app\nwith approvals"),
            "Build an expense app"
        );
        assert_eq!(headline("   "), "agent run");
    }

    #[test]
    fn only_project_tools_are_session_scoped() {
        // Project tools get the server's session id injected.
        assert!(project_scoped("entity_create"));
        assert!(project_scoped("page_add_component"));
        assert!(project_scoped("project_get"));
        // Workflow tools keep the workflow session the model was given.
        assert!(!project_scoped("add_node"));
        assert!(!project_scoped("validate_workflow"));
        assert!(!project_scoped("simulate_workflow"));
    }

    #[test]
    fn project_schemas_hide_the_session_argument() {
        let schema = json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string" },
                "name": { "type": "string" }
            },
            "required": ["session_id", "name"]
        });
        let hidden = hide_session_argument(schema);
        assert!(hidden["properties"].get("session_id").is_none());
        assert_eq!(hidden["properties"]["name"]["type"], "string");
        assert_eq!(hidden["required"], json!(["name"]));
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        let text = "é".repeat(10);
        assert_eq!(truncate(&text, 5).chars().count(), 6);
    }
}
