//! Configuration for agent nodes — the payload of
//! [`WorkflowNodeType::AgentTask`].
//!
//! This is the vocabulary shared by the canvas, the MCP authoring tools and the
//! runtime's executor registry. It lives in the model crate rather than in the
//! runtime so that a project can be *authored* (and validated, and exported to
//! BPMN) on a host that does not link the executor.
//!
//! Three rules hold this module together:
//!
//! 1. **A config carries only what the executor needs.** Anything secret is a
//!    *name* (`${secret:OPENAI_API_KEY}`), never a value. A project file is
//!    shared, committed and generated into a bundle; a key must not travel
//!    with it.
//! 2. **Every field has a serde default.** A half-filled node from the palette
//!    deserialises, so the inspector can open it before it is finished.
//! 3. **The kind is derived from the payload, not stored beside it.** Adding a
//!    variant to [`AgentNodeConfig`] therefore cannot leave a node claiming one
//!    kind while carrying another's config.

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::workflow::{HttpMethod, WorkflowId};

/// The nine executor kinds, keyed by the runtime's registry.
///
/// Deliberately a plain unit enum with no payload: the registry maps a kind to
/// an executor, and a kind that carried data would invite a second source of
/// truth for the config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentNodeKind {
    Llm,
    Shell,
    File,
    Git,
    Http,
    Sqlite,
    Memory,
    Approval,
    Subflow,
}

impl AgentNodeKind {
    /// Every kind, in palette order.
    pub const ALL: [AgentNodeKind; 9] = [
        AgentNodeKind::Llm,
        AgentNodeKind::Shell,
        AgentNodeKind::File,
        AgentNodeKind::Git,
        AgentNodeKind::Http,
        AgentNodeKind::Sqlite,
        AgentNodeKind::Memory,
        AgentNodeKind::Approval,
        AgentNodeKind::Subflow,
    ];

    /// The wire spelling, matching `#[serde(rename_all = "snake_case")]`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Llm => "llm",
            Self::Shell => "shell",
            Self::File => "file",
            Self::Git => "git",
            Self::Http => "http",
            Self::Sqlite => "sqlite",
            Self::Memory => "memory",
            Self::Approval => "approval",
            Self::Subflow => "subflow",
        }
    }

    /// Parse the wire spelling. Case-insensitive, because an MCP client should
    /// not have to guess whether the tag is `Llm` or `llm`.
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == normalized)
    }

    /// Whether re-running this kind after a crash is safe, at the kind level.
    ///
    /// `None` for a kind whose answer depends on its payload — use
    /// [`AgentNodeConfig::idempotent`], which reads the config. This one exists
    /// for the catalogue, which describes kinds before any payload exists.
    pub fn idempotent_by_default(&self) -> Option<bool> {
        match self {
            Self::Llm | Self::Memory | Self::Approval => Some(true),
            Self::Shell | Self::Git => Some(false),
            Self::File | Self::Http | Self::Sqlite | Self::Subflow => None,
        }
    }
}

/// The typed payload of an `AgentTask` node.
///
/// Adjacently tagged rather than internally tagged: `{"kind":"llm","config":{…}}`
/// keeps each payload a self-contained object, which schemars renders as a
/// clean `oneOf` and the inspector can switch on without flattening surprises.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", content = "config", rename_all = "snake_case")]
pub enum AgentNodeConfig {
    Llm(LlmNode),
    Shell(ShellNode),
    File(FileNode),
    Git(GitNode),
    Http(HttpNode),
    Sqlite(SqliteNode),
    Memory(MemoryNode),
    Approval(ApprovalNode),
    Subflow(SubflowNode),
}

impl AgentNodeConfig {
    /// The executor kind this payload belongs to.
    pub fn kind(&self) -> AgentNodeKind {
        match self {
            Self::Llm(_) => AgentNodeKind::Llm,
            Self::Shell(_) => AgentNodeKind::Shell,
            Self::File(_) => AgentNodeKind::File,
            Self::Git(_) => AgentNodeKind::Git,
            Self::Http(_) => AgentNodeKind::Http,
            Self::Sqlite(_) => AgentNodeKind::Sqlite,
            Self::Memory(_) => AgentNodeKind::Memory,
            Self::Approval(_) => AgentNodeKind::Approval,
            Self::Subflow(_) => AgentNodeKind::Subflow,
        }
    }

