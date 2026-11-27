//! The async token scheduler.
//!
//! Same graph the BPMN simulator walks, with three differences (§7.1): nodes
//! are async and fallible, state is journalled before and after every side
//! effect, and the run can be cancelled at any await point.
//!
//! The scheduling loop has three phases, in order, every iteration:
//!
//! 1. **Drain control flow.** Start/end events and gateways need no executor,
//!    so they are resolved inline until a token lands on an `AgentTask`.
//! 2. **Dispatch.** Ready agent tokens go to the registry, up to
//!    `max_parallel`, each with an intent record written first.
//! 3. **Await the first completion** (not all of them), apply its output, write
//!    the outcome, and loop.
//!
//! Resume is the same loop. A journal is turned into a per-node queue of
//! recorded states, and each visit pops one. That is what makes a cyclic graph
//! replay correctly: the second visit to a node consumes the second record, not
//! the first.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use serde_json::{json, Value};
use tokio::task::JoinSet;

use nocode_bpmn::engine::{BpmnEngine, Severity};
use nocode_model::{
    AgentNodeConfig, NodeId, StartEventType, Workflow, WorkflowEdge, WorkflowNode, WorkflowNodeType,
};

use crate::context::{RunContext, RunInput};
use crate::error::{NodeError, RunError};
use crate::executor::{
    agent_config, ExecutorRegistry, NodeContext, NodeOutcome, NodeOutput, Usage, WaitState,
};
use crate::interpolate::{Redactor, Scope};
use crate::journal::{
    graph_hash, Journal, JournalEvent, NodeState, NodeStatus, RunJournal, RunStatus,
};
use crate::limits::{Budget, Limit, RunLimits};
use crate::time::Clock;

/// Default cap on a journalled node output. The run keeps the full value; only
/// the record is truncated, so a 10 MB diff does not bloat the journal.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// How deep a `subflow` may nest before the runtime refuses.
///
/// Not a budget — it is a structural guard. Two workflows that call each other
/// would otherwise recurse until the stack died, and each level would look
/// perfectly reasonable on its own. Eight is deep enough for any real
/// composition and shallow enough that a cycle is caught instantly.
pub const MAX_SUBFLOW_DEPTH: usize = 8;

/// Where a token is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenStatus {
    Ready,
    Running,
    /// Parked until a human answers.
    Waiting,
    /// Parked at a parallel join, waiting for its siblings.
    Parked,
    Done,
    Failed,
}

#[derive(Debug, Clone)]
struct Token {
    id: u64,
    node: NodeId,
    status: TokenStatus,
    /// Port bindings carried by the edge this token arrived on.
    ///
    /// Held on the token, not in the shared scope: two branches of a fork can
    /// bind the same port name to different values, and a shared map would let
    /// one branch silently overwrite the other's input.
    bindings: IndexMap<String, Value>,
}

/// How a run ended.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub run_id: String,
    /// `Some` when the run suspended on a human gate and can be resumed.
    pub suspended: Option<WaitState>,
    /// Every completed node's output, merged in completion order.
    pub output: Value,
    pub usage: Usage,
    /// Node executions in this process, retries included.
    pub steps: usize,
    pub journal_path: Option<std::path::PathBuf>,
}

impl RunOutcome {
    pub fn is_suspended(&self) -> bool {
        self.suspended.is_some()
    }
}

/// One executor's result, carried back through the `JoinSet`.
struct InFlight {
    token: u64,
    node: NodeId,
    node_name: String,
    attempt: u32,
    result: Result<NodeOutcome, NodeError>,
}

/// Drives one graph to completion, suspension, or failure.
pub struct GraphRunner {
    registry: Arc<ExecutorRegistry>,
    journal: Arc<dyn Journal>,
    limits: RunLimits,
    clock: Arc<dyn Clock>,
    max_output_bytes: usize,
}

