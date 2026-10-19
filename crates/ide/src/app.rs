//! The egui-based IDE application shell.

use std::fmt::Debug;
use std::path::PathBuf;

use egui::{Context, Key, Ui};

use crate::backend::{IdeBackend, NoopBackend};
use crate::commands::CommandRegistry;
use crate::state::{IdeState, Selection, View};

/// The main egui IDE application.
pub struct IdeApp {
    /// Current application state.
    pub state: IdeState,
    /// Command registry and pending commands.
    pub commands: CommandRegistry,
    /// Host/backend for code generation and validation.
    pub backend: Box<dyn IdeBackend>,
    /// Whether the command palette is visible.
    pub show_palette: bool,
    /// Current palette search query.
    pub palette_query: String,
    /// Output directory for code generation.
    pub output_dir: String,
}

impl Debug for IdeApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdeApp")
            .field("state", &self.state)
            .field("commands", &self.commands)
            .field("show_palette", &self.show_palette)
            .field("palette_query", &self.palette_query)
            .field("output_dir", &self.output_dir)
            .finish()
    }
}

impl Default for IdeApp {
    fn default() -> Self {
        Self {
            state: IdeState::default(),
            commands: CommandRegistry::with_defaults(),
            backend: Box::new(NoopBackend),
            show_palette: false,
            palette_query: String::new(),
            output_dir: "output".to_string(),
        }
    }
}

impl IdeApp {
    /// Create a new app with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new app with a custom backend.
    pub fn with_backend<B>(backend: B) -> Self
    where
        B: IdeBackend + 'static,
    {
        Self {
            state: IdeState::default(),
            commands: CommandRegistry::with_defaults(),
            backend: Box::new(backend),
            show_palette: false,
            palette_query: String::new(),
            output_dir: "output".to_string(),
        }
    }

    /// Run the app's update loop logic against an egui context.
    pub fn update(&mut self, ctx: &Context) {
        self.handle_shortcuts(ctx);
        self.process_commands();

        // Apply theme
        ctx.set_visuals(if self.state.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });

        // Top menu
        egui::TopBottomPanel::top("nocode_menubar")
            .resizable(false)
            .show(ctx, |ui| self.menu_bar(ui));

        // Bottom status/log
        egui::TopBottomPanel::bottom("nocode_status")
            .resizable(false)
            .show(ctx, |ui| self.status_bar(ui));

