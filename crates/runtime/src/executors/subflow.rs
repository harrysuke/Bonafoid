//! A call into another workflow.
//!
//! The node is a *seam*, not an implementation: it asks the host's
//! [`SubflowResolver`] for the target workflow and runs it on a nested
//! [`GraphRunner`] with the same executors, limits, and clock.
//!
//! # What is shared and what is not
//!
//! Shared: the registry (so a subflow can call a subflow), the limits (a
//! subflow does not get a fresh budget — otherwise a cycle of two workflows
//! would be an infinite loop that each level thinks is its first), and the
//! clock.
//!
//! Not shared: the journal. A nested run gets a [`MemoryJournal`], because the
//! parent's intent/outcome record for *this node* already brackets everything
//! the child does. The consequence is stated plainly in [`SubflowExecutor`]:
//! an interrupted subflow is replayed from the top on resume, which is exactly
//! why a `subflow` node is not idempotent by default.
//!
//! # Suspension
//!
//! A child that suspends on an approval surfaces as [`NodeOutcome::Waiting`] on
//! the parent's subflow node, so the parent suspends too and the whole tree
//! stops at one process exit. On resume the child re-runs and asks its gate
//! again; an approval *inside* a subflow therefore restarts its own deadline
//! clock on each resume, unlike a top-level one. That is the honest cost of not
//! journalling the child, and it is noted here rather than hidden.

use std::sync::{Arc, RwLock, Weak};

use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use nocode_model::{AgentNodeConfig, AgentNodeKind, SubflowNode, WorkflowNode};

use crate::capability::Capability;
use crate::context::{RunContext, RunInput, SubflowResolver};
use crate::error::{NodeError, NodeErrorCode, RunError};
use crate::executor::{agent_config, ExecutorRegistry, NodeContext, NodeExecutor, NodeOutcome};
use crate::interpolate::{resolve_expression, Scope};
use crate::journal::MemoryJournal;
use crate::limits::RunLimits;
use crate::runner::{GraphRunner, MAX_SUBFLOW_DEPTH};
use crate::time::Clock;

/// Runs another workflow in this project.
pub struct SubflowExecutor {
    /// Weak so the registry → executor → registry cycle does not leak. By the
    /// time any subflow executes, the host has long since filled it in.
    registry: Arc<RwLock<Weak<ExecutorRegistry>>>,
    limits: RunLimits,
    clock: Arc<dyn Clock>,
}

impl SubflowExecutor {
    /// Built by [`ExecutorRegistry::with_control_executors`], which wires the
    /// handle for you.
    pub(crate) fn new(
        registry: Arc<RwLock<Weak<ExecutorRegistry>>>,
        limits: RunLimits,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            registry,
            limits,
            clock,
        }
    }

    fn config<'a>(&self, node: &'a WorkflowNode) -> Result<&'a SubflowNode, NodeError> {
        match agent_config(node)? {
            AgentNodeConfig::Subflow(subflow) => Ok(subflow),
            other => Err(NodeError::new(
                NodeErrorCode::InvalidConfig,
                format!(
                    "`{}` is a `{}` node; the subflow executor cannot run it",
                    node.name,
                    other.kind().as_str()
                ),
            )),
        }
    }

    fn registry(&self) -> Result<Arc<ExecutorRegistry>, NodeError> {
        self.registry
            .read()
            .ok()
            .and_then(|handle| handle.upgrade())
            .ok_or_else(|| {
                NodeError::internal(
                    "the subflow executor is not wired to a registry; build one with \
                     `ExecutorRegistry::with_control_executors`",
                )
            })
    }

    fn resolve(
        &self,
        subflow: &SubflowNode,
        resolver: &dyn SubflowResolver,
    ) -> Result<nocode_model::Workflow, NodeError> {
        if subflow.workflow_id.0.is_nil() {
            return Err(NodeError::invalid_config(
                "this subflow has no workflow selected",
            ));
        }
        let id = subflow.workflow_id.to_string();
        resolver.resolve(&id).ok_or_else(|| {
            NodeError::not_found(format!("workflow `{id}` is not in this project"))
        })
    }
}

#[async_trait]
impl NodeExecutor for SubflowExecutor {
    fn kind(&self) -> AgentNodeKind {
        AgentNodeKind::Subflow
    }

