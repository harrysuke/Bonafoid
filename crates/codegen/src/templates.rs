pub mod backend;
pub mod frontend;

use handlebars::{
    Context, Handlebars, Helper, HelperDef, HelperResult, Output, RenderContext, RenderError,
    ScopedJson,
};
use serde_json::Value;
use std::path::PathBuf;
use walkdir::WalkDir;

/// Register the helper functions used by the codegen templates.
///
/// Templates historically used Jinja/Twig-style filters (`{{x | lower}}`),
/// which Handlebars does not support. These helpers provide the equivalent
/// functionality in Handlebars call syntax (`{{lower x}}`), plus comparison
/// helpers for `{{#if (eq a b)}}` style conditions.
pub fn register_helpers(reg: &mut Handlebars) {
    // Code generation must never HTML-escape output: generic types such as
    // `Option<String>` would otherwise be emitted as `Option&lt;String&gt;`.
    reg.register_escape_fn(handlebars::no_escape);

    reg.register_helper("lower", Box::new(CaseHelper(CaseOp::Lower)));
    reg.register_helper("upper", Box::new(CaseHelper(CaseOp::Upper)));
    reg.register_helper("capitalize", Box::new(CaseHelper(CaseOp::Capitalize)));
    reg.register_helper("snake", Box::new(CaseHelper(CaseOp::Snake)));
    reg.register_helper("pascal", Box::new(CaseHelper(CaseOp::Pascal)));
    reg.register_helper("camel", Box::new(CaseHelper(CaseOp::Camel)));
    reg.register_helper("kebab", Box::new(CaseHelper(CaseOp::Kebab)));
    reg.register_helper("trim", Box::new(CaseHelper(CaseOp::Trim)));
    reg.register_helper("json", Box::new(CaseHelper(CaseOp::Json)));

    reg.register_helper("eq", Box::new(CmpHelper(CmpOp::Eq)));
    reg.register_helper("ne", Box::new(CmpHelper(CmpOp::Ne)));
    reg.register_helper("gt", Box::new(CmpHelper(CmpOp::Gt)));
    reg.register_helper("gte", Box::new(CmpHelper(CmpOp::Gte)));
    reg.register_helper("lt", Box::new(CmpHelper(CmpOp::Lt)));
    reg.register_helper("lte", Box::new(CmpHelper(CmpOp::Lte)));
    reg.register_helper("and", Box::new(CmpHelper(CmpOp::And)));
    reg.register_helper("or", Box::new(CmpHelper(CmpOp::Or)));
    reg.register_helper("not", Box::new(CmpHelper(CmpOp::Not)));
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

    fn populated_project() -> nocode_model::Project {
        use nocode_model::entity::{EntityField, EntityFieldType, FieldId as EntityFieldId};
        use nocode_model::form::{FieldId as FormFieldId, FieldType, FormField};
        use nocode_model::{Entity, Form, Page, Project, Workflow};

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
        entity.fields.push(EntityField {
            id: category_fk.clone(),
            name: "category_id".to_string(),
            field_type: EntityFieldType::Uuid,
            nullable: true,
            unique: false,
            default: None,
            auto_generate: None,
            validation: None,
            comment: None,
            order: 3,
        });

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
            EntityField, EntityFieldType, EntityRelation, FieldId, OnDelete, OnUpdate, RelationId,
            RelationType,
        };
        use nocode_model::{Entity, Project};

        fn uuid_field(name: &str, order: usize, nullable: bool) -> EntityField {
            EntityField {
                id: FieldId::new(),
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
            id: FieldId::new(),
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
