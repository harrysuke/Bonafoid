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
    AutoGenerate, ColorScale, ComponentType, DataSource, DataSourceType, DatabaseDriver, Entity,
    EntityField, EntityFieldType, EntityRelation, FieldType, Form, FormField, Page, PageAction,
    PageComponent, Project, RelationType, Workflow,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

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
    words(input)
        .iter()
        .map(|w| capitalize(w))
        .collect::<String>()
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
        EntityFieldType::Decimal { precision, scale } => {
            format!("DECIMAL({}, {})", precision, scale)
        }
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
        EntityFieldType::String { .. } | EntityFieldType::Text | EntityFieldType::Enum { .. } => {
            "String"
        }
        EntityFieldType::Integer => "i32",
        EntityFieldType::BigInteger => "i64",
        EntityFieldType::Float | EntityFieldType::Decimal { .. } => "f64",
        EntityFieldType::Boolean => "bool",
        EntityFieldType::Date => "NaiveDate",
        EntityFieldType::DateTime | EntityFieldType::Timestamp => "DateTime<Utc>",
        EntityFieldType::Time => "NaiveTime",
        EntityFieldType::Json | EntityFieldType::Jsonb => "serde_json::Value",
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
        // A `<input type="datetime-local">` yields "YYYY-MM-DDTHH:MM" with
        // no zone, but the backend field is a `DateTime<Utc>`, which
        // needs an offset to deserialize.
        EntityFieldType::DateTime | EntityFieldType::Timestamp => "parse_datetime",
        _ => "parse_string",
    };
    if optional {
        match ft {
            EntityFieldType::Integer => "parse_i32_opt",
            EntityFieldType::BigInteger => "parse_i64_opt",
            EntityFieldType::Float | EntityFieldType::Decimal { .. } => "parse_f64_opt",
            EntityFieldType::Boolean => "parse_bool_opt",
            EntityFieldType::Json | EntityFieldType::Jsonb => "parse_json_opt",
            EntityFieldType::DateTime | EntityFieldType::Timestamp => "parse_datetime_opt",
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

/// The field a related entity should be labelled with in a picker.
///
/// A relation's select shows the target entity's records, so each record
/// needs a human-readable label. Prefer a field called "name", then the
/// first short-text field, then the first non-key field.
fn relation_label_field(target: &Entity) -> String {
    let text_field = target.fields.iter().find(|f| {
        matches!(
            f.field_type,
            EntityFieldType::String { .. } | EntityFieldType::Text | EntityFieldType::Enum { .. }
        )
    });
    let named = target
        .fields
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case("name"));
    let chosen = named.or(text_field).or_else(|| target.fields.get(1));
    match chosen {
        Some(field) => rust_ident(&snake(&field.name)),
        None => String::new(),
    }
}

/// The key field of a related entity, used as the value of a picker.
fn relation_key_field(target: &Entity) -> String {
    match target.fields.first() {
        Some(field) => rust_ident(&snake(&field.name)),
        None => String::new(),
    }
}

pub fn field_view(
    field: &EntityField,
    index: usize,
    driver: &DatabaseDriver,
    project: &Project,
) -> Value {
    let ft = &field.field_type;
    let enum_values: Vec<String> = match ft {
        EntityFieldType::Enum { values } => values.clone(),
        _ => Vec::new(),
    };
    // Enum fields get the same `{value, label}` shape the form fields use,
    // so a template can render a select the same way for both.
    let options: Vec<Value> = enum_values
        .iter()
        .map(|value| json!({ "value": value, "label": value }))
        .collect();
    // A relation field knows which entity it points at, so the generated
    // page can offer the target's records instead of a raw id.
    let (relation_entity_snake, relation_entity_pascal, relation_label_field, relation_key_field) =
        match ft {
            EntityFieldType::Relation(nocode_model::entity::RelationType::BelongsTo {
                entity_id,
                ..
            }) => match project.entities.get(entity_id) {
                Some(target) => (
                    snake(&target.name),
                    pascal(&target.name),
                    relation_label_field(target),
                    relation_key_field(target),
                ),
                None => (String::new(), String::new(), String::new(), String::new()),
            },
            _ => (String::new(), String::new(), String::new(), String::new()),
        };
    let is_relation = !relation_entity_snake.is_empty();
    // A binary field has no sensible widget: the browser gives a file
    // input a path, not the bytes, and the backend field is `Vec<u8>`.
    // Rendering one would produce a request that cannot deserialize, so
    // the generated form leaves it out and says so.
    let can_edit = !matches!(ft, EntityFieldType::Bytes);
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
    // A date or time field needs chrono's types. The generated model only
    // imports them when it has to.
    let needs_chrono = matches!(
        ft,
        EntityFieldType::Date
            | EntityFieldType::DateTime
            | EntityFieldType::Timestamp
            | EntityFieldType::Time
    );
    // The `json!` macro expands recursively per key and this object is
    // already close to the default recursion limit, so the later keys are
    // inserted rather than added to the literal.
    let mut view = json!({
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
    });
    if let Some(object) = view.as_object_mut() {
        object.insert("is_editable_relation".into(), json!(is_relation));
        object.insert("needs_chrono".into(), json!(needs_chrono));
        object.insert("relation_entity_snake".into(), json!(relation_entity_snake));
        object.insert(
            "relation_entity_pascal".into(),
            json!(relation_entity_pascal),
        );
        object.insert("relation_label_field".into(), json!(relation_label_field));
        object.insert("relation_key_field".into(), json!(relation_key_field));
        object.insert("enum_values".into(), json!(enum_values));
        object.insert("options".into(), json!(options));
        object.insert("can_edit".into(), json!(can_edit));
        object.insert("max_length".into(), json!(max_length));
        object.insert("comment".into(), json!(field.comment));
    }
    view
}

/// The `RelationType` tag alone, e.g. `BelongsTo`.
///
/// `format!("{:?}", relation_type)` would include the variant's payload for the
/// struct variants, so the template would never match the `BelongsTo` arm.
fn relation_type_name(relation_type: &RelationType) -> &'static str {
    match relation_type {
        RelationType::BelongsTo { .. } => "BelongsTo",
        RelationType::HasOne { .. } => "HasOne",
        RelationType::HasMany { .. } => "HasMany",
        RelationType::ManyToMany { .. } => "ManyToMany",
    }
}

/// Project an entity relation into the shape the SeaORM model template needs.
///
/// Returns `None` when either endpoint no longer resolves (the target entity or
/// field was deleted), so a dangling relation is skipped instead of emitting a
/// reference to a column or module that does not exist.
fn relation_view(relation: &EntityRelation, owner: &Entity, project: &Project) -> Option<Value> {
    let from_field = owner
        .fields
        .iter()
        .find(|f| f.id == relation.from_field)
        .map(|f| f.name.clone())?;
    let to_entity = project.entities.get(&relation.to_entity)?;
    let to_entity_name = to_entity.name.clone();
    let to_table = to_entity
        .options
        .table_name
        .clone()
        .unwrap_or_else(|| snake(&to_entity.plural_name));
    let to_field = to_entity
        .fields
        .iter()
        .find(|f| f.id == relation.to_field)
        .map(|f| f.name.clone())?;
    Some(json!({
        "name": relation.name,
        "relation_name": relation.name,
        "relation_type": relation_type_name(&relation.relation_type),
        "from_field": from_field,
        "from_column": pascal(&from_field),
        "to_entity": { "name": to_entity_name },
        "to_entity_snake": snake(&to_entity_name),
        "to_table": to_table,
        "to_field": to_field,
        "to_column": pascal(&to_field),
        "on_delete": format!("{:?}", relation.on_delete),
        "on_update": format!("{:?}", relation.on_update),
    }))
}

/// Pick a unique Rust identifier for a relation variant, appending a suffix
/// when two relations in the same entity share a name.
fn unique_variant(base: &str, used: &mut Vec<String>) -> String {
    let mut variant = base.to_string();
    let mut suffix = 2;
    while used.contains(&variant) {
        variant = format!("{}{}", base, suffix);
        suffix += 1;
    }
    used.push(variant.clone());
    variant
}

