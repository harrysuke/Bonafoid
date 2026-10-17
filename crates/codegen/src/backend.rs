use nocode_model::{DatabaseDriver, Project};
use handlebars::Handlebars;
use serde_json::json;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::view;

/// Generate the backend application for a project.
pub async fn generate_backend(project: &Project, _output_dir: &std::path::Path) -> Result<BackendOutput> {
    let mut reg = Handlebars::new();
    register_templates(&mut reg)?;
    crate::templates::register_helpers(&mut reg);

    let driver = project.settings.database.driver.clone();
    let project_ctx = view::project_view(project, &driver);
    let entities = view::entities_view(project, &driver);

    let mut files = HashMap::new();

    let ctx = json!({ "project": &project_ctx, "entities": &entities });

    files.insert(PathBuf::from("Cargo.toml"), generate_cargo_toml(project)?);

    files.insert(PathBuf::from("src/main.rs"), reg.render("backend/main", &ctx)?);
    files.insert(PathBuf::from("src/config.rs"), reg.render("backend/config", &ctx)?);
    files.insert(PathBuf::from("src/db.rs"), reg.render("backend/db", &ctx)?);
    files.insert(PathBuf::from("src/error.rs"), reg.render("backend/error", &json!({}))?);
    files.insert(PathBuf::from("src/routes.rs"), reg.render("backend/routes", &ctx)?);

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

    for entity in &entities {
        let snake = entity.get("snake").and_then(|v| v.as_str()).unwrap_or("entity");
        files.insert(
            PathBuf::from(format!("src/models/{}.rs", snake)),
            reg.render("backend/model", &json!({ "entity": entity, "project": &project_ctx }))?,
        );
        files.insert(
            PathBuf::from(format!("src/handlers/{}.rs", snake)),
            reg.render("backend/handler", &json!({ "entity": entity, "project": &project_ctx }))?,
        );
    }

    // Migrations (used by db::run_migrations via include_str!)
    let schema = crate::database::generate_schema_sql(project, driver)?;
    files.insert(PathBuf::from("migrations/0001_initial.sql"), schema);

    let cargo_toml = generate_cargo_toml(project)?;

    Ok(BackendOutput { files, cargo_toml })
}

fn register_templates(reg: &mut Handlebars) -> Result<()> {
    reg.register_template_string("backend/main", include_str!("templates/backend/main.rs.hbs"))?;
    reg.register_template_string("backend/config", include_str!("templates/backend/config.rs.hbs"))?;
    reg.register_template_string("backend/db", include_str!("templates/backend/db.rs.hbs"))?;
    reg.register_template_string("backend/error", include_str!("templates/backend/error.rs.hbs"))?;
    reg.register_template_string("backend/model", include_str!("templates/backend/model.rs.hbs"))?;
    reg.register_template_string("backend/routes", include_str!("templates/backend/routes.rs.hbs"))?;
    reg.register_template_string("backend/handler", include_str!("templates/backend/handler.rs.hbs"))?;
    reg.register_template_string("backend/models_mod", include_str!("templates/backend/models_mod.rs.hbs"))?;
    reg.register_template_string("backend/handlers_mod", include_str!("templates/backend/handlers_mod.rs.hbs"))?;
    Ok(())
}

/// SeaORM driver feature flags for the given database driver.
fn seaorm_driver_features(driver: &DatabaseDriver) -> Vec<&'static str> {
    match driver {
        DatabaseDriver::SQLite => vec!["sqlx-sqlite"],
        DatabaseDriver::MySQL => vec!["sqlx-mysql"],
        _ => vec!["sqlx-postgres"],
    }
}

fn generate_cargo_toml(project: &Project) -> Result<String> {
    let driver = project.settings.database.driver.clone();
    let mut seaorm_features = vec!["macros", "with-chrono", "with-uuid", "with-json", "runtime-tokio-rustls"];
    seaorm_features.extend(seaorm_driver_features(&driver));
    let seaorm_features = seaorm_features
        .iter()
        .map(|f| format!("\"{}\"", f))
        .collect::<Vec<_>>()
        .join(", ");

    let deps = vec![
        ("axum", r#"{ version = "0.7", features = ["json", "form"] }"#.to_string()),
        ("tokio", r#"{ version = "1", features = ["full"] }"#.to_string()),
        ("serde", r#"{ version = "1", features = ["derive"] }"#.to_string()),
        ("serde_json", r#""1""#.to_string()),
        ("anyhow", r#""1""#.to_string()),
        ("thiserror", r#""1""#.to_string()),
        ("uuid", r#"{ version = "1", features = ["v4", "serde"] }"#.to_string()),
        ("chrono", r#"{ version = "0.4", features = ["serde"] }"#.to_string()),
        ("dotenvy", r#""0.15""#.to_string()),
        ("tower-http", r#"{ version = "0.5", features = ["cors", "trace"] }"#.to_string()),
        ("tracing", r#""0.1""#.to_string()),
        ("tracing-subscriber", r#"{ version = "0.3", features = ["env-filter", "fmt"] }"#.to_string()),
        ("sea-orm", format!(r#"{{ version = "0.12", default-features = false, features = [{}] }}"#, seaorm_features)),
    ];

    let mut cargo = format!(r#"[package]
name = "{}"
version = "{}"
edition = "2021"
description = "Generated backend for {}"

[workspace]

[dependencies]
"#, project.slug, project.version, project.name);

    for (name, spec) in deps {
        cargo.push_str(&format!("{} = {}\n", name, spec));
    }

    Ok(cargo)
}

#[derive(Debug, Default)]
pub struct BackendOutput {
    pub files: HashMap<PathBuf, String>,
    pub cargo_toml: String,
}
