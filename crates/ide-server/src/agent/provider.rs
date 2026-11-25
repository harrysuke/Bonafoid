//! `ProjectStateProvider` over the IDE's own tables.
//!
//! `crates/mcp` deliberately does not depend on SeaORM: the stdio binary and
//! its unit tests must not drag a database in. This module is the other side of
//! that seam, and it is the only place where a project tool reaches storage.
//!
//! Two invariants come from the trait's contract and are enforced here, not in
//! the tools:
//!
//! - every query is scoped by `tenant_id` **and** `user_id`, both taken from
//!   signed claims;
//! - a project that belongs to someone else is reported as `NotFound`, never
//!   `Forbidden`, so the API cannot be used to probe for existence.

use async_trait::async_trait;
use chrono::Utc;
use sea_orm::DatabaseConnection;

use nocode_mcp::error::McpError;
use nocode_mcp::provider::{LoadedProject, ProjectStateProvider, ProjectSummaryLite};
use nocode_model::Project;

use crate::error::ApiError;
use crate::projects::store;
use crate::versioning::store as revisions;

/// Project storage backed by the `projects` and `project_revisions` tables.
pub struct SeaOrmProjectProvider {
    db: DatabaseConnection,
}

impl SeaOrmProjectProvider {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// The branch AI edits are committed to.
    ///
    /// A constant rather than a parameter: the agent has no branch UI, and a
    /// model-chosen branch name would be an unvalidated string in a SQL value.
    const BRANCH: &'static str = "main";
}

/// Map a storage error onto the MCP error type.
///
/// `NotFound` is preserved because the tools distinguish "no such project" from
/// "the storage broke"; everything else is internal. The message is carried
/// through so the agent can quote it, but it never contains SQL.
fn map_error(error: ApiError) -> McpError {
    match error {
        ApiError::NotFound => McpError::NotFound("project".into()),
        other => McpError::Internal(other.to_string()),
    }
}

#[async_trait]
impl ProjectStateProvider for SeaOrmProjectProvider {
    async fn load_project(
        &self,
        tenant_id: &str,
        user_id: &str,
        project_id: &str,
    ) -> Result<LoadedProject, McpError> {
        let row = store::get_project(&self.db, tenant_id, user_id, project_id)
            .await
            .map_err(map_error)?
            .ok_or_else(|| McpError::NotFound(project_id.to_string()))?;

        let project: Project = serde_json::from_str(&row.data).map_err(|error| {
            // The document is written by this server, so a parse failure means
            // corruption, not user input. Log the id, not the payload.
            tracing::error!(error = %error, project_id, "stored project JSON is corrupt");
            McpError::Internal("stored project JSON is corrupt".into())
        })?;

        Ok(LoadedProject {
            project,
            // `revision` is the row version used for optimistic concurrency.
            revision: row.summary.revision.max(0) as u64,
        })
    }

    async fn save_project(
        &self,
        tenant_id: &str,
        user_id: &str,
        project: &Project,
        expected_revision: u64,
        message: Option<&str>,
    ) -> Result<u64, McpError> {
        let now = Utc::now().to_rfc3339();
        let data = serde_json::to_string(project).map_err(|error| {
            tracing::error!(error = %error, "could not serialise the project");
            McpError::Internal("could not serialise the project".into())
        })?;

        // The guard is the caller's belief about the row, always. This used to
        // accept `None` and read the current revision first, which made the
        // UPDATE guard a tautology — the save could never fail, and a second
        // writer silently clobbered the first. Plan §15.2.1.
        let written = store::update_project(
            &self.db,
            &project.id.to_string(),
            tenant_id,
            user_id,
            &project.name,
            project.description.as_deref(),
            &project.version,
            &data,
            expected_revision as i64,
            &now,
        )
        .await
        .map_err(map_error)?;

        if !written {
            // Either the row moved on or it belongs to someone else. Report the
            // revision conflict: the caller already knows the project exists.
            return Err(McpError::StaleRevision {
                expected: expected_revision,
                current: self.current_revision(tenant_id, user_id, project).await?,
            });
        }

        // Append the commit. A failure here must not fail the save: the project
        // row is already durable and the agent's edits are not lost. The gap in
        // history is logged so an operator can see it.
        let parent = revisions::latest_revision(&self.db, tenant_id, &project.id.to_string())
            .await
            .map_err(map_error)?
            .map(|row| row.id);
        if let Err(error) = revisions::insert_revision(
            &self.db,
            tenant_id,
            &project.id.to_string(),
            parent.as_deref(),
            Self::BRANCH,
            message.unwrap_or("AI edit"),
            user_id,
            &data,
            &now,
        )
        .await
        {
            tracing::warn!(
                error = %error,
                project_id = %project.id,
                "saved the project but could not append its revision"
            );
        }

        Ok(expected_revision + 1)
    }

    async fn list_projects(
        &self,
        tenant_id: &str,
        user_id: &str,
    ) -> Result<Vec<ProjectSummaryLite>, McpError> {
        let rows = store::list_projects(&self.db, tenant_id, user_id)
            .await
            .map_err(map_error)?;
        Ok(rows
            .into_iter()
            .map(|row| ProjectSummaryLite {
                id: row.id,
                name: row.name,
                slug: row.slug,
                revision: row.revision.max(0) as u64,
                updated_at: row.updated_at,
            })
            .collect())
    }
}

impl SeaOrmProjectProvider {
    /// The row's revision right now, for the `current` field of a conflict.
    ///
    /// A read failure here would replace a useful conflict with a confusing
    /// internal error, so it degrades to the expected revision.
    async fn current_revision(
        &self,
        tenant_id: &str,
        user_id: &str,
        project: &Project,
    ) -> Result<u64, McpError> {
        let row = store::get_project(&self.db, tenant_id, user_id, &project.id.to_string())
            .await
            .map_err(map_error)?;
        Ok(row
            .map(|row| row.summary.revision.max(0) as u64)
            .unwrap_or(0))
    }
}
