//! Phase 35's gate, as three runs.
//!
//! > A graph with parallel gateways and an approval suspends and resumes; a
//! > killed process resumes without re-running `ok` nodes; an `Unknown` node
//! > with `idempotent: false` stops the run.
//!
//! Each clause is one test below. They are integration tests rather than unit
//! tests because what is being proved is a property of the *journal across
//! processes*, and a test that never leaves memory cannot show it.
//!
//! The `llm` executor here is a stub. Nothing about the scheduler is specific
//! to a model call, and a stub keeps the gate about the runtime rather than
//! about a provider.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};

use nocode_model::{
    AgentNodeConfig, AgentNodeKind, ApprovalNode, EdgeId, EndEventType, LlmNode, NodeConfig, NodeId,
    NodePosition, StartEventType, Workflow, WorkflowEdge, WorkflowNode, WorkflowId, WorkflowNodeType,
};
use nocode_runtime::{
    graph_hash, ApprovalDecision, ApprovalGate, Capability, ExecutorRegistry, GraphRunner, Journal,
    JournalEvent, JsonlJournal, MemoryJournal, NodeContext, NodeError, NodeExecutor, NodeOutcome,
    NodeStatus, RunContext, RunError, RunLimits, RunStatus, SystemClock,
};

// ---------------------------------------------------------------------------
// Stubs
// ---------------------------------------------------------------------------

/// Counts executions per node *name*, so a test can say "A ran once" across
/// three separate processes.
#[derive(Default)]
struct Counter {
    runs: Mutex<HashMap<String, usize>>,
}

impl Counter {
    fn get(&self, name: &str) -> usize {
        *self.runs.lock().unwrap().get(name).unwrap_or(&0)
    }

    fn bump(&self, name: &str) {
        *self
            .runs
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default() += 1;
    }
}

struct CountingLlm {
    counter: Arc<Counter>,
}

#[async_trait]
impl NodeExecutor for CountingLlm {
    fn kind(&self) -> AgentNodeKind {
        AgentNodeKind::Llm
    }

    fn config_schema(&self) -> Value {
        json!({ "type": "object" })
    }

    fn capabilities(&self, _node: &WorkflowNode) -> Vec<Capability> {
        vec![Capability::net_llm()]
    }

    fn dry_run(&self, _node: &WorkflowNode, _cx: &RunContext) -> Result<(), NodeError> {
        Ok(())
    }

    async fn execute(
        &self,
        node: &WorkflowNode,
        _cx: &NodeContext<'_>,
    ) -> Result<NodeOutcome, NodeError> {
        self.counter.bump(&node.name);
        Ok(NodeOutcome::done(json!({ "ran": node.name })))
    }
}

/// Answers `approved` only after `remaining` polls, so a test can watch a card
/// be presented and then answered on a later attempt.
struct AnswersAfter {
    remaining: AtomicUsize,
}

impl ApprovalGate for AnswersAfter {
    fn poll(&self, _run_id: &str, _node_id: &str) -> Option<ApprovalDecision> {
        let left = self.remaining.load(Ordering::SeqCst);
        if left == 0 {
            return Some(ApprovalDecision::approved());
        }
        self.remaining.store(left - 1, Ordering::SeqCst);
        None
    }
}

// ---------------------------------------------------------------------------
// Graphs
// ---------------------------------------------------------------------------

fn node(name: &str, node_type: WorkflowNodeType) -> WorkflowNode {
    WorkflowNode {
        id: NodeId::new(),
        name: name.to_string(),
        node_type,
        position: NodePosition::default(),
        config: NodeConfig::default(),
        documentation: None,
    }
}

fn start() -> WorkflowNode {
    node(
        "Start",
        WorkflowNodeType::StartEvent {
            event_type: StartEventType::None,
        },
    )
}

fn end() -> WorkflowNode {
    node(
        "End",
        WorkflowNodeType::EndEvent {
            event_type: EndEventType::None,
        },
    )
}

