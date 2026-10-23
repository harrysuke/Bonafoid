use anyhow::Result;
use handlebars::Handlebars;
use nocode_model::{DatabaseDriver, Project};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::view;

/// Generate the backend application for a project.
pub async fn generate_backend(
    project: &Project,
    _output_dir: &std::path::Path,
) -> Result<BackendOutput> {
    let mut reg = Handlebars::new();
    register_templates(&mut reg)?;
    crate::templates::register_helpers(&mut reg);

    let driver = project.settings.database.driver.clone();
    let project_ctx = view::project_view(project, &driver);
    let entities = view::entities_view(project, &driver);
    let forms = view::forms_view(project);

    let mut files = HashMap::new();

    let ctx = json!({
        "project": &project_ctx,
        "entities": &entities,
        "forms": &forms,
        "has_forms": !forms.is_empty()
    });

    files.insert(PathBuf::from("Cargo.toml"), generate_cargo_toml(project)?);

    files.insert(
        PathBuf::from("src/main.rs"),
        reg.render("backend/main", &ctx)?,
    );
    files.insert(
        PathBuf::from("src/config.rs"),
        reg.render("backend/config", &ctx)?,
    );
    files.insert(PathBuf::from("src/db.rs"), reg.render("backend/db", &ctx)?);
    files.insert(
        PathBuf::from("src/error.rs"),
        reg.render("backend/error", &json!({}))?,
    );
    files.insert(
        PathBuf::from("src/routes.rs"),
        reg.render("backend/routes", &ctx)?,
    );

    // Models
    files.insert(
        PathBuf::from("src/models/mod.rs"),
        reg.render("backend/models_mod", &ctx)?,
    );

    // Handlers
    files.insert(
        PathBuf::from("src/handlers/mod.rs"),
        reg.render("backend/handlers_mod", &ctx)?,
    );

    // Form submission handlers
    files.insert(
        PathBuf::from("src/handlers/forms/mod.rs"),
        reg.render("backend/forms_mod", &ctx)?,
    );
    for form in &forms {
        let snake = form.get("snake").and_then(|v| v.as_str()).unwrap_or("form");
        files.insert(
            PathBuf::from(format!("src/handlers/forms/{snake}.rs")),
            reg.render(
                "backend/form",
                &json!({ "form": form, "project": &project_ctx }),
            )?,
        );
    }

    for entity in &entities {
        let snake = entity
            .get("snake")
            .and_then(|v| v.as_str())
            .unwrap_or("entity");
        files.insert(
            PathBuf::from(format!("src/models/{}.rs", snake)),
            reg.render(
                "backend/model",
                &json!({ "entity": entity, "project": &project_ctx }),
            )?,
        );
        files.insert(
            PathBuf::from(format!("src/handlers/{}.rs", snake)),
            reg.render(
                "backend/handler",
                &json!({ "entity": entity, "project": &project_ctx }),
            )?,
        );
    }

    // Migrations (used by db::run_migrations via include_str!)
    let schema = crate::database::generate_schema_sql(project, driver)?;
    files.insert(PathBuf::from("migrations/0001_initial.sql"), schema);

    let cargo_toml = generate_cargo_toml(project)?;

    Ok(BackendOutput { files, cargo_toml })
}

fn register_templates(reg: &mut Handlebars) -> Result<()> {
    reg.register_template_string(
        "backend/main",
        include_str!("templates/backend/main.rs.hbs"),
    )?;
    reg.register_template_string(
        "backend/config",
        include_str!("templates/backend/config.rs.hbs"),
    )?;
    reg.register_template_string("backend/db", include_str!("templates/backend/db.rs.hbs"))?;
    reg.register_template_string(
        "backend/error",
        include_str!("templates/backend/error.rs.hbs"),
    )?;
    reg.register_template_string(
        "backend/model",
        include_str!("templates/backend/model.rs.hbs"),
    )?;
    reg.register_template_string(
        "backend/form",
        include_str!("templates/backend/form.rs.hbs"),
    )?;
    reg.register_template_string(
        "backend/forms_mod",
        include_str!("templates/backend/forms_mod.rs.hbs"),
    )?;
    reg.register_template_string(
        "backend/routes",
        include_str!("templates/backend/routes.rs.hbs"),
    )?;
    reg.register_template_string(
        "backend/handler",
        include_str!("templates/backend/handler.rs.hbs"),
    )?;
    reg.register_template_string(
        "backend/models_mod",
        include_str!("templates/backend/models_mod.rs.hbs"),
    )?;
    reg.register_template_string(
        "backend/handlers_mod",
        include_str!("templates/backend/handlers_mod.rs.hbs"),
    )?;
    Ok(())
}

