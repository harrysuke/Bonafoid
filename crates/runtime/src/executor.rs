//! The executor seam.
//!
//! One trait, one registry. The IDE registers the full set and a compiled
//! bundle registers the same set from the same crate, so authoring and running
//! cannot drift — the property §7.2 asks for.
//!
//! [`NodeExecutor::execute`] returns a [`NodeOutcome`], not a bare output,
//! because an approval has to *suspend* the run rather than block it: the
//! process may exit while the human decides (§7.1, §7.4). Expressing that in
//! the return type keeps every executor honest about whether it can finish.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use nocode_model::{AgentNodeConfig, AgentNodeKind, WorkflowNode, WorkflowNodeType};

use crate::capability::Capability;
use crate::context::RunContext;
use crate::error::{NodeError, NodeErrorCode};
use crate::interpolate::{Redactor, Scope};
use crate::time::Clock;

/// What a node consumed. Attributed per node so the canvas can show a cost
/// breakdown and the run budget can be enforced.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tokens_in: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tokens_out: u64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub cost_usd: f64,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub tool_calls: u32,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

fn is_zero_f64(value: &f64) -> bool {
    *value == 0.0
}

impl Usage {
    pub fn total_tokens(&self) -> u64 {
        self.tokens_in.saturating_add(self.tokens_out)
    }
}

/// A node's result when it finished.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeOutput {
    /// The output ports. Conventionally an object, so `${nodes.<id>.<port>}`
    /// reads a named value; a scalar is accepted and journalled as-is.
    pub value: Value,
    #[serde(default, skip_serializing_if = "usage_is_empty")]
    pub usage: Usage,
}

fn usage_is_empty(usage: &Usage) -> bool {
    *usage == Usage::default()
}

impl NodeOutput {
    pub fn new(value: Value) -> Self {
        Self {
            value,
            usage: Usage::default(),
        }
    }

    pub fn with_usage(mut self, usage: Usage) -> Self {
        self.usage = usage;
        self
    }
}

/// Why a node stopped without finishing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaitState {
    /// The waiting mechanism: `approval` today, `signal` and `timer` later.
    pub kind: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// When the wait began, so a timeout can be measured across a process
    /// restart.
    pub since: DateTime<Utc>,
}

impl WaitState {
    pub fn approval(title: impl Into<String>, body: Option<String>, since: DateTime<Utc>) -> Self {
        Self {
            kind: "approval".to_string(),
            title: title.into(),
            body,
            since,
        }
    }
}

/// What an executor produced.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeOutcome {
    Done(NodeOutput),
    /// The node is parked until something outside the run answers. The
    /// scheduler journals this and suspends.
    Waiting(WaitState),
}

impl NodeOutcome {
    pub fn done(value: Value) -> Self {
        Self::Done(NodeOutput::new(value))
    }
}

/// Everything an executor is given for one attempt.
pub struct NodeContext<'a> {
    pub run: &'a RunContext,
    pub scope: &'a Scope,
    pub clock: &'a dyn Clock,
    /// The node's id as a string, for gates and logs.
    pub node_id: String,
    pub node_name: String,
    /// Set when the previous attempt on this node ended in `waiting`, so an
    /// executor can measure a timeout that started before the process exited.
    pub waiting_since: Option<DateTime<Utc>>,
    /// Scrubs secrets from anything the node reports.
    pub redactor: &'a Redactor,
}

impl NodeContext<'_> {
    /// Interpolate one template against this node's scope.
    pub fn interpolate(&self, template: &str) -> Result<String, NodeError> {
        crate::interpolate::interpolate(template, self.scope)
    }
}

impl std::fmt::Debug for NodeContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeContext")
            .field("node_id", &self.node_id)
            .field("node_name", &self.node_name)
            .field("run", &self.run)
            .finish_non_exhaustive()
    }
}

/// One node kind's implementation.
#[async_trait]
pub trait NodeExecutor: Send + Sync {
    fn kind(&self) -> AgentNodeKind;

    /// Schema for the inspector and the MCP authoring tools.
    fn config_schema(&self) -> Value;

    /// The capabilities this node will exercise, derived from its payload.
    fn capabilities(&self, node: &WorkflowNode) -> Vec<Capability>;

    /// A cheap, side-effect-free check for the canvas "Validate" button.
    ///
    /// Must not touch the filesystem, the network, or a model. It answers
    /// "would this node start at all", not "would it succeed".
    fn dry_run(&self, node: &WorkflowNode, cx: &RunContext) -> Result<(), NodeError>;

    async fn execute(
        &self,
        node: &WorkflowNode,
        cx: &NodeContext<'_>,
    ) -> Result<NodeOutcome, NodeError>;
}

/// Every executor the host registered.
#[derive(Default, Clone)]
pub struct ExecutorRegistry {
    executors: HashMap<AgentNodeKind, Arc<dyn NodeExecutor>>,
}

