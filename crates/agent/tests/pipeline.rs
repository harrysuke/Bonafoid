//! End-to-end runs: a scripted model, a fake store, the real tools.
//!
//! These tests exist because the properties that matter about the agent are not
//! unit-testable in isolation:
//!
//! - a run must never save an invalid project;
//! - a step must never reach a tool outside its allowlist;
//! - the context card must contain ids created earlier in the same run;
//! - a runaway model must be stopped by the turn budget, not by luck.
//!
//! The scripted provider stands in for the LLM. Everything below it is real:
//! `project_open`, `entity_create`, `page_create`, the integrity validator, and
//! `project_save`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use nocode_agent::llm::{ChatRequest, ChatResponse, LlmProvider, StopReason, ToolCall};
use nocode_agent::{
    run, AgentError, AgentEvent, ChatOptions, DeltaSink, Limits, Mode, RunContext, RunReport,
    RunRequest, RunStatus, Usage,
};
use nocode_mcp::error::McpError;
use nocode_mcp::executor::McpToolExecutor;
use nocode_mcp::provider::{LoadedProject, ProjectStateProvider, ProjectSummaryLite};
use nocode_mcp::CallerContext;
use nocode_model::{Entity, Project};

/* ------------------------------------------------------------------ *
 * A store in a Mutex
 * ------------------------------------------------------------------ */

struct FakeStore {
    project: Mutex<Project>,
    revision: Mutex<u64>,
    saves: Mutex<Vec<(u64, Option<String>)>>,
    /// A project written by "somebody else" just before the next save, to
    /// simulate a second tab saving while the run was thinking.
    inject: Mutex<Option<Project>>,
}

impl FakeStore {
    fn new(project: Project) -> Self {
        Self {
            project: Mutex::new(project),
            revision: Mutex::new(1),
            saves: Mutex::new(Vec::new()),
            inject: Mutex::new(None),
        }
    }

    fn project(&self) -> Project {
        self.project.lock().unwrap().clone()
    }

    fn revision(&self) -> u64 {
        *self.revision.lock().unwrap()
    }

    fn save_count(&self) -> usize {
        self.saves.lock().unwrap().len()
    }

    fn last_message(&self) -> Option<String> {
        self.saves
            .lock()
            .unwrap()
            .last()
            .and_then(|(_, message)| message.clone())
    }

    /// Queue a competing write that lands before the run's next save.
    fn save_elsewhere(&self, project: Project) {
        *self.inject.lock().unwrap() = Some(project);
    }
}

#[async_trait]
impl ProjectStateProvider for FakeStore {
    async fn load_project(
        &self,
        _tenant_id: &str,
        _user_id: &str,
        project_id: &str,
    ) -> Result<LoadedProject, McpError> {
        // The store's key is the project's own id, as it is on the server:
        // a fixture whose key and model disagree makes `rebase` re-load a
        // project id that was never stored.
        let stored = self.project().id.to_string();
        if project_id != stored {
            // Not "forbidden": a provider must not leak existence.
            return Err(McpError::NotFound(project_id.to_string()));
        }
        Ok(LoadedProject {
            project: self.project(),
            revision: self.revision(),
        })
    }

    async fn save_project(
        &self,
        _tenant_id: &str,
        _user_id: &str,
        project: &Project,
        expected_revision: u64,
        message: Option<&str>,
    ) -> Result<u64, McpError> {
        let mut revision = self.revision.lock().unwrap();

        // The competing writer lands first, which is exactly the race the
        // revision guard exists for.
        if let Some(external) = self.inject.lock().unwrap().take() {
            *self.project.lock().unwrap() = external;
            *revision += 1;
        }

        if expected_revision != *revision {
            return Err(McpError::StaleRevision {
                expected: expected_revision,
                current: *revision,
            });
        }
        *self.project.lock().unwrap() = project.clone();
        *revision += 1;
        self.saves
            .lock()
            .unwrap()
            .push((*revision, message.map(str::to_string)));
        Ok(*revision)
    }