fn llm(name: &str) -> WorkflowNode {
    let mut node = node(name, WorkflowNodeType::AgentTask);
    node.config.agent = Some(AgentNodeConfig::Llm(LlmNode::default()));
    node
}

fn edge(source: &WorkflowNode, target: &WorkflowNode) -> WorkflowEdge {
    WorkflowEdge {
        id: EdgeId::new(),
        source: source.id.clone(),
        target: target.id.clone(),
        source_handle: None,
        target_handle: None,
        condition: None,
        label: None,
        style: Default::default(),
        mapping: Default::default(),
    }
}

/// `start → fork ⇉ (A, B) ⇉ join → gate → after → end`
///
/// A real diamond with a real join, so the scheduler has to hold one token
/// parked while its sibling is still running, and the approval sits *after* the
/// join — where a naive implementation would already have merged.
struct Diamond {
    workflow: Workflow,
}

fn diamond() -> Diamond {
    let start = start();
    let fork = node("Fork", WorkflowNodeType::ParallelGateway);
    let a = llm("A");
    let b = llm("B");
    let join = node("Join", WorkflowNodeType::ParallelGateway);

    // The body reads A's output, which only exists if the join really did
    // synchronise before the gate was reached.
    let mut gate = node("Gate", WorkflowNodeType::AgentTask);
    gate.config.agent = Some(AgentNodeConfig::Approval(ApprovalNode {
        body: Some(format!("A ran as `${{nodes.{}.ran}}`", a.id)),
        ..Default::default()
    }));

    let after = llm("After");
    let end = end();

    let edges = vec![
        edge(&start, &fork),
        edge(&fork, &a),
        edge(&fork, &b),
        edge(&a, &join),
        edge(&b, &join),
        edge(&join, &gate),
        edge(&gate, &after),
        edge(&after, &end),
    ];

    let mut workflow = Workflow::new("Diamond");
    workflow.nodes = vec![start, fork, a, b, join, gate, after, end];
    workflow.edges = edges;
    Diamond { workflow }
}

/// A two-node line: `start → first → second → end`.
fn line() -> (Workflow, NodeId, NodeId) {
    let start = start();
    let first = llm("First");
    let second = llm("Second");
    let end = end();
    let ids = (first.id.clone(), second.id.clone());
    let edges = vec![
        edge(&start, &first),
        edge(&first, &second),
        edge(&second, &end),
    ];
    let mut workflow = Workflow::new("Line");
    workflow.nodes = vec![start, first, second, end];
    workflow.edges = edges;
    (workflow, ids.0, ids.1)
}

fn registry(counter: &Arc<Counter>, limits: &RunLimits) -> Arc<ExecutorRegistry> {
    ExecutorRegistry::new()
        .with(Arc::new(CountingLlm {
            counter: counter.clone(),
        }))
        .with_control_executors(limits.clone(), Arc::new(SystemClock))
}

fn runner(
    registry: Arc<ExecutorRegistry>,
    journal: Arc<dyn Journal>,
    limits: &RunLimits,
) -> GraphRunner {
    GraphRunner::new(registry, journal, limits.clone(), Arc::new(SystemClock))
}

