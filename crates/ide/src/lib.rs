//! No-Code IDE core library: state, commands, backend abstractions and egui app shell.

mod app;
mod backend;
mod bpmn;
mod commands;
mod service;
mod state;

pub use app::IdeApp;
pub use backend::{IdeBackend, NoopBackend};
pub use bpmn::{export_bpmn_xml, import_bpmn, validate_workflow, ValidationIssueDto, WorkflowValidation};
pub use service::{build_sample_project, project_json, GenerationReport, IdeService};
pub use commands::{CommandCategory, CommandDescriptor, CommandRegistry, DEFAULTS};
pub use state::{IdeState, LogEntry, LogLevel, ProjectEntry, Selection, View};