impl GraphRunner {
    pub fn new(
        registry: Arc<ExecutorRegistry>,
        journal: Arc<dyn Journal>,
        limits: RunLimits,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            registry,
            journal,
            limits,
            clock,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
        }
    }

    pub fn with_max_output_bytes(mut self, bytes: usize) -> Self {
        self.max_output_bytes = bytes;
        self
    }

    pub fn registry(&self) -> &Arc<ExecutorRegistry> {
        &self.registry
    }

    /// Run a graph from its start event.
    pub async fn run(
        &self,
        graph: &Workflow,
        input: RunInput,
        cx: &RunContext,
    ) -> Result<RunOutcome, RunError> {
        self.drive(graph, input, cx, HashMap::new(), HashMap::new())
            .await
    }

    /// Continue a run from its journal.
    ///
    /// The graph is supplied rather than read from the journal: the journal
    /// records a hash, and the caller is responsible for loading the workflow
    /// that hash describes. A mismatch is refused, because resuming against an
    /// edited graph would apply recorded outputs to nodes that no longer exist.
    pub async fn resume(
        &self,
        journal: RunJournal,
        graph: &Workflow,
        cx: &RunContext,
    ) -> Result<RunOutcome, RunError> {
        if journal.is_finished() {
            return Err(RunError::Journal(
                crate::error::JournalError::AlreadyFinished(journal.run_id),
            ));
        }
        if journal.graph_hash != graph_hash(graph) {
            return Err(RunError::InvalidGraph(format!(
                "the graph changed since run `{}` was journalled; start a new run",
                journal.run_id
            )));
        }
        let input = RunInput::new(serde_json::from_value(journal.input.clone()).unwrap_or_default());
        // Attempt numbers continue from the journal so a resumed node's second
        // attempt is journalled as attempt 2, not 1.
        let attempts = journal
            .records
            .iter()
            .filter_map(|record| match &record.event {
                JournalEvent::NodeIntent { node, attempt, .. } => Some((node.clone(), *attempt)),
                _ => None,
            })
            .fold(HashMap::new(), |mut map, (node, attempt)| {
                let entry = map.entry(node).or_insert(0u32);
                *entry = (*entry).max(attempt);
                map
            });
        self.drive(graph, input, cx, recorded_states(&journal), attempts)
            .await
    }

    async fn drive(
        &self,
        graph: &Workflow,
        input: RunInput,
        cx: &RunContext,
        mut recorded: HashMap<String, VecDeque<NodeState>>,
        mut attempts: HashMap<String, u32>,
    ) -> Result<RunOutcome, RunError> {
        // Checked before the graph is even validated: a cycle of workflows is
        // a structural problem, and there is no point inspecting the tenth copy
        // of a graph that should never have been entered.
        if cx.depth > MAX_SUBFLOW_DEPTH {
            return Err(RunError::SubflowDepth(cx.depth));
        }
        self.validate(graph)?;

        let start = graph
            .nodes
            .iter()
            .find(|node| matches!(node.node_type, WorkflowNodeType::StartEvent { .. }))
            .ok_or_else(|| RunError::InvalidGraph("workflow has no start event".into()))?;

        let mut scope = Scope::new(
            cx.run_id.clone(),
            cx.workspace.clone(),
            input.variables.clone(),
            cx.secrets.clone(),
        );
        scope.now = self.clock.now();
        scope.env_allowlist = cx.env_allowlist.clone();
        for variable in &graph.variables {
            if let Some(default) = &variable.default_value {
                scope
                    .vars
                    .entry(variable.name.clone())
                    .or_insert_with(|| default.clone());
            }
        }

        // A resume appends to the journal it came from; a fresh run starts one.
        if recorded.is_empty() && attempts.is_empty() {
            self.journal.append(JournalEvent::RunStarted {
                run_id: cx.run_id.clone(),
                workflow_id: graph.id.to_string(),
                workflow_name: graph.name.clone(),
                graph_hash: graph_hash(graph),
                input: serde_json::to_value(&input.variables).unwrap_or(Value::Null),
            })?;
        }

        let mut tokens = vec![Token {
            id: 0,
            node: start.id.clone(),
            status: TokenStatus::Ready,
            bindings: IndexMap::new(),
        }];
        let mut next_token = 1u64;
        let mut arrivals: HashMap<NodeId, usize> = HashMap::new();
        let mut budget = Budget::default();
        let mut usage = Usage::default();
        let started = Instant::now();
        let mut in_flight: JoinSet<InFlight> = JoinSet::new();
        // Per-token state that the journal does not need but the loop does.
        let mut redactors: HashMap<u64, Redactor> = HashMap::new();
        let mut waits: HashMap<u64, WaitState> = HashMap::new();

        loop {
            // --- Budget: the wall clock is checked before work, every pass ---
            if started.elapsed() > self.limits.max_wall_clock {
                let detail = format!("{:?} elapsed", started.elapsed());
                self.finish(RunStatus::Failed, Some(detail.clone()))?;
                return Err(RunError::BudgetExceeded {
                    limit: Limit::WallClock.as_str(),
                    detail,
                });
            }
            if let Some(limit) = budget.exceeded(&self.limits) {
                let detail = format!(
                    "{} tokens, ${:.2}, {} tool calls",
                    budget.tokens, budget.cost_usd, budget.tool_calls
                );
                self.finish(RunStatus::Failed, Some(detail.clone()))?;
                return Err(RunError::BudgetExceeded {
                    limit: limit.as_str(),
                    detail,
                });
            }

            // --- Phase 1: drain control flow until an executor is needed ---
            loop {
                let Some(index) = tokens
                    .iter()
                    .position(|token| token.status == TokenStatus::Ready)
                else {
                    break;
                };
                let node = node_of(graph, &tokens[index].node)?;
                if !matches!(node.node_type, WorkflowNodeType::AgentTask) {
                    self.step_control(
                        graph,
                        node,
                        index,
                        &mut tokens,
                        &mut next_token,
                        &mut arrivals,
                        &mut scope,
                    )?;
                    continue;
                }

                // An agent node whose visit is already recorded does not need
                // the registry: resolve it here and keep draining.
                let recorded_visit = recorded
                    .get_mut(&node.id.to_string())
                    .and_then(|queue| queue.pop_front());
                match recorded_visit {
                    Some(NodeState::Succeeded { output, usage: used }) => {
                        scope.set_node_output(&node.id.to_string(), output.clone());
                        usage.tokens_in += used.tokens_in;
                        usage.tokens_out += used.tokens_out;
                        usage.cost_usd += used.cost_usd;
                        tokens[index].status = TokenStatus::Done;
                        self.follow(graph, index, &mut tokens, &mut next_token, &mut scope)?;
                        continue;
                    }
                    Some(NodeState::Unknown { .. }) if !self.is_idempotent(node)? => {
                        self.finish(RunStatus::Failed, Some("needs attention".into()))?;
                        return Err(RunError::NeedsAttention {
                            node: node.id.to_string(),
                            node_name: node.name.clone(),
                        });
                    }
                    // `Unknown` on an idempotent node, `Waiting` (the executor
                    // asks the gate again) and `Failed` (its retry) all run.
                    _ => break,
                }
            }

            // --- Phase 2: dispatch ready agent tokens ---
            while in_flight.len() < self.limits.max_parallel {
                let Some(index) = tokens
                    .iter()
                    .position(|token| token.status == TokenStatus::Ready)
                else {
                    break;
                };
                let node = node_of(graph, &tokens[index].node)?.clone();
                let config = agent_config(&node)
                    .map_err(|error| RunError::InvalidGraph(error.to_string()))?;

                if let Some(limit) = budget.charge_step(&self.limits) {
                    let detail = format!("{} steps", budget.steps);
                    self.finish(RunStatus::Failed, Some(detail.clone()))?;
                    return Err(RunError::BudgetExceeded {
                        limit: limit.as_str(),
                        detail,
                    });
                }

                let attempt = attempts
                    .get(&node.id.to_string())
                    .copied()
                    .unwrap_or(0)
                    + 1;
                attempts.insert(node.id.to_string(), attempt);

                self.journal.append(JournalEvent::NodeIntent {
                    node: node.id.to_string(),
                    node_name: node.name.clone(),
                    kind: config.kind().as_str().to_string(),
                    attempt,
                    input_hash: hash_json(config),
                })?;

                let executor = self
                    .registry
                    .get(config.kind())
                    .cloned()
                    .ok_or_else(|| {
                        RunError::InvalidGraph(format!(
                            "no executor registered for `{}`",
                            config.kind().as_str()
                        ))
                    })?;

                let waiting_since = recorded
                    .get(&node.id.to_string())
                    .and_then(|queue| queue.front())
                    .and_then(|state| match state {
                        NodeState::Waiting { since, .. } => Some(*since),
                        _ => None,
                    });

                tokens[index].status = TokenStatus::Running;
                let token_id = tokens[index].id;

                let redactor = Redactor::for_config(
                    &serde_json::to_value(config).unwrap_or(Value::Null),
                    cx.secrets.as_ref(),
                );
                redactors.insert(token_id, redactor.clone());

                let run = cx.clone();
                let clock = self.clock.clone();
                // One scope per dispatch, so this token's edge bindings are
                // visible to its executor and to nothing else.
                let mut token_scope = scope.clone();
                token_scope.vars.extend(tokens[index].bindings.clone());
                let scope = Arc::new(token_scope);
                let node_name = node.name.clone();
                let node_id = node.id.to_string();
                let timeout = node_timeout(&node);

                in_flight.spawn(async move {
                    let context = NodeContext {
                        run: &run,
                        scope: scope.as_ref(),
                        clock: clock.as_ref(),
                        node_id,
                        node_name: node_name.clone(),
                        waiting_since,
                        redactor: &redactor,
                    };
                    let result = match timeout {
                        Some(limit) => {
                            match tokio::time::timeout(limit, executor.execute(&node, &context))
                                .await
                            {
                                Ok(result) => result,
                                Err(_) => Err(NodeError::timeout(format!(
                                    "node exceeded {} ms",
                                    limit.as_millis()
                                ))),
                            }
                        }
                        None => executor.execute(&node, &context).await,
                    };
                    InFlight {
                        token: token_id,
                        node: node.id.clone(),
                        node_name,
                        attempt,
                        result,
                    }
                });
            }

            // --- Phase 3: await the first completion, or a cancel ---
            if !in_flight.is_empty() {
                let joined = tokio::select! {
                    biased;
                    _ = cx.cancel.cancelled() => None,
                    joined = in_flight.join_next() => joined,
                };

                let Some(joined) = joined else {
                    // Cancelled. Dropping the tasks is the cancellation: every
                    // executor must be safe to drop at an await.
                    in_flight.abort_all();
                    self.finish(RunStatus::Cancelled, None)?;
                    return Err(RunError::Cancelled);
                };

                let completed = match joined {
                    Ok(completed) => completed,
                    Err(error) if error.is_cancelled() => {
                        self.finish(RunStatus::Cancelled, None)?;
                        return Err(RunError::Cancelled);
                    }
                    Err(error) => {
                        let detail = error.to_string();
                        self.finish(RunStatus::Failed, Some(detail.clone()))?;
                        return Err(RunError::InvalidGraph(format!(
                            "an executor panicked: {detail}"
                        )));
                    }
                };

                let index = tokens
                    .iter()
                    .position(|token| token.id == completed.token)
                    .ok_or_else(|| {
                        RunError::InvalidGraph("a completion referenced no live token".into())
                    })?;
                let redactor = redactors.remove(&completed.token).unwrap_or_default();

                match completed.result {
                    Ok(NodeOutcome::Done(output)) => {
                        usage.tokens_in += output.usage.tokens_in;
                        usage.tokens_out += output.usage.tokens_out;
                        usage.cost_usd += output.usage.cost_usd;
                        budget.charge_usage(
                            output.usage.total_tokens(),
                            output.usage.cost_usd,
                            output.usage.tool_calls as usize,
                        );

                        scope.set_node_output(&completed.node.to_string(), output.value.clone());
                        self.record_outcome(
                            &completed.node,
                            &completed.node_name,
                            NodeStatus::Ok,
                            completed.attempt,
                            Some(&output),
                            &redactor,
                        )?;

                        tokens[index].status = TokenStatus::Done;
                        self.follow(graph, index, &mut tokens, &mut next_token, &mut scope)?;
                    }
                    Ok(NodeOutcome::Waiting(wait)) => {
                        self.record_waiting(
                            &completed.node,
                            &completed.node_name,
                            completed.attempt,
                            &wait,
                        )?;
                        waits.insert(completed.token, wait);
                        tokens[index].status = TokenStatus::Waiting;
                    }
                    Err(error) => {
                        self.record_error(
                            &completed.node,
                            &completed.node_name,
                            completed.attempt,
                            &error,
                            &redactor,
                        )?;

                        if error.retryable && completed.attempt < self.limits.max_attempts {
                            // Re-queue: the next dispatch writes a new intent,
                            // so the journal shows both attempts.
                            tokens[index].status = TokenStatus::Ready;
                            continue;
                        }

                        tokens[index].status = TokenStatus::Failed;
                        self.finish(
                            RunStatus::Failed,
                            Some(format!("{}: {}", error.code, error.message)),
                        )?;
                        return Err(RunError::Node {
                            node: completed.node.to_string(),
                            node_name: completed.node_name,
                            error,
                        });
                    }
                }
                continue;
            }

            // --- Nothing in flight: decide why ---
            if tokens.iter().any(|token| token.status == TokenStatus::Waiting) {
                let wait = self.wait_state(graph, &tokens, &waits)?;
                // No `run_finished` record: suspended, not done.
                return Ok(RunOutcome {
                    run_id: cx.run_id.clone(),
                    suspended: Some(wait),
                    output: merged_output(&scope),
                    usage,
                    steps: budget.steps,
                    journal_path: self.journal.path().map(|path| path.to_path_buf()),
                });
            }

            if let Some(parked) = tokens
                .iter()
                .find(|token| token.status == TokenStatus::Parked)
            {
                let node = node_of(graph, &parked.node)?;
                let detail = format!(
                    "parallel gateway `{}` is waiting for an arrival that cannot come",
                    node.name
                );
                self.finish(RunStatus::Failed, Some(detail.clone()))?;
                return Err(RunError::InvalidGraph(detail));
            }

            self.finish(RunStatus::Completed, None)?;
            return Ok(RunOutcome {
                run_id: cx.run_id.clone(),
                suspended: None,
                output: merged_output(&scope),
                usage,
                steps: budget.steps,
                journal_path: self.journal.path().map(|path| path.to_path_buf()),
            });
        }
    }

    /// Resolve a node that needs no executor: events and gateways.
    fn step_control(
        &self,
        graph: &Workflow,
        node: &WorkflowNode,
        index: usize,
        tokens: &mut Vec<Token>,
        next_token: &mut u64,
        arrivals: &mut HashMap<NodeId, usize>,
        scope: &mut Scope,
    ) -> Result<(), RunError> {
        match &node.node_type {
            WorkflowNodeType::StartEvent { event_type } => {
                if let StartEventType::Conditional { condition } = event_type {
                    let vars = scope.condition_vars();
                    match nocode_bpmn::expr::evaluate(condition, &vars) {
                        Ok(true) => {}
                        Ok(false) => {
                            tokens[index].status = TokenStatus::Done;
                            return Ok(());
                        }
                        Err(error) => {
                            return Err(RunError::InvalidGraph(format!(
                                "start condition `{condition}` is not valid: {error}"
                            )))
                        }
                    }
                }
                tokens[index].status = TokenStatus::Done;
                self.follow(graph, index, tokens, next_token, scope)?;
            }

            WorkflowNodeType::EndEvent { .. } => {
                tokens[index].status = TokenStatus::Done;
            }

            WorkflowNodeType::ExclusiveGateway { default_flow } => {
                let chosen =
                    self.select_one(graph, node, default_flow.as_ref(), scope)?;
                tokens[index].status = TokenStatus::Done;
                let target = chosen.ok_or_else(|| {
                    RunError::InvalidGraph(format!(
                        "exclusive gateway `{}` has no matching outgoing flow",
                        node.name
                    ))
                })?;
                let edge = graph
                    .edges
                    .iter()
                    .find(|edge| edge.target == target && edge.source == node.id)
                    .ok_or_else(|| {
                        RunError::InvalidGraph(format!(
                            "exclusive gateway `{}` chose a flow that is not its own",
                            node.name
                        ))
                    })?;
                let token = self.token_along(edge, scope, next_token)?;
                tokens.push(token);
            }

            WorkflowNodeType::InclusiveGateway { default_flow } => {
                let mut targets = self.select_many(graph, node, scope)?;
                if targets.is_empty() {
                    if let Some(default) = default_flow.as_ref() {
                        targets = graph
                            .edges
                            .iter()
                            .filter(|edge| &edge.id == default)
                            .map(|edge| edge.target.clone())
                            .collect();
                    }
                }
                if targets.is_empty() {
                    return Err(RunError::InvalidGraph(format!(
                        "inclusive gateway `{}` has no matching outgoing flow",
                        node.name
                    )));
                }
                tokens[index].status = TokenStatus::Done;
                for target in targets {
                    let edge = graph
                        .edges
                        .iter()
                        .find(|edge| edge.target == target && edge.source == node.id)
                        .ok_or_else(|| {
                            RunError::InvalidGraph(format!(
                                "inclusive gateway `{}` chose a flow that is not its own",
                                node.name
                            ))
                        })?;
                    let token = self.token_along(edge, scope, next_token)?;
                    tokens.push(token);
                }
            }

            WorkflowNodeType::ParallelGateway => {
                let incoming = graph
                    .edges
                    .iter()
                    .filter(|edge| edge.target == node.id)
                    .count();
                if incoming <= 1 {
                    // A fork, or a pass-through: nothing to synchronise.
                    tokens[index].status = TokenStatus::Done;
                    self.follow(graph, index, tokens, next_token, scope)?;
                    return Ok(());
                }

                let arrived = arrivals.entry(node.id.clone()).or_insert(0);
                *arrived += 1;
                if *arrived < incoming {
                    tokens[index].status = TokenStatus::Parked;
                    return Ok(());
                }

                // The join is complete: consume every parked sibling and emit
                // one token per outgoing edge.
                *arrived = 0;
                for token in tokens.iter_mut() {
                    if token.node == node.id && token.status == TokenStatus::Parked {
                        token.status = TokenStatus::Done;
                    }
                }
                tokens[index].status = TokenStatus::Done;
                let outgoing: Vec<NodeId> = graph
                    .edges
                    .iter()
                    .filter(|edge| edge.source == node.id)
                    .map(|edge| edge.target.clone())
                    .collect();
                for target in outgoing {
                    let edge = graph
                        .edges
                        .iter()
                        .find(|edge| edge.target == target && edge.source == node.id)
                        .ok_or_else(|| {
                            RunError::InvalidGraph(format!(
                                "parallel gateway `{}` has a flow that is not its own",
                                node.name
                            ))
                        })?;
                    let token = self.token_along(edge, scope, next_token)?;
                    tokens.push(token);
                }
            }

            // Every other control type simply continues.
            _ => {
                tokens[index].status = TokenStatus::Done;
                self.follow(graph, index, tokens, next_token, scope)?;
            }
        }
        Ok(())
    }

    /// Move a finished token along its outgoing edges.
    fn follow(
        &self,
        graph: &Workflow,
        index: usize,
        tokens: &mut Vec<Token>,
        next_token: &mut u64,
        scope: &mut Scope,
    ) -> Result<(), RunError> {
        let node_id = tokens[index].node.clone();
        // Cloned rather than borrowed so the token pushes below cannot collide
        // with the iterator over `graph.edges`.
        let mut edges: Vec<WorkflowEdge> = Vec::new();
        for edge in graph.edges.iter().filter(|edge| edge.source == node_id) {
            if self.condition_holds(edge, scope)? {
                edges.push(edge.clone());
            }
        }

        for edge in &edges {
            let token = self.token_along(edge, scope, next_token)?;
            tokens.push(token);
        }
        Ok(())
    }

    /// Whether an edge's condition holds.
    ///
    /// A malformed condition is an error, not `false`. The BPMN simulator
    /// propagates it and so does the start-event path in [`Self::step_control`];
    /// treating it as `false` here would send the run down a branch the author
    /// did not choose, and the journal would record a perfectly ordinary
    /// success. Stopping with the offending expression named is the only
    /// outcome a person can act on.
    fn condition_holds(&self, edge: &WorkflowEdge, scope: &Scope) -> Result<bool, RunError> {
        let Some(condition) = edge.condition.as_deref() else {
            return Ok(true);
        };
        nocode_bpmn::expr::evaluate(condition, &scope.condition_vars()).map_err(|error| {
            RunError::InvalidGraph(format!(
                "the condition `{condition}` on edge {} is not valid: {error}",
                edge.id
            ))
        })
    }

    fn select_one(
        &self,
        graph: &Workflow,
        node: &WorkflowNode,
        default_flow: Option<&nocode_model::EdgeId>,
        scope: &Scope,
    ) -> Result<Option<NodeId>, RunError> {
        let outgoing: Vec<&WorkflowEdge> = graph
            .edges
            .iter()
            .filter(|edge| edge.source == node.id)
            .collect();

        for edge in &outgoing {
            if edge.condition.is_some() && self.condition_holds(edge, scope)? {
                return Ok(Some(edge.target.clone()));
            }
        }
        if let Some(edge) = outgoing
            .iter()
            .find(|edge| Some(&edge.id) == default_flow)
        {
            return Ok(Some(edge.target.clone()));
        }
        let unconditional: Vec<_> = outgoing
            .iter()
            .filter(|edge| edge.condition.is_none())
            .collect();
        if unconditional.len() == 1 {
            return Ok(Some(unconditional[0].target.clone()));
        }
        Ok(None)
    }

    fn select_many(
        &self,
        graph: &Workflow,
        node: &WorkflowNode,
        scope: &Scope,
    ) -> Result<Vec<NodeId>, RunError> {
        let mut targets = Vec::new();
        for edge in graph.edges.iter().filter(|edge| edge.source == node.id) {
            if self.condition_holds(edge, scope)? {
                targets.push(edge.target.clone());
            }
        }
        Ok(targets)
    }

    fn validate(&self, graph: &Workflow) -> Result<(), RunError> {
        let result = BpmnEngine::new().validate(graph);
        if let Some(issue) = result
            .issues
            .iter()
            .find(|issue| issue.severity == Severity::Error)
        {
            return Err(RunError::InvalidGraph(issue.message.clone()));
        }
        // Every agent node needs an executor; better to refuse before the first
        // side effect than to fail halfway through.
        for node in &graph.nodes {
            if matches!(node.node_type, WorkflowNodeType::AgentTask) {
                let kind = agent_config(node)
                    .map_err(|error| RunError::InvalidGraph(error.to_string()))?
                    .kind();
                if !self.registry.contains(kind) {
                    return Err(RunError::InvalidGraph(format!(
                        "no executor registered for `{}` (node `{}`)",
                        kind.as_str(),
                        node.name
                    )));
                }
            }
        }
        Ok(())
    }

    fn is_idempotent(&self, node: &WorkflowNode) -> Result<bool, RunError> {
        let config = agent_config(node)
            .map_err(|error| RunError::InvalidGraph(error.to_string()))?;
        Ok(config.idempotent(node.config.idempotent).unwrap_or(false))
    }

    /// A token for the node at the far end of `edge`, carrying that edge's port
    /// bindings.
    ///
    /// Bindings are resolved *now*, against the scope as it stands when the
    /// source node has just finished — which is the only moment the expression
    /// is guaranteed to have something to read.
    ///
    /// They are not journalled. On resume the source node's recorded output is
    /// replayed into the scope before the edge is followed again, so the same
    /// expression yields the same value; a journal entry would only be a second
    /// copy of something already recorded.
    fn token_along(
        &self,
        edge: &WorkflowEdge,
        scope: &Scope,
        next_token: &mut u64,
    ) -> Result<Token, RunError> {
        let mut bindings = IndexMap::new();
        for (port, expression) in &edge.mapping {
            let value = mapping_value(expression, scope).map_err(|error| {
                RunError::InvalidGraph(format!(
                    "the binding of port `{port}` to `{expression}` is not valid: {error}"
                ))
            })?;
            bindings.insert(port.clone(), value);
        }
        let token = Token {
            id: *next_token,
            node: edge.target.clone(),
            status: TokenStatus::Ready,
            bindings,
        };
        *next_token += 1;
        Ok(token)
    }

    fn record_outcome(
        &self,
        node: &NodeId,
        node_name: &str,
        status: NodeStatus,
        attempt: u32,
        output: Option<&NodeOutput>,
        redactor: &Redactor,
    ) -> Result<(), RunError> {
        let (value, usage) = match output {
            Some(output) => {
                let mut value = output.value.clone();
                redactor.redact_value(&mut value);
                (Some(truncate(value, self.max_output_bytes)), Some(output.usage.clone()))
            }
            None => (None, None),
        };
        self.journal.append(JournalEvent::NodeOutcome {
            node: node.to_string(),
            node_name: node_name.to_string(),
            status,
            attempt,
            output: value,
            usage,
            code: None,
            message: None,
            detail: None,
            since: None,
        })?;
        Ok(())
    }

    fn record_waiting(
        &self,
        node: &NodeId,
        node_name: &str,
        attempt: u32,
        wait: &WaitState,
    ) -> Result<(), RunError> {
        self.journal.append(JournalEvent::NodeOutcome {
            node: node.to_string(),
            node_name: node_name.to_string(),
            status: NodeStatus::Waiting,
            attempt,
            output: None,
            usage: None,
            code: None,
            message: Some(wait.title.clone()),
            detail: None,
            since: Some(wait.since),
        })?;
        Ok(())
    }

    fn record_error(
        &self,
        node: &NodeId,
        node_name: &str,
        attempt: u32,
        error: &NodeError,
        redactor: &Redactor,
    ) -> Result<(), RunError> {
        self.journal.append(JournalEvent::NodeOutcome {
            node: node.to_string(),
            node_name: node_name.to_string(),
            status: NodeStatus::Error,
            attempt,
            output: None,
            usage: None,
            code: Some(error.code.as_str().to_string()),
            message: Some(redactor.redact_str(&error.message)),
            detail: error
                .detail
                .as_deref()
                .map(|detail| redactor.redact_str(detail)),
            since: None,
        })?;
        Ok(())
    }

    fn finish(&self, status: RunStatus, detail: Option<String>) -> Result<(), RunError> {
        self.journal
            .append(JournalEvent::RunFinished { status, detail })?;
        Ok(())
    }

    /// The wait a suspended run is parked on.
    fn wait_state(
        &self,
        graph: &Workflow,
        tokens: &[Token],
        waits: &HashMap<u64, WaitState>,
    ) -> Result<WaitState, RunError> {
        let waiting = tokens
            .iter()
            .find(|token| token.status == TokenStatus::Waiting)
            .expect("caller checked for a waiting token");
        if let Some(wait) = waits.get(&waiting.id) {
            return Ok(wait.clone());
        }
        let node = node_of(graph, &waiting.node)?;
        Ok(WaitState {
            kind: "approval".to_string(),
            title: node.name.clone(),
            body: None,
            since: self.clock.now(),
        })
    }
}

