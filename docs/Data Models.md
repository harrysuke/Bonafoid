# Data Models

## No-Code Visual Application Development Platform

**Version:** 1.0  
**Date:** October 2026  
**Status:** Draft  

---

## 1. Core Data Model Overview

This document defines the core data structures for the No-Code IDE platform. These models represent the domain entities used throughout the application - from the visual builders in the IDE to the generated application schemas.

### 1.1 Model Architecture

The models are defined in `crates/model/src/` and follow these patterns:
- **Serde**: Serialization/deserialization with `serde` for JSON persistence
- **JSON Schema**: `schemars` derive for schema generation
- **Strong Types**: Newtype patterns for IDs (ProjectId, FormId, etc.)
- **IndexMap**: Used for preserving field order in UI components

---

## 2. Project Model

Represents the root container for all application artifacts.

```rust
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub version: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub settings: ProjectSettings,
    pub forms: IndexMap<FormId, Form>,
    pub entities: IndexMap<EntityId, Entity>,
    pub workflows: IndexMap<WorkflowId, Workflow>,
    pub pages: IndexMap<PageId, Page>,
    pub themes: IndexMap<ThemeId, Theme>,
    pub plugins: Vec<PluginConfig>,
    pub environments: IndexMap<String, EnvironmentConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ProjectId(pub Uuid);
```

---

## 3. Form Model

Defines dynamic forms with fields, validation, and conditional logic.

