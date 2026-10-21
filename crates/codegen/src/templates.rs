pub mod backend;
pub mod frontend;

use handlebars::{
    Context, Handlebars, Helper, HelperDef, HelperResult, Output, RenderContext, RenderError,
    ScopedJson,
};
use serde_json::Value;
use std::path::PathBuf;
use walkdir::WalkDir;

/// Every helper this module registers, as `(name, definition)` pairs.
///
/// A single table so the names and the registrations cannot drift apart,
/// and so the template lint can enumerate them. `register_helpers`
/// iterates it; `helper_names` exposes just the names.
fn helper_definitions() -> Vec<(&'static str, Box<dyn HelperDef + Send + Sync + 'static>)> {
    vec![
        ("lower", Box::new(CaseHelper(CaseOp::Lower))),
        ("upper", Box::new(CaseHelper(CaseOp::Upper))),
        ("capitalize", Box::new(CaseHelper(CaseOp::Capitalize))),
        ("snake", Box::new(CaseHelper(CaseOp::Snake))),
        ("pascal", Box::new(CaseHelper(CaseOp::Pascal))),
        ("camel", Box::new(CaseHelper(CaseOp::Camel))),
        ("kebab", Box::new(CaseHelper(CaseOp::Kebab))),
        ("trim", Box::new(CaseHelper(CaseOp::Trim))),
        ("json", Box::new(CaseHelper(CaseOp::Json))),
        ("eq", Box::new(CmpHelper(CmpOp::Eq))),
        ("ne", Box::new(CmpHelper(CmpOp::Ne))),
        ("gt", Box::new(CmpHelper(CmpOp::Gt))),
        ("gte", Box::new(CmpHelper(CmpOp::Gte))),
        ("lt", Box::new(CmpHelper(CmpOp::Lt))),
        ("lte", Box::new(CmpHelper(CmpOp::Lte))),
        ("and", Box::new(CmpHelper(CmpOp::And))),
        ("or", Box::new(CmpHelper(CmpOp::Or))),
        ("not", Box::new(CmpHelper(CmpOp::Not))),
    ]
}

/// The names of every registered helper.
///
/// The template lint uses this. A bare `{{snake}}` calls the `snake`
/// helper instead of resolving the field, and renders EMPTY with no error
/// at all — see `no_bare_helper_calls` in the tests.
pub fn helper_names() -> Vec<&'static str> {
    helper_definitions()
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// Register the helper functions used by the codegen templates.
///
/// Templates historically used Jinja/Twig-style filters (`{{x | lower}}`),
/// which Handlebars does not support. These helpers provide the equivalent
/// functionality in Handlebars call syntax (`{{lower x}}`), plus comparison
/// helpers for `{{#if (eq a b)}}` style conditions.
///
/// Note that Handlebars 5 ships `eq`, `ne`, `gt`, `gte`, `lt`, `lte`,
/// `and`, `or` and `not` as built-ins; these shadow them, which is
/// deliberate so the comparison semantics are ours and not the library's.
pub fn register_helpers(reg: &mut Handlebars) {
    // Code generation must never HTML-escape output: generic types such as
    // `Option<String>` would otherwise be emitted as `Option&lt;String&gt;`.
    reg.register_escape_fn(handlebars::no_escape);

    for (name, helper) in helper_definitions() {
        reg.register_helper(name, helper);
    }
}

#[derive(Clone, Copy)]
enum CaseOp {
    Lower,
    Upper,
    Capitalize,
    Snake,
    Pascal,
    Camel,
    Kebab,
    Trim,
    Json,
}

struct CaseHelper(CaseOp);

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Split a string into words on case boundaries and non-alphanumeric separators.
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

fn apply_case(op: CaseOp, value: &Value) -> String {
    let s = value_to_string(value);
    match op {
        CaseOp::Lower => s.to_lowercase(),
        CaseOp::Upper => s.to_uppercase(),
        CaseOp::Trim => s.trim().to_string(),
        CaseOp::Json => serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()),
        CaseOp::Capitalize => {
            let mut chars = s.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => s,
            }
        }
        CaseOp::Snake => words(&s)
            .iter()
            .map(|w| w.to_lowercase())
            .collect::<Vec<_>>()
            .join("_"),
        CaseOp::Kebab => words(&s)
            .iter()
            .map(|w| w.to_lowercase())
            .collect::<Vec<_>>()
            .join("-"),
        CaseOp::Pascal => words(&s)
            .iter()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        CaseOp::Camel => {
            let pascal = apply_case(CaseOp::Pascal, value);
            let mut chars = pascal.chars();
            match chars.next() {
                Some(f) => f.to_lowercase().collect::<String>() + chars.as_str(),
                None => pascal,
            }
        }
    }
}

impl HelperDef for CaseHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        _r: &'reg Handlebars<'reg>,
        _ctx: &'rc Context,
        _rc: &mut RenderContext<'reg, 'rc>,
        out: &mut dyn Output,
    ) -> HelperResult {
        let value = h.param(0).map(|p| p.value().clone()).unwrap_or(Value::Null);
        out.write(&apply_case(self.0, &value))?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum CmpOp {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    And,
    Or,
    Not,
}

struct CmpHelper(CmpOp);

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Number(_) => true,
    }
}