    /// A starter payload for a freshly dropped palette node.
    ///
    /// Every field is at its serde default, so this is exactly what
    /// `serde_json::from_value(json!({"kind": kind}))` produces — the two
    /// cannot drift.
    pub fn default_for(kind: AgentNodeKind) -> Self {
        match kind {
            AgentNodeKind::Llm => Self::Llm(LlmNode::default()),
            AgentNodeKind::Shell => Self::Shell(ShellNode::default()),
            AgentNodeKind::File => Self::File(FileNode::default()),
            AgentNodeKind::Git => Self::Git(GitNode::default()),
            AgentNodeKind::Http => Self::Http(HttpNode::default()),
            AgentNodeKind::Sqlite => Self::Sqlite(SqliteNode::default()),
            AgentNodeKind::Memory => Self::Memory(MemoryNode::default()),
            AgentNodeKind::Approval => Self::Approval(ApprovalNode::default()),
            AgentNodeKind::Subflow => Self::Subflow(SubflowNode::default()),
        }
    }

    /// The author's `idempotent` override, else the kind's resolved default.
    ///
    /// `None` means the node cannot answer for itself — today only a
    /// `subflow`, whose effects belong to the callee. The runtime treats
    /// `None` as "do not guess": a crash leaves the node `Unknown` and the run
    /// stops rather than risk repeating a side effect.
    pub fn idempotent(&self, override_flag: Option<bool>) -> Option<bool> {
        override_flag.or_else(|| self.resolved_idempotent())
    }

    /// Idempotency derived from the payload, not just the kind.
    ///
    /// `file:read` and `http:GET` are safe to repeat; `file:write` and
    /// `http:POST` are not. Answering at the kind level alone would force the
    /// author to annotate every node to avoid a needless stop, and would let a
    /// `POST` be silently retried.
    fn resolved_idempotent(&self) -> Option<bool> {
        match self {
            Self::File(node) => Some(matches!(
                node.op,
                FileOp::Read | FileOp::List
            )),
            Self::Http(node) => Some(matches!(
                node.method,
                HttpMethod::Get | HttpMethod::Head | HttpMethod::Options
            )),
            Self::Sqlite(node) => Some(node.mode == SqliteMode::Read),
            // A subflow's effects are its own; the callee's journal is the only
            // place that can answer.
            Self::Subflow(_) => None,
            other => other.kind().idempotent_by_default(),
        }
    }
}

/// A model call, optionally with a tool loop and a structured output.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct LlmNode {
    /// Catalog platform id (`openai`, `groq`, `ollama`, …). `None` uses the
    /// host's default provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Model id. `None` uses the provider's default from the catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Prompt template, interpolated with `${...}` over the node scope.
    #[serde(default)]
    pub prompt: String,
    /// Tool names the model may call, resolved against the run's registry.
    #[serde(default)]
    pub tools: Vec<String>,
    /// When set, the response must validate against this JSON Schema or the
    /// node fails with `invalid_output` after one repair turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Bounded tool-calling loop. `1` means a single completion, no tools.
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
    /// Stream deltas to the canvas. Off by default: a cron run has nowhere to
    /// put them.
    #[serde(default)]
    pub stream: bool,
}

fn default_max_turns() -> u32 {
    8
}

// Written out rather than derived: `#[derive(Default)]` would set `max_turns`
// to 0 and ignore `default_max_turns`, so a palette node and a deserialised
// node would disagree about how many turns it may take.
impl Default for LlmNode {
    fn default() -> Self {
        Self {
            provider: None,
            model: None,
            system: None,
            prompt: String::new(),
            tools: Vec::new(),
            output_schema: None,
            temperature: None,
            max_tokens: None,
            max_turns: default_max_turns(),
            stream: false,
        }
    }
}

/// How a shell command is invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShellMode {
    /// `program` + `args` go straight to `Command`. No shell parses them, so
    /// `;`, `|` and `$()` are literal.
    #[default]
    Argv,
    /// `program` is a command line handed to a shell. Always gated by an
    /// approval, even when the program is allowlisted.
    Shell,
}

/// A child process, killed as a process group on timeout or cancel.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ShellNode {
    #[serde(default)]
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory, relative to the workspace root. `None` is the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Extra environment. Everything else is scrubbed by the executor.
    #[serde(default)]
    pub env: IndexMap<String, String>,
    #[serde(default = "default_shell_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub mode: ShellMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
}

fn default_shell_timeout_ms() -> u64 {
    600_000
}

impl Default for ShellNode {
    fn default() -> Self {
        Self {
            program: String::new(),
            args: Vec::new(),
            cwd: None,
            env: IndexMap::new(),
            timeout_ms: default_shell_timeout_ms(),
            mode: ShellMode::Argv,
            stdin: None,
        }
    }
}