pub fn entity_view(entity: &Entity, project: &Project, driver: &DatabaseDriver) -> Value {
    let fields: Vec<Value> = entity
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| field_view(f, i, driver, project))
        .collect();
    let mut relations: Vec<Value> = Vec::new();
    let mut related: Vec<Value> = Vec::new();
    let mut used_variants: Vec<String> = Vec::new();
    for (index, relation) in entity.relations.iter().enumerate() {
        // ManyToMany needs a generated join entity to be meaningful, which the
        // generator does not emit yet. Skipping it avoids an empty `#[sea_orm]`
        // attribute, which SeaORM rejects at compile time.
        if matches!(relation.relation_type, RelationType::ManyToMany { .. }) {
            continue;
        }
        let Some(mut view) = relation_view(relation, entity, project) else {
            continue;
        };
        let base = if relation.name.trim().is_empty() {
            format!("Rel{}", index)
        } else {
            pascal(&relation.name)
        };
        let variant = unique_variant(&base, &mut used_variants);
        if let Some(obj) = view.as_object_mut() {
            obj.insert("variant".to_string(), Value::String(variant.clone()));
        }
        let to_snake = view
            .get("to_entity_snake")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        // SeaORM's `Related` impl is per target entity, so a second relation
        // to the same target must not emit a duplicate impl.
        if !related
            .iter()
            .any(|r| r.get("entity_snake").and_then(|v| v.as_str()) == Some(&to_snake))
        {
            related.push(json!({ "entity_snake": to_snake, "variant": variant }));
        }
        relations.push(view);
    }
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
    let pk_is_uuid = pk.get("is_uuid").and_then(|v| v.as_bool()).unwrap_or(true);
    let pk_is_integer = pk
        .get("is_integer")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // The generated model only imports chrono's types and serde_json's
    // when the entity actually has a field that needs them.
    let needs_chrono = fields
        .iter()
        .any(|f| f.get("needs_chrono").and_then(|v| v.as_bool()) == Some(true));

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
        "related": related,
        "indexes": indexes,
        "pk": pk,
        "pk_rust_type": pk_rust_type,
        "pk_rust_ident": pk_rust_ident,
        "pk_is_uuid": pk_is_uuid,
        "pk_is_integer": pk_is_integer,
        "needs_chrono": needs_chrono,
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
        FieldType::Select { .. } | FieldType::Radio { .. } | FieldType::Checkbox { .. } => {
            "string[]"
        }
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

/// Rust type used in the generated form-submission struct.
///
/// Form inputs are serialized as strings; parsing them into richer types is a
/// follow-up, so every field is `String` here.
fn form_rust_type(_ft: &FieldType) -> &'static str {
    "String"
}

/// Whether the field contributes a value to the submission payload.
///
/// `Section` and `Repeater` are structural: they group other fields rather than
/// hold a value, so they are rendered but never submitted.
fn is_submittable(ft: &FieldType) -> bool {
    !matches!(ft, FieldType::Section { .. } | FieldType::Repeater { .. })
}

/// Whether the field accepts more than one value.
fn is_multiple(ft: &FieldType) -> bool {
    match ft {
        FieldType::Select { multiple, .. } => *multiple,
        FieldType::Checkbox { .. } => true,
        _ => false,
    }
}

pub fn form_field_view(field: &FormField, index: usize) -> Value {
    json!({
        "name": field.name,
        "snake": snake(&field.name),
        "pascal": pascal(&field.name),
        "camel": camel(&field.name),
        "rust_ident": rust_ident(&snake(&field.name)),
        "label": if field.label.is_empty() { label(&field.name) } else { field.label.clone() },
        "field_type": form_field_type_name(&field.field_type),
        "input_type": form_input_type(&field.field_type),
        "ts_type": form_ts_type(&field.field_type),
        "rust_type": form_rust_type(&field.field_type),
        "required": field.required,
        "is_primary_key": index == 0,
        "not_null": field.required,
        "has_default": field.default_value.is_some(),
        "default_value": field.default_value.clone().unwrap_or(Value::Null),
        "placeholder": field.ui_props.placeholder.clone().unwrap_or_default(),
        "help_text": field.ui_props.help_text.clone().unwrap_or_default(),
        "options": select_options(&field.field_type),
        "is_relation": matches!(field.field_type, FieldType::Relation { .. }),
        "is_submittable": is_submittable(&field.field_type),
        "multiple": is_multiple(&field.field_type),
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

/// Normalize a page path into a Leptos router path.
///
/// The router already owns `/` (the generated home page), so a page whose path
/// is `/` is skipped by the router rather than colliding with it.
fn normalize_route(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return "/".to_string();
    }
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{}", trimmed)
    }
}

fn token_open(tag: &str, attrs: &str, depth: usize) -> Value {
    json!({
        "is_open": true, "is_close": false, "is_void": false, "is_text": false,
        "tag": tag, "attrs": attrs, "text": "", "depth": depth,
    })
}

fn token_close(tag: &str, depth: usize) -> Value {
    json!({
        "is_open": false, "is_close": true, "is_void": false, "is_text": false,
        "tag": tag, "attrs": "", "text": "", "depth": depth,
    })
}

fn token_void(tag: &str, attrs: &str, depth: usize) -> Value {
    json!({
        "is_open": false, "is_close": false, "is_void": true, "is_text": false,
        "tag": tag, "attrs": attrs, "text": "", "depth": depth,
    })
}

fn token_text(text: &str, depth: usize) -> Value {
    json!({
        "is_open": false, "is_close": false, "is_void": false, "is_text": true,
        "tag": "", "attrs": "", "text": text, "depth": depth,
    })
}

/// The serde tag of a component, e.g. `Container`.
fn component_type_name(component: &ComponentType) -> String {
    serde_json::to_value(component)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string))
        .unwrap_or_else(|| "Unknown".to_string())
}

/// The serde `config` payload of a component.
fn component_config(component: &ComponentType) -> Value {
    serde_json::to_value(component)
        .ok()
        .and_then(|v| v.get("config").cloned())
        .unwrap_or(Value::Null)
}

