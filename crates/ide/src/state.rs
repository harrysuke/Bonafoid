//! Application state for the native (egui) IDE shell.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use nocode_model::{FormId, PageId, Project, WorkflowId};

/// The primary views available in the IDE.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum View {
    Project,
    Forms,
    Workflows,
    Entities,
    Pages,
    Themes,
    Codegen,
    Plugins,
    Settings,
}

impl View {
    /// All views in display order.
    pub const ALL: [View; 9] = [
        View::Project,
        View::Forms,
        View::Workflows,
        View::Entities,
        View::Pages,
        View::Themes,
        View::Codegen,
        View::Plugins,
        View::Settings,
    ];

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            View::Project => "Project",
            View::Forms => "Forms",
            View::Workflows => "Workflows",
            View::Entities => "Entities",
            View::Pages => "Pages",
            View::Themes => "Themes",
            View::Codegen => "Code",
            View::Plugins => "Plugins",
            View::Settings => "Settings",
        }
    }

    /// Icon glyph used in the sidebar.
    pub fn icon(self) -> &'static str {
        match self {
            View::Project => "▤",
            View::Forms => "▭",
            View::Workflows => "⑃",
            View::Entities => "▦",
            View::Pages => "▣",
            View::Themes => "◑",
            View::Codegen => "⌨",
            View::Plugins => "◧",
            View::Settings => "⚙",
        }
    }
}

/// A lightweight reference to a project shown in the explorer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEntry {
    pub name: String,
    pub slug: String,
    pub path: PathBuf,
    pub modified: DateTime<Utc>,
}

/// Kind of element selected in the explorer / inspector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Form(FormId),
    Workflow(WorkflowId),
    Page(PageId),
    Entity(String),
    Theme(String),
}

/// A record in the activity log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub message: String,
}

/// Severity of a log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

/// The complete, serializable-ish state of the IDE session.
#[derive(Debug, Clone)]
pub struct IdeState {
    /// Recently opened projects.
    pub projects: Vec<ProjectEntry>,
    /// The currently open project, if any.
    pub active: Option<Project>,
    /// Filesystem path of the active project.
    pub active_path: Option<PathBuf>,
    /// Currently selected view.
    pub view: View,
    /// Currently selected element.
    pub selection: Option<Selection>,
    /// Explorer / palette search string.
    pub search: String,
    /// Status bar message.
    pub status: String,
    /// Undo flag — set when the model changes.
    pub dirty: bool,
    /// Activity log.
    pub log: Vec<LogEntry>,
    /// Whether the dark theme is active.
    pub dark_mode: bool,
}

impl Default for IdeState {
    fn default() -> Self {
        Self::new()
    }
}

impl IdeState {
    /// Create an empty state with a fresh activity log.
    pub fn new() -> Self {
        let mut state = Self {
            projects: Vec::new(),
            active: None,
            active_path: None,
            view: View::Project,
            selection: None,
            search: String::new(),
            status: "Ready".to_string(),
            dirty: false,
            log: Vec::new(),
            dark_mode: true,
        };
        state.log_info("IDE initialized");
        state
    }

    /// Create a new in-memory project and make it active.
    pub fn new_project(&mut self, name: &str) {
        let project = Project::new(name);
        self.log_info(format!("Created project `{}`", project.name));
        self.status = format!("New project: {}", project.name);
        self.active = Some(project);
        self.active_path = None;
        self.dirty = true;
        self.view = View::Project;
    }