// ---------------------------------------------------------------------------
// Clause 1: parallel gateways plus an approval suspend, then resume
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_parallel_diamond_with_an_approval_suspends_and_resumes() {
    let workflow = diamond().workflow;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.jsonl");
    let limits = RunLimits::default();

    // --- Process 1: nobody answers the card ---
    let counter = Arc::new(Counter::default());
    let first = runner(
        registry(&counter, &limits),
        Arc::new(JsonlJournal::create(&path).unwrap()),
        &limits,
    );
    let cx = RunContext::new("run-1", dir.path());

    let outcome = first
        .run(&workflow, Default::default(), &cx)
        .await
        .expect("a suspended run is not an error");

    assert!(outcome.is_suspended(), "the card was never answered");
    let wait = outcome.suspended.clone().unwrap();
    assert_eq!(wait.kind, "approval");
    assert_eq!(
        wait.body.as_deref(),
        Some("A ran as `A`"),
        "the join completed before the gate, and the body is interpolated"
    );
    assert_eq!(counter.get("A"), 1, "A ran once");
    assert_eq!(counter.get("B"), 1, "B ran once");
    assert_eq!(counter.get("After"), 0, "nothing past the gate");

    // A suspended run leaves no `run_finished`. That is the whole protocol.
    let suspended = JsonlJournal::load(&path).unwrap();
    assert!(!suspended.is_finished(), "suspended is not finished");
    assert_eq!(
        suspended.records.last().unwrap().event.tag(),
        "node_outcome",
        "the last thing written is the waiting outcome"
    );

    // --- Process 2: someone answers ---
    let counter = Arc::new(Counter::default());
    let resumed = runner(
        registry(&counter, &limits),
        Arc::new(JsonlJournal::open(&path).unwrap()),
        &limits,
    );
    let cx = RunContext::new("run-1", dir.path()).with_approvals(Arc::new(AnswersAfter {
        remaining: AtomicUsize::new(0),
    }));

    let outcome = resumed
        .resume(JsonlJournal::load(&path).unwrap(), &workflow, &cx)
        .await
        .expect("the resume should finish");

    assert!(!outcome.is_suspended(), "the card was answered");
    assert_eq!(counter.get("A"), 0, "A's `ok` record must be reused");
    assert_eq!(counter.get("B"), 0, "B's `ok` record must be reused");
    assert_eq!(counter.get("After"), 1, "only what was left ran");
    assert_eq!(outcome.output["ran"], json!("After"));

    // The strongest statement of "did not re-run": one intent per node, ever.
    let journal = JsonlJournal::load(&path).unwrap();
    assert!(journal.is_finished());
    assert_eq!(journal.records[0].event.tag(), "run_started");
    for name in ["A", "B", "After"] {
        let intents = journal
            .records
            .iter()
            .filter(|record| match &record.event {
                JournalEvent::NodeIntent { node_name, .. } => node_name == name,
                _ => false,
            })
            .count();
        assert_eq!(intents, 1, "`{name}` was dispatched {intents} times");
    }

    let presented = journal
        .records
        .iter()
        .filter(|record| match &record.event {
            JournalEvent::NodeOutcome {
                node_name, status, ..
            } => node_name == "Gate" && *status == NodeStatus::Waiting,
            _ => false,
        })
        .count();
    assert_eq!(presented, 1, "the card was presented exactly once");
}

#[tokio::test]
async fn a_suspended_run_can_be_answered_on_a_later_attempt() {
    // The same graph, but the gate answers only after being asked twice — what
    // a resume with a still-unanswered card looks like.
    let workflow = diamond().workflow;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.jsonl");
    let limits = RunLimits::default();

    // A fresh counter per process, so "0" means "did not run in *this*
    // process" rather than "did not run at all".
    let first_counter = Arc::new(Counter::default());
    let first = runner(
        registry(&first_counter, &limits),
        Arc::new(JsonlJournal::create(&path).unwrap()),
        &limits,
    );
    let cx = RunContext::new("run-1", dir.path());
    assert!(first
        .run(&workflow, Default::default(), &cx)
        .await
        .unwrap()
        .is_suspended());
    assert_eq!(first_counter.get("A"), 1);

    // Resume 1: the human still has not clicked.
    let second_counter = Arc::new(Counter::default());
    let resumed = runner(
        registry(&second_counter, &limits),
        Arc::new(JsonlJournal::open(&path).unwrap()),
        &limits,
    );
    let cx = RunContext::new("run-1", dir.path()).with_approvals(Arc::new(AnswersAfter {
        remaining: AtomicUsize::new(1),
    }));
    let outcome = resumed
        .resume(JsonlJournal::load(&path).unwrap(), &workflow, &cx)
        .await
        .unwrap();
    assert!(outcome.is_suspended(), "one poll, no answer");
    assert_eq!(second_counter.get("A"), 0, "A is still not re-run");
    assert_eq!(second_counter.get("After"), 0);

    // Resume 2: clicked.
    let third_counter = Arc::new(Counter::default());
    let resumed = runner(
        registry(&third_counter, &limits),
        Arc::new(JsonlJournal::open(&path).unwrap()),
        &limits,
    );
    let cx = RunContext::new("run-1", dir.path()).with_approvals(Arc::new(AnswersAfter {
        remaining: AtomicUsize::new(0),
    }));
    let outcome = resumed
        .resume(JsonlJournal::load(&path).unwrap(), &workflow, &cx)
        .await
        .unwrap();
    assert!(!outcome.is_suspended());
    assert_eq!(third_counter.get("A"), 0, "still not re-run, three processes later");
    assert_eq!(third_counter.get("After"), 1);
}

