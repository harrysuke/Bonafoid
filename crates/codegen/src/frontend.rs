use anyhow::Result;
use handlebars::Handlebars;
use nocode_model::Project;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::view;

/// Generate the Leptos (CSR / WASM) frontend for a project.
pub async fn generate_frontend(
    project: &Project,
    _output_dir: &std::path::Path,
) -> Result<FrontendOutput> {
    let mut reg = Handlebars::new();
    register_templates(&mut reg)?;
    crate::templates::register_helpers(&mut reg);

    let driver = project.settings.database.driver.clone();
    let project_ctx = view::project_view(project, &driver);
    let entities = view::entities_view(project, &driver);

    let ctx = json!({ "project": &project_ctx, "entities": &entities });

    let mut files = HashMap::new();

    // Build configuration
    files.insert(
        PathBuf::from("Cargo.toml"),
        reg.render("frontend/cargo_toml", &ctx)?,
    );
    files.insert(
        PathBuf::from("Trunk.toml"),
        include_str!("templates/frontend/Trunk.toml").to_string(),
    );
    files.insert(
        PathBuf::from("tailwind.config.js"),
        include_str!("templates/frontend/tailwind.config.js").to_string(),
    );
    files.insert(
        PathBuf::from("index.html"),
        reg.render("frontend/index_html", &ctx)?,
    );
    files.insert(
        PathBuf::from("style/input.css"),
        include_str!("templates/frontend/input.css").to_string(),
    );

    // Crate root + shared modules
    files.insert(
        PathBuf::from("src/main.rs"),
        include_str!("templates/frontend/main.rs").to_string(),
    );
    files.insert(
        PathBuf::from("src/convert.rs"),
        include_str!("templates/frontend/convert.rs").to_string(),
    );
    files.insert(
        PathBuf::from("src/app.rs"),
        reg.render("frontend/app", &ctx)?,
    );
    files.insert(
        PathBuf::from("src/api.rs"),
        reg.render("frontend/api", &ctx)?,
    );
    files.insert(
        PathBuf::from("src/types.rs"),
        reg.render("frontend/types", &ctx)?,
    );

    // Components
    files.insert(
        PathBuf::from("src/components/mod.rs"),
        include_str!("templates/frontend/components_mod.rs").to_string(),
    );
    files.insert(
        PathBuf::from("src/components/layout.rs"),
        reg.render("frontend/layout_component", &ctx)?,
    );

    // Pages
    files.insert(
        PathBuf::from("src/pages/mod.rs"),
        reg.render("frontend/pages_mod", &ctx)?,
    );
    files.insert(
        PathBuf::from("src/pages/home.rs"),
        reg.render("frontend/home", &ctx)?,
    );
    for entity in &entities {
        let snake = entity
            .get("snake")
            .and_then(|v| v.as_str())
            .unwrap_or("entity");
        files.insert(
            PathBuf::from(format!("src/pages/{}.rs", snake)),
            reg.render("frontend/entity_page", &entity)?,
        );
    }

    Ok(FrontendOutput { files })
}

fn register_templates(reg: &mut Handlebars) -> Result<()> {
    reg.register_template_string(
        "frontend/cargo_toml",
        include_str!("templates/frontend/Cargo.toml.hbs"),
    )?;
    reg.register_template_string(
        "frontend/index_html",
        include_str!("templates/frontend/index.html.hbs"),
    )?;
    reg.register_template_string(
        "frontend/app",
        include_str!("templates/frontend/app.rs.hbs"),
    )?;
    reg.register_template_string(
        "frontend/api",
        include_str!("templates/frontend/api.rs.hbs"),
    )?;
    reg.register_template_string(
        "frontend/types",
        include_str!("templates/frontend/types.rs.hbs"),
    )?;
    reg.register_template_string(
        "frontend/layout_component",
        include_str!("templates/frontend/layout_component.rs.hbs"),
    )?;
    reg.register_template_string(
        "frontend/pages_mod",
        include_str!("templates/frontend/pages_mod.rs.hbs"),
    )?;
    reg.register_template_string(
        "frontend/home",
        include_str!("templates/frontend/home.rs.hbs"),
    )?;
    reg.register_template_string(
        "frontend/entity_page",
        include_str!("templates/frontend/entity_page.rs.hbs"),
    )?;
    Ok(())
}

#[derive(Debug, Default)]
pub struct FrontendOutput {
    pub files: HashMap<PathBuf, String>,
}