fn config_str(config: &Value, key: &str) -> String {
    config
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn config_owned(config: &Value, key: &str) -> Option<String> {
    config.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

/// Component ids nested inside a component, so the tree can be walked.
fn child_ids(component: &ComponentType) -> Vec<nocode_model::page::ComponentId> {
    use nocode_model::page::ComponentId;
    match component {
        ComponentType::Container { children }
        | ComponentType::Grid { children, .. }
        | ComponentType::Flex { children, .. }
        | ComponentType::Card { children, .. }
        | ComponentType::Modal { children, .. }
        | ComponentType::Tabs { children, .. } => children.clone(),
        ComponentType::Drawer {
            children, trigger, ..
        } => {
            let mut ids = children.clone();
            ids.push(trigger.clone());
            ids
        }
        ComponentType::Accordion { items, children } => {
            let mut ids = children.clone();
            ids.extend(items.iter().flat_map(|item| item.children.clone()));
            ids
        }
        ComponentType::List {
            item_template,
            empty_state,
            ..
        } => {
            let mut ids = vec![item_template.clone()];
            if let Some(state) = empty_state {
                ids.push(state.clone());
            }
            ids
        }
        ComponentType::Popover {
            content, trigger, ..
        } => {
            let mut ids = content.clone();
            ids.push(trigger.clone());
            ids
        }
        ComponentType::Tooltip { trigger, .. } | ComponentType::Dropdown { trigger, .. } => {
            vec![trigger.clone()]
        }
        _ => {
            let _ = ComponentId::new();
            Vec::new()
        }
    }
}

fn emit_children(
    children: &[nocode_model::page::ComponentId],
    depth: usize,
    by_id: &HashMap<nocode_model::page::ComponentId, &PageComponent>,
    out: &mut Vec<Value>,
) {
    for id in children {
        if let Some(child) = by_id.get(id) {
            emit_component(child, depth, by_id, out);
        }
    }
}

/// Emit the Leptos `view!` tokens for one component and its subtree.
///
/// Components without a dedicated renderer fall back to a labelled box, so a
/// page using an unsupported type still generates a compiling app instead of
/// failing codegen.
fn emit_component(
    component: &PageComponent,
    depth: usize,
    by_id: &HashMap<nocode_model::page::ComponentId, &PageComponent>,
    out: &mut Vec<Value>,
) {
    use nocode_model::page::ComponentId;

    let name = component_type_name(&component.component);
    let config = component_config(&component.component);
    let children = child_ids(&component.component);
    let fallback = |out: &mut Vec<Value>| {
        out.push(token_open(
            "div",
            "class=\"rounded border border-dashed border-slate-300 p-2 text-xs text-slate-400\"",
            depth,
        ));
        out.push(token_text(&format!("[{name}]"), depth + 1));
        out.push(token_close("div", depth));
    };

    match name.as_str() {
        // ---- Layout ----
        "Container" => {
            out.push(token_open("div", "class=\"space-y-4\"", depth));
            emit_children(&children, depth + 1, by_id, out);
            out.push(token_close("div", depth));
        }
        "Grid" => {
            let columns = config.get("columns").and_then(|v| v.as_u64()).unwrap_or(12);
            out.push(token_open(
                "div",
                &format!(
                    "class=\"grid gap-4\" style=\"grid-template-columns: repeat({columns}, minmax(0, 1fr))\""
                ),
                depth,
            ));
            emit_children(&children, depth + 1, by_id, out);
            out.push(token_close("div", depth));
        }
        "Flex" => {
            let direction = config_str(&config, "direction");
            let direction = if direction.is_empty() {
                "row"
            } else {
                &direction
            };
            out.push(token_open(
                "div",
                &format!("class=\"flex flex-{direction} flex-wrap gap-4\""),
                depth,
            ));
            emit_children(&children, depth + 1, by_id, out);
            out.push(token_close("div", depth));
        }
        "Card" => {
            out.push(token_open(
                "div",
                "class=\"rounded-lg border border-slate-200 bg-white p-4 shadow-sm\"",
                depth,
            ));
            if let Some(header) = config_owned(&config, "header") {
                out.push(token_open("h3", "class=\"mb-2 font-semibold\"", depth + 1));
                out.push(token_text(&header, depth + 2));
                out.push(token_close("h3", depth + 1));
            }
            emit_children(&children, depth + 1, by_id, out);
            if let Some(footer) = config_owned(&config, "footer") {
                out.push(token_open(
                    "p",
                    "class=\"mt-2 text-sm text-slate-500\"",
                    depth + 1,
                ));
                out.push(token_text(&footer, depth + 2));
                out.push(token_close("p", depth + 1));
            }
            out.push(token_close("div", depth));
        }
        "Divider" => {
            out.push(token_void("hr", "class=\"border-slate-200\"", depth));
        }

        // ---- Basic ----
        "Html" => {
            out.push(token_text(&config_str(&config, "content"), depth));
        }
        "Image" | "Avatar" => {
            let src = config_str(&config, "src");
            let alt = config_str(&config, "alt");
            let class = if name == "Avatar" {
                "class=\"h-10 w-10 rounded-full object-cover\""
            } else {
                "class=\"max-w-full rounded\""
            };
            out.push(token_void(
                "img",
                &format!("src=\"{src}\" alt=\"{alt}\" {class}"),
                depth,
            ));
        }
        "Badge" | "Tag" => {
            let content = config_str(&config, "content");
            let class = if name == "Badge" {
                "class=\"rounded bg-blue-100 px-2 py-0.5 text-xs font-medium text-blue-700\""
            } else {
                "class=\"rounded border border-slate-300 px-2 py-0.5 text-xs\""
            };
            out.push(token_open("span", class, depth));
            out.push(token_text(&content, depth + 1));
            out.push(token_close("span", depth));
        }
        "Iframe" => {
            out.push(token_void(
                "iframe",
                &format!(
                    "src=\"{}\" class=\"w-full rounded border\"",
                    config_str(&config, "src")
                ),
                depth,
            ));
        }
        "Video" => {
            out.push(token_open(
                "video",
                &format!(
                    "src=\"{}\" controls class=\"max-w-full rounded\"",
                    config_str(&config, "src")
                ),
                depth,
            ));
            out.push(token_close("video", depth));
        }
        "Audio" => {
            out.push(token_open(
                "audio",
                &format!("src=\"{}\" controls", config_str(&config, "src")),
                depth,
            ));
            out.push(token_close("audio", depth));
        }

        // ---- Navigation ----
        "Link" => {
            let href = config_str(&config, "href");
            let label = config_str(&config, "label");
            out.push(token_open(
                "a",
                &format!("href=\"{href}\" class=\"text-blue-600 hover:underline\""),
                depth,
            ));
            out.push(token_text(&label, depth + 1));
            out.push(token_close("a", depth));
        }
        "Breadcrumb" => {
            out.push(token_open(
                "nav",
                "class=\"flex gap-1 text-sm text-slate-500\"",
                depth,
            ));
            if let Some(items) = config.get("items").and_then(|v| v.as_array()) {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(token_text("/", depth + 1));
                    }
                    out.push(token_text(
                        item.get("label").and_then(|v| v.as_str()).unwrap_or(""),
                        depth + 1,
                    ));
                }
            }
            out.push(token_close("nav", depth));
        }
        "Steps" => {
            out.push(token_open("ol", "class=\"flex gap-4 text-sm\"", depth));
            if let Some(items) = config.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    out.push(token_open("li", "class=\"text-slate-700\"", depth + 1));
                    out.push(token_text(title, depth + 2));
                    out.push(token_close("li", depth + 1));
                }
            }
            out.push(token_close("ol", depth));
        }
        "Menu" => {
            out.push(token_open("ul", "class=\"flex gap-4 text-sm\"", depth));
            if let Some(items) = config.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    let label = item.get("label").and_then(|v| v.as_str()).unwrap_or("");
                    out.push(token_open("li", "", depth + 1));
                    out.push(token_text(label, depth + 2));
                    out.push(token_close("li", depth + 1));
                }
            }
            out.push(token_close("ul", depth));
        }
        "Anchor" => {
            out.push(token_open(
                "div",
                "class=\"flex flex-col gap-1 text-sm\"",
                depth,
            ));
            if let Some(links) = config.get("links").and_then(|v| v.as_array()) {
                for link in links {
                    let title = link.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    let href = link.get("href").and_then(|v| v.as_str()).unwrap_or("#");
                    out.push(token_open(
                        "a",
                        &format!("href=\"{href}\" class=\"text-blue-600\""),
                        depth + 1,
                    ));
                    out.push(token_text(title, depth + 2));
                    out.push(token_close("a", depth + 1));
                }
            }
            out.push(token_close("div", depth));
        }
        "BackTop" => {
            out.push(token_open(
                "button",
                "class=\"rounded bg-slate-200 px-3 py-1 text-sm\"",
                depth,
            ));
            out.push(token_text("Back to top", depth + 1));
            out.push(token_close("button", depth));
        }
        "Pagination" => {
            out.push(token_open(
                "div",
                "class=\"flex items-center gap-2 text-sm\"",
                depth,
            ));
            for page_no in ["1", "2", "3"] {
                out.push(token_open(
                    "button",
                    "class=\"rounded border px-2 py-1\"",
                    depth + 1,
                ));
                out.push(token_text(page_no, depth + 2));
                out.push(token_close("button", depth + 1));
            }
            out.push(token_close("div", depth));
        }

        // ---- Feedback ----
        "Alert" => {
            let message = config_str(&config, "message");
            out.push(token_open(
                "div",
                "class=\"rounded bg-blue-50 p-3 text-sm text-blue-800\"",
                depth,
            ));
            out.push(token_text(&message, depth + 1));
            out.push(token_close("div", depth));
        }
        "Message" => {
            out.push(token_open("div", "class=\"text-sm\"", depth));
            out.push(token_text(&config_str(&config, "content"), depth + 1));
            out.push(token_close("div", depth));
        }
        "Notification" => {
            out.push(token_open(
                "div",
                "class=\"rounded border p-3 text-sm\"",
                depth,
            ));
            out.push(token_text(
                &format!(
                    "{}: {}",
                    config_str(&config, "title"),
                    config_str(&config, "content")
                ),
                depth + 1,
            ));
            out.push(token_close("div", depth));
        }
        "Loading" | "Spin" => {
            out.push(token_text("Loading…", depth));
        }
        "Skeleton" => {
            out.push(token_open(
                "div",
                "class=\"animate-pulse space-y-2\"",
                depth,
            ));
            for _ in 0..3 {
                out.push(token_void(
                    "div",
                    "class=\"h-4 rounded bg-slate-200\"",
                    depth + 1,
                ));
            }
            out.push(token_close("div", depth));
        }
        "QRCode" => {
            out.push(token_open(
                "div",
                "class=\"rounded border p-2 text-xs\"",
                depth,
            ));
            out.push(token_text(&config_str(&config, "value"), depth + 1));
            out.push(token_close("div", depth));
        }

        // ---- Static data ----
        "StatCard" => {
            out.push(token_open(
                "div",
                "class=\"rounded-lg border border-slate-200 bg-white p-4\"",
                depth,
            ));
            out.push(token_open("div", "class=\"text-2xl font-bold\"", depth + 1));
            out.push(token_text(&config_str(&config, "value"), depth + 2));
            out.push(token_close("div", depth + 1));
            out.push(token_open(
                "div",
                "class=\"text-sm text-slate-500\"",
                depth + 1,
            ));
            out.push(token_text(&config_str(&config, "label"), depth + 2));
            out.push(token_close("div", depth + 1));
            out.push(token_close("div", depth));
        }
        "ProgressBar" => {
            let value = config_str(&config, "value");
            let value = value.parse::<f64>().unwrap_or(0.0).clamp(0.0, 100.0);
            out.push(token_open(
                "div",
                "class=\"h-2 rounded bg-slate-200\"",
                depth,
            ));
            out.push(token_void(
                "div",
                &format!("class=\"h-2 rounded bg-blue-600\" style=\"width: {value}%\""),
                depth + 1,
            ));
            out.push(token_close("div", depth));
        }

        // ---- Input ----
        "Button" => {
            let label = config_str(&config, "label");
            out.push(token_open(
                "button",
                "class=\"rounded bg-blue-600 px-4 py-2 text-white hover:bg-blue-700\"",
                depth,
            ));
            out.push(token_text(&label, depth + 1));
            out.push(token_close("button", depth));
        }
        "FormField" => {
            let label = config_str(&config, "label");
            out.push(token_open(
                "label",
                "class=\"flex flex-col gap-1 text-sm\"",
                depth,
            ));
            out.push(token_open("span", "class=\"text-slate-600\"", depth + 1));
            out.push(token_text(&label, depth + 2));
            out.push(token_close("span", depth + 1));
            out.push(token_void(
                "input",
                "type=\"text\" class=\"rounded border border-slate-300 px-2 py-1\"",
                depth + 1,
            ));
            out.push(token_close("label", depth));
        }
        "ButtonGroup" => {
            out.push(token_open("div", "class=\"flex gap-2\"", depth));
            if let Some(buttons) = config.get("buttons").and_then(|v| v.as_array()) {
                for button in buttons {
                    let label = button.get("label").and_then(|v| v.as_str()).unwrap_or("");
                    out.push(token_open(
                        "button",
                        "class=\"rounded bg-blue-600 px-3 py-1 text-sm text-white\"",
                        depth + 1,
                    ));
                    out.push(token_text(label, depth + 2));
                    out.push(token_close("button", depth + 1));
                }
            }
            out.push(token_close("div", depth));
        }
        "FileUpload" => {
            out.push(token_void(
                "input",
                "type=\"file\" class=\"text-sm\"",
                depth,
            ));
        }
        "Form" => {
            out.push(token_open(
                "form",
                "class=\"space-y-4 rounded-lg border border-slate-200 bg-white p-4\"",
                depth,
            ));
            emit_children(&children, depth + 1, by_id, out);
            out.push(token_void(
                "button",
                "type=\"submit\" class=\"rounded bg-blue-600 px-4 py-2 text-white hover:bg-blue-700\"",
                depth + 1,
            ));
            out.push(token_close("form", depth));
        }
        "Table" => {
            let data_source = config_str(&config, "data_source");
            out.push(token_open(
                "div",
                "class=\"overflow-x-auto rounded-lg border border-slate-200\"",
                depth,
            ));
            out.push(token_open(
                "table",
                "class=\"min-w-full divide-y divide-slate-200 text-sm\"",
                depth + 1,
            ));
            out.push(token_open(
                "thead",
                "class=\"bg-slate-100 text-left text-slate-600\"",
                depth + 2,
            ));
            out.push(token_open("tr", "", depth + 3));
            if let Some(columns) = config.get("columns").and_then(|v| v.as_array()) {
                for col in columns {
                    let title = col.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    out.push(token_open(
                        "th",
                        "class=\"px-3 py-2 font-medium\"",
                        depth + 4,
                    ));
                    out.push(token_text(title, depth + 5));
                    out.push(token_close("th", depth + 4));
                }
            }
            out.push(token_close("tr", depth + 3));
            out.push(token_close("thead", depth + 2));
            out.push(token_open(
                "tbody",
                "class=\"divide-y divide-slate-100\"",
                depth + 2,
            ));
            out.push(token_open(
                "tr",
                "class=\"text-center text-slate-500\"",
                depth + 3,
            ));
            out.push(token_open(
                "td",
                "class=\"px-3 py-4\" colspan=\"99\"",
                depth + 4,
            ));
            out.push(token_text(&format!("(data from {data_source})"), depth + 5));
            out.push(token_close("td", depth + 4));
            out.push(token_close("tr", depth + 3));
            out.push(token_close("tbody", depth + 2));
            out.push(token_close("table", depth + 1));
            out.push(token_close("div", depth));
        }
        "DetailView" => {
            let data_source = config_str(&config, "data_source");
            out.push(token_open("div", "class=\"space-y-4\"", depth));
            if let Some(fields) = config.get("fields").and_then(|v| v.as_array()) {
                for field in fields {
                    let label = field.get("label").and_then(|v| v.as_str()).unwrap_or("");
                    out.push(token_open("div", "class=\"flex gap-4\"", depth + 1));
                    out.push(token_open(
                        "span",
                        "class=\"w-1/3 text-slate-600 font-medium\"",
                        depth + 2,
                    ));
                    out.push(token_text(label, depth + 3));
                    out.push(token_close("span", depth + 2));
                    out.push(token_open(
                        "span",
                        "class=\"flex-1 text-slate-900\"",
                        depth + 2,
                    ));
                    out.push(token_text("(data)", depth + 3));
                    out.push(token_close("span", depth + 2));
                    out.push(token_close("div", depth + 1));
                }
            } else {
                out.push(token_text(&format!("(data from {data_source})"), depth + 1));
            }
            out.push(token_close("div", depth));
        }
        "List" => {
            let data_source = config_str(&config, "data_source");
            out.push(token_open("div", "class=\"space-y-2\"", depth));
            out.push(token_text(&format!("(list from {data_source})"), depth + 1));
            out.push(token_close("div", depth));
        }

        // ---- Structural components with no data source ----
        //
        // These render as static HTML on purpose. The emitter writes raw
        // HTML strings into a Leptos `view!` macro, so there is no way to
        // attach a signal to an element, and a control that looks
        // interactive but does nothing is worse than one that does not
        // pretend to be interactive. The Phase 1 work removed the dead
        // buttons from the IDE for the same reason.
        "Tabs" => {
            // The model has no per-tab content — `children` hang off the
            // Tabs itself, not off individual tabs — so the strip is a
            // static list of the tab names.
            if let Some(tabs) = config.get("tabs").and_then(|v| v.as_array()) {
                if !tabs.is_empty() {
                    out.push(token_open(
                        "div",
                        "class=\"flex gap-1 border-b border-slate-200\"",
                        depth,
                    ));
                    for tab in tabs {
                        let label = tab.get("label").and_then(|v| v.as_str()).unwrap_or("");
                        let disabled = tab
                            .get("disabled")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let class = if disabled {
                            "px-3 py-2 text-sm font-medium text-slate-300"
                        } else {
                            "px-3 py-2 text-sm font-medium text-slate-700 border-b-2 border-blue-500"
                        };
                        out.push(token_open("span", &format!("class=\"{class}\""), depth + 1));
                        out.push(token_text(label, depth + 2));
                        out.push(token_close("span", depth + 1));
                    }
                    out.push(token_close("div", depth));
                }
            }
            out.push(token_open("div", "class=\"pt-4\"", depth));
            emit_children(&children, depth + 1, by_id, out);
            out.push(token_close("div", depth));
        }
        "Accordion" => {
            // `<details>`/`<summary>` gives a real, working accordion with
            // no JavaScript at all.
            out.push(token_open(
                "div",
                "class=\"divide-y divide-slate-200\"",
                depth,
            ));
            if let Some(items) = config.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    let label = item.get("label").and_then(|v| v.as_str()).unwrap_or("");
                    out.push(token_open("details", "class=\"py-2\"", depth + 1));
                    out.push(token_open(
                        "summary",
                        "class=\"cursor-pointer text-sm font-medium text-slate-700\"",
                        depth + 2,
                    ));
                    out.push(token_text(label, depth + 3));
                    out.push(token_close("summary", depth + 2));
                    out.push(token_open("div", "class=\"pl-4 pt-2\"", depth + 2));
                    if let Some(item_children) = item.get("children").and_then(|v| v.as_array()) {
                        let ids: Vec<nocode_model::page::ComponentId> = item_children
                            .iter()
                            .filter_map(|value| serde_json::from_value(value.clone()).ok())
                            .collect();
                        emit_children(&ids, depth + 3, by_id, out);
                    }
                    out.push(token_close("div", depth + 2));
                    out.push(token_close("details", depth + 1));
                }
            }
            emit_children(&children, depth + 1, by_id, out);
            out.push(token_close("div", depth));
        }

        // ---- Data-driven components ----
        //
        // These read from a data source. The page-level DataBinding fetches
        // the data, but the component tree does not receive it, so the
        // renderer states which source it expects. This is the same
        // contract the Table renderer already has.
        "Chart" => {
            let data_source = config_str(&config, "data_source");
            let chart_type = config_str(&config, "chart_type");
            let chart_type = if chart_type.is_empty() {
                "chart".to_string()
            } else {
                chart_type
            };
            out.push(token_open(
                "div",
                "class=\"rounded-lg border border-slate-200 bg-white p-4\"",
                depth,
            ));
            out.push(token_open(
                "div",
                "class=\"mb-2 text-xs font-medium uppercase tracking-wide text-slate-500\"",
                depth + 1,
            ));
            out.push(token_text(&chart_type, depth + 2));
            out.push(token_close("div", depth + 1));
            out.push(token_open(
                "div",
                "class=\"flex h-40 items-center justify-center rounded border border-dashed border-slate-200 text-sm text-slate-400\"",
                depth + 1,
            ));
            out.push(token_text(
                &format!("(chart data from {data_source})"),
                depth + 2,
            ));
            out.push(token_close("div", depth + 1));
            out.push(token_close("div", depth));
        }
        "Tree" | "Transfer" | "Cascader" | "TreeSelect" => {
            let data_source = config_str(&config, "data_source");
            let label = match name.as_str() {
                "Tree" => "tree",
                "Transfer" => "transfer",
                "Cascader" => "cascader",
                _ => "tree select",
            };
            out.push(token_open(
                "div",
                "class=\"rounded-lg border border-slate-200 bg-white p-4\"",
                depth,
            ));
            out.push(token_open(
                "div",
                "class=\"mb-2 text-xs font-medium uppercase tracking-wide text-slate-500\"",
                depth + 1,
            ));
            out.push(token_text(label, depth + 2));
            out.push(token_close("div", depth + 1));
            out.push(token_open(
                "div",
                "class=\"flex min-h-16 items-center justify-center rounded border border-dashed border-slate-200 text-sm text-slate-400\"",
                depth + 1,
            ));
            out.push(token_text(&format!("(data from {data_source})"), depth + 2));
            out.push(token_close("div", depth + 1));
            out.push(token_close("div", depth));
        }

        // ---- Native form controls ----
        //
        // These have a real HTML equivalent that works with no JavaScript
        // and no framework, which is what the raw-HTML emitter can express.
        "ColorPicker" => {
            let alpha = config
                .get("alpha")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            out.push(token_open(
                "div",
                "class=\"flex items-center gap-2\"",
                depth,
            ));
            out.push(token_void(
                "input",
                "type=\"color\" class=\"h-8 w-14 cursor-pointer rounded border border-slate-300\"",
                depth + 1,
            ));
            if alpha {
                out.push(token_void(
                    "input",
                    "type=\"range\" min=\"0\" max=\"1\" step=\"0.01\" class=\"w-32\"",
                    depth + 1,
                ));
            }
            if let Some(presets) = config.get("preset_colors").and_then(|v| v.as_array()) {
                for preset in presets {
                    let colour = preset.as_str().unwrap_or("");
                    if colour.is_empty() {
                        continue;
                    }
                    out.push(token_void(
                        "span",
                        &format!(
                            "title=\"{colour}\" class=\"inline-block h-5 w-5 rounded border border-slate-300\" style=\"background-color: {colour}\""
                        ),
                        depth + 1,
                    ));
                }
            }
            out.push(token_close("div", depth));
        }
        "DatePicker" => {
            let show_time = config
                .get("show_time")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let range = config
                .get("range")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let input_type = if show_time { "datetime-local" } else { "date" };
            let attrs = format!(
                "type=\"{input_type}\" class=\"rounded border border-slate-300 px-2 py-1\""
            );
            out.push(token_open(
                "div",
                "class=\"flex items-center gap-2\"",
                depth,
            ));
            out.push(token_void("input", &attrs, depth + 1));
            if range {
                out.push(token_text("to", depth + 1));
                out.push(token_void("input", &attrs, depth + 1));
            }
            out.push(token_close("div", depth));
        }
        "TimePicker" => {
            // The HTML `step` attribute is in seconds. Take the coarsest
            // unit the config names, so the default of hour=1, minute=1,
            // second=1 steps by the hour.
            let step = config
                .get("step")
                .map(|value| {
                    let hour = value.get("hour").and_then(|v| v.as_u64()).unwrap_or(0);
                    let minute = value.get("minute").and_then(|v| v.as_u64()).unwrap_or(0);
                    let second = value.get("second").and_then(|v| v.as_u64()).unwrap_or(0);
                    if hour > 0 {
                        hour * 3600
                    } else if minute > 0 {
                        minute * 60
                    } else {
                        second.max(1)
                    }
                })
                .unwrap_or(60);
            out.push(token_void(
                "input",
                &format!(
                    "type=\"time\" step=\"{step}\" class=\"rounded border border-slate-300 px-2 py-1\""
                ),
                depth,
            ));
        }
        "Rate" => {
            let count = config.get("count").and_then(|v| v.as_u64()).unwrap_or(5);
            let character = config_str(&config, "character");
            let character = if character.is_empty() {
                "*".to_string()
            } else {
                character
            };
            // All the radios in one group share a name, so selecting one
            // deselects the others. The component id identifies the group.
            let group = format!("rate-{}", component.id.0);
            out.push(token_open(
                "div",
                "class=\"flex items-center gap-1\" role=\"radiogroup\" aria-label=\"Rating\"",
                depth,
            ));
            for index in 1..=count.max(1) {
                out.push(token_void(
                    "input",
                    &format!(
                        "type=\"radio\" name=\"{group}\" class=\"h-4 w-4\" aria-label=\"{character} {index}\""
                    ),
                    depth + 1,
                ));
                out.push(token_text(&character, depth + 1));
            }
            out.push(token_close("div", depth));
        }
        "Slider" => {
            let min = config.get("min").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let max = config.get("max").and_then(|v| v.as_f64()).unwrap_or(100.0);
            let step = config.get("step").and_then(|v| v.as_f64()).unwrap_or(1.0);
            out.push(token_open("div", "class=\"space-y-1\"", depth));
            out.push(token_void(
                "input",
                &format!(
                    "type=\"range\" min=\"{min}\" max=\"{max}\" step=\"{step}\" class=\"w-full\""
                ),
                depth + 1,
            ));
            if let Some(marks) = config.get("marks").and_then(|v| v.as_object()) {
                if !marks.is_empty() {
                    out.push(token_open(
                        "div",
                        "class=\"flex justify-between text-xs text-slate-500\"",
                        depth + 1,
                    ));
                    for (label, value) in marks {
                        out.push(token_open("span", "", depth + 2));
                        out.push(token_text(&format!("{label}: {value}"), depth + 3));
                        out.push(token_close("span", depth + 2));
                    }
                    out.push(token_close("div", depth + 1));
                }
            }
            out.push(token_close("div", depth));
        }
        "Mention" => {
            let prefix = config_str(&config, "prefix");
            let suggestions = config
                .get("suggestions")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let list_id = format!("mention-{depth}");
            out.push(token_open("div", "class=\"space-y-1\"", depth));
            out.push(token_void(
                "input",
                &format!(
                    "type=\"text\" list=\"{list_id}\" placeholder=\"{prefix}\" class=\"w-full rounded border border-slate-300 px-2 py-1\""
                ),
                depth + 1,
            ));
            if !suggestions.is_empty() {
                out.push(token_open(
                    "datalist",
                    &format!("id=\"{list_id}\""),
                    depth + 1,
                ));
                for suggestion in suggestions {
                    let value = suggestion.as_str().unwrap_or("");
                    if value.is_empty() {
                        continue;
                    }
                    out.push(token_void(
                        "option",
                        &format!("value=\"{value}\""),
                        depth + 2,
                    ));
                }
                out.push(token_close("datalist", depth + 1));
            }
            out.push(token_close("div", depth));
        }
        "Custom" => {
            let component_name = config_str(&config, "component_name");
            let component_name = if component_name.is_empty() {
                "custom".to_string()
            } else {
                component_name
            };
            out.push(token_open(
                "div",
                "class=\"rounded-lg border border-dashed border-slate-300 p-3\"",
                depth,
            ));
            out.push(token_open(
                "div",
                "class=\"text-xs font-medium uppercase tracking-wide text-slate-500\"",
                depth + 1,
            ));
            out.push(token_text(&component_name, depth + 2));
            out.push(token_close("div", depth + 1));
            if let Some(props) = config.get("props").and_then(|v| v.as_object()) {
                if !props.is_empty() {
                    out.push(token_open(
                        "dl",
                        "class=\"mt-2 grid grid-cols-[auto,1fr] gap-x-3 gap-y-1 text-xs\"",
                        depth + 1,
                    ));
                    for (key, value) in props {
                        out.push(token_open("dt", "class=\"text-slate-500\"", depth + 2));
                        out.push(token_text(key, depth + 3));
                        out.push(token_close("dt", depth + 2));
                        out.push(token_open("dd", "class=\"text-slate-800\"", depth + 2));
                        out.push(token_text(&value.to_string(), depth + 3));
                        out.push(token_close("dd", depth + 2));
                    }
                    out.push(token_close("dl", depth + 1));
                }
            }
            out.push(token_close("div", depth));
        }

        _ => {
            fallback(out);
            // A fallback used to DROP the component's children: the labelled
            // box never called emit_children, so a Tabs containing a Button
            // lost the Button entirely. Emit them so nothing silently
            // disappears.
            emit_children(&children, depth + 1, by_id, out);
        }
    }

    let _ = ComponentId::new();
}