    async fn list_projects(
        &self,
        _tenant_id: &str,
        _user_id: &str,
    ) -> Result<Vec<ProjectSummaryLite>, McpError> {
        let project = self.project();
        Ok(vec![ProjectSummaryLite {
            id: project.id.to_string(),
            name: project.name.clone(),
            slug: "expense-tracker".into(),
            revision: self.revision(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }])
    }
}

/* ------------------------------------------------------------------ *
 * A scripted model
 * ------------------------------------------------------------------ */

/// One assistant turn.
enum Turn {
    /// Call these tools, then continue.
    Tools(Vec<(&'static str, Value)>),
    /// Say this and stop the step.
    Text(&'static str),
}

struct ScriptedModel {
    model: String,
    turns: Mutex<VecDeque<Turn>>,
    requests: Mutex<Vec<ChatRequest>>,
    /// How many one-character deltas to emit before the real answer.
    ///
    /// Zero by default, so a test that is not about streaming sees the plain
    /// request/response shape. The streaming test raises it to prove that
    /// deltas change what the drawer shows and nothing about what runs.
    deltas: std::sync::atomic::AtomicUsize,
}

impl ScriptedModel {
    fn new(turns: Vec<Turn>) -> Self {
        Self {
            model: "scripted-1".into(),
            turns: Mutex::new(turns.into()),
            requests: Mutex::new(Vec::new()),
            deltas: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Emit `count` one-character deltas before each turn's answer.
    fn set_deltas(&self, count: usize) {
        self.deltas
            .store(count, std::sync::atomic::Ordering::SeqCst);
    }

    /// The user messages the model was sent, in order.
    fn prompts(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .flat_map(|request| request.messages.iter())
            .filter_map(|message| match message {
                nocode_agent::llm::ChatMessage::User(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}

#[async_trait]
impl LlmProvider for ScriptedModel {
    fn name(&self) -> &'static str {
        "scripted"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, request: ChatRequest) -> Result<ChatResponse, AgentError> {
        self.requests.lock().unwrap().push(request);

        let turn = self
            .turns
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| AgentError::Internal("the script ran out of turns".into()))?;

        Ok(match turn {
            Turn::Tools(calls) => ChatResponse {
                text: None,
                tool_calls: calls
                    .into_iter()
                    .enumerate()
                    .map(|(index, (name, arguments))| ToolCall {
                        id: format!("call_{index}"),
                        name: name.to_string(),
                        arguments,
                    })
                    .collect(),
                usage: Usage {
                    tokens_in: 100,
                    tokens_out: 20,
                    tool_calls: 0,
                    turns: 0,
                },
                stop_reason: StopReason::ToolUse,
            },
            Turn::Text(text) => ChatResponse {
                text: Some(text.to_string()),
                tool_calls: Vec::new(),
                usage: Usage {
                    tokens_in: 100,
                    tokens_out: 20,
                    tool_calls: 0,
                    turns: 0,
                },
                stop_reason: StopReason::EndTurn,
            },
        })
    }

    async fn stream(
        &self,
        request: ChatRequest,
        sink: &mut (dyn DeltaSink + Send),
    ) -> Result<ChatResponse, AgentError> {
        let count = self.deltas.load(std::sync::atomic::Ordering::SeqCst);
        for _ in 0..count {
            // Mirror the real providers: a closed client stops the read.
            if sink.closed() {
                return Err(AgentError::Cancelled);
            }
            sink.text("x");
        }
        let response = self.complete(request).await?;
        if let Some(text) = response.text.as_deref() {
            sink.text(text);
        }
        Ok(response)
    }
}

/* ------------------------------------------------------------------ *
 * Harness
 * ------------------------------------------------------------------ */

struct Harness {
    executor: Arc<McpToolExecutor>,
    store: Arc<FakeStore>,
    model: Arc<ScriptedModel>,
    caller: CallerContext,
    session_id: String,
    limits: Limits,
    sender: mpsc::Sender<AgentEvent>,
    events: mpsc::Receiver<AgentEvent>,
    /// Earlier turns of the conversation, as the server would load them.
    history: Vec<nocode_agent::llm::ChatMessage>,
    /// A compaction summary of turns no longer in `history`.
    summary: Option<String>,
}

impl Harness {
    async fn new(turns: Vec<Turn>, limits: Limits) -> Self {
        Self::with_project(Project::new("Expense tracker"), turns, limits).await
    }

    /// Start from a project that already has content, for tests about merging
    /// into an existing model.
    async fn with_project(project: Project, turns: Vec<Turn>, limits: Limits) -> Self {
        let project_id = project.id.to_string();
        let store = Arc::new(FakeStore::new(project));
        let provider: Arc<dyn ProjectStateProvider> = store.clone();
        let executor = Arc::new(McpToolExecutor::with_provider(
            Arc::new(nocode_mcp::Config::from_env()),
            provider,
        ));
        let caller = CallerContext::authenticated(
            "user-1",
            "tenant-1",
            vec!["project:read".into(), "project:write".into()],
        );

        let opened = executor
            .call(
                &caller,
                "project_open",
                json!({ "project_id": project_id.clone() }),
            )
            .await
            .expect("project_open");
        let payload: Value =
            serde_json::from_str(&tool_text(&opened)).expect("project_open returns JSON");
        let session_id = payload["session_id"]
            .as_str()
            .expect("session_id")
            .to_string();

        let (sender, events) = mpsc::channel(512);
        let model = Arc::new(ScriptedModel::new(turns));

        Self {
            executor,
            store,
            model,
            caller,
            session_id,
            limits,
            sender,
            events,
            history: Vec::new(),
            summary: None,
        }
    }

    /// Seed the conversation the way the server would before a follow-up run.
    fn with_history(&mut self, history: Vec<nocode_agent::llm::ChatMessage>) -> &mut Self {
        self.history = history;
        self
    }

    async fn run(&mut self, prompt: &str, mode: Mode) -> (RunReport, Vec<AgentEvent>) {
        let report = run(
            RunContext {
                run_id: format!("run-{}", self.session_id),
                provider: self.model.clone(),
                executor: self.executor.clone(),
                caller: self.caller.clone(),
                session_id: self.session_id.clone(),
                limits: self.limits,
                chat: ChatOptions::default(),
                events: self.sender.clone(),
                history: self.history.clone(),
                summary: self.summary.clone(),
                conversation_id: Some("conversation-1".into()),
            },
            RunRequest {
                prompt: prompt.to_string(),
                mode,
            },
        )
        .await;

        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
        }
        (report, events)
    }
}

/// The text of an MCP tool-result envelope.
fn tool_text(value: &Value) -> String {
    value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|blocks| blocks.first())
        .and_then(|block| block.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn kinds(events: &[AgentEvent]) -> Vec<&'static str> {
    events.iter().map(AgentEvent::kind).collect()
}

/* ------------------------------------------------------------------ *
 * Tests
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn a_pipeline_run_builds_saves_and_streams() {
    let mut harness = Harness::new(
        vec![
            // Step 1, data.
            Turn::Tools(vec![(
                "entity_create",
                json!({
                    "name": "Expense",
                    "fields": [
                        { "name": "amount", "field_type": "Decimal" },
                        { "name": "note", "field_type": "Text", "nullable": true }
                    ]
                }),
            )]),
            Turn::Text("Created the Expense entity."),
            // Step 2, process: nothing to do for this app.
            Turn::Text("No workflows are needed for this request."),
            // Step 3, ui.
            Turn::Tools(vec![
                ("form_create", json!({ "name": "Expense form" })),
                (
                    "page_create",
                    json!({ "name": "Expenses", "path": "/expenses" }),
                ),
            ]),
            Turn::Text("Created the form and the list page."),
        ],
        Limits::default(),
    )
    .await;

    let (report, events) = harness
        .run("Build me an expense tracker", Mode::Pipeline)
        .await;

    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);
    assert!(report.saved, "a valid project must be saved");
    assert_eq!(report.revision, 2, "one save advances the revision once");
    assert!(report.issues.iter().all(|issue| issue.severity != "error"));

    // The store moved exactly once, with an AI-authored message.
    assert_eq!(harness.store.save_count(), 1);
    assert_eq!(harness.store.revision(), 2);
    let message = harness.store.last_message().expect("a commit message");
    assert!(message.starts_with("AI: "), "{message}");
    assert!(message.contains("expense tracker"), "{message}");

    // The saved project is what the model built.
    let saved = harness.store.project();
    assert_eq!(saved.entities.len(), 1);
    assert_eq!(saved.forms.len(), 1);
    assert_eq!(saved.pages.len(), 1);
    let entity = saved.entities.values().next().unwrap();
    assert_eq!(entity.fields.len(), 2);

    // Event contract: the drawer can render this run.
    let kinds = kinds(&events);
    assert_eq!(kinds.first(), Some(&"run_started"));
    assert_eq!(kinds.last(), Some(&"run_finished"));
    for expected in [
        "step_started",
        "tool_call",
        "tool_result",
        "model_patch",
        "validation",
        "project_updated",
    ] {
        assert!(kinds.contains(&expected), "missing {expected} in {kinds:?}");
    }

    // Three pipeline steps ran, and no verify step was needed.
    let steps: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::StepStarted { step, .. } => Some(step.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(steps, vec!["data", "process", "ui"]);

    // Patches are domain slices, not JSON Patch documents.
    let patched: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ModelPatch { domain, .. } => Some(domain.as_str()),
            _ => None,
        })
        .collect();
    assert!(patched.contains(&"entities"));
    assert!(patched.contains(&"pages"));

    // The validation event the drawer shows, and the terminal status.
    let (valid, issues) = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::Validation { valid, issues } => Some((*valid, issues.len())),
            _ => None,
        })
        .expect("a validation event");
    assert!(valid);
    assert_eq!(issues, 0);

    let (status, revision) = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::RunFinished {
                status, revision, ..
            } => Some((status.clone(), *revision)),
            _ => None,
        })
        .expect("a run_finished event");
    assert_eq!(status, "succeeded");
    assert_eq!(revision, 2);
}

#[tokio::test]
async fn the_ui_step_sees_ids_created_by_the_data_step() {
    let mut harness = Harness::new(
        vec![
            Turn::Tools(vec![("entity_create", json!({ "name": "Expense" }))]),
            Turn::Text("done"),
            Turn::Text("done"),
            Turn::Text("done"),
        ],
        Limits::default(),
    )
    .await;

    let (report, _) = harness
        .run("Build an expense tracker", Mode::Pipeline)
        .await;
    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);

    let entity_id = harness
        .store
        .project()
        .entities
        .keys()
        .next()
        .expect("one entity")
        .to_string();

    // The card is rebuilt between steps, so the third prompt carries the id
    // that did not exist when the run started.
    let prompts = harness.model.prompts();
    let ui_prompt = prompts
        .iter()
        .find(|prompt| prompt.contains("create the forms, pages and theme"))
        .expect("the ui prompt");
    assert!(
        ui_prompt.contains(&entity_id),
        "the ui step must receive the real entity id"
    );
}

#[tokio::test]
async fn an_invalid_project_is_never_saved() {
    let mut harness = Harness::new(
        vec![
            Turn::Text("nothing to model"),
            Turn::Text("nothing to process"),
            // Two pages on the same path: a validation error, not a tool error.
            Turn::Tools(vec![
                (
                    "page_create",
                    json!({ "name": "Expenses", "path": "/expenses" }),
                ),
                (
                    "page_create",
                    json!({ "name": "Expenses copy", "path": "/expenses" }),
                ),
            ]),
            Turn::Text("Created two pages."),
            // The verify step tries, and fails to fix it.
            Turn::Text("I cannot resolve the duplicate path."),
        ],
        Limits::default(),
    )
    .await;

    let (report, events) = harness
        .run("Build an expense tracker", Mode::Pipeline)
        .await;

    assert_eq!(report.status, RunStatus::ValidationFailed);
    assert!(!report.saved, "an invalid project must not be saved");
    assert_eq!(harness.store.save_count(), 0);
    assert_eq!(harness.store.revision(), 1);
    assert!(
        report.issues.iter().any(|issue| issue.severity == "error"),
        "expected an integrity error: {:?}",
        report.issues
    );

    // The verify step did run, because validation failed.
    let steps: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::StepStarted { step, .. } => Some(step.as_str()),
            _ => None,
        })
        .collect();
    assert!(steps.contains(&"verify"), "{steps:?}");
}

#[tokio::test]
async fn a_tool_outside_the_step_allowlist_is_refused() {
    let mut harness = Harness::new(
        vec![
            // `page_create` belongs to the ui step; the data step must refuse it.
            Turn::Tools(vec![("page_create", json!({ "name": "Expenses" }))]),
            Turn::Text("moving on"),
            Turn::Text("nothing to process"),
            Turn::Text("nothing to build"),
        ],
        Limits::default(),
    )
    .await;

    let (report, events) = harness
        .run("Build an expense tracker", Mode::Pipeline)
        .await;

    // The refusal is a tool error the model can read, not a run failure.
    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);

    let refused = events.iter().any(|event| match event {
        AgentEvent::ToolResult {
            tool, ok, summary, ..
        } => tool == "page_create" && !ok && summary.contains("not allowed"),
        _ => false,
    });
    assert!(
        refused,
        "the refusal must be reported as a failed tool call"
    );

    // Nothing was created by the refused call.
    assert_eq!(harness.store.project().pages.len(), 0);
}

#[tokio::test]
async fn the_turn_budget_stops_a_runaway_model() {
    let mut harness = Harness::new(
        // The model never stops calling tools.
        (0..16)
            .map(|index| {
                Turn::Tools(vec![(
                    "entity_create",
                    json!({ "name": format!("Entity{index}") }),
                )])
            })
            .collect(),
        Limits {
            max_turns: 3,
            ..Limits::default()
        },
    )
    .await;

    let (report, events) = harness
        .run("Build an expense tracker", Mode::Pipeline)
        .await;

    assert_eq!(report.status, RunStatus::Failed);
    assert!(!report.saved);
    let error = report.error.unwrap_or_default();
    assert!(error.contains("max_turns"), "{error}");

    let kinds = kinds(&events);
    assert!(kinds.contains(&"error"), "{kinds:?}");
    assert_eq!(kinds.last(), Some(&"run_finished"));
}

#[tokio::test]
async fn a_disconnect_mid_stream_stops_before_the_tool_runs() {
    let mut harness = Harness::new(
        vec![Turn::Tools(vec![(
            "entity_create",
            json!({ "name": "Note" }),
        )])],
        Limits::default(),
    )
    .await;
    harness.model.set_deltas(120);
    // The browser tab goes away while the model is still writing.
    harness.events.close();

    let (report, _) = harness.run("Add a Note entity", Mode::Auto).await;

    assert_eq!(report.status, RunStatus::Cancelled, "{:?}", report.error);
    // The provider saw the closed sink on its first delta and refused to hand
    // back a half-read response, so nothing was executed and nothing was saved.
    assert!(harness.store.project().entities.is_empty());
    assert_eq!(harness.store.save_count(), 0);
}

#[tokio::test]
async fn a_closed_channel_cancels_the_run() {
    let mut harness = Harness::new(vec![Turn::Text("nothing to model")], Limits::default()).await;
    // Simulate the browser tab going away.
    harness.events.close();

    let (report, _) = harness
        .run("Build an expense tracker", Mode::Pipeline)
        .await;
    assert_eq!(report.status, RunStatus::Cancelled);
    assert_eq!(harness.store.save_count(), 0);
}

#[tokio::test]
async fn streaming_deltas_change_what_is_shown_and_nothing_else() {
    let mut harness = Harness::new(
        vec![
            Turn::Tools(vec![("entity_create", json!({ "name": "Expense" }))]),
            Turn::Text("Created the Expense entity."),
        ],
        Limits::default(),
    )
    .await;
    // 120 single-character deltas per turn, which is the plan's gate.
    harness.model.set_deltas(120);

    let (report, events) = harness.run("Add an Expense entity", Mode::Auto).await;

    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);

    // The tool ran exactly once: streaming is a display concern and never
    // causes a call to be replayed.
    let tool_calls = events
        .iter()
        .filter(|event| matches!(event, AgentEvent::ToolCall { .. }))
        .count();
    assert_eq!(tool_calls, 1);
    assert_eq!(harness.store.project().entities.len(), 1);

    let deltas: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::AgentDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(!deltas.is_empty(), "the run produced no delta frames");
    // 240 characters of deltas coalesced at 48 characters each: a handful of
    // frames, not one per token.
    assert!(deltas.len() <= 8, "{} delta frames", deltas.len());
    assert!(
        deltas.iter().all(|text| !text.is_empty()),
        "an empty delta frame is pure overhead"
    );

    // The whole answer still arrives, and as narration it arrives *after* the
    // deltas it was flushed from — never text-after-tool.
    let joined = deltas.concat();
    assert!(joined.contains("Created the Expense entity."), "{joined}");
    let last_delta = events
        .iter()
        .rposition(|event| matches!(event, AgentEvent::AgentDelta { .. }))
        .expect("a delta frame");
    let thought = events
        .iter()
        .position(|event| {
            matches!(event, AgentEvent::AgentThought { text, .. }
                if text.contains("Created the Expense entity."))
        })
        .expect("an agent_thought frame");
    assert!(
        last_delta < thought,
        "text must not follow the turn's thought"
    );
}

#[tokio::test]
async fn an_auto_run_uses_one_loop_and_still_saves() {
    let mut harness = Harness::new(
        vec![
            Turn::Tools(vec![("entity_create", json!({ "name": "Note" }))]),
            Turn::Text("Added a Note entity."),
        ],
        Limits::default(),
    )
    .await;

    let (report, events) = harness.run("Add a Note entity", Mode::Auto).await;

    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);
    let steps: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::StepStarted { step, .. } => Some(step.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(steps, vec!["auto"]);
    assert_eq!(harness.store.project().entities.len(), 1);
}

#[tokio::test]
async fn a_save_that_races_another_writer_is_merged_not_lost() {
    let mut harness = Harness::new(
        vec![
            Turn::Tools(vec![("entity_create", json!({ "name": "Expense" }))]),
            Turn::Text("Created the Expense entity."),
            Turn::Text("nothing to process"),
            Turn::Text("nothing to build"),
        ],
        Limits::default(),
    )
    .await;

    // Somebody else saves between the run's load and its save. The objects are
    // disjoint, so the merge must keep both.
    let mut external = harness.store.project();
    let approval = Entity::new("Approval");
    external.entities.insert(approval.id.clone(), approval);
    harness.store.save_elsewhere(external);

    let (report, events) = harness
        .run("Build an expense tracker", Mode::Pipeline)
        .await;

    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);
    assert!(report.rebased, "a stale save must be merged");
    assert_eq!(
        report.revision, 3,
        "load 1, competing save 2, merged save 3"
    );

    let mut names: Vec<String> = harness
        .store
        .project()
        .entities
        .values()
        .map(|entity| entity.name.clone())
        .collect();
    names.sort();
    assert_eq!(names, vec!["Approval", "Expense"]);

    // The client is told the run was merged, so the drawer can say so.
    let rebased = events.iter().any(|event| match event {
        AgentEvent::RunFinished { rebased, .. } => *rebased,
        _ => false,
    });
    assert!(rebased);
}

#[tokio::test]
async fn an_overlapping_change_saves_nothing_and_reports_the_object() {
    // Start from a project that already has the entity both sides will touch.
    let mut base = Project::new("Expense tracker");
    let entity = Entity::new("Expense");
    let entity_id = entity.id.to_string();
    base.entities.insert(entity.id.clone(), entity);

    let mut harness = Harness::with_project(
        base,
        vec![
            Turn::Tools(vec![(
                "entity_add_field",
                json!({
                    "entity_id": entity_id.clone(),
                    "name": "amount",
                    "field_type": "Decimal"
                }),
            )]),
            Turn::Text("Added the amount field."),
            Turn::Text("nothing to process"),
            Turn::Text("nothing to build"),
        ],
        Limits::default(),
    )
    .await;

    // The other writer changes the *same* entity.
    let mut external = harness.store.project();
    for entity in external.entities.values_mut() {
        entity.description = Some("edited elsewhere".into());
    }
    harness.store.save_elsewhere(external);

    let (report, events) = harness.run("Add an amount field", Mode::Pipeline).await;

    assert_eq!(report.status, RunStatus::Conflict);
    assert!(!report.saved, "a conflicting merge must not write");
    assert_eq!(harness.store.save_count(), 0);

    let reported = events.iter().any(|event| match event {
        AgentEvent::Conflict { objects, .. } => objects
            .iter()
            .any(|object| object.domain == "entities" && object.object_id == entity_id),
        _ => false,
    });
    assert!(reported, "the conflict must name the object");
}

/* ------------------------------------------------------------------ *
 * Conversation memory (plan §15.3)
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn a_follow_up_sees_the_previous_turn() {
    use nocode_agent::llm::ChatMessage;

    let mut harness = Harness::new(
        vec![Turn::Text("Set the threshold to 1000.")],
        Limits::default(),
    )
    .await;

    // What the server loads from `agent_messages` before this run.
    harness.with_history(vec![
        ChatMessage::User("Build an expense tracker that escalates above 500.".into()),
        ChatMessage::Assistant {
            text: Some("Created the Expense entity with a 500 threshold.".into()),
            tool_calls: Vec::new(),
        },
    ]);

    let (report, _events) = harness
        .run("Now make the threshold 1000", Mode::Auto)
        .await;
    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);

    // The first request of this run must carry the earlier turn *before* the
    // new prompt, so "the threshold" has something to resolve against.
    let requests = harness.model.requests.lock().unwrap();
    let first = requests.first().expect("one request");
    let prompts: Vec<String> = first
        .messages
        .iter()
        .filter_map(|message| match message {
            ChatMessage::User(text) => Some(text.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(prompts.len(), 2, "{prompts:?}");
    assert!(prompts[0].contains("escalates above 500"), "{prompts:?}");
    assert!(prompts[1].contains("make the threshold 1000"), "{prompts:?}");
}

#[tokio::test]
async fn history_is_injected_once_not_once_per_step() {
    use nocode_agent::llm::ChatMessage;

    let mut harness = Harness::new(
        vec![
            Turn::Text("data done"),
            Turn::Text("process done"),
            Turn::Text("ui done"),
        ],
        Limits::default(),
    )
    .await;
    harness.with_history(vec![ChatMessage::User("earlier question".into())]);

    let (report, _events) = harness.run("build it", Mode::Pipeline).await;
    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);

    // Three steps, three requests: only the first may carry the transcript,
    // otherwise a pipeline run pays for it four times (plan §15.3.2).
    let requests = harness.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    let with_history = requests
        .iter()
        .filter(|request| {
            request
                .messages
                .iter()
                .any(|message| matches!(message, ChatMessage::User(text) if text == "earlier question"))
        })
        .count();
    assert_eq!(with_history, 1, "history must reach exactly one step");
}

#[tokio::test]
async fn a_compaction_summary_reaches_the_system_prompt() {
    let mut harness = Harness::new(vec![Turn::Text("done")], Limits::default()).await;
    harness.summary = Some("The user chose `Expense` as the entity name.".into());

    let (report, _events) = harness.run("continue", Mode::Auto).await;
    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);

    let requests = harness.model.requests.lock().unwrap();
    let system = &requests.first().expect("one request").system;
    assert!(system.contains("Expense"), "{system}");
    assert!(system.contains("authoritative"), "{system}");
}

#[tokio::test]
async fn the_report_carries_the_transcript_without_the_project_card() {
    let mut harness = Harness::new(
        vec![
            Turn::Tools(vec![("entity_create", json!({ "name": "Note" }))]),
            Turn::Text("Added Note."),
        ],
        Limits::default(),
    )
    .await;

    let (report, _events) = harness.run("Add a Note entity", Mode::Auto).await;
    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);

    // user prompt, assistant(tool call), tool result, assistant(text)
    assert_eq!(report.transcript.len(), 4, "{:?}", report.transcript);
    assert!(matches!(
        &report.transcript[0],
        nocode_agent::llm::ChatMessage::User(text) if text == "Add a Note entity"
    ));

    // The stored prompt is the user's words, never the augmented prompt with
    // the project card: the card is re-read every run and must not be replayed.
    let stored = serde_json::to_string(&report.transcript).unwrap();
    assert!(!stored.contains("Current project"), "{stored}");
}