impl ExecutorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an executor. A later registration of the same kind replaces the
    /// earlier one, so a host can override a default without a second registry.
    pub fn register(&mut self, executor: Arc<dyn NodeExecutor>) -> &mut Self {
        self.executors.insert(executor.kind(), executor);
        self
    }

    pub fn with(mut self, executor: Arc<dyn NodeExecutor>) -> Self {
        self.register(executor);
        self
    }

    pub fn get(&self, kind: AgentNodeKind) -> Option<&Arc<dyn NodeExecutor>> {
        self.executors.get(&kind)
    }

    pub fn contains(&self, kind: AgentNodeKind) -> bool {
        self.executors.contains_key(&kind)
    }

    /// The kinds this registry can run, for `doctor` and for the canvas.
    pub fn kinds(&self) -> Vec<AgentNodeKind> {
        let mut kinds: Vec<_> = self.executors.keys().copied().collect();
        kinds.sort_by_key(|kind| {
            AgentNodeKind::ALL
                .iter()
                .position(|candidate| candidate == kind)
                .unwrap_or(usize::MAX)
        });
        kinds
    }

    pub fn is_empty(&self) -> bool {
        self.executors.is_empty()
    }
}

impl std::fmt::Debug for ExecutorRegistry {
    /// Prints the kinds, not the executors: `dyn NodeExecutor` has no `Debug`,
    /// and the only thing worth seeing in a log is which kinds are covered.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutorRegistry")
            .field("kinds", &self.kinds())
            .finish()
    }
}

/// The payload of an `AgentTask`, with the invariant checked.
///
/// A node that is not an agent task, or one whose payload is missing, is a
/// defect in the graph rather than a runtime failure — but it must surface as
/// a node error, not a panic, because the graph came from a file.
pub fn agent_config(node: &WorkflowNode) -> Result<&AgentNodeConfig, NodeError> {
    if !matches!(node.node_type, WorkflowNodeType::AgentTask) {
        return Err(NodeError::new(
            NodeErrorCode::InvalidConfig,
            format!(
                "`{}` is a {} node, not an AgentTask",
                node.name,
                node.node_type.tag()
            ),
        ));
    }
    node.config.agent.as_ref().ok_or_else(|| {
        NodeError::new(
            NodeErrorCode::InvalidConfig,
            format!("`{}` has no agent config", node.name),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::RunContext;
    use serde_json::json;

    struct Stub {
        kind: AgentNodeKind,
    }

    #[async_trait]
    impl NodeExecutor for Stub {
        fn kind(&self) -> AgentNodeKind {
            self.kind
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
            _node: &WorkflowNode,
            _cx: &NodeContext<'_>,
        ) -> Result<NodeOutcome, NodeError> {
            Ok(NodeOutcome::done(json!({})))
        }
    }

    #[test]
    fn a_later_registration_replaces_the_earlier_one() {
        let registry = ExecutorRegistry::new()
            .with(Arc::new(Stub {
                kind: AgentNodeKind::Approval,
            }))
            .with(Arc::new(Stub {
                kind: AgentNodeKind::Approval,
            }));
        assert!(registry.contains(AgentNodeKind::Approval));
        assert_eq!(registry.kinds(), vec![AgentNodeKind::Approval]);
    }

    #[test]
    fn kinds_are_listed_in_palette_order() {
        let registry = ExecutorRegistry::new()
            .with(Arc::new(Stub {
                kind: AgentNodeKind::Subflow,
            }))
            .with(Arc::new(Stub {
                kind: AgentNodeKind::Llm,
            }));
        assert_eq!(
            registry.kinds(),
            vec![AgentNodeKind::Llm, AgentNodeKind::Subflow],
            "the list must not depend on hash order"
        );
    }

    #[test]
    fn an_empty_registry_reports_nothing() {
        let registry = ExecutorRegistry::new();
        assert!(registry.is_empty());
        assert!(registry.get(AgentNodeKind::Llm).is_none());
    }

    #[test]
    fn usage_omits_zeroes_from_the_journal() {
        let output = NodeOutput::new(json!({ "ok": true }));
        let value = serde_json::to_value(&output).unwrap();
        assert!(value.get("usage").is_none(), "{value}");
    }

    #[test]
    fn a_non_agent_node_is_refused_with_a_readable_message() {
        use nocode_model::{NodeConfig, NodeId, NodePosition, WorkflowNodeType};
        let node = WorkflowNode {
            id: NodeId::new(),
            name: "Call".into(),
            node_type: WorkflowNodeType::ManualTask,
            position: NodePosition::default(),
            config: NodeConfig::default(),
            documentation: None,
        };
        let error = agent_config(&node).unwrap_err();
        assert!(error.message.contains("ManualTask"), "{}", error.message);
    }
}