/// SeaORM driver feature flags for the given database driver.
///
/// Every driver gets an EXPLICIT arm. This used to end in
/// `_ => vec!["sqlx-postgres"]`, a catch-all that quietly compiled
/// SQL Server and Oracle projects as Postgres — a backend that spoke the
/// wrong dialect under the right name.
///
/// SeaORM 0.12 ships exactly three SQLx drivers: sqlite, mysql and
/// postgres. There is no SQL Server or Oracle driver, so a backend for
/// either cannot be generated. Refusing is the honest answer; the
/// alternative is an app that looks like it works and does not.
fn seaorm_driver_features(driver: &DatabaseDriver) -> Result<Vec<&'static str>> {
    match driver {
        DatabaseDriver::SQLite => Ok(vec!["sqlx-sqlite"]),
        DatabaseDriver::MySQL => Ok(vec!["sqlx-mysql"]),
        DatabaseDriver::Postgres => Ok(vec!["sqlx-postgres"]),
        DatabaseDriver::SqlServer => Err(anyhow::anyhow!(
            "SeaORM 0.12 has no SQL Server driver, so a backend cannot be generated \
             for a SQL Server project. Use PostgreSQL, MySQL or SQLite."
        )),
        DatabaseDriver::Oracle => Err(anyhow::anyhow!(
            "SeaORM 0.12 has no Oracle driver, so a backend cannot be generated \
             for an Oracle project. Use PostgreSQL, MySQL or SQLite."
        )),
    }
}

/// The generated backend's `Cargo.toml`.
///
/// Public because the manifest is a legitimate output in its own right,
/// and because a test needs to check that the project name cannot inject
/// TOML into it.
pub fn generate_cargo_toml(project: &Project) -> Result<String> {
    let driver = project.settings.database.driver.clone();
    let mut seaorm_features = vec![
        "macros",
        "with-chrono",
        "with-uuid",
        "with-json",
        "runtime-tokio-rustls",
    ];
    seaorm_features.extend(seaorm_driver_features(&driver)?);
    let seaorm_features = seaorm_features
        .iter()
        .map(|f| format!("\"{}\"", f))
        .collect::<Vec<_>>()
        .join(", ");

    let deps = vec![
        (
            "axum",
            r#"{ version = "0.7", features = ["json", "form"] }"#.to_string(),
        ),
        (
            "tokio",
            r#"{ version = "1", features = ["full"] }"#.to_string(),
        ),
        (
            "serde",
            r#"{ version = "1", features = ["derive"] }"#.to_string(),
        ),
        ("serde_json", r#""1""#.to_string()),
        ("anyhow", r#""1""#.to_string()),
        ("thiserror", r#""1""#.to_string()),
        (
            "uuid",
            r#"{ version = "1", features = ["v4", "serde"] }"#.to_string(),
        ),
        (
            "chrono",
            r#"{ version = "0.4", features = ["serde"] }"#.to_string(),
        ),
        ("dotenvy", r#""0.15""#.to_string()),
        (
            "tower-http",
            r#"{ version = "0.5", features = ["cors", "trace"] }"#.to_string(),
        ),
        ("tracing", r#""0.1""#.to_string()),
        (
            "tracing-subscriber",
            r#"{ version = "0.3", features = ["env-filter", "fmt"] }"#.to_string(),
        ),
        (
            "sea-orm",
            format!(
                r#"{{ version = "0.12", default-features = false, features = [{}] }}"#,
                seaorm_features
            ),
        ),
    ];

    let mut cargo = format!(
        r#"[package]
name = "{}"
version = "{}"
edition = "2021"
description = "{}"

[workspace]

[dependencies]
"#,
        toml_escape(&project.slug),
        toml_escape(&project.version),
        toml_escape(&format!("Generated backend for {}", project.name)),
    );
    for (name, spec) in deps {
        cargo.push_str(&format!("{} = {}\n", name, spec));
    }

    Ok(cargo)
}

/// Escape the contents of a TOML basic string.
///
/// The project name and version come from the user, and they are written
/// into the generated manifest. A name containing a quote and a newline
/// would otherwise close the string and inject TOML sections — and that
/// manifest is what cargo reads before it runs the generated crate's build
/// script, which is arbitrary code.
///
/// The surrounding quotes are the template's job; this only escapes what
/// goes between them.
fn toml_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Control characters are not allowed in a TOML basic string.
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

#[derive(Debug, Default)]
pub struct BackendOutput {
    pub files: HashMap<PathBuf, String>,
    pub cargo_toml: String,
}