    fn config_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": {
                    "type": "string",
                    "format": "uuid",
                    "description": "The workflow to call."
                },
                "input_mapping": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "Port name → expression in *this* scope. \
                                    `{\"prompt\": \"nodes.draft.text\"}`."
                },
                "output_mapping": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "Port name → expression in the child's output, which is \
                                    exposed as `input`. Empty passes the whole output through."
                }
            },
            "required": ["workflow_id"]
        })
    }

    fn capabilities(&self, node: &WorkflowNode) -> Vec<Capability> {
        match agent_config(node) {
            Ok(AgentNodeConfig::Subflow(subflow)) if !subflow.workflow_id.0.is_nil() => {
                vec![Capability::flow_call(subflow.workflow_id.to_string())]
            }
            // An unconfigured node claims nothing; the validator reports the
            // nil id separately.
            _ => Vec::new(),
        }
    }

    fn dry_run(&self, node: &WorkflowNode, cx: &RunContext) -> Result<(), NodeError> {
        let subflow = self.config(node)?;
        if cx.depth >= MAX_SUBFLOW_DEPTH {
            return Err(NodeError::invalid_config(format!(
                "already {MAX_SUBFLOW_DEPTH} levels deep"
            )));
        }
        self.resolve(subflow, cx.subflows.as_ref())?;
        Ok(())
    }

    async fn execute(
        &self,
        node: &WorkflowNode,
        cx: &NodeContext<'_>,
    ) -> Result<NodeOutcome, NodeError> {
        let subflow = self.config(node)?;
        let workflow = self.resolve(subflow, cx.run.subflows.as_ref())?;

        // The child's input is built in *this* scope, where the parent's node
        // outputs live.
        let mut inputs = IndexMap::new();
        for (port, expression) in &subflow.input_mapping {
            let value = resolve_expression(expression, cx.scope).map_err(|error| {
                NodeError::new(
                    NodeErrorCode::InvalidConfig,
                    format!("input `{port}` → `{expression}`: {error}"),
                )
            })?;
            inputs.insert(port.clone(), value);
        }

        let registry = self.registry()?;
        let child_run_id = format!("{}/{}", cx.run.run_id, cx.node_id);
        let runner = GraphRunner::new(
            registry,
            Arc::new(MemoryJournal::new()),
            self.limits.clone(),
            self.clock.clone(),
        );
        let child_cx = cx.run.nested(child_run_id);

        let outcome = runner
            .run(&workflow, RunInput::new(inputs), &child_cx)
            .await
            .map_err(child_error)?;

        // The child stopped at a gate. The parent stops at the same gate: one
        // process exit for the whole tree.
        if let Some(wait) = outcome.suspended {
            return Ok(NodeOutcome::Waiting(wait));
        }

        let value = if subflow.output_mapping.is_empty() {
            outcome.output
        } else {
            map_output(&subflow.output_mapping, &outcome.output, cx)?
        };

        Ok(NodeOutcome::Done(
            crate::executor::NodeOutput::new(value).with_usage(outcome.usage),
        ))
    }
}

/// Apply a child's output mapping.
///
/// The child's merged output becomes the mapping's `input`, so
/// `{"summary": "input.summary"}` reads the way it looks. A non-object output
/// is wrapped as `input.output` rather than refused, because a child that ends
/// on a node returning a scalar is legitimate.
fn map_output(
    mapping: &IndexMap<String, String>,
    output: &Value,
    cx: &NodeContext<'_>,
) -> Result<Value, NodeError> {
    let inputs = match output {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        other => IndexMap::from_iter([("output".to_string(), other.clone())]),
    };
    let scope = Scope::new(
        &cx.run.run_id,
        &cx.run.workspace,
        inputs,
        cx.run.secrets.clone(),
    );

    let mut mapped = Map::new();
    for (port, expression) in mapping {
        let value = resolve_expression(expression, &scope).map_err(|error| {
            NodeError::new(
                NodeErrorCode::InvalidConfig,
                format!("output `{port}` → `{expression}`: {error}"),
            )
        })?;
        mapped.insert(port.clone(), value);
    }
    Ok(Value::Object(mapped))
}