    /// Load a project from a JSON file on disk.
    pub fn open_project(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let project: Project =
            serde_json::from_str(&contents).map_err(|e| format!("invalid project JSON: {e}"))?;

        self.log_info(format!("Opened project `{}`", project.name));
        self.status = format!("Opened {}", project.name);
        self.push_recent(&project, path);
        self.active = Some(project);
        self.active_path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    /// Save the active project to the given path (or its current path).
    pub fn save_project(&mut self, path: Option<PathBuf>) -> Result<PathBuf, String> {
        let project = self
            .active
            .as_ref()
            .ok_or_else(|| "no active project".to_string())?;
        let target = path
            .or_else(|| self.active_path.clone())
            .ok_or_else(|| "no path specified; use `Save As` to choose a location".to_string())?;

        let json = serde_json::to_string_pretty(project)
            .map_err(|e| format!("failed to serialize project: {e}"))?;
        std::fs::write(&target, json)
            .map_err(|e| format!("failed to write {}: {e}", target.display()))?;

        self.dirty = false;
        self.active_path = Some(target.clone());
        self.log_info(format!("Saved project to {}", target.display()));
        self.status = format!("Saved to {}", target.display());
        Ok(target)
    }

    /// Close the active project, keeping it in the recent list.
    pub fn close_project(&mut self) {
        if let Some(project) = self.active.take() {
            self.log_info(format!("Closed project `{}`", project.name));
        }
        self.active_path = None;
        self.selection = None;
        self.dirty = false;
    }

    /// Whether a project is currently open.
    pub fn has_active(&self) -> bool {
        self.active.is_some()
    }

    /// Immutable reference to the active project, if any.
    pub fn project(&self) -> Option<&Project> {
        self.active.as_ref()
    }

    /// Mutable reference to the active project, if any.
    pub fn project_mut(&mut self) -> Option<&mut Project> {
        self.active.as_mut()
    }

    /// Whether the active project has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// The activity log entries.
    pub fn logs(&self) -> &[LogEntry] {
        &self.log
    }

    /// Set the active view.
    pub fn set_view(&mut self, view: View) {
        self.view = view;
    }

    /// Select an element.
    pub fn select(&mut self, selection: Selection) {
        self.selection = Some(selection);
    }

    /// Mark the model as modified.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.status = "Unsaved changes".to_string();
    }

    /// Append an info message to the activity log.
    pub fn log_info(&mut self, message: impl Into<String>) {
        self.push_log(LogLevel::Info, message);
    }

    /// Append a warning message to the activity log.
    pub fn log_warn(&mut self, message: impl Into<String>) {
        self.push_log(LogLevel::Warning, message);
    }

    /// Append an error message to the activity log.
    pub fn log_error(&mut self, message: impl Into<String>) {
        self.push_log(LogLevel::Error, message);
    }

    fn push_log(&mut self, level: LogLevel, message: impl Into<String>) {
        self.log.push(LogEntry {
            timestamp: Utc::now(),
            level,
            message: message.into(),
        });
        // Keep the log bounded.
        if self.log.len() > 1000 {
            self.log.drain(0..self.log.len() - 1000);
        }
    }

    fn push_recent(&mut self, project: &Project, path: &Path) {
        self.projects.retain(|p| p.path != path);
        self.projects.insert(
            0,
            ProjectEntry {
                name: project.name.clone(),
                slug: project.slug.clone(),
                path: path.to_path_buf(),
                modified: Utc::now(),
            },
        );
        self.projects.truncate(10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_no_project() {
        let state = IdeState::new();
        assert!(!state.has_active());
        assert_eq!(state.view, View::Project);
        assert_eq!(state.log.len(), 1);
    }

    #[test]
    fn new_project_becomes_active_and_dirty() {
        let mut state = IdeState::new();
        state.new_project("My App");
        assert!(state.has_active());
        assert!(state.dirty);
        assert_eq!(state.active.as_ref().unwrap().name, "My App");
    }

    #[test]
    fn save_and_open_round_trip() {
        let mut state = IdeState::new();
        state.new_project("Round Trip");
        let dir = std::env::temp_dir().join(format!("nocode-ide-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("project.json");

        let saved = state.save_project(Some(path.clone())).unwrap();
        assert_eq!(saved, path);
        assert!(!state.dirty);

        let mut reopened = IdeState::new();
        reopened.open_project(&path).unwrap();
        assert_eq!(reopened.active.as_ref().unwrap().name, "Round Trip");
        assert_eq!(reopened.projects.len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_without_path_errors() {
        let mut state = IdeState::new();
        state.new_project("NoPath");
        assert!(state.save_project(None).is_err());
    }

    #[test]
    fn open_missing_file_errors() {
        let mut state = IdeState::new();
        assert!(state.open_project("does-not-exist-xyz.json").is_err());
    }

    #[test]
    fn log_is_bounded() {
        let mut state = IdeState::new();
        for i in 0..1200 {
            state.log_info(format!("entry {i}"));
        }
        assert!(state.log.len() <= 1000);
    }

    #[test]
    fn view_labels_are_unique() {
        use std::collections::HashSet;
        let labels: HashSet<&str> = View::ALL.iter().map(|v| v.label()).collect();
        assert_eq!(labels.len(), View::ALL.len());
    }

    fn uuid_like() -> String {
        format!("{:?}", std::time::SystemTime::now())
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect()
    }
}
