//! Host services the IDE depends on, abstracted so the UI can run against the
//! real code generator (in the Tauri shell) or a no-op (in tests).

use std::fmt::Debug;
use std::path::Path;

use nocode_model::{Project, Workflow};

/// Summary of a generated project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GenerationReport {
    pub backend_files: usize,
    pub frontend_files: usize,
    pub migrations: usize,
}

impl GenerationReport {
    /// Total number of files written.
    pub fn total_files(&self) -> usize {
        self.backend_files + self.frontend_files + self.migrations
    }
}

/// Services provided by the host application.
pub trait IdeBackend: Debug {
    /// Generate the application code for `project` into `output`.
    fn generate(&mut self, project: &Project, output: &Path) -> Result<GenerationReport, String>;

    /// Validate a workflow, returning human-readable issue messages.
    fn validate_workflow(&self, workflow: &Workflow) -> Vec<String>;
}

/// A backend that does nothing; useful for tests and headless UI embedding.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopBackend;

impl IdeBackend for NoopBackend {
    fn generate(&mut self, _project: &Project, _output: &Path) -> Result<GenerationReport, String> {
        Ok(GenerationReport::default())
    }

    fn validate_workflow(&self, _workflow: &Workflow) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_backend_reports_nothing() {
        let mut backend = NoopBackend;
        let project = Project::new("x");
        let report = backend.generate(&project, Path::new(".")).unwrap();
        assert_eq!(report.total_files(), 0);
    }

    #[test]
    fn report_totals() {
        let report = GenerationReport {
            backend_files: 12,
            frontend_files: 8,
            migrations: 3,
        };
        assert_eq!(report.total_files(), 23);
    }
}
