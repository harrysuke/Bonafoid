//! Raw-SQL persistence for tenants and memberships.
//!
//! Every query is tenant-scoped. Membership is the ONLY source of a user's
//! roles; a caller-supplied tenant id is never trusted for authorization.

use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, Value,
};

use super::schema::DEFAULT_TENANT_ID;
use crate::error::ApiError;

/// A tenant row.
#[derive(Debug, Clone)]
pub struct TenantRow {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub plan_id: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A membership row.
#[derive(Debug, Clone)]
pub struct MemberRow {
    pub tenant_id: String,
    pub user_id: String,
    pub email: String,
    pub role: String,
    pub created_at: String,
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

fn map_tenant(row: &QueryResult) -> Result<TenantRow, ApiError> {
    Ok(TenantRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        name: row.try_get("", "name").map_err(ApiError::from)?,
        slug: row.try_get("", "slug").map_err(ApiError::from)?,
        plan_id: row.try_get("", "plan_id").map_err(ApiError::from)?,
        status: row.try_get("", "status").map_err(ApiError::from)?,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
        updated_at: row.try_get("", "updated_at").map_err(ApiError::from)?,
    })
}

fn map_member(row: &QueryResult) -> Result<MemberRow, ApiError> {
    Ok(MemberRow {
        tenant_id: row.try_get("", "tenant_id").map_err(ApiError::from)?,
        user_id: row.try_get("", "user_id").map_err(ApiError::from)?,
        email: row.try_get("", "email").map_err(ApiError::from)?,
        role: row.try_get("", "role").map_err(ApiError::from)?,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
    })
}

/// List tenants the user belongs to.
pub async fn list_for_user(
    db: &DatabaseConnection,
    user_id: &str,
) -> Result<Vec<TenantRow>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT t.id, t.name, t.slug, t.plan_id, t.status, t.created_at, t.updated_at \
             FROM tenants t JOIN tenant_members m ON m.tenant_id = t.id \
             WHERE m.user_id = ? ORDER BY t.name ASC",
            vec![text(user_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_tenant).collect()
}

/// Fetch a tenant by id. Caller MUST have checked membership first.
pub async fn get(db: &DatabaseConnection, id: &str) -> Result<Option<TenantRow>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT id, name, slug, plan_id, status, created_at, updated_at FROM tenants WHERE id = ?",
            vec![text(id)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_tenant).transpose()
}

