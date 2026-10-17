//! View/DTO layer for code generation.
//!
//! The Handlebars templates consume a *denormalized* view of the model: they
//! expect convenience strings such as `rust_type`, `sql_type`, `ts_type`,
//! `zod_schema`, `is_primary_key`, etc. The raw model (`EntityField`) only
//! stores the `field_type` enum plus flags, so the generators must project the
//! model into these views before rendering.
//!
//! Rendering with views (instead of the raw model) also removes the whole class
//! of `@root.<field>` scoping bugs: inside `{{#each entity.fields}}` the loop
//! item *is* the field view, so templates can reference `{{name}}` directly.

use nocode_model::{
    AutoGenerate, DatabaseDriver, Entity, EntityField, EntityFieldType, EntityRelation, FieldType,
    Form, FormField, Page, Project, Workflow,
};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Case helpers
// ---------------------------------------------------------------------------

fn words(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for ch in input.chars() {
        if ch.is_alphanumeric() {
            if ch.is_uppercase() && prev_lower && !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
            current.push(ch);
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
            prev_lower = false;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn capitalize(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

pub fn snake(input: &str) -> String {
    words(input)
        .iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

pub fn pascal(input: &str) -> String {
    words(input).iter().map(|w| capitalize(w)).collect::<String>()
}

pub fn camel(input: &str) -> String {
    let p = pascal(input);
    let mut c = p.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => p,
    }
}

pub fn label(input: &str) -> String {
    words(input)
        .iter()
        .map(|w| capitalize(w))
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Type mapping
// ---------------------------------------------------------------------------

fn sql_type(ft: &EntityFieldType, driver: &DatabaseDriver) -> String {
    match ft {
        EntityFieldType::Uuid => match driver {
            DatabaseDriver::Postgres => "UUID".to_string(),
            DatabaseDriver::MySQL => "CHAR(36)".to_string(),
            _ => "TEXT".to_string(),
        },
        EntityFieldType::String { max_length } => match driver {
            DatabaseDriver::SQLite => "TEXT".to_string(),
            _ => format!("VARCHAR({})", max_length.unwrap_or(255)),
        },
        EntityFieldType::Text => "TEXT".to_string(),
        EntityFieldType::Integer => match driver {
            DatabaseDriver::MySQL => "INT".to_string(),
            _ => "INTEGER".to_string(),
        },
        EntityFieldType::BigInteger => "BIGINT".to_string(),
        EntityFieldType::Float => match driver {
            DatabaseDriver::Postgres => "DOUBLE PRECISION".to_string(),
            DatabaseDriver::SQLite => "REAL".to_string(),
            _ => "DOUBLE".to_string(),
        },
        EntityFieldType::Decimal { precision, scale } => format!("DECIMAL({}, {})", precision, scale),
        EntityFieldType::Boolean => match driver {
            DatabaseDriver::SQLite => "INTEGER".to_string(),
            DatabaseDriver::MySQL => "TINYINT(1)".to_string(),
            _ => "BOOLEAN".to_string(),
        },
        EntityFieldType::Date => "DATE".to_string(),
        EntityFieldType::DateTime | EntityFieldType::Timestamp => match driver {
            DatabaseDriver::Postgres => "TIMESTAMPTZ".to_string(),
            DatabaseDriver::SQLite => "TEXT".to_string(),
            _ => "DATETIME".to_string(),
        },
        EntityFieldType::Time => "TIME".to_string(),
        EntityFieldType::Json | EntityFieldType::Jsonb => match driver {
            DatabaseDriver::Postgres => "JSONB".to_string(),
            DatabaseDriver::MySQL => "JSON".to_string(),
            _ => "TEXT".to_string(),
        },
        EntityFieldType::Enum { .. } => "VARCHAR(255)".to_string(),
        EntityFieldType::Bytes => match driver {
            DatabaseDriver::Postgres => "BYTEA".to_string(),
            _ => "BLOB".to_string(),
        },
        EntityFieldType::Relation(_) => match driver {
            DatabaseDriver::Postgres => "UUID".to_string(),
            DatabaseDriver::MySQL => "CHAR(36)".to_string(),
            _ => "TEXT".to_string(),
        },
    }
}

fn rust_type(ft: &EntityFieldType) -> &'static str {
    match ft {
        EntityFieldType::Uuid | EntityFieldType::Relation(_) => "Uuid",
        EntityFieldType::String { .. }
        | EntityFieldType::Text
        | EntityFieldType::Enum { .. } => "String",
        EntityFieldType::Integer => "i32",
        EntityFieldType::BigInteger => "i64",
        EntityFieldType::Float | EntityFieldType::Decimal { .. } => "f64",
        EntityFieldType::Boolean => "bool",
        EntityFieldType::Date => "NaiveDate",
        EntityFieldType::DateTime | EntityFieldType::Timestamp => "DateTime<Utc>",
        EntityFieldType::Time => "NaiveTime",
        EntityFieldType::Json | EntityFieldType::Jsonb => "Json",
        EntityFieldType::Bytes => "Vec<u8>",
    }
}

fn ts_type(ft: &EntityFieldType) -> &'static str {
    match ft {
        EntityFieldType::Uuid
        | EntityFieldType::String { .. }
        | EntityFieldType::Text
        | EntityFieldType::Enum { .. }
        | EntityFieldType::Relation(_)
        | EntityFieldType::Date
        | EntityFieldType::DateTime
        | EntityFieldType::Timestamp
        | EntityFieldType::Time
        | EntityFieldType::Bytes => "string",
        EntityFieldType::Integer
        | EntityFieldType::BigInteger
        | EntityFieldType::Float
        | EntityFieldType::Decimal { .. } => "number",
        EntityFieldType::Boolean => "boolean",
        EntityFieldType::Json | EntityFieldType::Jsonb => "Record<string, unknown>",
    }
}

/// Rust type used in the generated Leptos (wasm) DTOs. Kept `wasm`-friendly:
/// UUIDs/dates are plain strings, JSON is `serde_json::Value`.
fn wasm_type(ft: &EntityFieldType) -> &'static str {
    match ft {
        EntityFieldType::Integer => "i32",
        EntityFieldType::BigInteger => "i64",
        EntityFieldType::Float | EntityFieldType::Decimal { .. } => "f64",
        EntityFieldType::Boolean => "bool",
        EntityFieldType::Json | EntityFieldType::Jsonb => "serde_json::Value",
        _ => "String",
    }
}

/// Name of a `crate::convert` helper that turns a form string into this type.
fn wasm_ctor(ft: &EntityFieldType, optional: bool) -> &'static str {
    let required = match ft {
        EntityFieldType::Integer => "parse_i32",
        EntityFieldType::BigInteger => "parse_i64",
        EntityFieldType::Float | EntityFieldType::Decimal { .. } => "parse_f64",
        EntityFieldType::Boolean => "parse_bool",
        EntityFieldType::Json | EntityFieldType::Jsonb => "parse_json",
        _ => "parse_string",
    };
    if optional {
        match ft {
            EntityFieldType::Integer => "parse_i32_opt",
            EntityFieldType::BigInteger => "parse_i64_opt",
            EntityFieldType::Float | EntityFieldType::Decimal { .. } => "parse_f64_opt",
            EntityFieldType::Boolean => "parse_bool_opt",
            EntityFieldType::Json | EntityFieldType::Jsonb => "parse_json_opt",
            _ => "parse_string_opt",
        }
    } else {
        required
    }
}

fn zod_schema(ft: &EntityFieldType) -> String {
    match ft {
        EntityFieldType::Uuid | EntityFieldType::Relation(_) => "z.string().uuid()".to_string(),
        EntityFieldType::String { max_length } => match max_length {
            Some(n) => format!("z.string().max({})", n),
            None => "z.string()".to_string(),
        },
        EntityFieldType::Text => "z.string()".to_string(),
        EntityFieldType::Integer | EntityFieldType::BigInteger => "z.number().int()".to_string(),
        EntityFieldType::Float | EntityFieldType::Decimal { .. } => "z.number()".to_string(),
        EntityFieldType::Boolean => "z.boolean()".to_string(),
        EntityFieldType::Date
        | EntityFieldType::DateTime
        | EntityFieldType::Timestamp
        | EntityFieldType::Time
        | EntityFieldType::Bytes => "z.string()".to_string(),
        EntityFieldType::Json | EntityFieldType::Jsonb => "z.any()".to_string(),
        EntityFieldType::Enum { values } => format!(
            "z.enum([{}])",
            values
                .iter()
                .map(|v| format!("\"{}\"", v))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn input_type(ft: &EntityFieldType) -> &'static str {
    match ft {
        EntityFieldType::Uuid => "text",
        EntityFieldType::String { .. } => "text",
        EntityFieldType::Text => "textarea",
        EntityFieldType::Integer
        | EntityFieldType::BigInteger
        | EntityFieldType::Float
        | EntityFieldType::Decimal { .. } => "number",
        EntityFieldType::Boolean => "checkbox",
        EntityFieldType::Date => "date",
        EntityFieldType::DateTime | EntityFieldType::Timestamp => "datetime-local",
        EntityFieldType::Time => "time",
        EntityFieldType::Json | EntityFieldType::Jsonb => "textarea",
        EntityFieldType::Enum { .. } => "select",
        EntityFieldType::Bytes => "file",
        EntityFieldType::Relation(_) => "select",
    }
}

fn field_type_name(ft: &EntityFieldType) -> &'static str {
    match ft {
        EntityFieldType::String { .. } => "string",
        EntityFieldType::Text => "text",
        EntityFieldType::Integer => "integer",
        EntityFieldType::BigInteger => "big_integer",
        EntityFieldType::Float => "float",
        EntityFieldType::Decimal { .. } => "decimal",
        EntityFieldType::Boolean => "boolean",
        EntityFieldType::Date => "date",
        EntityFieldType::DateTime => "date_time",
        EntityFieldType::Time => "time",
        EntityFieldType::Timestamp => "timestamp",
        EntityFieldType::Json => "json",
        EntityFieldType::Jsonb => "jsonb",
        EntityFieldType::Uuid => "uuid",
        EntityFieldType::Enum { .. } => "enum",
        EntityFieldType::Bytes => "bytes",
        EntityFieldType::Relation(_) => "relation",
    }
}

fn auto_name(a: &AutoGenerate) -> String {
    match a {
        AutoGenerate::Uuid => "uuid",
        AutoGenerate::UuidV7 => "uuid_v7",
        AutoGenerate::Nanoid => "nanoid",
        AutoGenerate::Increment => "increment",
        AutoGenerate::Timestamp => "timestamp",
        AutoGenerate::Custom(s) => s.as_str(),
    }
    .to_string()
}

fn sql_literal(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::Bool(b) => Some(if *b { "true" } else { "false" }.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(format!("'{}'", s.replace('\'', "''"))),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Field / entity views
// ---------------------------------------------------------------------------

const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "async", "await", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "try", "typeof", "unsized", "virtual", "yield", "union",
];

/// Produce a valid Rust identifier, escaping reserved keywords with `r#`.
fn rust_ident(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{}", name)
    } else {
        name.to_string()
    }
}

pub fn field_view(field: &EntityField, index: usize, driver: &DatabaseDriver) -> Value {
    let ft = &field.field_type;
    let enum_values: Vec<String> = match ft {
        EntityFieldType::Enum { values } => values.clone(),
        _ => Vec::new(),
    };
    let max_length = match ft {
        EntityFieldType::String { max_length } => max_length.unwrap_or(255),
        _ => 0,
    };
    let is_primary_key = index == 0;
    let base = rust_type(ft);
    let is_optional = field.nullable && !is_primary_key;
    let rust_field_type = if is_optional {
        format!("Option<{}>", base)
    } else {
        base.to_string()
    };
    let column = snake(&field.name);
    let rust_ident = rust_ident(&column);
    let wasm = wasm_type(ft);
    let wasm_field_type = if is_optional {
        format!("Option<{}>", wasm)
    } else {
        wasm.to_string()
    };
    let wasm_ctor = wasm_ctor(ft, is_optional);
    json!({
        "name": field.name,
        "column": column,
        "snake": column,
        "pascal": pascal(&field.name),
        "camel": camel(&field.name),
        "label": label(&field.name),
        "rust_ident": rust_ident,
        "rust_field_name": rust_ident,
        "field_type": field_type_name(ft),
        "input_type": input_type(ft),
        "rust_type": rust_type(ft),
        "rust_type_dto": rust_type(ft),
        "rust_field_type": rust_field_type,
        "rust_struct_field": format!("pub {}: {}", rust_ident, rust_field_type),
        "wasm_type": wasm,
        "wasm_field_type": wasm_field_type,
        "wasm_ctor": wasm_ctor,
        "is_optional": is_optional,
        "is_uuid": base == "Uuid",
        "is_integer": matches!(ft, EntityFieldType::Integer | EntityFieldType::BigInteger),
        "is_string": matches!(
            ft,
            EntityFieldType::String { .. } | EntityFieldType::Text | EntityFieldType::Enum { .. }
        ),
        "is_bool": matches!(ft, EntityFieldType::Boolean),
        "sql_type": sql_type(ft, driver),
        "ts_type": ts_type(ft),
        "zod_schema": zod_schema(ft),
        "is_primary_key": is_primary_key,
        "not_null": !field.nullable || is_primary_key,
        "is_unique": field.unique,
        "has_default": field.default.is_some(),
        "default_value": field.default.as_ref().and_then(sql_literal).unwrap_or_default(),
        "is_auto_generate": field.auto_generate.is_some(),
        "auto_generate": field.auto_generate.as_ref().map(auto_name).unwrap_or_default(),
        "is_relation": matches!(ft, EntityFieldType::Relation(_)),
        "enum_values": enum_values,
        "max_length": max_length,
        "comment": field.comment,
    })
}

fn relation_view(relation: &EntityRelation, owner: &Entity, project: &Project) -> Value {
    let from_field = owner
        .fields
        .iter()
        .find(|f| f.id == relation.from_field)
        .map(|f| f.name.clone())
        .unwrap_or_default();
    let to_entity = project.entities.get(&relation.to_entity);
    let to_entity_name = to_entity.map(|e| e.name.clone()).unwrap_or_default();
    let to_table = to_entity
        .map(|e| {
            e.options
                .table_name
                .clone()
                .unwrap_or_else(|| snake(&e.plural_name))
        })
        .unwrap_or_default();
    let to_field = to_entity
        .and_then(|e| e.fields.iter().find(|f| f.id == relation.to_field))
        .map(|f| f.name.clone())
        .unwrap_or_default();
    json!({
        "name": relation.name,
        "relation_name": relation.name,
        "from_field": from_field,
        "to_entity": { "name": to_entity_name },
        "to_table": to_table,
        "to_field": to_field,
        "on_delete": format!("{:?}", relation.on_delete),
        "on_update": format!("{:?}", relation.on_update),
    })
}

pub fn entity_view(entity: &Entity, project: &Project, driver: &DatabaseDriver) -> Value {
    let fields: Vec<Value> = entity
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| field_view(f, i, driver))
        .collect();
    let relations: Vec<Value> = entity
        .relations
        .iter()
        .map(|r| relation_view(r, entity, project))
        .collect();
    let indexes: Vec<Value> = entity
        .indexes
        .iter()
        .map(|idx| {
            let columns: Vec<String> = idx
                .fields
                .iter()
                .filter_map(|fid| entity.fields.iter().find(|f| &f.id == fid))
                .map(|f| f.name.clone())
                .collect();
            json!({
                "name": idx.name,
                "columns": columns.join(", "),
                "unique": idx.unique,
            })
        })
        .collect();
    let table = entity
        .options
        .table_name
        .clone()
        .unwrap_or_else(|| snake(&entity.plural_name));

    let pk = fields.first().cloned().unwrap_or_else(|| json!({}));
    let pk_rust_type = pk
        .get("rust_type")
        .and_then(|v| v.as_str())
        .unwrap_or("Uuid")
        .to_string();
    let pk_rust_ident = pk
        .get("rust_ident")
        .and_then(|v| v.as_str())
        .unwrap_or("id")
        .to_string();
    let pk_is_uuid = pk
        .get("is_uuid")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let pk_is_integer = pk
        .get("is_integer")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    json!({
        "name": entity.name,
        "plural_name": entity.plural_name,
        "table": table,
        "snake": snake(&entity.name),
        "pascal": pascal(&entity.name),
        "camel": camel(&entity.name),
        "lower": entity.name.to_lowercase(),
        "description": entity.description,
        "fields": fields,
        "relations": relations,
        "indexes": indexes,
        "pk": pk,
        "pk_rust_type": pk_rust_type,
        "pk_rust_ident": pk_rust_ident,
        "pk_is_uuid": pk_is_uuid,
        "pk_is_integer": pk_is_integer,
    })
}

pub fn entities_view(project: &Project, driver: &DatabaseDriver) -> Vec<Value> {
    project
        .entities
        .values()
        .map(|e| entity_view(e, project, driver))
        .collect()
}

// ---------------------------------------------------------------------------
// Form views
// ---------------------------------------------------------------------------

fn form_field_type_name(ft: &FieldType) -> &'static str {
    match ft {
        FieldType::Text { .. } => "text",
        FieldType::TextArea { .. } => "textarea",
        FieldType::Number { .. } => "number",
        FieldType::Integer { .. } => "integer",
        FieldType::Boolean => "boolean",
        FieldType::Select { .. } => "select",
        FieldType::Radio { .. } => "radio",
        FieldType::Checkbox { .. } => "checkbox",
        FieldType::Date => "date",
        FieldType::DateTime => "datetime",
        FieldType::Time => "time",
        FieldType::Email => "email",
        FieldType::Url => "url",
        FieldType::Phone => "phone",
        FieldType::File { .. } => "file",
        FieldType::Image { .. } => "image",
        FieldType::Signature => "signature",
        FieldType::RichText => "rich_text",
        FieldType::Json => "json",
        FieldType::Relation { .. } => "relation",
        FieldType::Computed { .. } => "computed",
        FieldType::Section { .. } => "section",
        FieldType::Repeater { .. } => "repeater",
    }
}