/// A child's failure, as the parent's node error.
///
/// The child's own node error is passed through untouched when there is one:
/// the code (`not_found`, `rejected`, …) is what a host routes on, and
/// replacing it with `internal` would erase the reason.
fn child_error(error: RunError) -> NodeError {
    match error {
        RunError::Node { error, .. } => error,
        RunError::Cancelled => NodeError::cancelled(),
        RunError::InvalidGraph(message) => NodeError::invalid_config(message),
        RunError::SubflowDepth(depth) => NodeError::invalid_config(format!(
            "sub-workflow nesting exceeded {depth} levels"
        )),
        RunError::NeedsAttention { node_name, .. } => NodeError::new(
            NodeErrorCode::Internal,
            format!("sub-workflow stopped at `{node_name}`, which may have run already"),
        ),
        RunError::BudgetExceeded { limit, detail } => NodeError::new(
            NodeErrorCode::Internal,
            format!("sub-workflow exceeded its {limit} budget ({detail})"),
        ),
        RunError::Journal(error) => NodeError::internal(format!("sub-workflow journal: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::Utc;

    use crate::context::{AlwaysApproves, MapSecrets, NoSubflows};
    use crate::interpolate::Redactor;
    use crate::time::{ManualClock, SystemClock};

    #[test]
    fn a_nil_workflow_id_is_refused_before_anything_runs() {
        let executor = SubflowExecutor::new(
            Arc::new(RwLock::new(Weak::new())),
            RunLimits::default(),
            Arc::new(SystemClock),
        );
        let node = WorkflowNode {
            id: nocode_model::NodeId::new(),
            name: "Call".into(),
            node_type: nocode_model::WorkflowNodeType::AgentTask,
            position: nocode_model::NodePosition::default(),
            config: nocode_model::NodeConfig {
                agent: Some(AgentNodeConfig::Subflow(SubflowNode::default())),
                ..Default::default()
            },
            documentation: None,
        };
        let cx = RunContext::new("r1", ".");
        let error = executor.dry_run(&node, &cx).unwrap_err();
        assert_eq!(error.code, NodeErrorCode::InvalidConfig);
        assert!(error.message.contains("no workflow selected"), "{}", error.message);
    }

    #[test]
    fn a_missing_workflow_is_not_found_rather_than_a_crash() {
        let executor = SubflowExecutor::new(
            Arc::new(RwLock::new(Weak::new())),
            RunLimits::default(),
            Arc::new(SystemClock),
        );
        let node = WorkflowNode {
            id: nocode_model::NodeId::new(),
            name: "Call".into(),
            node_type: nocode_model::WorkflowNodeType::AgentTask,
            position: nocode_model::NodePosition::default(),
            config: nocode_model::NodeConfig {
                agent: Some(AgentNodeConfig::Subflow(SubflowNode {
                    workflow_id: nocode_model::WorkflowId::new(),
                    ..Default::default()
                })),
                ..Default::default()
            },
            documentation: None,
        };
        let cx = RunContext::new("r1", ".");
        let error = executor.dry_run(&node, &cx).unwrap_err();
        assert_eq!(error.code, NodeErrorCode::NotFound);
    }

    #[test]
    fn an_unwired_executor_says_how_to_wire_it() {
        let executor = SubflowExecutor::new(
            Arc::new(RwLock::new(Weak::new())),
            RunLimits::default(),
            Arc::new(SystemClock),
        );
        let error = executor.registry().unwrap_err();
        assert!(
            error.message.contains("with_control_executors"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_child_failure_keeps_its_own_code() {
        let error = child_error(RunError::Node {
            node: "n1".into(),
            node_name: "Draft".into(),
            error: NodeError::not_found("no such file"),
        });
        assert_eq!(error.code, NodeErrorCode::NotFound);
    }

    #[test]
    fn a_non_object_child_output_is_wrapped_not_refused() {
        let run = RunContext::new("r1", ".");
        let scope = Scope::new("r1", ".", Default::default(), Arc::new(MapSecrets::default()));
        let clock = ManualClock::at(Utc::now());
        let redactor = Redactor::new();
        let cx = NodeContext {
            run: &run,
            scope: &scope,
            clock: &clock,
            node_id: "sub-1".into(),
            node_name: "Call".into(),
            waiting_since: None,
            redactor: &redactor,
        };
        let mut mapping = IndexMap::new();
        mapping.insert("answer".to_string(), "input.output".to_string());
        let value = map_output(&mapping, &json!(42), &cx).unwrap();
        assert_eq!(value, json!({ "answer": 42 }));
    }

    #[test]
    fn an_output_mapping_reads_the_child_as_input() {
        let run = RunContext::new("r1", ".");
        let scope = Scope::new("r1", ".", Default::default(), Arc::new(MapSecrets::default()));
        let clock = ManualClock::at(Utc::now());
        let redactor = Redactor::new();
        let cx = NodeContext {
            run: &run,
            scope: &scope,
            clock: &clock,
            node_id: "sub-1".into(),
            node_name: "Call".into(),
            waiting_since: None,
            redactor: &redactor,
        };
        let mut mapping = IndexMap::new();
        mapping.insert("summary".to_string(), "input.summary".to_string());
        let value = map_output(&mapping, &json!({ "summary": "all good" }), &cx).unwrap();
        assert_eq!(value, json!({ "summary": "all good" }));
    }

    /// The cycle guard: a workflow that calls itself must stop at the depth
    /// ceiling rather than recurse until the stack dies.
    #[tokio::test]
    async fn a_self_calling_workflow_stops_at_the_depth_ceiling() {
        use nocode_model::{Workflow, WorkflowEdge, WorkflowNodeType};

        struct SelfCalls {
            workflow: RwLock<Option<Workflow>>,
            calls: AtomicUsize,
        }
        impl SubflowResolver for SelfCalls {
            fn resolve(&self, _workflow_id: &str) -> Option<Workflow> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.workflow.read().unwrap().clone()
            }
        }

        // start → subflow → end, where the subflow is this same graph.
        let workflow_id = nocode_model::WorkflowId::new();
        let mut workflow = Workflow::new("loop");
        let start = WorkflowNode {
            id: nocode_model::NodeId::new(),
            name: "Start".into(),
            node_type: WorkflowNodeType::StartEvent {
                event_type: nocode_model::StartEventType::None,
            },
            position: Default::default(),
            config: Default::default(),
            documentation: None,
        };
        let call = WorkflowNode {
            id: nocode_model::NodeId::new(),
            name: "Again".into(),
            node_type: WorkflowNodeType::AgentTask,
            position: Default::default(),
            config: nocode_model::NodeConfig {
                agent: Some(AgentNodeConfig::Subflow(SubflowNode {
                    workflow_id,
                    ..Default::default()
                })),
                ..Default::default()
            },
            documentation: None,
        };
        let end = WorkflowNode {
            id: nocode_model::NodeId::new(),
            name: "End".into(),
            node_type: WorkflowNodeType::EndEvent {
                event_type: nocode_model::EndEventType::None,
            },
            position: Default::default(),
            config: Default::default(),
            documentation: None,
        };
        let edge = |source, target| WorkflowEdge {
            id: nocode_model::EdgeId::new(),
            source,
            target,
            source_handle: None,
            target_handle: None,
            condition: None,
            label: None,
            style: Default::default(),
            mapping: Default::default(),
        };
        workflow.edges = vec![
            edge(start.id.clone(), call.id.clone()),
            edge(call.id.clone(), end.id.clone()),
        ];
        workflow.nodes = vec![start, call, end];

        let resolver = Arc::new(SelfCalls {
            workflow: RwLock::new(Some(workflow.clone())),
            calls: AtomicUsize::new(0),
        });
        let registry = ExecutorRegistry::new().with_control_executors(
            RunLimits::default(),
            Arc::new(SystemClock),
        );
        let cx = RunContext::new("r1", ".")
            .with_subflows(resolver.clone())
            .with_approvals(Arc::new(AlwaysApproves));

        let error = GraphRunner::new(
            registry,
            Arc::new(MemoryJournal::new()),
            RunLimits::default(),
            Arc::new(SystemClock),
        )
        .run(&workflow, RunInput::empty(), &cx)
        .await
        .unwrap_err();

        match error {
            RunError::Node { error, .. } => {
                assert_eq!(error.code, NodeErrorCode::InvalidConfig);
                assert!(
                    error.message.contains("nesting exceeded"),
                    "{}",
                    error.message
                );
            }
            other => panic!("expected a node error, got {other:?}"),
        }
        assert!(
            resolver.calls.load(Ordering::SeqCst) <= MAX_SUBFLOW_DEPTH + 1,
            "the recursion must be bounded"
        );
    }

    #[tokio::test]
    async fn an_unresolvable_subflow_fails_the_node_not_the_run() {
        let run = RunContext::new("r1", ".").with_subflows(Arc::new(NoSubflows));
        let scope = Scope::new("r1", ".", Default::default(), Arc::new(MapSecrets::default()));
        let clock = ManualClock::at(Utc::now());
        let redactor = Redactor::new();
        let node = WorkflowNode {
            id: nocode_model::NodeId::new(),
            name: "Call".into(),
            node_type: nocode_model::WorkflowNodeType::AgentTask,
            position: Default::default(),
            config: nocode_model::NodeConfig {
                agent: Some(AgentNodeConfig::Subflow(SubflowNode {
                    workflow_id: nocode_model::WorkflowId::new(),
                    ..Default::default()
                })),
                ..Default::default()
            },
            documentation: None,
        };
        let executor = SubflowExecutor::new(
            Arc::new(RwLock::new(Weak::new())),
            RunLimits::default(),
            Arc::new(SystemClock),
        );
        let cx = NodeContext {
            run: &run,
            scope: &scope,
            clock: &clock,
            node_id: "sub-1".into(),
            node_name: "Call".into(),
            waiting_since: None,
            redactor: &redactor,
        };
        let error = executor.execute(&node, &cx).await.unwrap_err();
        assert_eq!(error.code, NodeErrorCode::NotFound);
    }
}