/// Flatten a page's component tree into Leptos `view!` tokens.
fn component_tokens(components: &[PageComponent]) -> Vec<Value> {
    use nocode_model::page::ComponentId;
    let by_id: HashMap<ComponentId, &PageComponent> =
        components.iter().map(|c| (c.id.clone(), c)).collect();
    let mut child_id_set: HashSet<ComponentId> = HashSet::new();
    for component in components {
        child_id_set.extend(child_ids(&component.component));
    }
    let mut out = Vec::new();
    for component in components {
        if !child_id_set.contains(&component.id) {
            emit_component(component, 0, &by_id, &mut out);
        }
    }
    out
}

/// Data source view for the page template.
fn data_source_view(ds: &DataSource) -> Value {
    let cfg = &ds.config;
    let source_type = match &ds.source_type {
        DataSourceType::Api {
            endpoint, method, ..
        } => {
            json!({
                "type": "api",
                "endpoint": endpoint,
                "method": format!("{:?}", method),
                "snake": snake(&ds.id),
            })
        }
        DataSourceType::Static { data } => {
            json!({
                "type": "static",
                "data": data,
                "snake": snake(&ds.id),
            })
        }
        DataSourceType::Database { entity_id, .. } => {
            json!({
                "type": "database",
                "entity_id": format!("{:?}", entity_id),
                "snake": snake(&ds.id),
            })
        }
        _ => json!({
            "type": format!("{:?}", ds.source_type),
            "snake": snake(&ds.id),
        }),
    };

    json!({
        "id": ds.id,
        "name": ds.name,
        "snake": snake(&ds.id),
        "source_type": source_type,
        "auto_load": ds.auto_load,
        "polling_interval": ds.polling_interval,
        "config": cfg,
    })
}