// ---------------------------------------------------------------------------
// Clause 2: a killed process resumes without re-running `ok` nodes
// ---------------------------------------------------------------------------

/// The journal a process leaves if it is killed immediately after `First`
/// succeeded: `run_started`, an intent, an `ok`, and nothing else.
fn journal_killed_after_first(workflow: &Workflow, first: &NodeId, path: &std::path::Path) {
    let journal = JsonlJournal::create(path).unwrap();
    journal
        .append(JournalEvent::RunStarted {
            run_id: "run-killed".into(),
            workflow_id: workflow.id.to_string(),
            workflow_name: workflow.name.clone(),
            graph_hash: graph_hash(workflow),
            input: Value::Null,
        })
        .unwrap();
    journal
        .append(JournalEvent::NodeIntent {
            node: first.to_string(),
            node_name: "First".into(),
            kind: "llm".into(),
            attempt: 1,
            input_hash: "irrelevant".into(),
        })
        .unwrap();
    journal
        .append(JournalEvent::NodeOutcome {
            node: first.to_string(),
            node_name: "First".into(),
            status: NodeStatus::Ok,
            attempt: 1,
            output: Some(json!({ "ran": "First" })),
            usage: None,
            code: None,
            message: None,
            detail: None,
            since: None,
        })
        .unwrap();
    // No `run_finished`: the process died here.
}

#[tokio::test]
async fn a_killed_process_resumes_without_re_running_ok_nodes() {
    let (workflow, first, _second) = line();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.jsonl");
    journal_killed_after_first(&workflow, &first, &path);

    let counter = Arc::new(Counter::default());
    let limits = RunLimits::default();
    let resumed = runner(
        registry(&counter, &limits),
        Arc::new(JsonlJournal::open(&path).unwrap()),
        &limits,
    );
    let cx = RunContext::new("run-killed", dir.path());

    let outcome = resumed
        .resume(JsonlJournal::load(&path).unwrap(), &workflow, &cx)
        .await
        .expect("a clean journal resumes");

    assert!(!outcome.is_suspended());
    assert_eq!(counter.get("First"), 0, "`ok` means done, not done again");
    assert_eq!(counter.get("Second"), 1, "everything after the kill ran");
    assert_eq!(outcome.output["ran"], json!("Second"));
    assert!(JsonlJournal::load(&path).unwrap().is_finished());
}