/// Resolve one edge binding's right-hand side.
///
/// Two spellings, because two documents describe the same thing. See
/// [`crate::interpolate::resolve_expression`].
fn mapping_value(expression: &str, scope: &Scope) -> Result<Value, NodeError> {
    crate::interpolate::resolve_expression(expression, scope)
}

fn node_of<'a>(graph: &'a Workflow, id: &NodeId) -> Result<&'a WorkflowNode, RunError> {
    graph.nodes.iter().find(|node| &node.id == id).ok_or_else(|| {
        RunError::InvalidGraph(format!("token references unknown node {id}"))
    })
}

/// A node's own timeout, where its config declares one.
///
/// The approval `timeout` is deliberately *not* here: it is the human's
/// deadline, handled by the executor across a restart, not a hard kill.
fn node_timeout(node: &WorkflowNode) -> Option<Duration> {
    match node.agent_config()? {
        AgentNodeConfig::Shell(shell) => Some(Duration::from_millis(shell.timeout_ms)),
        AgentNodeConfig::Http(http) => Some(Duration::from_millis(http.timeout_ms)),
        _ => None,
    }
}

/// Turn a journal into one queue of states per node, in visit order.
///
/// Intents matter as much as outcomes. An intent with no matching outcome is
/// §7.3's "may have happened, cannot say" — the `Unknown` that §7.4's resume
/// table turns into a decision. Building the queue from outcomes alone would
/// make that case invisible, and the run would silently repeat a node whose
/// side effect may already have landed.
///
/// A queue rather than a map because a graph may visit the same node twice:
/// the second visit must consume the second record, not the first.
fn recorded_states(journal: &RunJournal) -> HashMap<String, VecDeque<NodeState>> {
    let mut queues: HashMap<String, VecDeque<NodeState>> = HashMap::new();

    for record in &journal.records {
        match &record.event {
            // A visit begins here. It stays `Unknown` unless an outcome for the
            // same attempt replaces it.
            JournalEvent::NodeIntent {
                node, attempt, ..
            } => {
                queues
                    .entry(node.clone())
                    .or_default()
                    .push_back(NodeState::Unknown { attempt: *attempt });
            }
            JournalEvent::NodeOutcome {
                node,
                status,
                attempt,
                output,
                usage,
                code,
                message,
                since,
                ..
            } => {
                let state = match status {
                    NodeStatus::Ok | NodeStatus::Skipped => NodeState::Succeeded {
                        output: output.clone().unwrap_or(Value::Null),
                        usage: usage.clone().unwrap_or_default(),
                    },
                    NodeStatus::Error => NodeState::Failed {
                        code: code.clone().unwrap_or_else(|| "internal".into()),
                        message: message.clone().unwrap_or_default(),
                        attempt: *attempt,
                    },
                    NodeStatus::Waiting => NodeState::Waiting {
                        since: since.unwrap_or(record.ts),
                        attempt: *attempt,
                    },
                };
                let queue = queues.entry(node.clone()).or_default();
                // Replace this visit's placeholder when there is one, so a
                // completed attempt is one entry rather than two. An outcome
                // with no intent (a hand-written journal) is still recorded.
                match queue.back() {
                    Some(NodeState::Unknown { attempt: pending }) if pending == attempt => {
                        queue.pop_back();
                        queue.push_back(state);
                    }
                    _ => queue.push_back(state),
                }
            }
            _ => {}
        }
    }

    queues
}

/// Cap a journalled value. The run keeps the full one; only the record shrinks.
fn truncate(value: Value, limit: usize) -> Value {
    let encoded = serde_json::to_string(&value).unwrap_or_default();
    if encoded.len() <= limit {
        return value;
    }
    json!({
        "truncated": true,
        "bytes": encoded.len(),
        "preview": encoded.chars().take(limit / 2).collect::<String>(),
    })
}

fn merged_output(scope: &Scope) -> Value {
    let mut merged = serde_json::Map::new();
    for output in scope.nodes.values() {
        if let Value::Object(map) = output {
            for (key, value) in map {
                merged.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(merged)
}

/// A stable hash of a node's config, for the intent record.
fn hash_json(value: &impl serde::Serialize) -> String {
    let text = serde_json::to_string(value).unwrap_or_default();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}