/// Page action view for the template.
fn action_view(action: &PageAction) -> Value {
    let cfg = match &action.config {
        nocode_model::page::ActionConfig::Navigate { path, .. } => {
            json!({ "type": "navigate", "path": path })
        }
        nocode_model::page::ActionConfig::CallApi { data_source_id, .. } => {
            json!({ "type": "call_api", "data_source_id": data_source_id })
        }
        _ => json!({ "type": format!("{:?}", action.config) }),
    };
    json!({
        "id": action.id,
        "name": action.name,
        "config": cfg,
    })
}

pub fn page_view(page: &Page, project: &Project) -> Value {
    let _ = project;
    let route = normalize_route(&page.path);
    let tokens = component_tokens(&page.components);
    let data_sources: Vec<Value> = page.data_sources.iter().map(data_source_view).collect();
    let actions: Vec<Value> = page.actions.iter().map(action_view).collect();
    json!({
        "name": page.name,
        "pascal": pascal(&page.name),
        "camel": camel(&page.name),
        "snake": snake(&page.name),
        "lower": page.name.to_lowercase(),
        "path": page.path,
        "route": route,
        "is_index": route == "/",
        "title": if page.title.is_empty() { page.name.clone() } else { page.title.clone() },
        "description": page.description,
        "components": page.components,
        "tokens": tokens,
        "data_sources": data_sources,
        "actions": actions,
    })
}