fn num(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn compare(op: CmpOp, h: &Helper) -> bool {
    let a = h.param(0).map(|p| p.value().clone()).unwrap_or(Value::Null);
    let b = h.param(1).map(|p| p.value().clone()).unwrap_or(Value::Null);
    match op {
        CmpOp::Eq => a == b,
        CmpOp::Ne => a != b,
        CmpOp::Gt => num(&a).zip(num(&b)).map(|(x, y)| x > y).unwrap_or(false),
        CmpOp::Gte => num(&a).zip(num(&b)).map(|(x, y)| x >= y).unwrap_or(false),
        CmpOp::Lt => num(&a).zip(num(&b)).map(|(x, y)| x < y).unwrap_or(false),
        CmpOp::Lte => num(&a).zip(num(&b)).map(|(x, y)| x <= y).unwrap_or(false),
        CmpOp::And => h.params().iter().all(|p| truthy(p.value())),
        CmpOp::Or => h.params().iter().any(|p| truthy(p.value())),
        CmpOp::Not => !truthy(&a),
    }
}

impl HelperDef for CmpHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        _r: &'reg Handlebars<'reg>,
        _ctx: &'rc Context,
        _rc: &mut RenderContext<'reg, 'rc>,
    ) -> Result<ScopedJson<'rc>, RenderError> {
        Ok(ScopedJson::Derived(Value::Bool(compare(self.0, h))))
    }

    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        _r: &'reg Handlebars<'reg>,
        _ctx: &'rc Context,
        _rc: &mut RenderContext<'reg, 'rc>,
        out: &mut dyn Output,
    ) -> HelperResult {
        out.write(if compare(self.0, h) { "true" } else { "false" })?;
        Ok(())
    }
}

pub fn register_templates(reg: &mut Handlebars) -> Result<(), handlebars::TemplateError> {
    let template_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/templates");
    for entry in WalkDir::new(&template_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("hbs") {
            if let Ok(rel) = path.strip_prefix(&template_dir) {
                if let Some(name) = rel.to_str() {
                    let name = name.replace("\\", "/").replace(".hbs", "");
                    let content = std::fs::read_to_string(path).unwrap_or_default();
                    reg.register_template_string(&name, content)?;
                }
            }
        }
    }
    Ok(())
}

