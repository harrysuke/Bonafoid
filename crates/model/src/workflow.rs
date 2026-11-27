use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Workflow {
    pub id: WorkflowId,
    pub name: String,
    pub description: Option<String>,
    pub version: String,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    pub variables: Vec<WorkflowVariable>,
    pub signals: Vec<SignalDefinition>,
    pub timers: Vec<TimerDefinition>,
    pub settings: WorkflowSettings,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Workflow {
    /// Create a new workflow with a default version.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: WorkflowId::new(),
            name: name.into(),
            description: None,
            version: "1.0.0".to_string(),
            nodes: Vec::new(),
            edges: Vec::new(),
            variables: Vec::new(),
            signals: Vec::new(),
            timers: Vec::new(),
            settings: WorkflowSettings::default(),
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct WorkflowId(pub Uuid);

impl WorkflowId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for WorkflowId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorkflowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct WorkflowNode {
    pub id: NodeId,
    pub name: String,
    pub node_type: WorkflowNodeType,
    pub position: NodePosition,
    pub config: NodeConfig,
    pub documentation: Option<String>,
}

impl WorkflowNode {
    /// The agent payload, when this node has one.
    pub fn agent_config(&self) -> Option<&crate::agent::AgentNodeConfig> {
        self.config.agent.as_ref()
    }

    /// The hard rule tying `node_type` to `config.agent`.
    ///
    /// Checked in the model rather than in the UI so that a project arriving
    /// from a file, an MCP client or a bundle is held to the same standard as
    /// one built on the canvas. Returns the first violation: an agent task with
    /// no payload cannot run, and a BPMN node carrying one is a document that
    /// two writers disagree about.
    pub fn check_agent_invariant(&self) -> Result<(), String> {
        match (&self.node_type, &self.config.agent) {
            (WorkflowNodeType::AgentTask, None) => Err(format!(
                "node `{}` is an AgentTask with no agent config",
                self.name
            )),
            (WorkflowNodeType::AgentTask, Some(_)) => Ok(()),
            (other, Some(_)) => Err(format!(
                "node `{}` is a {} and must not carry an agent config",
                self.name,
                other.tag()
            )),
            (_, None) => Ok(()),
        }
    }

    /// Configurations that are structurally legal but will not run.
    ///
    /// Separate from [`Self::check_agent_invariant`] because these are
    /// warnings: a half-filled node is normal while it is being authored, and
    /// refusing to save it would make the inspector hostile.
    pub fn agent_warnings(&self) -> Vec<String> {
        let Some(agent) = &self.config.agent else {
            if self.config.idempotent.is_some() {
                return vec![format!(
                    "node `{}` sets `idempotent`, which only applies to an AgentTask",
                    self.name
                )];
            }
            return Vec::new();
        };
        let mut warnings = Vec::new();
        match agent {
            crate::agent::AgentNodeConfig::Llm(llm) if llm.prompt.trim().is_empty() => {
                warnings.push(format!("node `{}` has an empty prompt", self.name));
            }
            crate::agent::AgentNodeConfig::Shell(shell) if shell.program.trim().is_empty() => {
                warnings.push(format!("node `{}` has no program to run", self.name));
            }
            crate::agent::AgentNodeConfig::Subflow(subflow)
                if subflow.workflow_id.0.is_nil() =>
            {
                warnings.push(format!(
                    "node `{}` calls no workflow; pick one before running",
                    self.name
                ));
            }
            _ => {}
        }
        warnings
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct NodeId(pub Uuid);

impl NodeId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum WorkflowNodeType {
    StartEvent {
        event_type: StartEventType,
    },
    EndEvent {
        event_type: EndEventType,
    },
    UserTask {
        assignee: Option<String>,
        candidate_groups: Vec<String>,
        form_id: Option<crate::form::FormId>,
        due_date: Option<String>,
    },
    ServiceTask {
        implementation: ServiceTaskImplementation,
        r#async: bool,
        exclusive: bool,
    },
    ScriptTask {
        script: String,
        script_format: String,
    },
    BusinessRuleTask {
        decision_ref: String,
        decision_table: Option<String>,
    },
    SendTask {
        message_ref: String,
    },
    ReceiveTask {
        message_ref: String,
    },
    ManualTask,
    ExclusiveGateway {
        default_flow: Option<EdgeId>,
    },
    InclusiveGateway {
        default_flow: Option<EdgeId>,
    },
    ParallelGateway,
    EventBasedGateway,
    SubProcess {
        is_event_subprocess: bool,
        is_transaction: bool,
        workflow_id: Option<WorkflowId>,
    },
    CallActivity {
        called_workflow_id: WorkflowId,
        input_mapping: IndexMap<String, String>,
        output_mapping: IndexMap<String, String>,
    },
    Task {
        task_type: TaskType,
    },
    /// A node executed by the agent runtime rather than the BPMN simulator.
    ///
    /// Carries no payload of its own: the configuration lives in
    /// [`NodeConfig::agent`], so the node type stays a cheap discriminant and
    /// the inspector reads one place. A unit variant also serialises as
    /// `{"type":"AgentTask"}`, which keeps existing documents untouched.
    AgentTask,
}

impl WorkflowNodeType {
    /// The stable wire tag, matching `#[serde(tag = "type")]`.
    ///
    /// Exists so the MCP catalogue, the canvas palette and validation messages
    /// stop each hard-coding their own copy of these strings.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::StartEvent { .. } => "StartEvent",
            Self::EndEvent { .. } => "EndEvent",
            Self::UserTask { .. } => "UserTask",
            Self::ServiceTask { .. } => "ServiceTask",
            Self::ScriptTask { .. } => "ScriptTask",
            Self::BusinessRuleTask { .. } => "BusinessRuleTask",
            Self::SendTask { .. } => "SendTask",
            Self::ReceiveTask { .. } => "ReceiveTask",
            Self::ManualTask => "ManualTask",
            Self::ExclusiveGateway { .. } => "ExclusiveGateway",
            Self::InclusiveGateway { .. } => "InclusiveGateway",
            Self::ParallelGateway => "ParallelGateway",
            Self::EventBasedGateway => "EventBasedGateway",
            Self::SubProcess { .. } => "SubProcess",
            Self::CallActivity { .. } => "CallActivity",
            Self::Task { .. } => "Task",
            Self::AgentTask => "AgentTask",
        }
    }

    /// Whether this node is executed by the agent runtime.
    pub fn is_agent_task(&self) -> bool {
        matches!(self, Self::AgentTask)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StartEventType {
    None,
    Message {
        message_ref: String,
    },
    Timer {
        time_duration: Option<String>,
        time_cycle: Option<String>,
    },
    Signal {
        signal_ref: String,
    },
    Error {
        error_ref: String,
    },
    Escalation {
        escalation_ref: String,
    },
    Compensation {
        activity_ref: String,
    },
    Conditional {
        condition: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EndEventType {
    None,
    Message { message_ref: String },
    Error { error_ref: String },
    Escalation { escalation_ref: String },
    Terminate,
    Compensation { activity_ref: String },
    Signal { signal_ref: String },
    Link { target_ref: String },
    Multiple,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum ServiceTaskImplementation {
    Http {
        url: String,
        method: HttpMethod,
        headers: IndexMap<String, String>,
        body_template: Option<String>,
        timeout_ms: u64,
        retry: RetryConfig,
    },
    Function {
        function_name: String,
        input_mapping: IndexMap<String, String>,
        output_mapping: IndexMap<String, String>,
    },
    Plugin {
        plugin_id: String,
        config: serde_json::Value,
    },
    External {
        connector_id: String,
        operation: String,
        config: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

// `RetryConfig` now lives in the shared `retry` module: the page, workflow
// and project modules each defined their own, which made
// `nocode_model::RetryConfig` ambiguous.
use crate::retry::RetryConfig;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    User,
    Service,
    Script,
    BusinessRule,
    Send,
    Receive,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct NodePosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct NodeConfig {
    pub inputs: IndexMap<String, NodeInput>,
    pub outputs: IndexMap<String, NodeOutput>,
    pub properties: IndexMap<String, serde_json::Value>,
    pub execution_listeners: Vec<ExecutionListener>,
    pub multi_instance: Option<MultiInstanceConfig>,
    pub boundary_events: Vec<BoundaryEvent>,

    /// Present iff `node_type` is `AgentTask`. `#[serde(default)]` keeps every
    /// existing document valid, and `skip_serializing_if` keeps a round-trip of
    /// an untouched project byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<crate::agent::AgentNodeConfig>,

    /// Author override for crash-recovery re-execution. `None` defers to the
    /// agent kind's default; it is meaningless on a BPMN node, and the
    /// validator says so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct NodeInput {
    pub name: String,
    pub input_type: String,
    pub required: bool,
    pub default: Option<serde_json::Value>,
    pub expression: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct NodeOutput {
    pub name: String,
    pub output_type: String,
    pub expression: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ExecutionListener {
    pub event: ExecutionEvent,
    pub implementation: ListenerImplementation,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionEvent {
    Start,
    End,
    Take,
    Error,
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum ListenerImplementation {
    Script { script: String },
    Expression { expression: String },
    Delegate { class_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct MultiInstanceConfig {
    pub mode: MultiInstanceMode,
    pub collection_expression: String,
    pub element_variable: String,
    pub completion_condition: Option<String>,
    pub cardinality: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MultiInstanceMode {
    #[default]
    Sequential,
    Parallel,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct BoundaryEvent {
    pub id: BoundaryEventId,
    pub event_type: BoundaryEventType,
    pub attached_to: NodeId,
    pub cancel_activity: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct BoundaryEventId(pub Uuid);

impl BoundaryEventId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for BoundaryEventId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum BoundaryEventType {
    Timer {
        time_duration: Option<String>,
        time_cycle: Option<String>,
    },
    Error {
        error_ref: String,
    },
    Escalation {
        escalation_ref: String,
    },
    Signal {
        signal_ref: String,
    },
    Message {
        message_ref: String,
    },
    Compensation {
        activity_ref: String,
    },
    Conditional {
        condition: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct EdgeId(pub Uuid);

impl EdgeId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for EdgeId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for EdgeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct WorkflowEdge {
    pub id: EdgeId,
    pub source: NodeId,
    pub target: NodeId,
    pub source_handle: Option<String>,
    pub target_handle: Option<String>,
    pub condition: Option<String>,
    pub label: Option<String>,
    pub style: EdgeStyle,

    /// Port bindings this edge carries: `{"files": "input.files"}` maps the
    /// target's `files` port from the source scope. Empty for a plain
    /// control-flow line, which is every edge the BPMN editor draws.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub mapping: IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct EdgeStyle {
    pub stroke_color: Option<String>,
    pub stroke_width: Option<f32>,
    pub stroke_dasharray: Option<String>,
    pub marker_end: Option<String>,
    pub animated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct WorkflowVariable {
    pub id: VariableId,
    pub name: String,
    pub variable_type: VariableType,
    pub default_value: Option<serde_json::Value>,
    pub scope: VariableScope,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct VariableId(pub Uuid);

impl VariableId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for VariableId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for VariableId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum VariableType {
    String,
    Integer,
    Float,
    Boolean,
    Date,
    DateTime,
    Json,
    Object {
        properties: IndexMap<String, VariableType>,
    },
    Array {
        items: Box<VariableType>,
    },
    File,
    Binary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VariableScope {
    Global,
    Process,
    Local,
    Task,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SignalDefinition {
    pub id: SignalId,
    pub name: String,
    pub scope: SignalScope,
    pub payload_type: Option<VariableType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct SignalId(pub Uuid);

impl SignalId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SignalId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SignalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SignalScope {
    Global,
    Process,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TimerDefinition {
    pub id: TimerId,
    pub name: String,
    pub timer_type: TimerType,
    pub time_duration: Option<String>,
    pub time_cycle: Option<String>,
    pub time_date: Option<DateTime<Utc>>,
    pub attached_to: Option<NodeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct TimerId(pub Uuid);

impl TimerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TimerId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TimerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TimerType {
    Duration,
    Cycle,
    Date,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct WorkflowSettings {
    pub is_executable: bool,
    pub history_level: HistoryLevel,
    pub async_continuations: bool,
    pub exclusive_gateway_default: bool,
    pub timeout: Option<String>,
    pub retry_config: Option<RetryConfig>,
    pub variables_serialization: SerializationFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HistoryLevel {
    None,
    #[default]
    Activity,
    Audit,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SerializationFormat {
    #[default]
    Json,
    Protobuf,
    Avro,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentNodeConfig, AgentNodeKind, LlmNode};
    use serde_json::{json, Value};

    fn node(node_type: WorkflowNodeType, config: NodeConfig) -> WorkflowNode {
        WorkflowNode {
            id: NodeId::new(),
            name: "n".into(),
            node_type,
            position: NodePosition::default(),
            config,
            documentation: None,
        }
    }

    /// The Phase 34 gate: a document written before this milestone must still
    /// parse, and must serialise back to the same bytes.
    #[test]
    fn a_document_from_before_the_agent_node_parses_and_round_trips() {
        let original = json!({
            "id": "3f1c1a2e-0000-4000-8000-000000000001",
            "name": "Approve",
            "node_type": {
                "type": "UserTask",
                "config": {
                    "assignee": "alice",
                    "candidate_groups": [],
                    "form_id": null,
                    "due_date": null
                }
            },
            "position": { "x": 1.0, "y": 2.0 },
            "config": {
                "inputs": {},
                "outputs": {},
                "properties": {},
                "execution_listeners": [],
                "multi_instance": null,
                "boundary_events": []
            },
            "documentation": null
        });

        let parsed: WorkflowNode = serde_json::from_value(original.clone()).expect("parses");
        assert!(parsed.agent_config().is_none());

        let reserialised = serde_json::to_value(&parsed).expect("serialises");
        assert_eq!(
            reserialised, original,
            "the new optional fields must not appear in a document that omits them"
        );
    }

    #[test]
    fn an_agent_task_round_trips_with_its_payload() {
        let mut config = NodeConfig::default();
        config.agent = Some(AgentNodeConfig::Llm(LlmNode {
            prompt: "summarise ${input.diff}".into(),
            ..Default::default()
        }));

        let node = node(WorkflowNodeType::AgentTask, config);
        let value = serde_json::to_value(&node).expect("serialises");
        assert_eq!(value["node_type"]["type"], "AgentTask");
        assert_eq!(value["config"]["agent"]["kind"], "llm");

        let parsed: WorkflowNode = serde_json::from_value(value).expect("parses");
        assert_eq!(parsed, node);
        assert_eq!(
            parsed.agent_config().map(AgentNodeConfig::kind),
            Some(AgentNodeKind::Llm)
        );
    }

    #[test]
    fn the_invariant_refuses_both_mismatches() {
        let empty = node(WorkflowNodeType::AgentTask, NodeConfig::default());
        let message = empty.check_agent_invariant().unwrap_err();
        assert!(message.contains("no agent config"), "{message}");

        let mut bpmn_with_agent = NodeConfig::default();
        bpmn_with_agent.agent = Some(AgentNodeConfig::default_for(AgentNodeKind::Shell));
        let message = node(WorkflowNodeType::ManualTask, bpmn_with_agent)
            .check_agent_invariant()
            .unwrap_err();
        assert!(message.contains("ManualTask"), "{message}");

        let mut good = NodeConfig::default();
        good.agent = Some(AgentNodeConfig::default_for(AgentNodeKind::Shell));
        assert!(node(WorkflowNodeType::AgentTask, good)
            .check_agent_invariant()
            .is_ok());
        assert!(node(WorkflowNodeType::ManualTask, NodeConfig::default())
            .check_agent_invariant()
            .is_ok());
    }

    #[test]
    fn warnings_flag_a_node_that_cannot_run_without_blocking_the_save() {
        let mut config = NodeConfig::default();
        config.agent = Some(AgentNodeConfig::default_for(AgentNodeKind::Shell));
        let warnings = node(WorkflowNodeType::AgentTask, config).agent_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no program"), "{warnings:?}");
    }

    #[test]
    fn idempotent_on_a_bpmn_node_is_a_warning_not_an_error() {
        let mut config = NodeConfig::default();
        config.idempotent = Some(true);
        let node = node(WorkflowNodeType::ManualTask, config);
        assert!(node.check_agent_invariant().is_ok());
        assert_eq!(node.agent_warnings().len(), 1);
    }

    #[test]
    fn tags_are_unique_and_match_the_serialised_discriminant() {
        let variants = vec![
            WorkflowNodeType::StartEvent {
                event_type: StartEventType::None,
            },
            WorkflowNodeType::EndEvent {
                event_type: EndEventType::None,
            },
            WorkflowNodeType::ManualTask,
            WorkflowNodeType::ParallelGateway,
            WorkflowNodeType::AgentTask,
        ];
        let mut seen = std::collections::HashSet::new();
        for variant in variants {
            assert!(seen.insert(variant.tag()), "duplicate tag for {variant:?}");
            let value = serde_json::to_value(&variant).expect("serialises");
            assert_eq!(value["type"], variant.tag());
        }
    }

    #[test]
    fn an_empty_edge_mapping_stays_out_of_the_document() {
        let edge = WorkflowEdge {
            id: EdgeId::new(),
            source: NodeId::new(),
            target: NodeId::new(),
            source_handle: None,
            target_handle: None,
            condition: None,
            label: None,
            style: EdgeStyle::default(),
            mapping: IndexMap::new(),
        };
        let value: Value = serde_json::to_value(&edge).expect("serialises");
        assert!(value.get("mapping").is_none());
    }
}
