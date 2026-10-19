use crate::entity::EntityId;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Form {
    pub id: FormId,
    pub name: String,
    pub description: Option<String>,
    pub fields: Vec<FormField>,
    pub layout: FormLayout,
    pub validation: FormValidation,
    pub permissions: FormPermissions,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Form {
    /// Create a new empty form with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: FormId::new(),
            name: name.into(),
            description: None,
            fields: Vec::new(),
            layout: FormLayout::default(),
            validation: FormValidation::default(),
            permissions: FormPermissions::default(),
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct FormId(pub Uuid);

impl FormId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for FormId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for FormId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct FormField {
    pub id: FieldId,
    pub name: String,
    pub label: String,
    pub field_type: FieldType,
    pub required: bool,
    pub default_value: Option<serde_json::Value>,
    pub validation_rules: Vec<ValidationRule>,
    pub conditional_visibility: Option<ConditionalVisibility>,
    pub ui_props: FieldUiProps,
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
pub enum FieldType {
    Text {
        max_length: Option<usize>,
        placeholder: Option<String>,
    },
    TextArea {
        max_length: Option<usize>,
        rows: Option<u8>,
    },
    Number {
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
    },
    Integer {
        min: Option<i64>,
        max: Option<i64>,
    },
    Boolean,
    Select {
        options: Vec<SelectOption>,
        multiple: bool,
        searchable: bool,
    },
    Radio {
        options: Vec<SelectOption>,
    },
    Checkbox {
        options: Vec<SelectOption>,
    },
    Date,
    DateTime,
    Time,
    Email,
    Url,
    Phone,
    File {
        accept: Option<String>,
        max_size_mb: Option<u64>,
        multiple: bool,
    },
    Image {
        max_width: Option<u32>,
        max_height: Option<u32>,
    },
    Signature,
    RichText,
    Json,
    Relation {
        entity_id: EntityId,
        display_field: String,
        multiple: bool,
    },
    Computed {
        expression: String,
        return_type: Box<FieldType>,
    },
    Section {
        fields: Vec<FormField>,
    },
    Repeater {
        fields: Vec<FormField>,
        min: usize,
        max: Option<usize>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SelectOption {
    pub value: serde_json::Value,
    pub label: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub enum ValidationRule {
    Required { message: String },
    MinLength { min: usize, message: String },
    MaxLength { max: usize, message: String },
    Pattern { regex: String, message: String },
    Min { value: f64, message: String },
    Max { value: f64, message: String },
    Email { message: String },
    Url { message: String },
    Custom { expression: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ConditionalVisibility {
    pub field_id: FieldId,
    pub operator: VisibilityOperator,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VisibilityOperator {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    GreaterThan,
    LessThan,
    In,
    NotIn,
    IsEmpty,
    IsNotEmpty,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FieldUiProps {
    pub width: Option<String>,
    pub label_position: LabelPosition,
    pub help_text: Option<String>,
    pub placeholder: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub readonly: bool,
    pub disabled: bool,
    pub hidden: bool,
    pub css_classes: Vec<String>,
    pub custom_attrs: IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LabelPosition {
    #[default]
    Top,
    Left,
    Right,
    Inline,
    Floating,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FormLayout {
    pub columns: u8,
    pub column_gap: String,
    pub row_gap: String,
    pub fields: IndexMap<FieldId, FieldLayout>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct FieldLayout {
    pub column_start: u8,
    pub column_span: u8,
    pub row_start: Option<u16>,
    pub row_span: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FormValidation {
    pub validate_on_change: bool,
    pub validate_on_blur: bool,
    pub validate_on_submit: bool,
    pub custom_validators: Vec<CustomValidator>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct CustomValidator {
    pub name: String,
    pub expression: String,
    pub message: String,
    pub fields: Vec<FieldId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FormPermissions {
    pub create_roles: Vec<String>,
    pub read_roles: Vec<String>,
    pub update_roles: Vec<String>,
    pub delete_roles: Vec<String>,
    pub field_permissions: IndexMap<FieldId, FieldPermissions>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FieldPermissions {
    pub read_roles: Vec<String>,
    pub write_roles: Vec<String>,
}