/// A file operation, jailed to the workspace root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum FileOp {
    #[default]
    Read,
    Write,
    Append,
    Patch,
    List,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FileNode {
    #[serde(default)]
    pub op: FileOp,
    /// Path relative to the workspace root. Checked after canonicalisation, so
    /// `..` and symlinks cannot escape.
    #[serde(default)]
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// For `list`: the glob to expand under `path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob: Option<String>,
}

/// A git operation. Read ops are free; the write ops need `git:write`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum GitOp {
    #[default]
    Status,
    Diff,
    Log,
    Branch,
    Commit,
    Push,
    Pr,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct GitNode {
    #[serde(default)]
    pub op: GitOp,
    /// Extra arguments passed to `git`. Never a credential: those come from the
    /// deploy vault.
    #[serde(default)]
    pub args: Vec<String>,
    /// Commit message, interpolated. Used by `commit` and `pr`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// An HTTP request, restricted to a per-node host allowlist.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct HttpNode {
    #[serde(default)]
    pub url: String,
    #[serde(default = "default_http_method")]
    pub method: HttpMethod,
    #[serde(default)]
    pub headers: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default = "default_http_timeout_ms")]
    pub timeout_ms: u64,
    /// Statuses that count as success. Empty means "any 2xx".
    #[serde(default)]
    pub expect: Vec<u16>,
}

fn default_http_method() -> HttpMethod {
    HttpMethod::Get
}

fn default_http_timeout_ms() -> u64 {
    30_000
}

impl Default for HttpNode {
    fn default() -> Self {
        Self {
            url: String::new(),
            method: default_http_method(),
            headers: IndexMap::new(),
            body: None,
            timeout_ms: default_http_timeout_ms(),
            expect: Vec::new(),
        }
    }
}

/// Whether a SQLite node may write. Read-only is the default connection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum SqliteMode {
    #[default]
    Read,
    Write,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct SqliteNode {
    /// Database file, relative to the workspace root.
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub params: Vec<serde_json::Value>,
    #[serde(default)]
    pub mode: SqliteMode,
}

/// A memory-store operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryOp {
    #[default]
    Get,
    Put,
    Append,
    Search,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct MemoryNode {
    /// Store file. `None` is the workspace's `.bonafoid/memory.db`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    #[serde(default)]
    pub op: MemoryOp,
    /// `workflow:<id>`, `agent:<name>` or `global`.
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Lifetime, e.g. `30d` or `12h`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

fn default_namespace() -> String {
    "global".to_string()
}

impl Default for MemoryNode {
    fn default() -> Self {
        Self {
            store: None,
            op: MemoryOp::Get,
            namespace: default_namespace(),
            key: None,
            query: None,
            top_k: None,
            ttl: None,
        }
    }
}

/// What a run does when an approval is never answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnTimeout {
    #[default]
    Fail,
    Skip,
}

/// The title an approval card gets when the author did not write one.
///
/// Public because it is not merely a default: on the wire, an unset title and a
/// title set to this string are indistinguishable, so a host that wants to fall
/// back to the node's name has to compare against this constant. A literal
/// `"Approval required"` in a host would silently stop matching the day this
/// changed.
pub const DEFAULT_APPROVAL_TITLE: &str = "Approval required";

/// A human gate. Suspends the run until someone answers.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ApprovalNode {
    #[serde(default = "default_approval_title")]
    pub title: String,
    /// Body shown with the card, interpolated. Usually a summary of what the
    /// next node is about to do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// How long to wait, e.g. `1h`. `None` waits indefinitely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(default)]
    pub on_timeout: OnTimeout,
}

impl Default for ApprovalNode {
    fn default() -> Self {
        Self {
            title: default_approval_title(),
            body: None,
            timeout: None,
            on_timeout: OnTimeout::Fail,
        }
    }
}

fn default_approval_title() -> String {
    DEFAULT_APPROVAL_TITLE.to_string()
}

/// A call into another workflow in the same project.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SubflowNode {
    /// Deterministic placeholder (nil) until the author picks a workflow; the
    /// validator reports a nil id rather than silently running nothing.
    #[serde(default = "nil_workflow_id")]
    pub workflow_id: WorkflowId,
    #[serde(default)]
    pub input_mapping: IndexMap<String, String>,
    #[serde(default)]
    pub output_mapping: IndexMap<String, String>,
}

fn nil_workflow_id() -> WorkflowId {
    WorkflowId(Uuid::nil())
}