        // Left sidebar
        egui::SidePanel::left("nocode_sidebar")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| self.sidebar(ui));

        // Right inspector
        egui::SidePanel::right("nocode_inspector")
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| self.inspector(ui));

        // Central
        egui::CentralPanel::default().show(ctx, |ui| self.main_area(ui));

        // Command palette
        if self.show_palette {
            let mut open = true;
            egui::Window::new("Command Palette")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(320.0)
                .show(ctx, |ui| self.palette(ui));
            self.show_palette = open;
        }
    }

    fn handle_shortcuts(&mut self, ctx: &Context) {
        let mut close_palette = false;
        ctx.input(|i| {
            // Close palette with Esc
            if i.key_pressed(Key::Escape) && self.show_palette {
                close_palette = true;
                return;
            }

            let ctrl = i.modifiers.ctrl || i.modifiers.command;
            if ctrl {
                if i.key_pressed(Key::N) {
                    self.commands.trigger("file.new");
                } else if i.key_pressed(Key::O) {
                    self.commands.trigger("file.open");
                } else if i.key_pressed(Key::S) {
                    self.commands.trigger("file.save");
                } else if i.key_pressed(Key::G) {
                    self.commands.trigger("code.generate");
                } else if i.key_pressed(Key::P) {
                    self.commands.trigger("app.command_palette");
                } else if i.key_pressed(Key::Num1) {
                    self.commands.trigger("view.project");
                } else if i.key_pressed(Key::Num2) {
                    self.commands.trigger("view.forms");
                } else if i.key_pressed(Key::Num3) {
                    self.commands.trigger("view.workflows");
                } else if i.key_pressed(Key::Num4) {
                    self.commands.trigger("view.entities");
                } else if i.key_pressed(Key::Num5) {
                    self.commands.trigger("view.pages");
                } else if i.key_pressed(Key::Num6) {
                    self.commands.trigger("view.themes");
                } else if i.key_pressed(Key::Num7) {
                    self.commands.trigger("view.codegen");
                } else if i.key_pressed(Key::Num8) {
                    self.commands.trigger("view.plugins");
                } else if i.key_pressed(Key::Num9) {
                    self.commands.trigger("view.settings");
                }
            }
        });
        if close_palette {
            self.show_palette = false;
        }
    }

    fn process_commands(&mut self) {
        let ids = self.commands.drain();
        for id in ids {
            match id {
                "file.new" => {
                    self.state.new_project("Untitled Project");
                }
                "file.open" => {
                    self.state.log_warn(
                        "Open project requires a host file dialog (not available in headless mode)",
                    );
                }
                "file.save" => match self.state.save_project(None) {
                    Ok(path) => self.state.log_info(format!("Saved: {}", path.display())),
                    Err(err) => self.state.log_error(err),
                },
                "file.close" => {
                    self.state.close_project();
                }
                "view.project" => self.state.set_view(View::Project),
                "view.forms" => self.state.set_view(View::Forms),
                "view.workflows" => self.state.set_view(View::Workflows),
                "view.entities" => self.state.set_view(View::Entities),
                "view.pages" => self.state.set_view(View::Pages),
                "view.themes" => self.state.set_view(View::Themes),
                "view.codegen" => self.state.set_view(View::Codegen),
                "view.plugins" => self.state.set_view(View::Plugins),
                "view.settings" => self.state.set_view(View::Settings),
                "model.add_form" => {
                    self.add_form();
                }
                "model.add_entity" => {
                    self.add_entity();
                }
                "model.add_workflow" => {
                    self.add_workflow();
                }
                "model.add_page" => {
                    self.add_page();
                }
                "code.generate" => {
                    self.generate();
                }
                "code.validate" => {
                    self.validate_workflows();
                }
                "app.command_palette" => {
                    self.show_palette = true;
                    self.palette_query.clear();
                }
                "app.toggle_theme" => {
                    self.state.dark_mode = !self.state.dark_mode;
                }
                _ => {}
            }
        }
    }

    fn add_form(&mut self) {
        let added = self.state.project_mut().map(|project| {
            let id = nocode_model::FormId::new();
            let name = format!("Form {}", project.forms.len() + 1);
            project
                .forms
                .insert(id.clone(), nocode_model::Form::new(name.clone()));
            (id, name)
        });
        match added {
            Some((id, name)) => {
                self.state.mark_dirty();
                self.state.log_info(format!("Added form: {name}"));
                self.state.select(Selection::Form(id));
            }
            None => self.state.log_warn("No active project to add a form"),
        }
    }

    fn add_entity(&mut self) {
        let added = self.state.project_mut().map(|project| {
            let id = nocode_model::EntityId::new();
            let name = format!("Entity {}", project.entities.len() + 1);
            project
                .entities
                .insert(id, nocode_model::Entity::new(name.clone()));
            name
        });
        match added {
            Some(name) => {
                self.state.mark_dirty();
                self.state.log_info(format!("Added entity: {name}"));
                self.state.select(Selection::Entity(name));
            }
            None => self.state.log_warn("No active project to add an entity"),
        }
    }

    fn add_workflow(&mut self) {
        let added = self.state.project_mut().map(|project| {
            let id = nocode_model::WorkflowId::new();
            let name = format!("Workflow {}", project.workflows.len() + 1);
            project
                .workflows
                .insert(id.clone(), nocode_model::Workflow::new(name.clone()));
            (id, name)
        });
        match added {
            Some((id, name)) => {
                self.state.mark_dirty();
                self.state.log_info(format!("Added workflow: {name}"));
                self.state.select(Selection::Workflow(id));
            }
            None => self.state.log_warn("No active project to add a workflow"),
        }
    }

    fn add_page(&mut self) {
        let added = self.state.project_mut().map(|project| {
            let id = nocode_model::PageId::new();
            let name = format!("Page {}", project.pages.len() + 1);
            project
                .pages
                .insert(id.clone(), nocode_model::Page::new(name.clone()));
            (id, name)
        });
        match added {
            Some((id, name)) => {
                self.state.mark_dirty();
                self.state.log_info(format!("Added page: {name}"));
                self.state.select(Selection::Page(id));
            }
            None => self.state.log_warn("No active project to add a page"),
        }
    }

    fn generate(&mut self) {
        if !self.state.has_active() {
            self.state.log_warn("No active project to generate");
            return;
        }
        let outcome = {
            let output = PathBuf::from(&self.output_dir);
            let project = self.state.project_mut().unwrap();
            self.backend.generate(project, &output)
        };
        match outcome {
            Ok(report) => self.state.log_info(format!(
                "Generated {} files ({} backend, {} frontend, {} migrations)",
                report.total_files(),
                report.backend_files,
                report.frontend_files,
                report.migrations
            )),
            Err(err) => self.state.log_error(format!("Generation failed: {err}")),
        }
    }

    fn validate_workflows(&mut self) {
        if !self.state.has_active() {
            self.state
                .log_warn("No active project to validate workflows");
            return;
        }
        let mut messages: Vec<(String, String)> = Vec::new();
        {
            let project = self.state.project().unwrap();
            for (id, wf) in &project.workflows {
                for msg in self.backend.validate_workflow(wf) {
                    messages.push((id.to_string(), msg));
                }
            }
        }
        if messages.is_empty() {
            self.state
                .log_info("Workflow validation passed (no issues)");
        } else {
            for (id, msg) in messages {
                self.state.log_warn(format!("{id}: {msg}"));
            }
        }
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New Project").clicked() {
                    self.commands.trigger("file.new");
                }
                if ui.button("Open Project…").clicked() {
                    self.commands.trigger("file.open");
                }
                if ui.button("Save Project").clicked() {
                    self.commands.trigger("file.save");
                }
                if ui.button("Close Project").clicked() {
                    self.commands.trigger("file.close");
                }
            });

            ui.menu_button("View", |ui| {
                for &view in View::ALL.iter() {
                    let selected = self.state.view == view;
                    if ui.selectable_label(selected, view.label()).clicked() {
                        self.state.set_view(view);
                    }
                }
            });

            ui.menu_button("Model", |ui| {
                if ui.button("Add Form").clicked() {
                    self.commands.trigger("model.add_form");
                }
                if ui.button("Add Entity").clicked() {
                    self.commands.trigger("model.add_entity");
                }
                if ui.button("Add Workflow").clicked() {
                    self.commands.trigger("model.add_workflow");
                }
                if ui.button("Add Page").clicked() {
                    self.commands.trigger("model.add_page");
                }
            });

            ui.menu_button("Code", |ui| {
                if ui.button("Generate Code").clicked() {
                    self.commands.trigger("code.generate");
                }
                if ui.button("Validate Workflows").clicked() {
                    self.commands.trigger("code.validate");
                }
            });

            ui.menu_button("Tools", |ui| {
                if ui.button("Command Palette").clicked() {
                    self.commands.trigger("app.command_palette");
                }
                if ui.button("Toggle Dark Mode").clicked() {
                    self.commands.trigger("app.toggle_theme");
                }
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let dirty = if self.state.has_active() && self.state.is_dirty() {
                    "● Dirty"
                } else {
                    "Clean"
                };
                ui.label(dirty);
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let status = if self.state.has_active() {
                format!(
                    "{} — View: {}",
                    self.state.project().unwrap().name,
                    self.state.view.label()
                )
            } else {
                "No active project".to_string()
            };
            ui.label(status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("Logs: {}", self.state.logs().len()));
                ui.label("Ctrl+P: Palette");
            });
        });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        ui.heading("Explorer");
        for &view in View::ALL.iter() {
            let selected = self.state.view == view;
            if ui.selectable_label(selected, view.label()).clicked() {
                self.state.set_view(view);
            }
        }

        ui.separator();
        ui.heading("Project");
        if let Some(project) = self.state.project() {
            ui.label(format!("Forms: {}", project.forms.len()));
            ui.label(format!("Entities: {}", project.entities.len()));
            ui.label(format!("Workflows: {}", project.workflows.len()));
            ui.label(format!("Pages: {}", project.pages.len()));
            ui.label(format!("Themes: {}", project.themes.len()));
        } else {
            ui.label("No project open");
        }
    }

    fn inspector(&mut self, ui: &mut Ui) {
        ui.heading("Inspector");
        match &self.state.selection {
            Some(Selection::Form(id)) => {
                if let Some(project) = self.state.project() {
                    if let Some(form) = project.forms.get(id) {
                        ui.label(format!("Form: {}", form.name));
                        ui.label(format!("Fields: {}", form.fields.len()));
                    }
                }
            }
            Some(Selection::Entity(name)) => {
                if let Some(project) = self.state.project() {
                    if let Some(entity) = project.entities.values().find(|e| &e.name == name) {
                        ui.label(format!("Entity: {}", entity.name));
                        ui.label(format!("Fields: {}", entity.fields.len()));
                    }
                }
            }
            Some(Selection::Workflow(id)) => {
                if let Some(project) = self.state.project() {
                    if let Some(wf) = project.workflows.get(id) {
                        ui.label(format!("Workflow: {}", wf.name));
                        ui.label(format!("Nodes: {}", wf.nodes.len()));
                        ui.label(format!("Edges: {}", wf.edges.len()));
                    }
                }
            }
            Some(Selection::Page(id)) => {
                if let Some(project) = self.state.project() {
                    if let Some(page) = project.pages.get(id) {
                        ui.label(format!("Page: {}", page.name));
                        ui.label(format!("Path: {}", page.path));
                    }
                }
            }
            Some(Selection::Theme(name)) => {
                ui.label(format!("Theme: {name}"));
            }
            None => {
                ui.label("Nothing selected");
            }
        }
    }

    fn main_area(&mut self, ui: &mut Ui) {
        match self.state.view {
            View::Project => self.view_project(ui),
            View::Forms => self.view_forms(ui),
            View::Workflows => self.view_workflows(ui),
            View::Entities => self.view_entities(ui),
            View::Pages => self.view_pages(ui),
            View::Themes => self.view_themes(ui),
            View::Codegen => self.view_codegen(ui),
            View::Plugins => self.view_plugins(ui),
            View::Settings => self.view_settings(ui),
        }
    }

    fn view_project(&mut self, ui: &mut Ui) {
        ui.heading("Project Overview");
        if let Some(project) = self.state.project() {
            ui.label(format!("Name: {}", project.name));
            ui.label(format!("Slug: {}", project.slug));
            ui.label(format!("Version: {}", project.version));
        } else {
            ui.label("No project is currently open.");
            if ui.button("Create New Project").clicked() {
                self.commands.trigger("file.new");
            }
        }
    }

    fn view_forms(&mut self, ui: &mut Ui) {
        ui.heading("Forms");
        let mut clicked: Option<nocode_model::FormId> = None;
        if let Some(project) = self.state.project() {
            for (id, form) in &project.forms {
                let selected = matches!(&self.state.selection, Some(Selection::Form(s)) if s == id);
                if ui.selectable_label(selected, &form.name).clicked() {
                    clicked = Some(id.clone());
                }
            }
        }
        if let Some(id) = clicked {
            self.state.select(Selection::Form(id));
        }
        if ui.button("+ Add Form").clicked() {
            self.commands.trigger("model.add_form");
        }
    }

    fn view_workflows(&mut self, ui: &mut Ui) {
        ui.heading("Workflows");
        let mut clicked: Option<nocode_model::WorkflowId> = None;
        if let Some(project) = self.state.project() {
            for (id, wf) in &project.workflows {
                let selected =
                    matches!(&self.state.selection, Some(Selection::Workflow(s)) if s == id);
                if ui.selectable_label(selected, &wf.name).clicked() {
                    clicked = Some(id.clone());
                }
            }
        }
        if let Some(id) = clicked {
            self.state.select(Selection::Workflow(id));
        }
        if ui.button("+ Add Workflow").clicked() {
            self.commands.trigger("model.add_workflow");
        }
    }

    fn view_entities(&mut self, ui: &mut Ui) {
        ui.heading("Entities");
        let mut clicked: Option<String> = None;
        if let Some(project) = self.state.project() {
            for entity in project.entities.values() {
                let selected = matches!(&self.state.selection, Some(Selection::Entity(s)) if s == &entity.name);
                if ui.selectable_label(selected, &entity.name).clicked() {
                    clicked = Some(entity.name.clone());
                }
            }
        }
        if let Some(name) = clicked {
            self.state.select(Selection::Entity(name));
        }
        if ui.button("+ Add Entity").clicked() {
            self.commands.trigger("model.add_entity");
        }
    }

    fn view_pages(&mut self, ui: &mut Ui) {
        ui.heading("Pages");
        let mut clicked: Option<nocode_model::PageId> = None;
        if let Some(project) = self.state.project() {
            for (id, page) in &project.pages {
                let selected = matches!(&self.state.selection, Some(Selection::Page(s)) if s == id);
                if ui.selectable_label(selected, &page.name).clicked() {
                    clicked = Some(id.clone());
                }
            }
        }
        if let Some(id) = clicked {
            self.state.select(Selection::Page(id));
        }
        if ui.button("+ Add Page").clicked() {
            self.commands.trigger("model.add_page");
        }
    }

    fn view_themes(&mut self, ui: &mut Ui) {
        ui.heading("Themes");
        let mut clicked: Option<String> = None;
        if let Some(project) = self.state.project() {
            for theme in project.themes.values() {
                let selected =
                    matches!(&self.state.selection, Some(Selection::Theme(s)) if s == &theme.name);
                if ui.selectable_label(selected, &theme.name).clicked() {
                    clicked = Some(theme.name.clone());
                }
            }
        }
        if let Some(name) = clicked {
            self.state.select(Selection::Theme(name));
        }
    }

    fn view_codegen(&mut self, ui: &mut Ui) {
        ui.heading("Code Generation");
        ui.text_edit_singleline(&mut self.output_dir);
        ui.label(format!("Output: {}", self.output_dir));
        if ui.button("Generate").clicked() {
            self.commands.trigger("code.generate");
        }
    }

    fn view_plugins(&mut self, ui: &mut Ui) {
        ui.heading("Plugins");
        if let Some(project) = self.state.project() {
            ui.label(format!("Installed: {}", project.plugins.len()));
        }
    }

    fn view_settings(&mut self, ui: &mut Ui) {
        ui.heading("Settings");
        ui.checkbox(&mut self.state.dark_mode, "Dark Mode");
        ui.separator();
        ui.label("UI primitives only; real settings TBD");
    }

    fn palette(&mut self, ui: &mut Ui) {
        ui.text_edit_singleline(&mut self.palette_query);
        let query = self.palette_query.clone();
        let matches: Vec<&'static str> =
            self.commands.search(&query).iter().map(|c| c.id).collect();
        for id in matches {
            let label = self.commands.find(id).map(|c| c.label).unwrap_or(id);
            if ui.button(label).clicked() {
                self.show_palette = false;
                self.commands.trigger(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(app: &mut IdeApp) {
        let ctx = Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.update(ctx));
    }

    #[test]
    fn app_updates_headlessly() {
        let mut app = IdeApp::new();
        run(&mut app);
        assert!(!app.state.has_active());

        app.commands.trigger("file.new");
        run(&mut app);
        assert!(app.state.has_active());
    }

    #[test]
    fn view_switch_commands_change_view() {
        let mut app = IdeApp::new();
        app.commands.trigger("view.workflows");
        run(&mut app);
        assert_eq!(app.state.view, View::Workflows);
    }

    #[test]
    fn add_model_commands_populate_project() {
        let mut app = IdeApp::new();
        app.state.new_project("Demo");
        app.commands.trigger("model.add_form");
        app.commands.trigger("model.add_entity");
        app.commands.trigger("model.add_workflow");
        app.commands.trigger("model.add_page");
        run(&mut app);
        let project = app.state.project().unwrap();
        assert_eq!(project.forms.len(), 1);
        assert_eq!(project.entities.len(), 1);
        assert_eq!(project.workflows.len(), 1);
        assert_eq!(project.pages.len(), 1);
        assert!(app.state.is_dirty());
    }

    #[test]
    fn generate_command_reports_backend_result() {
        let mut app = IdeApp::new();
        app.state.new_project("Gen");
        app.commands.trigger("code.generate");
        run(&mut app);
        assert!(app
            .state
            .logs()
            .iter()
            .any(|l| l.message.contains("Generated")));
    }

    #[test]
    fn toggle_theme_flips_dark_mode() {
        let mut app = IdeApp::new();
        let before = app.state.dark_mode;
        app.commands.trigger("app.toggle_theme");
        run(&mut app);
        assert_eq!(app.state.dark_mode, !before);
    }
}