fn form_input_type(ft: &FieldType) -> &'static str {
    match ft {
        FieldType::Text { .. } => "text",
        FieldType::TextArea { .. } => "textarea",
        FieldType::Number { .. } => "number",
        FieldType::Integer { .. } => "number",
        FieldType::Boolean => "checkbox",
        FieldType::Select { .. } => "select",
        FieldType::Radio { .. } => "radio",
        FieldType::Checkbox { .. } => "checkbox",
        FieldType::Date => "date",
        FieldType::DateTime => "datetime-local",
        FieldType::Time => "time",
        FieldType::Email => "email",
        FieldType::Url => "url",
        FieldType::Phone => "tel",
        FieldType::File { .. } => "file",
        FieldType::Image { .. } => "file",
        FieldType::Json => "textarea",
        _ => "text",
    }
}

fn form_ts_type(ft: &FieldType) -> &'static str {
    match ft {
        FieldType::Number { .. } | FieldType::Integer { .. } => "number",
        FieldType::Boolean => "boolean",
        FieldType::Select { .. } | FieldType::Radio { .. } | FieldType::Checkbox { .. } => "string[]",
        FieldType::Date | FieldType::DateTime | FieldType::Time => "string",
        FieldType::Text { .. }
        | FieldType::TextArea { .. }
        | FieldType::Email
        | FieldType::Url
        | FieldType::Phone
        | FieldType::Signature
        | FieldType::RichText
        | FieldType::File { .. }
        | FieldType::Image { .. }
        | FieldType::Relation { .. } => "string",
        FieldType::Json => "Record<string, unknown>",
        _ => "string",
    }
}