### 3.1 Form Definition

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct Form {
    pub id: FormId,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub fields: IndexMap<FieldId, FormField>,
    pub layout: FormLayout,
    pub validation: FormValidation,
    pub actions: Vec<FormAction>,
    pub settings: FormSettings,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FormId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FieldId(pub Uuid);
```

### 3.2 Form Field

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Textarea,
    Email,
    Password,
    Number,
    Integer,
    Float,
    Decimal,
    Date,
    Datetime,
    Time,
    Boolean,
    Checkbox,
    Radio,
    Select,
    MultiSelect,
    File,
    Image,
    RichText,
    Json,
    Color,
    Range,
    Slider,
    Rating,
    Signature,
    Relation,
    Computed,
    Section,
    Repeater,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct FormField {
    pub id: FieldId,
    pub name: String,
    pub label: String,
    pub r#type: FieldType,
    pub placeholder: Option<String>,
    pub help_text: Option<String>,
    pub default_value: Option<serde_json::Value>,
    pub required: bool,
    pub validation: FieldValidation,
    pub options: Option<FieldOptions>,
    pub conditional: Option<ConditionalLogic>,
    pub permissions: FieldPermissions,
    pub settings: IndexMap<String, serde_json::Value>,
    pub order: u32,
}
```

### 3.3 Field Validation

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FieldValidation {
    pub min_length: Option<u32>,
    pub max_length: Option<u32>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub pattern: Option<String>,
    pub email: bool,
    pub url: bool,
    pub regex: Option<String>,
    pub custom: Option<String>,
    pub error_message: Option<String>,
}
```

### 3.4 Conditional Logic

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOperator {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    In,
    NotIn,
    IsEmpty,
    IsNotEmpty,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ConditionalLogic {
    pub conditions: Vec<Condition>,
    pub operator: LogicalOperator,
    pub action: ConditionAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LogicalOperator {
    And,
    Or,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConditionAction {
    Show,
    Hide,
    Enable,
    Disable,
}
```

---

## 4. Entity Model (Data Modeler)

Defines database entities, fields, relationships for ERD modeling.

### 4.1 Entity Definition

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    pub table_name: String,
    pub description: Option<String>,
    pub fields: IndexMap<FieldId, EntityField>,
    pub relationships: Vec<Relationship>,
    pub indexes: Vec<Index>,
    pub constraints: Vec<Constraint>,
    pub timestamps: bool,
    pub soft_delete: bool,
    pub audit_log: bool,
    pub rls: Option<RlsPolicy>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct EntityId(pub Uuid);
```

### 4.2 Entity Field

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EntityFieldType {
    String,
    Text,
    Integer,
    BigInt,
    Float,
    Double,
    Decimal,
    Boolean,
    Date,
    Datetime,
    Timestamp,
    Json,
    Jsonb,
    Uuid,
    Enum,
    Bytes,
    Relation,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EntityField {
    pub id: FieldId,
    pub name: String,
    pub column_name: String,
    pub r#type: EntityFieldType,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub auto_increment: bool,
    pub default_value: Option<String>,
    pub comment: Option<String>,
    pub length: Option<u32>,
    pub precision: Option<u8>,
    pub scale: Option<u8>,
    pub enum_values: Option<Vec<String>>,
    pub order: u32,
}
```

### 4.3 Relationships

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipType {
    BelongsTo,
    HasOne,
    HasMany,
    ManyToMany,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CascadeType {
    Cascade,
    SetNull,
    Restrict,
    NoAction,
    SetDefault,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Relationship {
    pub id: Uuid,
    pub name: String,
    pub r#type: RelationshipType,
    pub source_entity: EntityId,
    pub target_entity: EntityId,
    pub foreign_key: Option<String>,
    pub local_key: Option<String>,
    pub through: Option<EntityId>,
    pub on_delete: CascadeType,
    pub on_update: CascadeType,
    pub is_junction: bool,
}
```

### 4.4 Indexes & Constraints

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IndexType {
    BTree,
    Hash,
    Gin,
    Gist,
    Brin,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Index {
    pub id: Uuid,
    pub name: String,
    pub r#type: IndexType,
    pub columns: Vec<String>,
    pub unique: bool,
    pub partial: bool,
    pub where_clause: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintType {
    Check,
    ForeignKey,
    Unique,
    PrimaryKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Constraint {
    pub id: Uuid,
    pub name: String,
    pub r#type: ConstraintType,
    pub expression: String,
    pub columns: Vec<String>,
}
```

---

## 5. Workflow Model (BPMN 2.0)

Comprehensive BPMN workflow modeling with full execution support.

### 5.1 Workflow Definition

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct Workflow {
    pub id: WorkflowId,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub version: String,
    pub is_executable: bool,
    pub nodes: IndexMap<NodeId, WorkflowNode>,
    pub edges: IndexMap<EdgeId, WorkflowEdge>,
    pub variables: IndexMap<String, WorkflowVariable>,
    pub messages: Vec<BpmnMessage>,
    pub signals: Vec<BpmnSignal>,
    pub errors: Vec<BpmnError>,
    pub settings: WorkflowSettings,
    pub bpmn_xml: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct WorkflowId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct NodeId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct EdgeId(pub Uuid);
```

### 5.2 Workflow Nodes

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum WorkflowNodeType {
    StartEvent {
        event_type: StartEventType,
    },
    EndEvent {
        event_type: EndEventType,
    },
    IntermediateCatchEvent {
        event_type: IntermediateCatchEventType,
    },
    IntermediateThrowEvent {
        event_type: IntermediateThrowEventType,
    },
    UserTask {
        assignee: Option<String>,
        candidate_groups: Vec<String>,
        candidate_users: Vec<String>,
        form_id: Option<FormId>,
        due_date: Option<String>,
        priority: Option<String>,
    },
    ServiceTask {
        implementation: ServiceTaskImplementation,
        r#async: bool,
        exclusive: bool,
        retry_config: Option<RetryConfig>,
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
        called_element: String,
    },
    BoundaryEvent {
        event_type: BoundaryEventType,
        attached_to: NodeId,
        cancel_activity: bool,
    },
}
```

### 5.3 Node Position & Flow

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct WorkflowNode {
    pub id: NodeId,
    pub name: String,
    pub r#type: WorkflowNodeType,
    pub position: Position,
    pub dimensions: Dimensions,
    pub properties: IndexMap<String, serde_json::Value>,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Dimensions {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct WorkflowEdge {
    pub id: EdgeId,
    pub source: NodeId,
    pub target: NodeId,
    pub name: Option<String>,
    pub condition: Option<String>,
    pub r#type: EdgeType,
    pub source_handle: Option<String>,
    pub target_handle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    SequenceFlow,
    MessageFlow,
    Association,
}
```

---

## 6. Page Model (Page Builder)

Visual page composition with components, bindings, and events.

### 6.1 Page Definition

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct Page {
    pub id: PageId,
    pub name: String,
    pub slug: String,
    pub path: String,
    pub title: String,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub layout: PageLayout,
    pub components: Vec<PageComponent>,
    pub data_sources: Vec<DataSource>,
    pub actions: Vec<PageAction>,
    pub seo: SeoConfig,
    pub permissions: PagePermissions,
    pub meta: IndexMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct PageId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ComponentId(pub Uuid);
```

### 6.2 Page Layout

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct PageLayout {
    pub r#type: LayoutType,
    pub containers: Vec<LayoutContainer>,
    pub breakpoints: Breakpoints,
    pub global_styles: IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LayoutType {
    SingleColumn,
    TwoColumn,
    ThreeColumn,
    Grid,
    Flex,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct LayoutContainer {
    pub id: Uuid,
    pub name: String,
    pub order: u32,
    pub columns: u8,
    pub gap: String,
    pub padding: String,
    pub margin: String,
    pub background: Option<String>,
    pub children: Vec<ComponentId>,
}
```

### 6.3 Page Components

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum ComponentType {
    Container { children: Vec<ComponentId> },
    Grid { columns: u8, gap: String, children: Vec<ComponentId> },
    Flex { direction: String, gap: String, children: Vec<ComponentId> },
    Card { header: Option<String>, footer: Option<String>, children: Vec<ComponentId> },
    Tabs { tabs: Vec<TabConfig>, children: Vec<ComponentId> },
    Accordion { items: Vec<AccordionItem>, children: Vec<ComponentId> },
    Text { content: String, r#type: TextType },
    Heading { content: String, level: u8 },
    Image { src: String, alt: String, width: Option<String>, height: Option<String> },
    Button { label: String, variant: ButtonVariant, size: ButtonSize, action: String },
    Input { label: String, placeholder: Option<String>, r#type: String },
    Select { label: String, options: Vec<SelectOption>, multiple: bool },
    Checkbox { label: String },
    Radio { label: String, options: Vec<SelectOption> },
    Form { form_id: FormId, submit_action: String },
    Table { columns: Vec<TableColumn>, data_source: String },
    List { item_template: ComponentId, data_source: String },
    Chart { chart_type: ChartType, data_source: String, config: ChartConfig },
    StatCard { value: String, label: String, icon: Option<String> },
    Alert { message: String, r#type: AlertType, closable: bool },
    Modal { trigger: ComponentId, children: Vec<ComponentId> },
    Html { content: String, sanitize: bool },
}
```

### 6.4 Component Properties & Events

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct PageComponent {
    pub id: ComponentId,
    pub component: ComponentType,
    pub props: IndexMap<String, serde_json::Value>,
    pub bindings: Vec<DataBinding>,
    pub events: Vec<ComponentEvent>,
    pub conditions: Vec<RenderCondition>,
    pub styles: IndexMap<String, String>,
    pub order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct DataBinding {
    pub id: Uuid,
    pub source: String,
    pub target: String,
    pub r#type: BindingType,
    pub default_value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BindingType {
    Static,
    Variable,
    Api,
    Query,
    Expression,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ComponentEvent {
    pub id: Uuid,
    pub trigger: EventTrigger,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventTrigger {
    Click,
    Change,
    Submit,
    Focus,
    Blur,
    MouseEnter,
    MouseLeave,
    KeyPress,
    Load,
}
```

---

## 7. Theme Model

Design system with tokens, theming, and dark mode support.

### 7.1 Theme Definition

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct Theme {
    pub id: ThemeId,
    pub name: String,
    pub description: Option<String>,
    pub is_default: bool,
    pub tokens: DesignTokens,
    pub components: ComponentTheming,
    pub dark_mode: DarkModeConfig,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ThemeId(pub Uuid);
```

### 7.2 Design Tokens

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct DesignTokens {
    pub colors: ColorTokens,
    pub typography: TypographyTokens,
    pub spacing: SpacingTokens,
    pub border: BorderTokens,
    pub shadow: ShadowTokens,
    pub radius: RadiusTokens,
    pub z_index: IndexMap<String, u32>,
    pub breakpoints: Breakpoints,
    pub transitions: TransitionTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ColorTokens {
    pub primary: ColorScale,
    pub secondary: ColorScale,
    pub success: ColorScale,
    pub warning: ColorScale,
    pub error: ColorScale,
    pub info: ColorScale,
    pub neutral: ColorScale,
    pub background: BackgroundColors,
    pub text: TextColors,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ColorScale {
    pub 50: String,
    pub 100: String,
    pub 200: String,
    pub 300: String,
    pub 400: String,
    pub 500: String,
    pub 600: String,
    pub 700: String,
    pub 800: String,
    pub 900: String,
    pub 950: String,
}
```

### 7.3 Dark Mode

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct DarkModeConfig {
    pub enabled: bool,
    pub strategy: DarkModeStrategy,
    pub selector: String,
    pub tokens: Option<DesignTokens>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DarkModeStrategy {
    #[default]
    Class,
    MediaQuery,
    DataAttribute,
    Manual,
}
```

---

## 8. Plugin Model

WASM plugin system configuration and metadata.

### 8.1 Plugin Config

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct PluginConfig {
    pub id: PluginId,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub manifest: PluginManifest,
    pub settings: IndexMap<String, serde_json::Value>,
    pub permissions: Vec<PluginPermission>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct PluginId(pub Uuid);
```

### 8.2 Plugin Manifest

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: String,
    pub license: String,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub keywords: Vec<String>,
    pub entry: String,
    pub engine: PluginEngine,
    pub permissions: Vec<PluginPermission>,
    pub extension_points: Vec<ExtensionPoint>,
    pub dependencies: IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct PluginEngine {
    pub wasm: WasmEngine,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct WasmEngine {
    pub version: String,
    pub features: Vec<String>,
}
```

### 8.3 Plugin Permissions & Extension Points

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermission {
    Database,
    FileSystem,
    Network,
    Storage,
    Email,
    Auth,
    Api,
    Workflow,
    Scheduler,
    Clipboard,
    Notification,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionPointType {
    Component,
    Action,
    DataSource,
    FieldType,
    WorkflowNode,
    AuthProvider,
    StorageDriver,
    EmailDriver,
    Hook,
    Middleware,
    Command,
    Page,
    Layout,
    Util,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ExtensionPoint {
    pub r#type: ExtensionPointType,
    pub name: String,
    pub config: IndexMap<String, serde_json::Value>,
}
```

---

## 9. Project Settings Model

Comprehensive configuration for generated applications.

### 9.1 Project Settings

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ProjectSettings {
    pub general: GeneralSettings,
    pub database: DatabaseSettings,
    pub authentication: AuthenticationSettings,
    pub email: EmailSettings,
    pub api: ApiSettings,
    pub storage: StorageSettings,
    pub features: FeatureFlags,
    pub notifications: NotificationSettings,
}
```

### 9.2 Database Settings

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct DatabaseSettings {
    pub driver: DatabaseDriver,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub url: Option<String>,
    pub ssl: bool,
    pub ssl_mode: SslMode,
    pub pool_min: u32,
    pub pool_max: u32,
    pub auto_migrate: bool,
    pub migrations_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseDriver {
    #[default]
    PostgreSQL,
    MySQL,
    SQLite,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SslMode {
    #[default]
    Prefer,
    Require,
    Disable,
    VerifyCa,
    VerifyFull,
}
```

### 9.3 Authentication Settings

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct AuthenticationSettings {
    pub enabled: bool,
    pub jwt_secret: Option<String>,
    pub jwt_expiration: u32,
    pub refresh_expiration: u32,
    pub password_policy: PasswordPolicy,
    pub mfa: MfaSettings,
    pub social: SocialAuthSettings,
    pub sso: SsoConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct PasswordPolicy {
    pub min_length: u32,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_numbers: bool,
    pub require_symbols: bool,
    pub history: u32,
    pub lockout_after: u32,
    pub lockout_duration: u32,
}
```

### 9.4 Feature Flags

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FeatureFlags {
    pub auth: bool,
    pub api: bool,
    pub workflows: bool,
    pub realtime: bool,
    pub files: bool,
    pub email: bool,
    pub webhooks: bool,
    pub scheduler: bool,
    pub audit: bool,
    pub soft_delete: bool,
    pub multi_tenant: bool,
    pub localization: bool,
    pub theming: bool,
    pub plugins: bool,
    pub marketplace: bool,
    pub ai: bool,
}
```

---

## 10. Workflow Runtime Models

Runtime state models for workflow execution (for bpmn-server integration).

### 10.1 Workflow Instance

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct WorkflowInstance {
    pub id: InstanceId,
    pub workflow_id: WorkflowId,
    pub status: InstanceStatus,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub variables: IndexMap<String, serde_json::Value>,
    pub current_node: Option<NodeId>,
    pub parent_instance: Option<InstanceId>,
    pub correlation_id: Option<String>,
    pub business_key: Option<String>,
    pub created_by: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct InstanceId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    Active,
    Completed,
    Suspended,
    Terminated,
    Failed,
    Cancelled,
    Error,
}
```

### 10.2 Task Instance

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TaskInstance {
    pub id: TaskId,
    pub instance_id: InstanceId,
    pub node_id: NodeId,
    pub name: String,
    pub status: TaskStatus,
    pub assignee: Option<String>,
    pub candidate_groups: Vec<String>,
    pub candidate_users: Vec<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub priority: Option<String>,
    pub form_id: Option<FormId>,
    pub created_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct TaskId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Created,
    Ready,
    Reserved,
    InProgress,
    Suspended,
    Completed,
    Failed,
    Error,
    Exited,
    Obsolete,
}
```

---

## 11. Generated Application Models

Models used in generated applications (auto-generated from Entity definitions).

### 11.1 User Model (Auth)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub username: Option<String>,
    pub password_hash: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub avatar_url: Option<String>,
    pub is_active: bool,
    pub is_verified: bool,
    pub last_login: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
```

### 11.2 Session Model

```rust
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token: String,
    pub refresh_token: String,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}
```

### 11.3 Audit Log Model

```rust
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AuditLog {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub action: String,
    pub resource: String,
    pub resource_id: Option<String>,
    pub old_values: Option<serde_json::Value>,
    pub new_values: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
}
```

---

## 12. Type Aliases & Common Types

```rust
// Common type aliases used throughout
pub type JsonValue = serde_json::Value;
pub type JsonObject = serde_json::Map<String, JsonValue>;
pub type JsonArray = Vec<JsonValue>;

// Duration & Time
pub type DurationMs = u64;

// Pagination
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct PaginationParams {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
    pub has_next: bool,
    pub has_prev: bool,
}
```

---

## 13. Model Relationships Summary

| Model | Relations | Description |
|-------|-----------|-------------|
| **Project** | Has many Forms, Entities, Workflows, Pages, Themes | Root aggregate |
| **Form** | Belongs to Project, Has many Fields, Can trigger Workflows | Dynamic form definitions |
| **Entity** | Belongs to Project, Has many Fields & Relationships | Database schema definitions |
| **Workflow** | Belongs to Project, Has many Nodes & Edges | BPMN process definitions |
| **Page** | Belongs to Project, Has many Components | UI page compositions |
| **Theme** | Belongs to Project | Design system configuration |
| **WorkflowInstance** | Belongs to Workflow, Has many TaskInstances & Variables | Runtime execution state |
| **PluginConfig** | Belongs to Project | Plugin configuration |

---

## 14. Web IDE Server Models (Multi-User)

These models live in the IDE server database (`ide-server`, SeaORM). They model users, project persistence, and refresh tokens for the web IDE.

### 14.1 User

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String, // Argon2id
    pub is_active: bool,
    pub is_verified: bool,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### 14.2 IDE Project

Projects stored server-side for web IDE; mirrors on-disk project metadata (content is the serialized `Project` JSON).

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct IdeProject {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub version: String,
    pub data: serde_json::Value, // Serialized Project (JSON)
    pub revision: i64, // Optimistic concurrency
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### 14.3 Refresh Token

Refresh tokens for JWT rotation (stored as hashes in production; jti used for revocation).

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub jti: Uuid,
    pub token_hash: String,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
```

### 14.4 API Sessions (Optional)

Optional server-side session view; tokens issued as HttpOnly cookies.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub refresh_token_id: Uuid,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub last_seen_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
```

### 14.5 Web IDE Relationships

| Model | Relations | Description |
|-------|-----------|-------------|
| **User** | Has many IdeProject, Has many RefreshToken | Account owner |
| **IdeProject** | Belongs to User | Per-user project isolation |
| **RefreshToken** | Belongs to User | JWT refresh rotation + revocation |

---

**Document Control**

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-10-15 | Data Architect | Initial Draft - Complete data models for No-Code IDE platform |
| 1.1 | 2026-10-16 | Engineering | Add Web IDE server models (User, IdeProject, RefreshToken, Session) |