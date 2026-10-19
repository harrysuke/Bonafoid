use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    pub plural_name: String,
    pub description: Option<String>,
    pub fields: Vec<EntityField>,
    pub relations: Vec<EntityRelation>,
    pub indexes: Vec<EntityIndex>,
    pub constraints: Vec<EntityConstraint>,
    pub options: EntityOptions,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Entity {
    /// Create a new entity with a generated plural name ("{name}s").
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        let name = name.into();
        let plural_name = format!("{name}s");
        Self {
            id: EntityId::new(),
            name,
            plural_name,
            description: None,
            fields: Vec::new(),
            relations: Vec::new(),
            indexes: Vec::new(),
            constraints: Vec::new(),
            options: EntityOptions::default(),
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct EntityId(pub Uuid);

impl EntityId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for EntityId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EntityField {
    pub id: FieldId,
    pub name: String,
    pub field_type: EntityFieldType,
    pub nullable: bool,
    pub unique: bool,
    pub default: Option<serde_json::Value>,
    pub auto_generate: Option<AutoGenerate>,
    pub validation: Option<String>,
    pub comment: Option<String>,
    pub order: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct FieldId(pub Uuid);

impl FieldId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for FieldId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum EntityFieldType {
    String { max_length: Option<u32> },
    Text,
    Integer,
    BigInteger,
    Float,
    Decimal { precision: u8, scale: u8 },
    Boolean,
    Date,
    DateTime,
    Time,
    Timestamp,
    Json,
    Jsonb,
    Uuid,
    Enum { values: Vec<String> },
    Bytes,
    Relation(RelationType),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum RelationType {
    BelongsTo {
        entity_id: EntityId,
        field: String,
        on_delete: OnDelete,
        on_update: OnUpdate,
    },
    HasOne {
        entity_id: EntityId,
        field: String,
    },
    HasMany {
        entity_id: EntityId,
        field: String,
    },
    ManyToMany {
        entity_id: EntityId,
        through: Option<String>,
        field: String,
        related_field: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OnDelete {
    Cascade,
    SetNull,
    Restrict,
    NoAction,
    SetDefault,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OnUpdate {
    Cascade,
    SetNull,
    Restrict,
    NoAction,
    SetDefault,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AutoGenerate {
    Uuid,
    UuidV7,
    Nanoid,
    Increment,
    Timestamp,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EntityRelation {
    pub id: RelationId,
    pub name: String,
    pub relation_type: RelationType,
    pub from_field: FieldId,
    pub to_entity: EntityId,
    pub to_field: FieldId,
    pub on_delete: OnDelete,
    pub on_update: OnUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct RelationId(pub Uuid);

impl RelationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for RelationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EntityIndex {
    pub id: IndexId,
    pub name: String,
    pub fields: Vec<FieldId>,
    pub unique: bool,
    pub index_type: IndexType,
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct IndexId(pub Uuid);

impl IndexId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for IndexId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IndexType {
    BTree,
    Hash,
    Gin,
    Gist,
    SpGist,
    Brin,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EntityConstraint {
    pub id: ConstraintId,
    pub name: String,
    pub constraint_type: ConstraintType,
    pub fields: Vec<FieldId>,
    pub expression: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct ConstraintId(pub Uuid);

impl ConstraintId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ConstraintId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintType {
    PrimaryKey,
    Unique,
    Check,
    Exclusion,
    ForeignKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct EntityOptions {
    pub table_name: Option<String>,
    pub schema: Option<String>,
    pub comment: Option<String>,
    pub enable_rls: bool,
    pub rls_policies: Vec<RlsPolicy>,
    pub audit_log: bool,
    pub soft_delete: bool,
    pub timestamps: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct RlsPolicy {
    pub name: String,
    pub command: RlsCommand,
    pub roles: Vec<String>,
    pub using_expression: String,
    pub with_check_expression: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RlsCommand {
    All,
    Select,
    Insert,
    Update,
    Delete,
}