#[tokio::test]
async fn a_recorded_output_is_replayed_into_scope_on_resume() {
    // "Did not re-run" is only half the promise. The other half is that the
    // nodes after the crash still see what the earlier ones produced — without
    // that, a resume is a fresh run with a misleading journal.
    let start = start();
    let first = llm("First");
    let first_id = first.id.clone();

    // The probe reads First's output through the ordinary interpolation path,
    // using `documentation` as a free-form template.
    let mut probe = llm("Probe");
    probe.documentation = Some(format!("${{nodes.{first_id}.ran}}"));
    let end = end();

    let mut workflow = Workflow::new("Replay");
    workflow.edges = vec![edge(&start, &first), edge(&first, &probe), edge(&probe, &end)];
    workflow.nodes = vec![start, first, probe, end];

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.jsonl");
    journal_killed_after_first(&workflow, &first_id, &path);

    struct Probe(Arc<Counter>);

    #[async_trait]
    impl NodeExecutor for Probe {
        fn kind(&self) -> AgentNodeKind {
            AgentNodeKind::Llm
        }
        fn config_schema(&self) -> Value {
            json!({ "type": "object" })
        }
        fn capabilities(&self, _node: &WorkflowNode) -> Vec<Capability> {
            Vec::new()
        }
        fn dry_run(&self, _node: &WorkflowNode, _cx: &RunContext) -> Result<(), NodeError> {
            Ok(())
        }
        async fn execute(
            &self,
            node: &WorkflowNode,
            cx: &NodeContext<'_>,
        ) -> Result<NodeOutcome, NodeError> {
            self.0.bump(&node.name);
            let seen = cx.interpolate(node.documentation.as_deref().unwrap_or(""))?;
            Ok(NodeOutcome::done(json!({ "saw": seen })))
        }
    }

    let counter = Arc::new(Counter::default());
    let limits = RunLimits::default();
    let registry = ExecutorRegistry::new()
        .with(Arc::new(Probe(counter.clone())))
        .with_control_executors(limits.clone(), Arc::new(SystemClock));
    let run = runner(
        registry,
        Arc::new(JsonlJournal::open(&path).unwrap()),
        &limits,
    );

    let outcome = run
        .resume(
            JsonlJournal::load(&path).unwrap(),
            &workflow,
            &RunContext::new("run-killed", dir.path()),
        )
        .await
        .unwrap();

    assert_eq!(counter.get("First"), 0, "replayed, not re-executed");
    assert_eq!(counter.get("Probe"), 1);
    assert_eq!(
        outcome.output["saw"],
        json!("First"),
        "the probe read the recorded output of a node it never saw run"
    );
}

#[tokio::test]
async fn a_finished_run_is_refused_rather_than_replayed() {
    let (workflow, _, _) = line();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.jsonl");
    let counter = Arc::new(Counter::default());
    let limits = RunLimits::default();
    let cx = RunContext::new("run-done", dir.path());

    let run = runner(
        registry(&counter, &limits),
        Arc::new(JsonlJournal::create(&path).unwrap()),
        &limits,
    );
    run.run(&workflow, Default::default(), &cx).await.unwrap();

    let run = runner(
        registry(&counter, &limits),
        Arc::new(JsonlJournal::open(&path).unwrap()),
        &limits,
    );
    let error = run
        .resume(JsonlJournal::load(&path).unwrap(), &workflow, &cx)
        .await
        .unwrap_err();
    assert!(
        matches!(error, RunError::Journal(_)),
        "a completed run is not resumable: {error:?}"
    );
    assert_eq!(counter.get("First"), 1, "and nothing ran again");
}

#[tokio::test]
async fn a_journal_from_an_edited_graph_is_refused() {
    // Resuming against a graph that changed since the journal was written would
    // apply recorded outputs to nodes that no longer mean the same thing.
    let (workflow, first, _) = line();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.jsonl");
    journal_killed_after_first(&workflow, &first, &path);

    let mut edited = workflow.clone();
    edited.nodes[1].name = "First, renamed".into();

    let counter = Arc::new(Counter::default());
    let limits = RunLimits::default();
    let run = runner(
        registry(&counter, &limits),
        Arc::new(JsonlJournal::open(&path).unwrap()),
        &limits,
    );
    let error = run
        .resume(
            JsonlJournal::load(&path).unwrap(),
            &edited,
            &RunContext::new("run-killed", dir.path()),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RunError::InvalidGraph(_)), "{error:?}");
    assert_eq!(counter.get("First"), 0);
    assert_eq!(counter.get("Second"), 0);
}

