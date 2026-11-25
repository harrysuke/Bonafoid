//! The one async seam between the tool executor and project storage.
//!
//! `crates/mcp` must not depend on SeaORM (the stdio binary and the unit tests
//! would drag a database in), so the executor talks to this trait instead.
//! `nocode-ide-server` implements it over its existing project tables; tests
//! implement it over a `HashMap`.
//!
//! Every method takes `tenant_id` *and* `user_id`. A provider must scope its
//! query by both: a project belonging to another tenant is reported as
//! "not found", never "forbidden", so the API cannot be used to probe for
//! existence.

use async_trait::async_trait;
use nocode_model::Project;

use crate::error::McpError;

/// A project as loaded for editing, plus the revision it came from.
#[derive(Debug, Clone)]
pub struct LoadedProject {
    pub project: Project,
    /// The database revision, used to detect a concurrent save.
    pub revision: u64,
}

/// A project as listed for discovery. Deliberately tiny: this is what the
/// agent may see without loading the whole model.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectSummaryLite {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub revision: u64,
    pub updated_at: String,
}

/// Storage for projects, implemented by the IDE server.
#[async_trait]
pub trait ProjectStateProvider: Send + Sync {
    /// Load one project, scoped to `tenant_id` + `user_id`.
    async fn load_project(
        &self,
        tenant_id: &str,
        user_id: &str,
        project_id: &str,
    ) -> Result<LoadedProject, McpError>;

    /// Persist the project and append a revision. Returns the new revision.
    ///
    /// `expected_revision` is the row version the caller believes it is
    /// writing over; a provider **must** refuse with
    /// [`McpError::StaleRevision`] when the row has moved on.
    ///
    /// It is not an `Option` on purpose. The previous signature accepted
    /// `None`, which the IDE server implemented as "read the current revision,
    /// then write against it" — a guard computed from the row it was about to
    /// overwrite, so the save could never fail. Making the argument mandatory
    /// makes that escape hatch unexpressible rather than merely discouraged
    /// (plan §15.2.1).
    async fn save_project(
        &self,
        tenant_id: &str,
        user_id: &str,
        project: &Project,
        expected_revision: u64,
        message: Option<&str>,
    ) -> Result<u64, McpError>;

    /// Projects visible to the caller, newest first.
    async fn list_projects(
        &self,
        tenant_id: &str,
        user_id: &str,
    ) -> Result<Vec<ProjectSummaryLite>, McpError>;
}

/// The error returned when a project tool is called without a provider.
///
/// This is a configuration fact, not a bug: the stdio binary has no database,
/// and the plan (§5.2) requires a typed, actionable message rather than a
/// generic failure.
pub fn provider_required() -> McpError {
    McpError::Forbidden("project tools require an IDE server connection".into())
}