fn select_options(ft: &FieldType) -> Vec<Value> {
    let opts = match ft {
        FieldType::Select { options, .. } => options,
        FieldType::Radio { options } => options,
        FieldType::Checkbox { options } => options,
        _ => return Vec::new(),
    };
    opts.iter()
        .map(|o| {
            json!({
                "value": o.value.as_str().map(|s| s.to_string()).unwrap_or_else(|| o.value.to_string()),
                "label": o.label,
            })
        })
        .collect()
}

pub fn form_field_view(field: &FormField, index: usize) -> Value {
    json!({
        "name": field.name,
        "snake": snake(&field.name),
        "pascal": pascal(&field.name),
        "camel": camel(&field.name),
        "label": if field.label.is_empty() { label(&field.name) } else { field.label.clone() },
        "field_type": form_field_type_name(&field.field_type),
        "input_type": form_input_type(&field.field_type),
        "ts_type": form_ts_type(&field.field_type),
        "required": field.required,
        "is_primary_key": index == 0,
        "not_null": field.required,
        "has_default": field.default_value.is_some(),
        "default_value": field.default_value.clone().unwrap_or(Value::Null),
        "placeholder": field.ui_props.placeholder.clone().unwrap_or_default(),
        "options": select_options(&field.field_type),
        "is_relation": matches!(field.field_type, FieldType::Relation { .. }),
    })
}