// Again not derived: `WorkflowId::default()` mints a fresh uuid, which would
// make a palette subflow look configured while a deserialised one is nil.
impl Default for SubflowNode {
    fn default() -> Self {
        Self {
            workflow_id: nil_workflow_id(),
            input_mapping: IndexMap::new(),
            output_mapping: IndexMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_kind_round_trips_through_its_tag() {
        for kind in AgentNodeKind::ALL {
            let config = AgentNodeConfig::default_for(kind);
            let value = serde_json::to_value(&config).expect("serialises");
            assert_eq!(
                value["kind"],
                json!(kind.as_str()),
                "{kind:?} serialised with the wrong tag: {value}"
            );
            let parsed: AgentNodeConfig = serde_json::from_value(value).expect("deserialises");
            assert_eq!(parsed, config);
            assert_eq!(parsed.kind(), kind);

            // The palette's default and the empty wire form must be the same
            // value, or a node created on the canvas and one read from a file
            // would differ before anyone edited anything.
            let from_empty: AgentNodeConfig =
                serde_json::from_value(json!({ "kind": kind.as_str(), "config": {} }))
                    .expect("empty config deserialises");
            assert_eq!(from_empty, config, "{kind:?} default drifted");
        }
    }

    #[test]
    fn the_minimal_wire_form_is_a_kind_and_an_empty_config() {
        // `{"kind":"shell","config":{}}` is the smallest document that parses,
        // and it must equal the palette's default. The `config` key itself is
        // required by the adjacent tagging, so an author who omits it gets a
        // serde error naming the missing field rather than a silent default.
        let parsed: AgentNodeConfig =
            serde_json::from_value(json!({ "kind": "shell", "config": {} }))
                .expect("deserialises");
        assert_eq!(parsed, AgentNodeConfig::default_for(AgentNodeKind::Shell));

        let error = serde_json::from_value::<AgentNodeConfig>(json!({ "kind": "shell" }))
            .expect_err("the content key is required");
        assert!(error.to_string().contains("config"), "{error}");
    }

    #[test]
    fn parse_is_case_insensitive_and_rejects_junk() {
        assert_eq!(AgentNodeKind::parse("LLM"), Some(AgentNodeKind::Llm));
        assert_eq!(AgentNodeKind::parse(" sqlite "), Some(AgentNodeKind::Sqlite));
        assert_eq!(AgentNodeKind::parse("bogus"), None);
    }

    #[test]
    fn an_unknown_kind_is_refused_rather_than_defaulted() {
        // The alternative — falling back to `llm` — would turn a typo into a
        // node that quietly calls a model with an empty prompt.
        let value = json!({ "kind": "telepathy", "config": {} });
        let parsed: Result<AgentNodeConfig, _> = serde_json::from_value(value);
        assert!(parsed.is_err());
    }

    #[test]
    fn idempotency_defaults_are_explicit_where_it_matters() {
        let llm = AgentNodeConfig::default_for(AgentNodeKind::Llm);
        assert_eq!(llm.idempotent(None), Some(true));
        let shell = AgentNodeConfig::default_for(AgentNodeKind::Shell);
        assert_eq!(shell.idempotent(None), Some(false));
        // Resolved from the payload, not the kind: a GET may repeat, a POST
        // may not, and both are `http` nodes.
        let http = AgentNodeConfig::default_for(AgentNodeKind::Http);
        assert_eq!(http.idempotent(None), Some(true), "a fresh http node is a GET");
        assert_eq!(http.idempotent(Some(false)), Some(false));
        let post = AgentNodeConfig::Http(HttpNode {
            method: HttpMethod::Post,
            ..Default::default()
        });
        assert_eq!(post.idempotent(None), Some(false));
    }

    #[test]
    fn file_reads_are_idempotent_and_writes_are_not() {
        let read = AgentNodeConfig::default_for(AgentNodeKind::File);
        assert_eq!(read.idempotent(None), Some(true));
        let write = AgentNodeConfig::File(FileNode {
            op: FileOp::Write,
            path: "a.txt".into(),
            ..Default::default()
        });
        assert_eq!(write.idempotent(None), Some(false));
    }

    #[test]
    fn a_subflow_admits_it_cannot_answer() {
        // The callee's journal is the only thing that knows whether its own
        // side effects happened, so the runtime must stop rather than guess.
        let subflow = AgentNodeConfig::default_for(AgentNodeKind::Subflow);
        assert_eq!(subflow.idempotent(None), None);
        assert_eq!(subflow.idempotent(Some(true)), Some(true));
    }

    #[test]
    fn a_fresh_shell_node_cannot_silently_run_forever() {
        let node = ShellNode::default();
        assert_eq!(node.mode, ShellMode::Argv);
        assert_eq!(node.timeout_ms, 600_000);
        let llm = LlmNode::default();
        assert_eq!(llm.max_turns, 8);
        assert!(!llm.stream);
    }
}
