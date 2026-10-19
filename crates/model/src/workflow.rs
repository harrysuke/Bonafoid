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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_interval_ms: u64,
    pub max_interval_ms: u64,
    pub multiplier: f64,
    pub retryable_status_codes: Vec<u16>,
}

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
