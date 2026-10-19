//! Raw-SQL persistence for projects.
//!
//! Projects are stored as a single JSON document in the `data` column, with a
//! handful of mirrored columns (`name`, `slug`, `description`, `version`) for
//! listing and uniqueness checks. `revision` backs optimistic concurrency.

use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, Value,
};

use crate::error::ApiError;

const SUMMARY_COLUMNS: &str =
    "id, name, slug, description, version, revision, created_at, updated_at";

/// Listing projection of a project row (no JSON document).
#[derive(Debug, Clone)]
pub struct ProjectSummaryRow {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub version: String,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Full project row including the serialized [`nocode_model::Project`] JSON.
#[derive(Debug, Clone)]
pub struct ProjectRow {
    pub summary: ProjectSummaryRow,
    pub data: String,
}

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn text(value: &str) -> Value {
    Value::from(value.to_string())
}

fn opt_text(value: Option<&str>) -> Value {
    Value::String(value.map(|v| Box::new(v.to_string())))
}

fn map_summary(row: &QueryResult) -> Result<ProjectSummaryRow, ApiError> {
    Ok(ProjectSummaryRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        name: row.try_get("", "name").map_err(ApiError::from)?,
        slug: row.try_get("", "slug").map_err(ApiError::from)?,
        description: row.try_get("", "description").map_err(ApiError::from)?,
        version: row.try_get("", "version").map_err(ApiError::from)?,
        revision: row.try_get("", "revision").map_err(ApiError::from)?,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
        updated_at: row.try_get("", "updated_at").map_err(ApiError::from)?,
    })
}

/// List every project owned by `user_id`, most recently updated first.
pub async fn list_projects(
    db: &DatabaseConnection,
    user_id: &str,
) -> Result<Vec<ProjectSummaryRow>, ApiError> {
    let sql = format!(
        "SELECT {SUMMARY_COLUMNS} FROM projects WHERE user_id = ? \
         ORDER BY updated_at DESC, name ASC"
    );
    let rows = db
        .query_all(statement(&sql, vec![text(user_id)]))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_summary).collect()
}

/// Fetch one project (summary + JSON document) owned by `user_id`.
pub async fn get_project(
    db: &DatabaseConnection,
    user_id: &str,
    id: &str,
) -> Result<Option<ProjectRow>, ApiError> {
    let sql = format!("SELECT {SUMMARY_COLUMNS}, data FROM projects WHERE id = ? AND user_id = ?");
    let row = db
        .query_one(statement(&sql, vec![text(id), text(user_id)]))
        .await
        .map_err(ApiError::from)?;
    match row {
        None => Ok(None),
        Some(row) => {
            let summary = map_summary(&row)?;
            let data: String = row.try_get("", "data").map_err(ApiError::from)?;
            Ok(Some(ProjectRow { summary, data }))
        }
    }
}

/// Insert a new project. `revision` defaults to 1.
#[allow(clippy::too_many_arguments)]
pub async fn insert_project(
    db: &DatabaseConnection,
    id: &str,
    user_id: &str,
    name: &str,
    slug: &str,
    description: Option<&str>,
    version: &str,
    data: &str,
    created_at: &str,
    updated_at: &str,
) -> Result<(), ApiError> {
    let sql = "INSERT INTO projects \
               (id, user_id, name, slug, description, version, data, created_at, updated_at) \
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)";
    db.execute(statement(
        sql,
        vec![
            text(id),
            text(user_id),
            text(name),
            text(slug),
            opt_text(description),
            text(version),
            text(data),
            text(created_at),
            text(updated_at),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Update a project guarded by `expected_revision`. Returns whether a row was
/// written: `false` means the id/revision did not match (missing or stale).
#[allow(clippy::too_many_arguments)]
pub async fn update_project(
    db: &DatabaseConnection,
    id: &str,
    user_id: &str,
    name: &str,
    description: Option<&str>,
    version: &str,
    data: &str,
    expected_revision: i64,
    updated_at: &str,
) -> Result<bool, ApiError> {
    let sql = "UPDATE projects SET \
               name = ?, description = ?, version = ?, data = ?, \
               revision = revision + 1, updated_at = ? \
               WHERE id = ? AND user_id = ? AND revision = ?";
    let result = db
        .execute(statement(
            sql,
            vec![
                text(name),
                opt_text(description),
                text(version),
                text(data),
                text(updated_at),
                text(id),
                text(user_id),
                Value::from(expected_revision),
            ],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

/// Delete a project owned by `user_id`. Returns whether a row was removed.
pub async fn delete_project(
    db: &DatabaseConnection,
    user_id: &str,
    id: &str,
) -> Result<bool, ApiError> {
    let sql = "DELETE FROM projects WHERE id = ? AND user_id = ?";
    let result = db
        .execute(statement(sql, vec![text(id), text(user_id)]))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

/// Whether `slug` is already taken by another project of `user_id`.
pub async fn slug_exists(
    db: &DatabaseConnection,
    user_id: &str,
    slug: &str,
    exclude_id: Option<&str>,
) -> Result<bool, ApiError> {
    let (sql, values) = match exclude_id {
        Some(exclude) => (
            "SELECT COUNT(*) AS count FROM projects \
             WHERE user_id = ? AND slug = ? AND id <> ?",
            vec![text(user_id), text(slug), text(exclude)],
        ),
        None => (
            "SELECT COUNT(*) AS count FROM projects WHERE user_id = ? AND slug = ?",
            vec![text(user_id), text(slug)],
        ),
    };
    let row = db
        .query_one(statement(sql, values))
        .await
        .map_err(ApiError::from)?;
    let count: i64 = match row {
        Some(row) => row.try_get("", "count").map_err(ApiError::from)?,
        None => 0,
    };
    Ok(count > 0)
}

/// Derive a slug unique to `user_id`, appending `-2`, `-3`, ... on collision.
pub async fn unique_slug(
    db: &DatabaseConnection,
    user_id: &str,
    base: &str,
    exclude_id: Option<&str>,
) -> Result<String, ApiError> {
    let base = if base.is_empty() { "project" } else { base };
    if !slug_exists(db, user_id, base, exclude_id).await? {
        return Ok(base.to_string());
    }
    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if !slug_exists(db, user_id, &candidate, exclude_id).await? {
            return Ok(candidate);
        }
        suffix += 1;
    }
}
