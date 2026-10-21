use anyhow::Result;
use handlebars::Handlebars;
use nocode_model::Project;
use serde_json::{json, Value};
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
    let forms = view::forms_view(project);
    let pages = view::pages_view(project);

    // Collect all data sources from all pages for the API client
    let all_data_sources: Vec<Value> = pages
        .iter()
        .flat_map(|p| {
            p.get("data_sources")
                .and_then(|v| v.as_array())
                .map(|arr| arr.to_vec())
                .unwrap_or_default()
        })
        .collect();

    let theme = view::theme_view(project);
    let ctx = json!({
        "project": &project_ctx,
        "entities": &entities,
        "forms": &forms,
        "pages": &pages,
        "data_sources": &all_data_sources,
        "theme": &theme
    });

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
        reg.render("frontend/tailwind_config", &ctx)?,
    );
    files.insert(
        PathBuf::from("index.html"),
        reg.render("frontend/index_html", &ctx)?,
    );
    files.insert(
        PathBuf::from("style/input.css"),
        reg.render("frontend/input_css", &ctx)?,
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

    // Forms
    for form in &forms {
        let snake = form.get("snake").and_then(|v| v.as_str()).unwrap_or("form");
        files.insert(
            PathBuf::from(format!("src/pages/form_{}.rs", snake)),
            reg.render("frontend/form_page", &form)?,
        );
    }

    // Pages (user-defined)
    for page in &pages {
        let is_index = page
            .get("is_index")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_index {
            continue;
        }
        let snake = page.get("snake").and_then(|v| v.as_str()).unwrap_or("page");
        files.insert(
            PathBuf::from(format!("src/pages/{}.rs", snake)),
            reg.render("frontend/page", &page)?,
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
        "frontend/tailwind_config",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/frontend/tailwind.config.js.hbs"
        )),
    )?;
    reg.register_template_string(
        "frontend/input_css",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/frontend/input.css.hbs"
        )),
    )?;
    reg.register_template_string(
        "frontend/index_html",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/frontend/index.html.hbs"
        )),
    )?;
    reg.register_template_string(
        "frontend/app",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/frontend/app.rs.hbs"
        )),
    )?;
    reg.register_template_string(
        "frontend/api",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/frontend/api.rs.hbs"
        )),
    )?;
    reg.register_template_string(
        "frontend/types",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/frontend/types.rs.hbs"
        )),
    )?;
    reg.register_template_string(
        "frontend/layout_component",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/frontend/layout_component.rs.hbs"
        )),
    )?;
    reg.register_template_string(
        "frontend/pages_mod",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/frontend/pages_mod.rs.hbs"
        )),
    )?;
    reg.register_template_string(
        "frontend/home",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/frontend/home.rs.hbs"
        )),
    )?;
    reg.register_template_string(
        "frontend/entity_page",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/frontend/entity_page.rs.hbs"
        )),
    )?;
    reg.register_template_string(
        "frontend/form_page",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/frontend/form_page.rs.hbs"
        )),
    )?;
    reg.register_template_string(
        "frontend/page",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/frontend/page.rs.hbs"
        )),
    )?;
    Ok(())
}

#[derive(Debug, Default)]
pub struct FrontendOutput {
    pub files: HashMap<PathBuf, String>,
}