pub fn form_view(form: &Form) -> Value {
    let fields: Vec<Value> = form
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| form_field_view(f, i))
        .collect();
    json!({
        "name": form.name,
        "pascal": pascal(&form.name),
        "camel": camel(&form.name),
        "snake": snake(&form.name),
        "lower": form.name.to_lowercase(),
        "description": form.description,
        "fields": fields,
    })
}

pub fn forms_view(project: &Project) -> Vec<Value> {
    project.forms.values().map(form_view).collect()
}

// ---------------------------------------------------------------------------
// Page / workflow views
// ---------------------------------------------------------------------------

pub fn page_view(page: &Page) -> Value {
    json!({
        "name": page.name,
        "pascal": pascal(&page.name),
        "camel": camel(&page.name),
        "snake": snake(&page.name),
        "lower": page.name.to_lowercase(),
        "path": page.path,
        "title": if page.title.is_empty() { page.name.clone() } else { page.title.clone() },
        "description": page.description,
        "components": page.components,
        "data_sources": page.data_sources,
    })
}

pub fn pages_view(project: &Project) -> Vec<Value> {
    project.pages.values().map(page_view).collect()
}

pub fn workflow_view(workflow: &Workflow) -> Value {
    json!({
        "name": workflow.name,
        "pascal": pascal(&workflow.name),
        "camel": camel(&workflow.name),
        "snake": snake(&workflow.name),
        "lower": workflow.name.to_lowercase(),
        "description": null,
    })
}

pub fn workflows_view(project: &Project) -> Vec<Value> {
    project.workflows.values().map(workflow_view).collect()
}

// ---------------------------------------------------------------------------
// Project view
// ---------------------------------------------------------------------------

pub fn project_view(project: &Project, _driver: &DatabaseDriver) -> Value {
    // Escape the database URL so it can be embedded directly inside a Rust
    // string literal (Windows paths contain backslashes).
    let database_url_literal = project
        .settings
        .database
        .url
        .replace('\\', "\\\\")
        .replace('"', "\\\"");

    json!({
        "id": project.id.to_string(),
        "name": project.name,
        "slug": project.slug,
        "version": project.version,
        "description": project.description,
        "database": project.settings.database,
        "database_url_literal": database_url_literal,
        "frontend": project.settings.frontend,
        "settings": project.settings,
    })
}