// ---------------------------------------------------------------------------
// Clause 3: an `Unknown` node with `idempotent: false` stops the run
// ---------------------------------------------------------------------------

/// A journal with an intent and no outcome: the effect may or may not have
/// happened, and the journal cannot say.
fn journal_killed_mid_node(workflow: &Workflow, node: &NodeId, name: &str, path: &std::path::Path) {
    let journal = JsonlJournal::create(path).unwrap();
    journal
        .append(JournalEvent::RunStarted {
            run_id: "run-unknown".into(),
            workflow_id: workflow.id.to_string(),
            workflow_name: workflow.name.clone(),
            graph_hash: graph_hash(workflow),
            input: Value::Null,
        })
        .unwrap();
    journal
        .append(JournalEvent::NodeIntent {
            node: node.to_string(),
            node_name: name.into(),
            kind: "llm".into(),
            attempt: 1,
            input_hash: "irrelevant".into(),
        })
        .unwrap();
}

#[tokio::test]
async fn an_unknown_node_that_is_not_idempotent_stops_the_run() {
    let (mut workflow, first, _) = line();
    // The author says: do not guess about this one.
    for node in workflow.nodes.iter_mut() {
        if node.id == first {
            node.config.idempotent = Some(false);
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.jsonl");
    journal_killed_mid_node(&workflow, &first, "First", &path);

    let counter = Arc::new(Counter::default());
    let limits = RunLimits::default();
    let run = runner(
        registry(&counter, &limits),
        Arc::new(JsonlJournal::open(&path).unwrap()),
        &limits,
    );
    let cx = RunContext::new("run-unknown", dir.path());

    let error = run
        .resume(JsonlJournal::load(&path).unwrap(), &workflow, &cx)
        .await
        .unwrap_err();

    match error {
        RunError::NeedsAttention { node_name, .. } => assert_eq!(node_name, "First"),
        other => panic!("expected NeedsAttention, got {other:?}"),
    }
    assert_eq!(
        counter.get("First"),
        0,
        "the node that might have run must not run again"
    );
    // The run is closed, so a second resume cannot quietly try again.
    assert_eq!(
        JsonlJournal::load(&path)
            .unwrap()
            .records
            .last()
            .unwrap()
            .event
            .tag(),
        "run_finished"
    );
}

#[tokio::test]
async fn an_unknown_node_that_is_idempotent_is_re_run() {
    // The same journal, one flag apart. This is what makes the test above mean
    // something: the decision is keyed on the flag, not on "unknown ⇒ stop".
    let (mut workflow, first, _) = line();
    for node in workflow.nodes.iter_mut() {
        if node.id == first {
            node.config.idempotent = Some(true);
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.jsonl");
    journal_killed_mid_node(&workflow, &first, "First", &path);

    let counter = Arc::new(Counter::default());
    let limits = RunLimits::default();
    let run = runner(
        registry(&counter, &limits),
        Arc::new(JsonlJournal::open(&path).unwrap()),
        &limits,
    );
    let cx = RunContext::new("run-unknown", dir.path());

    let outcome = run
        .resume(JsonlJournal::load(&path).unwrap(), &workflow, &cx)
        .await
        .expect("an idempotent node may be repeated");

    assert!(!outcome.is_suspended());
    assert_eq!(counter.get("First"), 1, "re-run because it is safe to");
    assert_eq!(counter.get("Second"), 1);

    // Attempt 2, not attempt 1: numbering continues, so a reader can see that
    // this node ran twice.
    let journal = JsonlJournal::load(&path).unwrap();
    let attempts: Vec<u32> = journal
        .records
        .iter()
        .filter_map(|record| match &record.event {
            JournalEvent::NodeIntent {
                node_name, attempt, ..
            } if node_name == "First" => Some(*attempt),
            _ => None,
        })
        .collect();
    assert_eq!(attempts, vec![1, 2]);
}

// ---------------------------------------------------------------------------
// Supporting properties the gate leans on
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_cancelled_run_is_journalled_and_its_journal_stays_well_formed() {
    let (workflow, _, _) = line();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.jsonl");
    let counter = Arc::new(Counter::default());
    let limits = RunLimits::default();
    let run = runner(
        registry(&counter, &limits),
        Arc::new(JsonlJournal::create(&path).unwrap()),
        &limits,
    );
    let cx = RunContext::new("run-cancel", dir.path());
    cx.cancel.cancel();

    let error = run
        .run(&workflow, Default::default(), &cx)
        .await
        .unwrap_err();
    assert!(matches!(error, RunError::Cancelled), "{error:?}");

    let journal = JsonlJournal::load(&path).unwrap();
    assert!(journal.is_finished());
    match &journal.records.last().unwrap().event {
        JournalEvent::RunFinished { status, .. } => assert_eq!(*status, RunStatus::Cancelled),
        other => panic!("expected run_finished, got {other:?}"),
    }
}

#[tokio::test]
async fn a_step_budget_stops_a_cycle() {
    // A graph that loops back on itself would run forever without the step
    // ceiling. The budget is what makes a cyclic workflow safe to open.
    let start = start();
    let looped = llm("Looped");
    let mut back = edge(&looped, &looped);
    back.condition = None;
    let mut workflow = Workflow::new("Cycle");
    workflow.edges = vec![edge(&start, &looped), back];
    workflow.nodes = vec![start, looped];

    let counter = Arc::new(Counter::default());
    let limits = RunLimits {
        max_steps: 5,
        ..RunLimits::default()
    };
    let run = runner(
        registry(&counter, &limits),
        Arc::new(MemoryJournal::new()),
        &limits,
    );
    let cx = RunContext::new("run-cycle", ".");

    let error = run
        .run(&workflow, Default::default(), &cx)
        .await
        .unwrap_err();
    match error {
        RunError::BudgetExceeded { limit, .. } => assert_eq!(limit, "steps"),
        other => panic!("expected a step budget error, got {other:?}"),
    }
    assert_eq!(counter.get("Looped"), 5, "exactly the budget, not one more");
}

#[tokio::test]
async fn an_edge_binding_reaches_the_target_and_not_its_siblings() {
    // Two branches from one fork, each binding the same port name to a
    // different value. A shared map would let one overwrite the other.
    let start = start();
    let fork = node("Fork", WorkflowNodeType::ParallelGateway);
    let left = llm("Left");
    let right = llm("Right");
    let end = end();

    let mut to_left = edge(&fork, &left);
    to_left
        .mapping
        .insert("side".into(), "${run_id}-left".into());
    let mut to_right = edge(&fork, &right);
    to_right
        .mapping
        .insert("side".into(), "${run_id}-right".into());

    let mut workflow = Workflow::new("Binding");
    workflow.edges = vec![
        edge(&start, &fork),
        to_left,
        to_right,
        edge(&left, &end),
        edge(&right, &end),
    ];
    workflow.nodes = vec![start, fork, left, right, end];

    let seen: Arc<Mutex<HashMap<String, Value>>> = Arc::new(Mutex::new(HashMap::new()));

    struct RecordsSide(Arc<Mutex<HashMap<String, Value>>>);

    #[async_trait]
    impl NodeExecutor for RecordsSide {
        fn kind(&self) -> AgentNodeKind {
            AgentNodeKind::Llm
        }
        fn config_schema(&self) -> Value {
            json!({ "type": "object" })
        }
        fn capabilities(&self, _node: &WorkflowNode) -> Vec<Capability> {
            Vec::new()
        }
        fn dry_run(&self, _node: &WorkflowNode, _cx: &RunContext) -> Result<(), NodeError> {
            Ok(())
        }
        async fn execute(
            &self,
            node: &WorkflowNode,
            cx: &NodeContext<'_>,
        ) -> Result<NodeOutcome, NodeError> {
            let side = cx.scope.vars.get("side").cloned().unwrap_or(Value::Null);
            self.0.lock().unwrap().insert(node.name.clone(), side);
            Ok(NodeOutcome::done(json!({ "ok": true })))
        }
    }

    let limits = RunLimits::default();
    let registry = ExecutorRegistry::new()
        .with(Arc::new(RecordsSide(seen.clone())))
        .with_control_executors(limits.clone(), Arc::new(SystemClock));
    let run = runner(registry, Arc::new(MemoryJournal::new()), &limits);
    let cx = RunContext::new("run-1", ".");
    run.run(&workflow, Default::default(), &cx).await.unwrap();

    let seen = seen.lock().unwrap();
    assert_eq!(seen["Left"], json!("run-1-left"));
    assert_eq!(
        seen["Right"],
        json!("run-1-right"),
        "each branch keeps its own binding"
    );
}

#[tokio::test]
async fn a_malformed_edge_condition_stops_the_run_instead_of_taking_a_branch() {
    // The simulator propagates an invalid condition and so does the start-event
    // path. Treating it as `false` here would send the run down a branch nobody
    // chose and journal a perfectly ordinary success.
    let start = start();
    let fork = node(
        "Fork",
        WorkflowNodeType::ExclusiveGateway { default_flow: None },
    );
    let left = llm("Left");
    let right = llm("Right");
    let end = end();

    let mut to_left = edge(&fork, &left);
    to_left.condition = Some("this is not an expression".into());
    let mut to_right = edge(&fork, &right);
    to_right.condition = None;

    let mut workflow = Workflow::new("Broken");
    workflow.edges = vec![
        edge(&start, &fork),
        to_left,
        to_right,
        edge(&left, &end),
        edge(&right, &end),
    ];
    workflow.nodes = vec![start, fork, left, right, end];

    let counter = Arc::new(Counter::default());
    let limits = RunLimits::default();
    let run = runner(
        registry(&counter, &limits),
        Arc::new(MemoryJournal::new()),
        &limits,
    );
    let error = run
        .run(
            &workflow,
            Default::default(),
            &RunContext::new("r1", "."),
        )
        .await
        .unwrap_err();

    match error {
        RunError::InvalidGraph(message) => assert!(
            message.contains("this is not an expression"),
            "the offending condition is named: {message}"
        ),
        other => panic!("expected InvalidGraph, got {other:?}"),
    }
    assert_eq!(counter.get("Left"), 0, "no branch was taken");
    assert_eq!(counter.get("Right"), 0);
}

#[tokio::test]
async fn a_node_without_an_executor_is_refused_before_anything_runs() {
    // Better to refuse the graph than to fail on the third node after two side
    // effects.
    let (workflow, _, _) = line();
    let limits = RunLimits::default();
    let registry =
        ExecutorRegistry::new().with_control_executors(limits.clone(), Arc::new(SystemClock));
    let run = runner(registry, Arc::new(MemoryJournal::new()), &limits);
    let error = run
        .run(&workflow, Default::default(), &RunContext::new("r1", "."))
        .await
        .unwrap_err();
    match error {
        RunError::InvalidGraph(message) => {
            assert!(message.contains("no executor registered"), "{message}")
        }
        other => panic!("expected InvalidGraph, got {other:?}"),
    }
}

#[test]
fn a_graph_hash_is_stable_across_processes() {
    // Written by hand in the crate because `DefaultHasher` is not stable across
    // Rust releases, and a journal written yesterday must still match today.
    let (workflow, _, _) = line();
    let hash = graph_hash(&workflow);
    assert_eq!(hash, graph_hash(&workflow));
    assert_eq!(hash.len(), 16, "fixed-width hex");
    let mut edited = workflow.clone();
    edited.name = "Renamed".into();
    assert_ne!(hash, graph_hash(&edited));
    assert_ne!(WorkflowId::new(), workflow.id, "sanity: ids are fresh");
}