pub fn pages_view(project: &Project) -> Vec<Value> {
    project
        .pages
        .values()
        .map(|p| page_view(p, project))
        .collect()
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
// Theme view
// ---------------------------------------------------------------------------

/// The project's active theme, flattened for the Tailwind/CSS templates.
///
/// Falls back to an empty `is_default` view when the project has no themes, so
/// generated apps still build with Tailwind's stock palette.
pub fn theme_view(project: &Project) -> Value {
    let active = project
        .active_theme_id
        .as_ref()
        .and_then(|id| project.themes.get(id))
        .or_else(|| {
            // Deterministic fallback: the alphabetically first theme keeps
            // codegen output stable when active_theme_id is stale.
            project.themes.values().min_by(|a, b| a.name.cmp(&b.name))
        });

    let Some(theme) = active else {
        return json!({ "is_default": true, "name": "tailwind-default" });
    };

    let tokens = &theme.tokens;
    let scale_of = |scale: &ColorScale| {
        let mut map = serde_json::Map::new();
        map.insert("base".to_string(), json!(scale.base));
        let shades: serde_json::Map<String, Value> = scale
            .scale
            .iter()
            .map(|(shade, value)| (shade.to_string(), json!(value)))
            .collect();
        map.insert("scale".to_string(), Value::Object(shades));
        Value::Object(map)
    };

    let mut color_scales = serde_json::Map::new();
    for (role, scale) in [
        ("primary", &tokens.colors.primary),
        ("secondary", &tokens.colors.secondary),
        ("success", &tokens.colors.success),
        ("warning", &tokens.colors.warning),
        ("error", &tokens.colors.error),
        ("info", &tokens.colors.info),
        ("neutral", &tokens.colors.neutral),
    ] {
        color_scales.insert(role.to_string(), scale_of(scale));
    }

    let spacing: serde_json::Map<String, Value> = tokens
        .spacing
        .scale
        .iter()
        .map(|(step, value)| (step.to_string(), json!(value)))
        .collect();

    // CSS custom properties, flattened: handlebars-rust cannot reach a parent
    // key from a nested `each`, so nested maps become a flat var list here.
    let mut css_vars: Vec<Value> = Vec::new();
    let mut push_var = |name: String, value: &str| {
        css_vars.push(json!({ "var": name, "value": value }));
    };
    for (role, scale) in [
        ("primary", &tokens.colors.primary),
        ("secondary", &tokens.colors.secondary),
        ("success", &tokens.colors.success),
        ("warning", &tokens.colors.warning),
        ("error", &tokens.colors.error),
        ("info", &tokens.colors.info),
        ("neutral", &tokens.colors.neutral),
    ] {
        push_var(format!("--nc-color-{role}"), &scale.base);
        for (shade, value) in &scale.scale {
            push_var(format!("--nc-color-{role}-{shade}"), value);
        }
    }
    for (key, value) in serde_json::to_value(&tokens.colors.background)
        .unwrap_or(Value::Null)
        .as_object()
        .into_iter()
        .flat_map(|map| map.iter())
    {
        push_var(
            format!("--nc-background-{key}"),
            value.as_str().unwrap_or_default(),
        );
    }
    for (key, value) in serde_json::to_value(&tokens.colors.text)
        .unwrap_or(Value::Null)
        .as_object()
        .into_iter()
        .flat_map(|map| map.iter())
    {
        push_var(
            format!("--nc-text-{key}"),
            value.as_str().unwrap_or_default(),
        );
    }
    for (key, value) in serde_json::to_value(&tokens.colors.border)
        .unwrap_or(Value::Null)
        .as_object()
        .into_iter()
        .flat_map(|map| map.iter())
    {
        push_var(
            format!("--nc-border-{key}"),
            value.as_str().unwrap_or_default(),
        );
    }
    push_var("--nc-radius-md".to_string(), &tokens.borders.radii.md);
    push_var("--nc-radius-lg".to_string(), &tokens.borders.radii.lg);
    push_var("--nc-shadow-sm".to_string(), &tokens.shadows.sm);
    push_var("--nc-shadow-md".to_string(), &tokens.shadows.md);

    let dark_tokens = theme.dark_mode.enabled.then(|| {
        let colors = theme.dark_mode.tokens.as_ref().map(|dark| {
            json!({
                "background": dark.colors.background,
                "text": dark.colors.text,
                "border": dark.colors.border,
            })
        });
        json!({
            "selector": theme.dark_mode.selector,
            "colors": colors,
        })
    });

    let families = &tokens.typography.font_families;
    let mut fonts = serde_json::Map::new();
    fonts.insert("sans".to_string(), json!(families.sans));
    fonts.insert("serif".to_string(), json!(families.serif));
    fonts.insert("mono".to_string(), json!(families.mono));
    if let Some(display) = &families.display {
        fonts.insert("display".to_string(), json!(display));
    }

    json!({
        "is_default": false,
        "name": theme.name,
        "color_scales": Value::Object(color_scales),
        "background": tokens.colors.background,
        "text": tokens.colors.text,
        "border": tokens.colors.border,
        "fonts": Value::Object(fonts),
        "font_sizes": tokens.typography.font_sizes,
        "radii": tokens.borders.radii,
        "shadows": {
            "sm": tokens.shadows.sm,
            "md": tokens.shadows.md,
            "lg": tokens.shadows.lg,
            "xl": tokens.shadows.xl,
        },
        "breakpoints": tokens.breakpoints,
        "spacing": Value::Object(spacing),
        "css_vars": css_vars,
        "fonts_css": {
            "sans": families.sans,
            "serif": families.serif,
            "mono": families.mono,
        },
        "global_styles": theme.global_styles,
        "dark": dark_tokens,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use nocode_model::page::{ComponentId, ComponentType, Page, PageComponent};
    use nocode_model::Project;

    /// Build a page with one component of the given type.
    fn page_with(component: ComponentType) -> Page {
        let mut page = Page::new("Test");
        page.components.push(PageComponent {
            id: ComponentId::new(),
            component,
            props: Default::default(),
            bindings: Vec::new(),
            events: Vec::new(),
            conditions: Vec::new(),
            slots: Default::default(),
            style_overrides: Default::default(),
        });
        page
    }

    /// The rendered HTML of a single-component page, flattened to a string.
    fn render(component: ComponentType) -> String {
        let page = page_with(component);
        let tokens = component_tokens(&page.components);
        tokens
            .iter()
            .map(|token| {
                if token.get("is_text").and_then(|v| v.as_bool()) == Some(true) {
                    token
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                } else if token.get("is_void").and_then(|v| v.as_bool()) == Some(true) {
                    format!(
                        "<{tag} {attrs}>",
                        tag = token.get("tag").and_then(|v| v.as_str()).unwrap_or(""),
                        attrs = token.get("attrs").and_then(|v| v.as_str()).unwrap_or(""),
                    )
                } else if token.get("is_open").and_then(|v| v.as_bool()) == Some(true) {
                    format!(
                        "<{tag} {attrs}>",
                        tag = token.get("tag").and_then(|v| v.as_str()).unwrap_or(""),
                        attrs = token.get("attrs").and_then(|v| v.as_str()).unwrap_or(""),
                    )
                } else {
                    format!(
                        "</{tag}>",
                        tag = token.get("tag").and_then(|v| v.as_str()).unwrap_or(""),
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// The fallback box, as it appears in the rendered output.
    ///
    /// Matched on the whole class list, not just `border-dashed` — several
    /// real renderers use a dashed border for their own empty state.
    const FALLBACK_MARKER: &str = "border-slate-300 p-2 text-xs text-slate-400";

    #[test]
    fn every_palette_available_type_has_a_renderer() {
        // These 13 are the types the IDE palette offers that used to fall
        // back to a labelled box.
        let cases: Vec<(&str, ComponentType)> = vec![
            (
                "Tabs",
                ComponentType::Tabs {
                    tabs: vec![nocode_model::page::TabConfig {
                        key: "a".into(),
                        label: "First".into(),
                        icon: None,
                        disabled: false,
                        closable: false,
                    }],
                    children: Vec::new(),
                },
            ),
            (
                "Accordion",
                ComponentType::Accordion {
                    items: vec![nocode_model::page::AccordionItem {
                        key: "a".into(),
                        label: "Item".into(),
                        icon: None,
                        disabled: false,
                        children: Vec::new(),
                    }],
                    children: Vec::new(),
                },
            ),
            (
                "Chart",
                ComponentType::Chart {
                    chart_type: nocode_model::page::ChartType::Bar,
                    data_source: "orders".into(),
                    config: Box::new(nocode_model::page::ChartConfig::default()),
                },
            ),
            (
                "Tree",
                ComponentType::Tree {
                    data_source: "categories".into(),
                    field_names: nocode_model::page::TreeFieldNames {
                        title: "name".into(),
                        key: "id".into(),
                        children: "children".into(),
                    },
                    checkable: false,
                    draggable: false,
                },
            ),
            (
                "ColorPicker",
                ComponentType::ColorPicker {
                    format: "hex".into(),
                    alpha: false,
                    preset_colors: vec!["#ff0000".into()],
                },
            ),
            (
                "DatePicker",
                ComponentType::DatePicker {
                    format: "YYYY-MM-DD".into(),
                    show_time: false,
                    range: false,
                    disabled_date: None,
                },
            ),
            (
                "TimePicker",
                ComponentType::TimePicker {
                    format: "HH:mm".into(),
                    use12_hours: false,
                    step: nocode_model::page::TimeStep {
                        hour: 1,
                        minute: 0,
                        second: 0,
                    },
                },
            ),
            (
                "Rate",
                ComponentType::Rate {
                    count: 5,
                    allow_half: false,
                    allow_clear: true,
                    character: "*".into(),
                },
            ),
            (
                "Slider",
                ComponentType::Slider {
                    min: 0.0,
                    max: 10.0,
                    step: 1.0,
                    range: false,
                    marks: Default::default(),
                    tooltip: false,
                },
            ),
            (
                "Transfer",
                ComponentType::Transfer {
                    data_source: "users".into(),
                    titles: Vec::new(),
                    operations: Vec::new(),
                    filterable: false,
                },
            ),
            (
                "Cascader",
                ComponentType::Cascader {
                    data_source: "places".into(),
                    field_names: nocode_model::page::CascaderFieldNames {
                        label: "name".into(),
                        value: "id".into(),
                        children: "children".into(),
                    },
                    expand_trigger: "click".into(),
                    change_on_select: false,
                },
            ),
            (
                "TreeSelect",
                ComponentType::TreeSelect {
                    data_source: "places".into(),
                    field_names: nocode_model::page::TreeSelectFieldNames {
                        title: "name".into(),
                        key: "id".into(),
                        children: "children".into(),
                    },
                    tree_default_expand_all: false,
                    show_search: false,
                },
            ),
            (
                "Mention",
                ComponentType::Mention {
                    suggestions: vec!["alice".into()],
                    prefix: "@".into(),
                    split: " ".into(),
                },
            ),
        ];

        for (name, component) in cases {
            let html = render(component);
            assert!(
                !html.contains(FALLBACK_MARKER),
                "{name} still renders the fallback box:\n{html}"
            );
            assert!(!html.is_empty(), "{name} rendered nothing at all");
        }
    }

    #[test]
    fn the_fallback_keeps_its_children() {
        // A fallback used to DROP the component's children entirely, so a
        // Tabs containing a Button lost the Button.
        let button_id = ComponentId::new();
        let mut page = Page::new("Test");
        page.components.push(PageComponent {
            id: ComponentId::new(),
            component: ComponentType::Tabs {
                tabs: Vec::new(),
                children: vec![button_id.clone()],
            },
            props: Default::default(),
            bindings: Vec::new(),
            events: Vec::new(),
            conditions: Vec::new(),
            slots: Default::default(),
            style_overrides: Default::default(),
        });
        page.components.push(PageComponent {
            id: button_id,
            component: ComponentType::Button {
                label: "Save".into(),
                variant: nocode_model::page::ButtonVariant::Primary,
                size: nocode_model::page::ButtonSize::Middle,
                icon: None,
                loading: false,
                disabled: false,
                action: String::new(),
            },
            props: Default::default(),
            bindings: Vec::new(),
            events: Vec::new(),
            conditions: Vec::new(),
            slots: Default::default(),
            style_overrides: Default::default(),
        });

        let tokens = component_tokens(&page.components);
        let text: String = tokens
            .iter()
            .filter_map(|token| token.get("text").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join(" ");

        assert!(
            text.contains("Save"),
            "the nested button was dropped:\n{text}"
        );
    }

    #[test]
    fn the_pickers_use_native_controls() {
        assert!(render(ComponentType::ColorPicker {
            format: "hex".into(),
            alpha: false,
            preset_colors: Vec::new(),
        })
        .contains("type=\"color\""));

        assert!(render(ComponentType::DatePicker {
            format: "YYYY-MM-DD".into(),
            show_time: false,
            range: false,
            disabled_date: None,
        })
        .contains("type=\"date\""));

        assert!(render(ComponentType::DatePicker {
            format: "YYYY-MM-DD HH:mm".into(),
            show_time: true,
            range: false,
            disabled_date: None,
        })
        .contains("type=\"datetime-local\""));

        assert!(render(ComponentType::TimePicker {
            format: "HH:mm".into(),
            use12_hours: false,
            step: nocode_model::page::TimeStep {
                hour: 0,
                minute: 15,
                second: 0,
            },
        })
        .contains("type=\"time\""));

        assert!(render(ComponentType::Slider {
            min: 0.0,
            max: 10.0,
            step: 1.0,
            range: false,
            marks: Default::default(),
            tooltip: false,
        })
        .contains("type=\"range\""));
    }

    #[test]
    fn the_rate_radios_share_one_group_name() {
        // Every radio in a group must have the SAME name, or they are not
        // a group at all and selecting one leaves the others checked.
        let html = render(ComponentType::Rate {
            count: 5,
            allow_half: false,
            allow_clear: true,
            character: "*".into(),
        });

        let names: Vec<&str> = html
            .match_indices("name=\"")
            .map(|(index, _)| {
                let rest = &html[index + 6..];
                &rest[..rest.find('"').expect("closing quote")]
            })
            .collect();

        assert_eq!(names.len(), 5, "expected five radios in:\n{html}");
        assert!(
            names.windows(2).all(|pair| pair[0] == pair[1]),
            "the radios are not one group, they have different names: {names:?}"
        );
    }

    #[test]
    fn an_enum_field_gets_options() {
        let mut project = Project::new("Acme");
        let mut entity = nocode_model::Entity::new("Product");
        entity.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "id".to_string(),
            field_type: nocode_model::entity::EntityFieldType::Uuid,
            nullable: false,
            unique: true,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 0,
        });
        entity.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "status".to_string(),
            field_type: nocode_model::entity::EntityFieldType::Enum {
                values: vec!["draft".to_string(), "live".to_string()],
            },
            nullable: false,
            unique: false,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 1,
        });
        let entity_id = entity.id.clone();
        project.entities.insert(entity_id.clone(), entity);

        let view = entity_view(
            &project.entities[&entity_id],
            &project,
            &nocode_model::DatabaseDriver::SQLite,
        );
        let status = &view["fields"][1];

        assert_eq!(status["field_type"], "enum");
        assert_eq!(status["options"][0]["value"], "draft");
        assert_eq!(status["options"][0]["label"], "draft");
        assert_eq!(status["options"][1]["value"], "live");
        assert_eq!(status["enum_values"][1], "live");
    }

    #[test]
    fn a_relation_field_resolves_its_target_entity() {
        let mut project = Project::new("Acme");

        let mut category = nocode_model::Entity::new("Category");
        category.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "id".to_string(),
            field_type: nocode_model::entity::EntityFieldType::Uuid,
            nullable: false,
            unique: true,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 0,
        });
        category.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "name".to_string(),
            field_type: nocode_model::entity::EntityFieldType::String {
                max_length: Some(80),
            },
            nullable: false,
            unique: false,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 1,
        });
        let category_id = category.id.clone();
        project.entities.insert(category_id.clone(), category);

        let mut product = nocode_model::Entity::new("Product");
        product.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "id".to_string(),
            field_type: nocode_model::entity::EntityFieldType::Uuid,
            nullable: false,
            unique: true,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 0,
        });
        product.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "category_id".to_string(),
            field_type: nocode_model::entity::EntityFieldType::Relation(
                nocode_model::entity::RelationType::BelongsTo {
                    entity_id: category_id.clone(),
                    field: "id".to_string(),
                    on_delete: nocode_model::entity::OnDelete::SetNull,
                    on_update: nocode_model::entity::OnUpdate::Cascade,
                },
            ),
            nullable: true,
            unique: false,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 1,
        });
        let product_id = product.id.clone();
        project.entities.insert(product_id.clone(), product);

        let view = entity_view(
            &project.entities[&product_id],
            &project,
            &nocode_model::DatabaseDriver::SQLite,
        );
        let relation = &view["fields"][1];

        assert_eq!(relation["is_editable_relation"], true);
        assert_eq!(relation["relation_entity_snake"], "category");
        assert_eq!(relation["relation_entity_pascal"], "Category");
        assert_eq!(relation["relation_label_field"], "name");
        assert_eq!(relation["relation_key_field"], "id");
    }

    #[test]
    fn a_relation_to_a_missing_entity_is_not_editable() {
        let mut project = Project::new("Acme");
        let mut product = nocode_model::Entity::new("Product");
        product.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "id".to_string(),
            field_type: nocode_model::entity::EntityFieldType::Uuid,
            nullable: false,
            unique: true,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 0,
        });
        product.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "ghost_id".to_string(),
            field_type: nocode_model::entity::EntityFieldType::Relation(
                nocode_model::entity::RelationType::BelongsTo {
                    entity_id: nocode_model::entity::EntityId::new(),
                    field: "id".to_string(),
                    on_delete: nocode_model::entity::OnDelete::SetNull,
                    on_update: nocode_model::entity::OnUpdate::Cascade,
                },
            ),
            nullable: true,
            unique: false,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 1,
        });
        let product_id = product.id.clone();
        project.entities.insert(product_id.clone(), product);

        let view = entity_view(
            &project.entities[&product_id],
            &project,
            &nocode_model::DatabaseDriver::SQLite,
        );
        let relation = &view["fields"][1];

        assert_eq!(
            relation["is_editable_relation"], false,
            "a relation to an entity that is not in the project cannot be picked"
        );
    }

    #[test]
    fn a_binary_field_is_not_editable() {
        let mut project = Project::new("Acme");
        let mut entity = nocode_model::Entity::new("Attachment");
        entity.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "id".to_string(),
            field_type: nocode_model::entity::EntityFieldType::Uuid,
            nullable: false,
            unique: true,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 0,
        });
        entity.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "blob".to_string(),
            field_type: nocode_model::entity::EntityFieldType::Bytes,
            nullable: true,
            unique: false,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 1,
        });
        let entity_id = entity.id.clone();
        project.entities.insert(entity_id.clone(), entity);

        let view = entity_view(
            &project.entities[&entity_id],
            &project,
            &nocode_model::DatabaseDriver::SQLite,
        );

        assert_eq!(view["fields"][1]["can_edit"], false);
        assert_eq!(view["fields"][0]["can_edit"], true);
    }

    #[test]
    fn a_datetime_field_uses_the_datetime_parser() {
        // `<input type="datetime-local">` gives "YYYY-MM-DDTHH:MM" with no
        // zone, but the backend field is a DateTime<Utc>.
        let mut project = Project::new("Acme");
        let mut entity = nocode_model::Entity::new("Event");
        entity.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "id".to_string(),
            field_type: nocode_model::entity::EntityFieldType::Uuid,
            nullable: false,
            unique: true,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 0,
        });
        entity.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "starts_at".to_string(),
            field_type: nocode_model::entity::EntityFieldType::DateTime,
            nullable: true,
            unique: false,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 1,
        });
        let entity_id = entity.id.clone();
        project.entities.insert(entity_id.clone(), entity);

        let view = entity_view(
            &project.entities[&entity_id],
            &project,
            &nocode_model::DatabaseDriver::SQLite,
        );

        assert_eq!(view["fields"][1]["wasm_ctor"], "parse_datetime_opt");
        assert_eq!(view["fields"][1]["input_type"], "datetime-local");
        assert_eq!(view["needs_chrono"], true);
    }

    #[test]
    fn an_entity_without_dates_does_not_ask_for_chrono() {
        let mut project = Project::new("Acme");
        let mut entity = nocode_model::Entity::new("Thing");
        entity.fields.push(EntityField {
            id: nocode_model::entity::EntityFieldId::new(),
            name: "id".to_string(),
            field_type: nocode_model::entity::EntityFieldType::Uuid,
            nullable: false,
            unique: true,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 0,
        });
        let entity_id = entity.id.clone();
        project.entities.insert(entity_id.clone(), entity);

        let view = entity_view(
            &project.entities[&entity_id],
            &project,
            &nocode_model::DatabaseDriver::SQLite,
        );

        assert_eq!(view["needs_chrono"], false);
    }

    #[test]
    fn an_empty_config_does_not_produce_a_broken_widget() {
        // The IDE's palette starts every component with an empty config, so
        // every renderer has to cope with that.
        for component in [
            ComponentType::Tabs {
                tabs: Vec::new(),
                children: Vec::new(),
            },
            ComponentType::Accordion {
                items: Vec::new(),
                children: Vec::new(),
            },
            ComponentType::Mention {
                suggestions: Vec::new(),
                prefix: "@".into(),
                split: " ".into(),
            },
            ComponentType::Custom {
                component_name: String::new(),
                props: Default::default(),
            },
        ] {
            let html = render(component);
            assert!(!html.is_empty());
            assert!(
                !html.contains("undefined") && !html.contains("null"),
                "an empty config leaked a placeholder value:\n{html}"
            );
        }
    }

    #[test]
    fn a_page_view_carries_its_route_and_components() {
        let mut page = Page::new("Products");
        page.path = "/products".into();
        page.components.push(PageComponent {
            id: ComponentId::new(),
            component: ComponentType::Html {
                content: "Hello".into(),
                sanitize: true,
            },
            props: Default::default(),
            bindings: Vec::new(),
            events: Vec::new(),
            conditions: Vec::new(),
            slots: Default::default(),
            style_overrides: Default::default(),
        });

        let project = Project::new("Acme");
        let view = page_view(&page, &project);

        assert_eq!(
            view.get("route").and_then(|v| v.as_str()),
            Some("/products")
        );
        assert!(view.get("components").is_some());
    }
}