/// The default tenant for a user who has no membership yet (legacy rows).
pub async fn ensure_default_membership(
    db: &DatabaseConnection,
    user_id: &str,
) -> Result<(), ApiError> {
    db.execute(statement(
        "INSERT OR IGNORE INTO tenant_members (tenant_id, user_id, role) VALUES (?, ?, 'owner')",
        vec![text(DEFAULT_TENANT_ID), text(user_id)],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Insert a tenant. `id` is a uuid; the creating user is added as owner by the
/// caller in the SAME handler, so a tenant is never left without an owner.
pub async fn insert(db: &DatabaseConnection, row: &TenantRow) -> Result<(), ApiError> {
    db.execute(statement(
        "INSERT INTO tenants (id, name, slug, plan_id, status) VALUES (?, ?, ?, ?, ?)",
        vec![
            text(&row.id),
            text(&row.name),
            text(&row.slug),
            opt_text(row.plan_id.as_deref()),
            text(&row.status),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Update the mutable tenant fields, scoped by tenant id.
pub async fn update(
    db: &DatabaseConnection,
    id: &str,
    name: &str,
    status: &str,
) -> Result<bool, ApiError> {
    let result = db
        .execute(statement(
            "UPDATE tenants SET name = ?, status = ?, updated_at = datetime('now') WHERE id = ?",
            vec![text(name), text(status), text(id)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

/// A user's role in a tenant, or None when they are not a member.
pub async fn role_in(
    db: &DatabaseConnection,
    tenant_id: &str,
    user_id: &str,
) -> Result<Option<String>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT role FROM tenant_members WHERE tenant_id = ? AND user_id = ?",
            vec![text(tenant_id), text(user_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.map(|row| row.try_get::<String>("", "role").map_err(ApiError::from))
        .transpose()
}

/// All roles a user holds, across every tenant (used when minting a token).
pub async fn roles_for_user(
    db: &DatabaseConnection,
    user_id: &str,
) -> Result<Vec<String>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT role FROM tenant_members WHERE user_id = ? ORDER BY role",
            vec![text(user_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.into_iter()
        .map(|row| row.try_get::<String>("", "role").map_err(ApiError::from))
        .collect()
}

/// Members of a tenant with their email addresses.
pub async fn list_members(
    db: &DatabaseConnection,
    tenant_id: &str,
) -> Result<Vec<MemberRow>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT m.tenant_id, m.user_id, u.email, m.role, m.created_at \
             FROM tenant_members m JOIN users u ON u.id = m.user_id \
             WHERE m.tenant_id = ? ORDER BY m.created_at, u.email",
            vec![text(tenant_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_member).collect()
}

/// Add or replace a membership. Returns false when the user does not exist.
pub async fn set_member(
    db: &DatabaseConnection,
    tenant_id: &str,
    user_id: &str,
    role: &str,
) -> Result<bool, ApiError> {
    let exists = db
        .query_one(statement(
            "SELECT 1 FROM users WHERE id = ?",
            vec![text(user_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    if exists.is_none() {
        return Ok(false);
    }
    db.execute(statement(
        "INSERT INTO tenant_members (tenant_id, user_id, role) VALUES (?, ?, ?) \
         ON CONFLICT (tenant_id, user_id) DO UPDATE SET role = excluded.role",
        vec![text(tenant_id), text(user_id), text(role)],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(true)
}

/// Remove a membership. The last owner of a tenant cannot be removed; that
/// invariant is enforced here so it holds for every caller.
pub async fn remove_member(
    db: &DatabaseConnection,
    tenant_id: &str,
    user_id: &str,
) -> Result<bool, ApiError> {
    let role = role_in(db, tenant_id, user_id).await?;
    if role.as_deref() == Some("owner") {
        let owners = db
            .query_one(statement(
                "SELECT COUNT(*) AS count FROM tenant_members WHERE tenant_id = ? AND role = 'owner'",
                vec![text(tenant_id)],
            ))
            .await
            .map_err(ApiError::from)?;
        let count: i64 = owners
            .map(|row| row.try_get("", "count").map_err(ApiError::from))
            .transpose()?
            .unwrap_or(0);
        if count <= 1 {
            return Err(ApiError::Conflict(
                "the last owner cannot be removed from a tenant".to_string(),
            ));
        }
    }
    let result = db
        .execute(statement(
            "DELETE FROM tenant_members WHERE tenant_id = ? AND user_id = ?",
            vec![text(tenant_id), text(user_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

/// Read a user's platform role (`Some("super_admin")` or None).
pub async fn platform_role(
    db: &DatabaseConnection,
    user_id: &str,
) -> Result<Option<String>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT platform_role FROM users WHERE id = ?",
            vec![text(user_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(row
        .and_then(|row| row.try_get::<Option<String>>("", "platform_role").ok())
        .flatten()
        .filter(|value| !value.is_empty()))
}

/// Set a user's platform role. `None` clears it (an ordinary user).
pub async fn set_platform_role(
    db: &DatabaseConnection,
    user_id: &str,
    role: Option<&str>,
) -> Result<(), ApiError> {
    db.execute(statement(
        "UPDATE users SET platform_role = ?, updated_at = datetime('now') WHERE id = ?",
        vec![opt_text(role), text(user_id)],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Set a user's active tenant. Must be a tenant they belong to.
pub async fn set_active_tenant(
    db: &DatabaseConnection,
    user_id: &str,
    tenant_id: &str,
) -> Result<(), ApiError> {
    db.execute(statement(
        "UPDATE users SET tenant_id = ?, updated_at = datetime('now') WHERE id = ?",
        vec![text(tenant_id), text(user_id)],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// The user's home tenant id, used when minting claims.
pub async fn active_tenant(db: &DatabaseConnection, user_id: &str) -> Result<String, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT tenant_id FROM users WHERE id = ?",
            vec![text(user_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(row
        .and_then(|row| row.try_get::<String>("", "tenant_id").ok())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_TENANT_ID.to_string()))
}