/// Return every `.hbs` template in the crate as `(relative-name, contents)`.
pub fn all_templates() -> Vec<(String, String)> {
    let template_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/templates");
    let mut out = Vec::new();
    for entry in WalkDir::new(&template_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("hbs") {
            if let Ok(rel) = path.strip_prefix(&template_dir) {
                if let Some(name) = rel.to_str() {
                    let name = name.replace('\\', "/").replace(".hbs", "");
                    if let Ok(content) = std::fs::read_to_string(path) {
                        out.push((name, content));
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_parses() {
        let mut failures = Vec::new();
        for (name, content) in all_templates() {
            let mut reg = Handlebars::new();
            if let Err(err) = reg.register_template_string("t", &content) {
                failures.push(format!("{name}: {err}"));
            }
        }
        assert!(
            failures.is_empty(),
            "templates failed to parse:\n{}",
            failures.join("\n")
        );
    }

    /// Find every `{{helper}}` in `line` — a helper call with NO argument.
    ///
    /// Returns the helper name and the column it was found at.
    ///
    /// A bare `{{snake}}` does not resolve the field called `snake`. It
    /// calls the registered `snake` *helper*, which then converts a missing
    /// argument to the empty string, so the template renders an empty
    /// identifier and nothing anywhere reports an error. It has bitten
    /// this codebase twice: the forms template lost its PascalCase names,
    /// and the API client emitted `submit_` instead of
    /// `submit_products_api` (commit 877e0b3).
    fn bare_helper_calls<'a>(line: &str, helpers: &[&'a str]) -> Vec<(&'a str, usize)> {
        let bytes = line.as_bytes();
        let mut found = Vec::new();
        let mut index = 0;

        while index + 1 < bytes.len() {
            if bytes[index] != b'{' || bytes[index + 1] != b'{' {
                index += 1;
                continue;
            }

            // `{{{` is the unescaped variant; its expression starts one
            // character later.
            let mut expr_start = index + 2;
            if expr_start < bytes.len() && bytes[expr_start] == b'{' {
                expr_start += 1;
            }

            // Find the closing `}}`.
            let rest = &line[expr_start..];
            let Some(end) = rest.find("}}") else {
                break;
            };
            let expr = rest[..end].trim();

            // Strip the block/comment markers so `{{#if ...}}`, `{{/if}}`
            // and `{{! ...}}` are not mistaken for a call.
            let expr = expr
                .trim_start_matches(['#', '/', '!', '>', '^'])
                .trim_start_matches('&')
                .trim();

            // A path like `{{this.snake}}` is a field access, not a call.
            if !expr.contains('.') && !expr.contains(' ') {
                if let Some(name) = helpers.iter().find(|name| **name == expr) {
                    found.push((*name, index));
                }
            }

            index = expr_start + end + 2;
        }

        found
    }

    /// No template may call a registered helper without an argument.
    ///
    /// `{{snake}}` silently renders an empty string where a field was
    /// meant. Catching it here is far cheaper than finding it in a
    /// generated-code compile fixture, or in a user's generated app.
    #[test]
    fn no_bare_helper_calls() {
        let helpers = helper_names();
        assert!(
            !helpers.is_empty(),
            "the helper list is empty, so this lint would pass vacuously"
        );

        let mut failures = Vec::new();
        for (name, content) in all_templates() {
            for (index, line) in content.lines().enumerate() {
                for (helper, column) in bare_helper_calls(line, &helpers) {
                    failures.push(format!(
                        "{name}:{}:{}  `{{{{{helper}}}}}` calls the {helper} helper \
                         instead of resolving the field — write `{{{{this.{helper}}}}}`",
                        index + 1,
                        column + 1,
                    ));
                }
            }
        }

        assert!(
            failures.is_empty(),
            "templates call a helper with no argument:\n{}",
            failures.join("\n")
        );
    }

    /// The lint must actually detect the bug it exists for.
    #[test]
    fn the_bare_helper_lint_detects_a_bare_call() {
        let helpers = helper_names();

        assert_eq!(
            bare_helper_calls("pub async fn submit_{{snake}}(", &helpers),
            vec![("snake", 20)],
            "the column is the 0-based index of the opening brace"
        );
        assert_eq!(
            bare_helper_calls("{{this.snake}}", &helpers),
            Vec::new(),
            "a field access is not a helper call"
        );
        assert_eq!(
            bare_helper_calls("{{snake this.name}}", &helpers),
            Vec::new(),
            "a helper call with an argument is fine"
        );
        assert_eq!(
            bare_helper_calls("{{#if (eq a b)}}", &helpers),
            Vec::new(),
            "a block helper is not a bare call"
        );
        assert_eq!(
            bare_helper_calls("{{/if}}", &helpers),
            Vec::new(),
            "a block close is not a bare call"
        );
        assert_eq!(
            bare_helper_calls("{{! a {{snake}} comment }}", &helpers),
            Vec::new(),
            "a comment is not a bare call"
        );
    }

    fn populated_project() -> nocode_model::Project {
        use nocode_model::entity::{EntityField, EntityFieldId, EntityFieldType};
        use nocode_model::form::{FieldType, FormField, FormFieldId};
        use nocode_model::page::{
            ActionConfig, ActionType, BindingSource, BindingTarget, ComponentType, DataSource,
            DataSourceConfig, DataSourceType, Page, PageAction, PageComponent,
        };
        use nocode_model::{Entity, Form, Project, Workflow};

        let mut project = Project::new("Acme Portal");
        project.description = Some("A demo application".to_string());

        let mut category = Entity::new("Category");
        category.fields.push(EntityField {
            id: EntityFieldId::new(),
            name: "id".to_string(),
            field_type: EntityFieldType::Uuid,
            nullable: false,
            unique: true,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 0,
        });
        category.fields.push(EntityField {
            id: EntityFieldId::new(),
            name: "name".to_string(),
            field_type: EntityFieldType::String {
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

        let mut entity = Entity::new("Product");
        entity.fields.push(EntityField {
            id: EntityFieldId::new(),
            name: "id".to_string(),
            field_type: EntityFieldType::Uuid,
            nullable: false,
            unique: true,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 0,
        });
        entity.fields.push(EntityField {
            id: EntityFieldId::new(),
            name: "name".to_string(),
            field_type: EntityFieldType::String {
                max_length: Some(120),
            },
            nullable: false,
            unique: false,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 1,
        });
        entity.fields.push(EntityField {
            id: EntityFieldId::new(),
            name: "price".to_string(),
            field_type: EntityFieldType::Decimal {
                precision: 10,
                scale: 2,
            },
            nullable: true,
            unique: false,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 2,
        });
        let category_fk = EntityFieldId::new();
        // A relation field: the IDE stores the target entity and field on
        // the field itself, so the generated page can offer the target's
        // records in a picker instead of asking for a raw id.
        entity.fields.push(EntityField {
            id: category_fk.clone(),
            name: "category_id".to_string(),
            field_type: EntityFieldType::Relation(nocode_model::entity::RelationType::BelongsTo {
                entity_id: category.id.clone(),
                field: "id".to_string(),
                on_delete: nocode_model::entity::OnDelete::SetNull,
                on_update: nocode_model::entity::OnUpdate::Cascade,
            }),
            nullable: true,
            unique: false,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 3,
        });

        // The remaining field types, so every branch of the generated
        // entity form is exercised by the compile fixture.
        let mut extra_order = 4;
        let mut push_field =
            |name: &str, field_type: EntityFieldType, nullable: bool, order: &mut usize| {
                entity.fields.push(EntityField {
                    id: EntityFieldId::new(),
                    name: name.to_string(),
                    field_type,
                    nullable,
                    unique: false,
                    default: None,
                    auto_generate: None,
                    validation: None,
                    comment: None,
                    order: *order,
                });
                *order += 1;
            };
        push_field("description", EntityFieldType::Text, true, &mut extra_order);
        push_field(
            "in_stock",
            EntityFieldType::Boolean,
            false,
            &mut extra_order,
        );
        push_field(
            "quantity",
            EntityFieldType::Integer,
            false,
            &mut extra_order,
        );
        push_field("weight", EntityFieldType::Float, true, &mut extra_order);
        push_field("released_on", EntityFieldType::Date, true, &mut extra_order);
        push_field(
            "published_at",
            EntityFieldType::DateTime,
            true,
            &mut extra_order,
        );
        push_field(
            "available_at",
            EntityFieldType::Time,
            true,
            &mut extra_order,
        );
        push_field("metadata", EntityFieldType::Json, true, &mut extra_order);
        push_field(
            "status",
            EntityFieldType::Enum {
                values: vec!["draft".to_string(), "live".to_string()],
            },
            false,
            &mut extra_order,
        );
        push_field("thumbnail", EntityFieldType::Bytes, true, &mut extra_order);

        let category_id = category.id.clone();
        let category_pk = category.fields[0].id.clone();
        entity.relations.push(nocode_model::entity::EntityRelation {
            id: nocode_model::entity::RelationId::new(),
            name: "Category".to_string(),
            relation_type: nocode_model::entity::RelationType::BelongsTo {
                entity_id: category_id,
                field: "id".to_string(),
                on_delete: nocode_model::entity::OnDelete::SetNull,
                on_update: nocode_model::entity::OnUpdate::Cascade,
            },
            from_field: category_fk,
            to_entity: category.id.clone(),
            to_field: category_pk,
            on_delete: nocode_model::entity::OnDelete::SetNull,
            on_update: nocode_model::entity::OnUpdate::Cascade,
        });

        project.entities.insert(category.id.clone(), category);
        project.entities.insert(entity.id.clone(), entity);

        let mut form = Form::new("ContactForm");
        form.fields.push(FormField {
            id: FormFieldId::new(),
            name: "email".to_string(),
            label: "Email".to_string(),
            field_type: FieldType::Email,
            required: true,
            default_value: None,
            validation_rules: Vec::new(),
            conditional_visibility: None,
            ui_props: Default::default(),
            order: 0,
        });
        form.fields.push(FormField {
            id: FormFieldId::new(),
            name: "message".to_string(),
            label: "Message".to_string(),
            field_type: FieldType::TextArea {
                max_length: None,
                rows: Some(4),
            },
            required: false,
            default_value: None,
            validation_rules: Vec::new(),
            conditional_visibility: None,
            ui_props: Default::default(),
            order: 1,
        });
        project.forms.insert(form.id.clone(), form);

        let workflow = Workflow::new("Approval");
        project.workflows.insert(workflow.id.clone(), workflow);

        let page = Page::new("Dashboard");
        project.pages.insert(page.id.clone(), page);

        // Page with data sources and actions
        let mut products_page = Page::new("Products");
        products_page.path = "/products".to_string();
        products_page.title = "Products".to_string();
        products_page.description = Some("Product catalog with live data".to_string());

        // Add a data source
        let ds_id = "products_api".to_string();
        products_page.data_sources.push(DataSource {
            id: ds_id.clone(),
            name: "Products API".to_string(),
            source_type: DataSourceType::Api {
                endpoint: "/api/v1/products".to_string(),
                method: nocode_model::workflow::HttpMethod::Get,
                headers: Default::default(),
                params: Default::default(),
                body: None,
            },
            config: DataSourceConfig {
                timeout_ms: 5000,
                retry: Default::default(),
                cache: Default::default(),
                debounce_ms: None,
                transform_response: None,
                transform_error: None,
            },
            auto_load: true,
            polling_interval: None,
            transform: None,
        });

        // Add an action
        products_page.actions.push(PageAction {
            id: nocode_model::page::ActionId::new(),
            name: "RefreshProducts".to_string(),
            action_type: ActionType::CallApi,
            config: ActionConfig::CallApi {
                data_source_id: ds_id.clone(),
                params: Default::default(),
                on_success: vec![],
                on_error: vec![],
            },
            permissions: vec![],
        });

        // Add components (StatCard, Table, Button)
        let stat_card = PageComponent {
            id: nocode_model::page::ComponentId::new(),
            component: ComponentType::StatCard {
                label: "Total Products".to_string(),
                value: "0".to_string(),
                trend: None,
                icon: None,
            },
            props: Default::default(),
            bindings: vec![],
            events: vec![],
            conditions: vec![],
            slots: Default::default(),
            style_overrides: Default::default(),
        };

        let table = PageComponent {
            id: nocode_model::page::ComponentId::new(),
            component: ComponentType::Table {
                columns: vec![
                    nocode_model::page::TableColumn {
                        key: "id".to_string(),
                        title: "ID".to_string(),
                        data_index: "id".to_string(),
                        width: None,
                        min_width: None,
                        max_width: None,
                        fixed: None,
                        align: None,
                        ellipsis: false,
                        sortable: true,
                        filterable: false,
                        render: None,
                        children: vec![],
                    },
                    nocode_model::page::TableColumn {
                        key: "name".to_string(),
                        title: "Name".to_string(),
                        data_index: "name".to_string(),
                        width: None,
                        min_width: None,
                        max_width: None,
                        fixed: None,
                        align: None,
                        ellipsis: false,
                        sortable: true,
                        filterable: true,
                        render: None,
                        children: vec![],
                    },
                ],
                data_source: ds_id.clone(),
                pagination: Default::default(),
                sorting: true,
                filtering: true,
                selection: Default::default(),
            },
            props: Default::default(),
            bindings: vec![nocode_model::page::DataBinding {
                target: BindingTarget::Prop("data".to_string()),
                source: BindingSource::DataSource {
                    source_id: ds_id.clone(),
                    path: "data".to_string(),
                },
                transform: None,
                two_way: false,
            }],
            events: vec![],
            conditions: vec![],
            slots: Default::default(),
            style_overrides: Default::default(),
        };

        let refresh_button = PageComponent {
            id: nocode_model::page::ComponentId::new(),
            component: ComponentType::Button {
                label: "Refresh".to_string(),
                variant: nocode_model::page::ButtonVariant::Primary,
                size: nocode_model::page::ButtonSize::Middle,
                icon: None,
                loading: false,
                disabled: false,
                action: "RefreshProducts".to_string(),
            },
            props: Default::default(),
            bindings: vec![],
            events: vec![],
            conditions: vec![],
            slots: Default::default(),
            style_overrides: Default::default(),
        };

        products_page.components.push(stat_card);
        products_page.components.push(table);
        products_page.components.push(refresh_button);

        project
            .pages
            .insert(products_page.id.clone(), products_page);

        // A page that exercises every component type the palette offers
        // which has a dedicated renderer. The compile fixture then proves
        // the generated Leptos `view!` macro is valid for all of them,
        // not just the handful the first page used.
        let mut gallery_page = Page::new("Gallery");
        gallery_page.path = "/gallery".to_string();
        gallery_page.title = "Gallery".to_string();

        let mut gallery_components: Vec<PageComponent> = Vec::new();
        let mut push = |component: ComponentType| {
            gallery_components.push(PageComponent {
                id: nocode_model::page::ComponentId::new(),
                component,
                props: Default::default(),
                bindings: vec![],
                events: vec![],
                conditions: vec![],
                slots: Default::default(),
                style_overrides: Default::default(),
            });
        };

        push(ComponentType::Tabs {
            tabs: vec![nocode_model::page::TabConfig {
                key: "one".to_string(),
                label: "One".to_string(),
                icon: None,
                disabled: false,
                closable: false,
            }],
            children: vec![],
        });
        push(ComponentType::Accordion {
            items: vec![nocode_model::page::AccordionItem {
                key: "one".to_string(),
                label: "Details".to_string(),
                icon: None,
                disabled: false,
                children: vec![],
            }],
            children: vec![],
        });
        push(ComponentType::Chart {
            chart_type: nocode_model::page::ChartType::Bar,
            data_source: "products_api".to_string(),
            config: Box::default(),
        });
        push(ComponentType::Tree {
            data_source: "products_api".to_string(),
            field_names: nocode_model::page::TreeFieldNames {
                title: "name".to_string(),
                key: "id".to_string(),
                children: "children".to_string(),
            },
            checkable: false,
            draggable: false,
        });
        push(ComponentType::ColorPicker {
            format: "hex".to_string(),
            alpha: true,
            preset_colors: vec!["#ff0000".to_string()],
        });
        push(ComponentType::DatePicker {
            format: "YYYY-MM-DD".to_string(),
            show_time: true,
            range: true,
            disabled_date: None,
        });
        push(ComponentType::TimePicker {
            format: "HH:mm".to_string(),
            use12_hours: false,
            step: nocode_model::page::TimeStep {
                hour: 0,
                minute: 15,
                second: 0,
            },
        });
        push(ComponentType::Rate {
            count: 5,
            allow_half: false,
            allow_clear: true,
            character: "*".to_string(),
        });
        push(ComponentType::Slider {
            min: 0.0,
            max: 100.0,
            step: 1.0,
            range: false,
            marks: Default::default(),
            tooltip: false,
        });
        push(ComponentType::Transfer {
            data_source: "products_api".to_string(),
            titles: vec!["Available".to_string(), "Selected".to_string()],
            operations: vec![],
            filterable: false,
        });
        push(ComponentType::Cascader {
            data_source: "products_api".to_string(),
            field_names: nocode_model::page::CascaderFieldNames {
                label: "name".to_string(),
                value: "id".to_string(),
                children: "children".to_string(),
            },
            expand_trigger: "click".to_string(),
            change_on_select: false,
        });
        push(ComponentType::TreeSelect {
            data_source: "products_api".to_string(),
            field_names: nocode_model::page::TreeSelectFieldNames {
                title: "name".to_string(),
                key: "id".to_string(),
                children: "children".to_string(),
            },
            tree_default_expand_all: false,
            show_search: false,
        });
        push(ComponentType::Mention {
            suggestions: vec!["alice".to_string()],
            prefix: "@".to_string(),
            split: " ".to_string(),
        });
        push(ComponentType::Custom {
            component_name: "MyWidget".to_string(),
            props: Default::default(),
        });

        gallery_page.components.extend(gallery_components);
        project.pages.insert(gallery_page.id.clone(), gallery_page);

        project
    }

    /// Extract the body of `pub struct <name> { ... }` from generated code.
    fn struct_body<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source
            .find(&format!("pub struct {name} {{"))
            .unwrap_or_else(|| panic!("struct {name} not found"));
        let rest = &source[start..];
        let end = rest.find('}').expect("struct end");
        &rest[..end]
    }

    /// Two entities joined by a `BelongsTo` relation: Order.customer_id -> Customer.id.
    fn project_with_relation() -> nocode_model::Project {
        use nocode_model::entity::{
            EntityField, EntityFieldId, EntityFieldType, EntityRelation, OnDelete, OnUpdate,
            RelationId, RelationType,
        };
        use nocode_model::{Entity, Project};

        fn uuid_field(name: &str, order: usize, nullable: bool) -> EntityField {
            EntityField {
                id: EntityFieldId::new(),
                name: name.to_string(),
                field_type: EntityFieldType::Uuid,
                nullable,
                unique: false,
                default: None,
                auto_generate: None,
                validation: None,
                comment: None,
                order,
            }
        }

        let mut project = Project::new("Acme Portal");

        let mut customer = Entity::new("Customer");
        customer.fields.push(uuid_field("id", 0, false));
        customer.fields.push(EntityField {
            id: EntityFieldId::new(),
            name: "name".to_string(),
            field_type: EntityFieldType::String {
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

        let mut order = Entity::new("Order");
        order.fields.push(uuid_field("id", 0, false));
        let customer_fk = uuid_field("customer_id", 1, true);
        let from_field = customer_fk.id.clone();
        order.fields.push(customer_fk);

        order.relations.push(EntityRelation {
            id: RelationId::new(),
            name: "Customer".to_string(),
            relation_type: RelationType::BelongsTo {
                entity_id: customer.id.clone(),
                field: "id".to_string(),
                on_delete: OnDelete::Cascade,
                on_update: OnUpdate::Cascade,
            },
            from_field,
            to_entity: customer.id.clone(),
            to_field: customer.fields[0].id.clone(),
            on_delete: OnDelete::Cascade,
            on_update: OnUpdate::Cascade,
        });

        project.entities.insert(customer.id.clone(), customer);
        project.entities.insert(order.id.clone(), order);
        project
    }

    /// A relation must become a real foreign key, not just a CREATE TABLE.
    #[test]
    fn relations_become_foreign_keys_in_the_ddl() {
        use nocode_model::DatabaseDriver;

        let project = project_with_relation();
        let sql = crate::database::generate_schema_sql(&project, DatabaseDriver::Postgres).unwrap();

        assert!(
            sql.contains(
                "ALTER TABLE orders ADD CONSTRAINT fk_orders_customer_id FOREIGN KEY \
                 (customer_id) REFERENCES customers (id) ON DELETE CASCADE ON UPDATE CASCADE;"
            ),
            "foreign key missing from schema:\n{sql}"
        );
    }

    /// SQLite cannot `ALTER TABLE ... ADD CONSTRAINT`, so its FKs are inlined.
    #[test]
    fn sqlite_inlines_foreign_keys_in_create_table() {
        use nocode_model::DatabaseDriver;

        let project = project_with_relation();
        let sql = crate::database::generate_schema_sql(&project, DatabaseDriver::SQLite).unwrap();

        assert!(
            sql.contains(
                "FOREIGN KEY (customer_id) REFERENCES customers (id) \
                 ON DELETE CASCADE ON UPDATE CASCADE"
            ),
            "inlined foreign key missing from schema:\n{sql}"
        );
        assert!(
            !sql.contains("ALTER TABLE orders ADD CONSTRAINT"),
            "SQLite must not emit ALTER TABLE ADD CONSTRAINT:\n{sql}"
        );
    }

    /// A relation whose target entity was deleted must not emit a dangling FK.
    #[test]
    fn relations_to_a_deleted_entity_are_skipped() {
        use nocode_model::entity::EntityId;
        use nocode_model::DatabaseDriver;

        let mut project = project_with_relation();
        let order = project
            .entities
            .values_mut()
            .find(|e| e.name == "Order")
            .expect("order entity");
        order.relations[0].to_entity = EntityId::new();

        let sql = crate::database::generate_schema_sql(&project, DatabaseDriver::Postgres).unwrap();
        assert!(
            !sql.contains("FOREIGN KEY"),
            "dangling relation emitted a foreign key:\n{sql}"
        );
    }

    /// ManyToMany needs a join table the generator does not emit yet, so it must
    /// produce neither a foreign key nor a broken `#[sea_orm]` attribute.
    #[tokio::test]
    async fn many_to_many_relations_are_skipped() {
        use nocode_model::entity::RelationType;
        use nocode_model::DatabaseDriver;

        let mut project = project_with_relation();
        let order = project
            .entities
            .values_mut()
            .find(|e| e.name == "Order")
            .expect("order entity");
        order.relations[0].relation_type = RelationType::ManyToMany {
            entity_id: order.relations[0].to_entity.clone(),
            through: None,
            field: "id".to_string(),
            related_field: "id".to_string(),
        };

        let sql = crate::database::generate_schema_sql(&project, DatabaseDriver::Postgres).unwrap();
        assert!(
            !sql.contains("FOREIGN KEY"),
            "many-to-many emitted a direct foreign key:\n{sql}"
        );

        let backend = crate::backend::generate_backend(&project, &std::env::temp_dir())
            .await
            .expect("backend generation runs");
        let model = backend
            .files
            .get(&std::path::PathBuf::from("src/models/order.rs"))
            .expect("order model generated");
        assert!(
            !model.contains("belongs_to = ")
                && !model.contains("has_one = ")
                && !model.contains("has_many = "),
            "many-to-many emitted a relation attribute:\n{model}"
        );
    }

    /// The generated SeaORM model must carry the relation, not an empty enum.
    #[tokio::test]
    async fn generated_model_emits_seaorm_relations() {
        let project = project_with_relation();
        let backend = crate::backend::generate_backend(&project, &std::env::temp_dir())
            .await
            .expect("backend generation runs");

        let model = backend
            .files
            .get(&std::path::PathBuf::from("src/models/order.rs"))
            .expect("order model generated");

        assert!(
            model.contains("pub enum Relation {"),
            "relation enum missing:\n{model}"
        );
        assert!(
            model.contains("belongs_to = \"super::customer::Entity\""),
            "belongs_to attribute missing:\n{model}"
        );
        assert!(
            model.contains("from = \"Column::CustomerId\""),
            "from column missing:\n{model}"
        );
        assert!(
            model.contains("to = \"super::customer::Column::Id\""),
            "to column missing:\n{model}"
        );
        assert!(
            model.contains("impl Related<super::customer::Entity> for Entity"),
            "Related impl missing:\n{model}"
        );
        assert!(
            model.contains("Relation::Customer.def()"),
            "Related impl body missing:\n{model}"
        );
    }

    /// The generated backend must expose a submission handler per form.
    #[tokio::test]
    async fn generated_backend_exposes_form_submission_handlers() {
        let project = populated_project();
        let backend = crate::backend::generate_backend(&project, &std::env::temp_dir())
            .await
            .expect("backend generation runs");

        let handler = backend
            .files
            .get(&std::path::PathBuf::from(
                "src/handlers/forms/contact_form.rs",
            ))
            .expect("contact_form handler generated");

        assert!(
            handler.contains("pub struct SubmitContactForm {"),
            "submission struct missing:\n{handler}"
        );
        assert!(
            handler.contains("pub email: String,"),
            "required field should be non-optional String:\n{handler}"
        );
        assert!(
            handler.contains("pub message: Option<String>,"),
            "optional field should be Option<String>:\n{handler}"
        );
        assert!(
            handler.contains("pub async fn submit("),
            "submit handler missing:\n{handler}"
        );

        let routes = backend
            .files
            .get(&std::path::PathBuf::from("src/routes.rs"))
            .expect("routes generated");
        assert!(
            routes.contains(".nest(\"/api/v1/forms\", forms_routes())"),
            "forms route not nested:\n{routes}"
        );
        assert!(
            routes.contains("post(handlers::forms::contact_form::submit)"),
            "form submit route missing:\n{routes}"
        );

        let handlers_mod = backend
            .files
            .get(&std::path::PathBuf::from("src/handlers/mod.rs"))
            .expect("handlers mod generated");
        assert!(
            handlers_mod.contains("pub mod forms;"),
            "forms module not declared:\n{handlers_mod}"
        );
    }

    /// The generated frontend must route to and render a page per form.
    #[tokio::test]
    async fn generated_frontend_renders_form_pages() {
        let project = populated_project();
        let frontend = crate::frontend::generate_frontend(&project, &std::env::temp_dir())
            .await
            .expect("frontend generation runs");

        let page = frontend
            .files
            .get(&std::path::PathBuf::from("src/pages/form_contact_form.rs"))
            .expect("form page generated");
        assert!(
            page.contains("pub fn ContactForm() -> impl IntoView"),
            "form component missing:\n{page}"
        );
        assert!(
            page.contains("api::submit_contact_form(&payload)"),
            "form submit call missing:\n{page}"
        );
        assert!(
            page.contains("on:submit=on_submit"),
            "submit handler not wired:\n{page}"
        );

        let api = frontend
            .files
            .get(&std::path::PathBuf::from("src/api.rs"))
            .expect("api client generated");
        assert!(
            api.contains("pub async fn submit_contact_form("),
            "form api client missing:\n{api}"
        );
        assert!(
            api.contains("/api/v1/forms/contact_form"),
            "form api path missing:\n{api}"
        );

        let pages_mod = frontend
            .files
            .get(&std::path::PathBuf::from("src/pages/mod.rs"))
            .expect("pages mod generated");
        assert!(
            pages_mod.contains("pub mod form_contact_form;"),
            "form page module not declared:\n{pages_mod}"
        );

        let app = frontend
            .files
            .get(&std::path::PathBuf::from("src/app.rs"))
            .expect("app generated");
        assert!(
            app.contains("<Route path=path!(\"/forms/contact_form\") view=pages::form_contact_form::ContactForm />"),
            "form route missing:\n{app}"
        );
    }

    /// A form with no fields must still generate valid, empty handlers and pages.
    #[tokio::test]
    async fn empty_form_generates_empty_payload() {
        let mut project = populated_project();
        let form = project.forms.values_mut().next().expect("fixture form");
        form.fields.clear();

        let backend = crate::backend::generate_backend(&project, &std::env::temp_dir())
            .await
            .expect("backend generation runs");
        let frontend = crate::frontend::generate_frontend(&project, &std::env::temp_dir())
            .await
            .expect("frontend generation runs");

        let handler = backend
            .files
            .get(&std::path::PathBuf::from(
                "src/handlers/forms/contact_form.rs",
            ))
            .expect("handler generated");
        assert!(
            !handler.contains("pub email:") && !handler.contains("pub message:"),
            "empty form should emit an empty payload:\n{handler}"
        );
        let page = frontend
            .files
            .get(&std::path::PathBuf::from("src/pages/form_contact_form.rs"))
            .expect("page generated");
        assert!(
            page.contains("pub fn ContactForm()"),
            "empty form page should still render:\n{page}"
        );
    }

    /// The Update payload must be `Option<base>`, never `Option<Option<T>>`.
    #[tokio::test]
    async fn update_payload_is_option_of_base_type() {
        let project = project_with_relation();
        let backend = crate::backend::generate_backend(&project, &std::env::temp_dir())
            .await
            .expect("backend generation runs");

        let model = backend
            .files
            .get(&std::path::PathBuf::from("src/models/order.rs"))
            .expect("order model generated");

        assert!(
            model.contains("pub struct UpdateOrder {"),
            "update struct missing:\n{model}"
        );
        assert!(
            model.contains("pub customer_id: Option<Uuid>,"),
            "nullable field should be Option<Uuid>:\n{model}"
        );
        assert!(
            !struct_body(model, "UpdateOrder").contains("Option<Option<"),
            "Update payload double-wraps an already-optional type:\n{model}"
        );
    }

    #[tokio::test]
    async fn populated_project_renders_every_generator() {
        let project = populated_project();
        let dir = std::env::temp_dir();

        let backend = crate::backend::generate_backend(&project, &dir).await;
        assert!(
            backend.is_ok(),
            "backend generation failed: {:?}",
            backend.err()
        );
        assert!(!backend.unwrap().files.is_empty());

        let frontend = crate::frontend::generate_frontend(&project, &dir).await;
        assert!(
            frontend.is_ok(),
            "frontend generation failed: {:?}",
            frontend.err()
        );
        assert!(!frontend.unwrap().files.is_empty());

        let database = crate::database::generate_database(&project, &dir).await;
        assert!(
            database.is_ok(),
            "database generation failed: {:?}",
            database.err()
        );
        assert!(!database.unwrap().schema_sql.is_empty());
    }

    /// The active theme must reach the generated Tailwind config and CSS.
    #[tokio::test]
    async fn active_theme_tokens_reach_the_frontend_assets() {
        use nocode_model::theme::{ColorScale, Theme};

        let mut project = populated_project();
        let mut theme = Theme::new("Brand");
        theme.tokens.colors.primary = ColorScale {
            base: "#ff00aa".to_string(),
            scale: [(500, "#ff00aa".to_string())].into_iter().collect(),
            contrast_text: Some("#ffffff".to_string()),
        };
        theme.tokens.typography.font_families.sans = "Brand Sans".to_string();
        theme.global_styles = "body { letter-spacing: 0.01em; }".to_string();
        let theme_id = theme.id.clone();
        project.themes.insert(theme_id.clone(), theme);
        project.active_theme_id = Some(theme_id);

        let frontend = crate::frontend::generate_frontend(&project, &std::env::temp_dir())
            .await
            .expect("frontend generation runs");

        let tailwind = frontend
            .files
            .get(&std::path::PathBuf::from("tailwind.config.js"))
            .expect("tailwind config generated");
        assert!(
            tailwind.contains("\"#ff00aa\""),
            "primary base missing from tailwind config:\n{tailwind}"
        );
        assert!(
            tailwind.contains("\"Brand Sans\""),
            "font family missing from tailwind config"
        );

        let css = frontend
            .files
            .get(&std::path::PathBuf::from("style/input.css"))
            .expect("input.css generated");
        assert!(
            css.contains("--nc-color-primary-500: #ff00aa"),
            "primary shade missing from css:\n{css}"
        );
        assert!(
            css.contains("--nc-font-sans: Brand Sans"),
            "font variable missing from css"
        );
        assert!(
            css.contains("letter-spacing: 0.01em"),
            "global styles missing from css"
        );
    }

    /// Without a theme the frontend still builds, using Tailwind's palette.
    #[tokio::test]
    async fn projects_without_a_theme_keep_the_default_palette() {
        let project = populated_project();
        assert!(project.active_theme_id.is_none());

        let frontend = crate::frontend::generate_frontend(&project, &std::env::temp_dir())
            .await
            .expect("frontend generation runs");

        let tailwind = frontend
            .files
            .get(&std::path::PathBuf::from("tailwind.config.js"))
            .expect("tailwind config generated");
        assert!(tailwind.contains("extend: {}"), "tailwind default kept");
        let css = frontend
            .files
            .get(&std::path::PathBuf::from("style/input.css"))
            .expect("input.css generated");
        assert!(!css.contains("--nc-color"), "no token vars without a theme");
    }

    #[test]
    fn schema_sql_is_generated_for_each_driver() {
        use nocode_model::DatabaseDriver;

        let mut project = populated_project();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gen-schema");
        std::fs::create_dir_all(&dir).unwrap();

        for driver in [DatabaseDriver::SQLite, DatabaseDriver::Postgres] {
            project.settings.database.driver = driver.clone();
            let sql = crate::database::generate_schema_sql(&project, driver.clone()).unwrap();
            assert!(sql.contains("CREATE TABLE"), "{:?} schema empty", driver);

            let name = match driver {
                DatabaseDriver::SQLite => "sqlite",
                _ => "postgres",
            };
            std::fs::write(dir.join(format!("{name}.sql")), sql).unwrap();
        }
    }

    /// End-to-end check that the generated backend is a valid Rust crate.
    ///
    /// Ignored by default because it invokes `cargo check` and therefore needs
    /// network access (to fetch dependencies) and a few minutes.
    ///
    /// Run with: `cargo test -p nocode-codegen -- --ignored generated_backend_compiles`
    #[tokio::test]
    #[ignore]
    async fn generated_backend_compiles() {
        let mut project = populated_project();

        let sqlite = std::env::var("NOCODE_TEST_DRIVER").as_deref() == Ok("sqlite");
        let sub = if sqlite {
            "gen-check-sqlite"
        } else {
            "gen-check"
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(sub)
            .join("backend");

        if sqlite {
            project.settings.database.driver = nocode_model::DatabaseDriver::SQLite;
            project.settings.database.url =
                format!("sqlite://{}?mode=rwc", dir.join("test.db").display());
        }

        // Preserve `target/` so repeat runs reuse compiled dependencies
        // (fresh builds are prone to transient file locks on Windows).
        let _ = std::fs::remove_dir_all(dir.join("src"));
        let _ = std::fs::remove_dir_all(dir.join("migrations"));
        std::fs::create_dir_all(&dir).unwrap();

        let backend = crate::backend::generate_backend(&project, &dir)
            .await
            .unwrap();
        for (path, content) in &backend.files {
            let full = dir.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, content).unwrap();
        }

        let status = std::process::Command::new("cargo")
            .arg("check")
            .arg("--quiet")
            .current_dir(&dir)
            .status()
            .expect("failed to run cargo check");

        assert!(
            status.success(),
            "generated backend failed to compile at {}",
            dir.display()
        );
    }

    /// A bare `{{snake}}` in a template invokes the registered `snake` *helper*
    /// instead of resolving the field, which silently produced empty names
    /// (`submit_` instead of `submit_products_api`). Guard the affected names.
    #[tokio::test]
    async fn data_source_client_functions_use_the_real_id() {
        let project = populated_project();
        let frontend = crate::frontend::generate_frontend(&project, &std::env::temp_dir())
            .await
            .expect("frontend generation runs");

        let api = frontend
            .files
            .get(&std::path::PathBuf::from("src/api.rs"))
            .expect("api client generated");

        assert!(
            api.contains("pub async fn submit_products_api("),
            "data source client function is missing or misnamed:\n{api}"
        );
        assert!(
            !api.contains("pub async fn submit_("),
            "data source client function has an empty name:\n{api}"
        );
    }

    /// End-to-end check that the generated Leptos frontend is a valid
    /// `wasm32-unknown-unknown` crate.
    ///
    /// Ignored by default because it invokes `cargo check` and therefore needs
    /// network access (to fetch dependencies) and a few minutes.
    ///
    /// Run with: `cargo test -p nocode-codegen -- --ignored generated_frontend_compiles`
    #[tokio::test]
    #[ignore]
    async fn generated_frontend_compiles() {
        let project = populated_project();

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join("gen-check-frontend")
            .join("frontend");

        // Preserve `target/` so repeat runs reuse compiled dependencies
        // (fresh builds are prone to transient file locks on Windows).
        let _ = std::fs::remove_dir_all(dir.join("src"));
        std::fs::create_dir_all(&dir).unwrap();

        let frontend = crate::frontend::generate_frontend(&project, &dir)
            .await
            .unwrap();
        for (path, content) in &frontend.files {
            let full = dir.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, content).unwrap();
        }

        let status = std::process::Command::new("cargo")
            .arg("check")
            .arg("--quiet")
            .arg("--target")
            .arg("wasm32-unknown-unknown")
            .current_dir(&dir)
            .status()
            .expect("failed to run cargo check");

        assert!(
            status.success(),
            "generated frontend failed to compile at {}",
            dir.display()
        );
    }
}
