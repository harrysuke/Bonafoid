//! No-Code IDE core library: state, commands, backend abstractions and egui app shell.

pub mod app;
pub mod backend;
pub mod bpmn;
pub mod commands;
pub mod service;
pub mod state;

pub use app::IdeApp;
pub use backend::{IdeBackend, NoopBackend};
pub use bpmn::{
    export_bpmn_xml, import_bpmn, run_workflow, validate_workflow, OpenTask, RunEvent, RunOptions,
    RunStep, RunSummary, ValidationIssueDto, WorkflowValidation,
};
pub use commands::{CommandCategory, CommandDescriptor, CommandRegistry, DEFAULTS};
pub use service::{build_sample_project, project_json, GenerationReport, IdeService};
pub use state::{IdeState, LogEntry, LogLevel, ProjectEntry, Selection, View};
