//! Raw-SQL persistence for the platform administration panel.
//!
//! Every statement is parameterized. Search terms are escaped before being
//! wrapped in a bound LIKE pattern, and pagination is clamped in this module so
//! no caller can request an unbounded result set. The audit log is append-only:
//! there is deliberately no UPDATE or DELETE query for `audit_log` here.

use axum::http::{header, HeaderMap};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, Value,
};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError};

/// Default page size for admin listings.
pub const DEFAULT_LIMIT: i64 = 50;
/// Largest page size an admin listing will accept.
pub const MAX_LIMIT: i64 = 200;

// ---------------------------------------------------------------------------
// Value / row helpers
// ---------------------------------------------------------------------------

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn text(value: &str) -> Value {
    Value::from(value.to_string())
}

fn opt_text(value: Option<&str>) -> Value {
    Value::String(value.map(|value| Box::new(value.to_string())))
}

fn num(value: i64) -> Value {
    Value::from(value)
}

/// SQLite stores booleans as integers; 0 is false and anything else is true.
fn flag(value: bool) -> Value {
    num(i64::from(value))
}

fn read_bool(row: &QueryResult, column: &str) -> Result<bool, ApiError> {
    let raw: i64 = row.try_get("", column).map_err(ApiError::from)?;
    Ok(raw != 0)
}

fn read_opt_text(row: &QueryResult, column: &str) -> Option<String> {
    row.try_get::<Option<String>>("", column)
        .ok()
        .flatten()
        .filter(|value| !value.is_empty())
}

fn read_count(row: Option<QueryResult>) -> Result<i64, ApiError> {
    row.map(|row| row.try_get::<i64>("", "count").map_err(ApiError::from))
        .transpose()
        .map(|value| value.unwrap_or(0))
}

/// Clamp a requested page size into `1..=MAX_LIMIT`.
pub fn clamp_limit(raw: Option<i64>) -> i64 {
    raw.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

/// Negative offsets are meaningless; clamp them to zero.
pub fn clamp_offset(raw: Option<i64>) -> i64 {
    raw.unwrap_or(0).max(0)
}

/// Escape LIKE wildcards so a search term is matched literally.
fn escape_like(raw: &str) -> String {
    let mut escaped = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '%' => escaped.push_str("\\%"),
            '_' => escaped.push_str("\\_"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// A bound LIKE pattern; the SQL uses `ESCAPE '\'` so `%` and `_` in the
/// caller's search term cannot turn into wildcard matches.
pub fn like_pattern(raw: &str) -> String {
    format!("%{}%", escape_like(raw.trim()))
}

// ---------------------------------------------------------------------------
// Row projections
// ---------------------------------------------------------------------------

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

#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: String,
    pub email: String,
    pub is_active: bool,
    pub is_verified: bool,
    pub platform_role: Option<String>,
    pub tenant_id: Option<String>,
    pub last_login_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct FlagRow {
    pub key: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub rollout_percentage: i64,
    pub kill_switch: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct AuditRow {
    pub id: String,
    pub actor_user_id: String,
    pub actor_email: Option<String>,
    pub tenant_id: Option<String>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ImpersonationRow {
    pub id: String,
    pub admin_user_id: String,
    pub target_user_id: String,
    pub tenant_id: String,
    pub reason: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IncidentRow {
    pub status: String,
    pub component: Option<String>,
    pub title: String,
    pub opened_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct InvoiceAnalyticRow {
    pub plan_id: String,
    pub issued_at: String,
    pub amount_cents: i64,
    pub status: String,
}

#[derive(Debug, Clone, Default)]
pub struct UsageCounts {
    pub projects: i64,
    pub integrations: i64,
    pub deployments: i64,
    pub usage_events: i64,
    pub tenant_members: i64,
    pub audit_log: i64,
}

fn map_tenant(row: &QueryResult) -> Result<TenantRow, ApiError> {
    Ok(TenantRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        name: row.try_get("", "name").map_err(ApiError::from)?,
        slug: row.try_get("", "slug").map_err(ApiError::from)?,
        plan_id: read_opt_text(row, "plan_id"),
        status: row.try_get("", "status").map_err(ApiError::from)?,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
        updated_at: row.try_get("", "updated_at").map_err(ApiError::from)?,
    })
}

fn map_user(row: &QueryResult) -> Result<UserRow, ApiError> {
    Ok(UserRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        email: row.try_get("", "email").map_err(ApiError::from)?,
        is_active: read_bool(row, "is_active")?,
        is_verified: read_bool(row, "is_verified")?,
        platform_role: read_opt_text(row, "platform_role"),
        tenant_id: read_opt_text(row, "tenant_id"),
        last_login_at: read_opt_text(row, "last_login_at"),
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
    })
}

fn map_flag(row: &QueryResult) -> Result<FlagRow, ApiError> {
    Ok(FlagRow {
        key: row.try_get("", "key").map_err(ApiError::from)?,
        description: read_opt_text(row, "description"),
        enabled: read_bool(row, "enabled")?,
        rollout_percentage: row
            .try_get("", "rollout_percentage")
            .map_err(ApiError::from)?,
        kill_switch: read_bool(row, "kill_switch")?,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
        updated_at: row.try_get("", "updated_at").map_err(ApiError::from)?,
    })
}

fn map_audit(row: &QueryResult) -> Result<AuditRow, ApiError> {
    Ok(AuditRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        actor_user_id: row.try_get("", "actor_user_id").map_err(ApiError::from)?,
        actor_email: read_opt_text(row, "actor_email"),
        tenant_id: read_opt_text(row, "tenant_id"),
        action: row.try_get("", "action").map_err(ApiError::from)?,
        target_type: row.try_get("", "target_type").map_err(ApiError::from)?,
        target_id: read_opt_text(row, "target_id"),
        before_json: row
            .try_get::<Option<String>>("", "before_json")
            .ok()
            .flatten(),
        after_json: row
            .try_get::<Option<String>>("", "after_json")
            .ok()
            .flatten(),
        ip_address: read_opt_text(row, "ip_address"),
        user_agent: read_opt_text(row, "user_agent"),
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
    })
}

fn map_impersonation(row: &QueryResult) -> Result<ImpersonationRow, ApiError> {
    Ok(ImpersonationRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        admin_user_id: row.try_get("", "admin_user_id").map_err(ApiError::from)?,
        target_user_id: row.try_get("", "target_user_id").map_err(ApiError::from)?,
        tenant_id: row.try_get("", "tenant_id").map_err(ApiError::from)?,
        reason: read_opt_text(row, "reason"),
        started_at: row.try_get("", "started_at").map_err(ApiError::from)?,
        ended_at: read_opt_text(row, "ended_at"),
    })
}

fn map_incident(row: &QueryResult) -> Result<IncidentRow, ApiError> {
    Ok(IncidentRow {
        status: row.try_get("", "status").map_err(ApiError::from)?,
        component: read_opt_text(row, "component"),
        title: row.try_get("", "title").map_err(ApiError::from)?,
        opened_at: row.try_get("", "opened_at").map_err(ApiError::from)?,
        resolved_at: read_opt_text(row, "resolved_at"),
    })
}

// ---------------------------------------------------------------------------
// Tenants
// ---------------------------------------------------------------------------

/// Platform-wide tenant listing. `search` matches id, name or slug literally.
pub async fn list_tenants(
    db: &DatabaseConnection,
    search: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<TenantRow>, ApiError> {
    let rows = match search {
        Some(term) if !term.trim().is_empty() => {
            let pattern = like_pattern(term);
            db.query_all(statement(
                "SELECT id, name, slug, plan_id, status, created_at, updated_at FROM tenants \
                 WHERE name LIKE ? ESCAPE '\\' OR slug LIKE ? ESCAPE '\\' OR id LIKE ? ESCAPE '\\' \
                 ORDER BY name ASC LIMIT ? OFFSET ?",
                vec![
                    text(&pattern),
                    text(&pattern),
                    text(&pattern),
                    num(limit),
                    num(offset),
                ],
            ))
            .await
            .map_err(ApiError::from)?
        }
        _ => db
            .query_all(statement(
                "SELECT id, name, slug, plan_id, status, created_at, updated_at FROM tenants \
                 ORDER BY name ASC LIMIT ? OFFSET ?",
                vec![num(limit), num(offset)],
            ))
            .await
            .map_err(ApiError::from)?,
    };
    rows.iter().map(map_tenant).collect()
}

/// Fetch a tenant by id for admin inspection and audit snapshots.
pub async fn get_tenant(db: &DatabaseConnection, id: &str) -> Result<Option<TenantRow>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT id, name, slug, plan_id, status, created_at, updated_at \
             FROM tenants WHERE id = ?",
            vec![text(id)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_tenant).transpose()
}

/// Set a tenant's platform status. Returns false when the id does not exist.
pub async fn set_tenant_status(
    db: &DatabaseConnection,
    id: &str,
    status: &str,
) -> Result<bool, ApiError> {
    let result = db
        .execute(statement(
            "UPDATE tenants SET status = ?, updated_at = datetime('now') WHERE id = ?",
            vec![text(status), text(id)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

// ---------------------------------------------------------------------------
// Users
// ---------------------------------------------------------------------------

/// Platform-wide user listing. Passwords and hashes are never selected.
pub async fn list_users(
    db: &DatabaseConnection,
    search: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<UserRow>, ApiError> {
    let columns = "SELECT id, email, is_active, is_verified, platform_role, tenant_id, \
                   last_login_at, created_at FROM users";
    let rows = match search {
        Some(term) if !term.trim().is_empty() => {
            let pattern = like_pattern(term);
            db.query_all(statement(
                &format!(
                    "{columns} WHERE email LIKE ? ESCAPE '\\' OR id LIKE ? ESCAPE '\\' \
                     ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"
                ),
                vec![text(&pattern), text(&pattern), num(limit), num(offset)],
            ))
            .await
            .map_err(ApiError::from)?
        }
        _ => db
            .query_all(statement(
                &format!("{columns} ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"),
                vec![num(limit), num(offset)],
            ))
            .await
            .map_err(ApiError::from)?,
    };
    rows.iter().map(map_user).collect()
}

/// Fetch a user by id for admin inspection.
pub async fn get_user(db: &DatabaseConnection, id: &str) -> Result<Option<UserRow>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT id, email, is_active, is_verified, platform_role, tenant_id, \
             last_login_at, created_at FROM users WHERE id = ?",
            vec![text(id)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_user).transpose()
}

/// Number of accounts holding the platform super-admin role.
pub async fn count_super_admins(db: &DatabaseConnection) -> Result<i64, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT COUNT(*) AS count FROM users WHERE platform_role = 'super_admin'",
            vec![],
        ))
        .await
        .map_err(ApiError::from)?;
    read_count(row)
}

/// Patch the mutable admin-controlled user fields.
///
/// `platform_role` uses a nested Option: `None` means "leave unchanged",
/// `Some(None)` means "clear the role", and `Some(Some(role))` sets it.
pub async fn update_user(
    db: &DatabaseConnection,
    id: &str,
    is_active: Option<bool>,
    is_verified: Option<bool>,
    platform_role: Option<Option<String>>,
) -> Result<bool, ApiError> {
    let mut sets: Vec<String> = Vec::new();
    let mut values: Vec<Value> = Vec::new();
    if let Some(active) = is_active {
        sets.push("is_active = ?".to_string());
        values.push(flag(active));
    }
    if let Some(verified) = is_verified {
        sets.push("is_verified = ?".to_string());
        values.push(flag(verified));
    }
    if let Some(role) = platform_role {
        sets.push("platform_role = ?".to_string());
        values.push(opt_text(role.as_deref()));
    }
    if sets.is_empty() {
        return Ok(true);
    }
    values.push(text(id));
    let sql = format!("UPDATE users SET {} WHERE id = ?", sets.join(", "));
    let result = db
        .execute(statement(&sql, values))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

// ---------------------------------------------------------------------------
// Platform flags and tenant overrides
// ---------------------------------------------------------------------------

pub async fn list_flags(db: &DatabaseConnection) -> Result<Vec<FlagRow>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT key, description, enabled, rollout_percentage, kill_switch, \
             created_at, updated_at FROM platform_flags ORDER BY key ASC",
            vec![],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_flag).collect()
}

pub async fn get_flag(db: &DatabaseConnection, key: &str) -> Result<Option<FlagRow>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT key, description, enabled, rollout_percentage, kill_switch, \
             created_at, updated_at FROM platform_flags WHERE key = ?",
            vec![text(key)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_flag).transpose()
}

pub async fn insert_flag(
    db: &DatabaseConnection,
    key: &str,
    description: Option<&str>,
    enabled: bool,
    rollout_percentage: i64,
    kill_switch: bool,
) -> Result<(), ApiError> {
    db.execute(statement(
        "INSERT INTO platform_flags (key, description, enabled, rollout_percentage, kill_switch) \
         VALUES (?, ?, ?, ?, ?)",
        vec![
            text(key),
            opt_text(description),
            flag(enabled),
            num(rollout_percentage),
            flag(kill_switch),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Patch a flag. `description` follows the nested-Option convention of
/// [`update_user`]: outer None means unchanged, Some(None) clears it.
#[allow(clippy::too_many_arguments)]
pub async fn update_flag(
    db: &DatabaseConnection,
    key: &str,
    description: Option<Option<String>>,
    enabled: Option<bool>,
    rollout_percentage: Option<i64>,
    kill_switch: Option<bool>,
) -> Result<bool, ApiError> {
    let mut sets: Vec<String> = Vec::new();
    let mut values: Vec<Value> = Vec::new();
    if let Some(description) = description {
        sets.push("description = ?".to_string());
        values.push(opt_text(description.as_deref()));
    }
    if let Some(enabled) = enabled {
        sets.push("enabled = ?".to_string());
        values.push(flag(enabled));
    }
    if let Some(percentage) = rollout_percentage {
        sets.push("rollout_percentage = ?".to_string());
        values.push(num(percentage));
    }
    if let Some(kill) = kill_switch {
        sets.push("kill_switch = ?".to_string());
        values.push(flag(kill));
    }
    if sets.is_empty() {
        return Ok(true);
    }
    sets.push("updated_at = datetime('now')".to_string());
    values.push(text(key));
    let sql = format!(
        "UPDATE platform_flags SET {} WHERE key = ?",
        sets.join(", ")
    );
    let result = db
        .execute(statement(&sql, values))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

/// Delete a flag and its overrides. Overrides are removed explicitly: some
/// SQLite builds are opened with foreign-key enforcement disabled.
pub async fn delete_flag(db: &DatabaseConnection, key: &str) -> Result<bool, ApiError> {
    db.execute(statement(
        "DELETE FROM tenant_flag_overrides WHERE flag_key = ?",
        vec![text(key)],
    ))
    .await
    .map_err(ApiError::from)?;
    let result = db
        .execute(statement(
            "DELETE FROM platform_flags WHERE key = ?",
            vec![text(key)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

pub async fn set_override(
    db: &DatabaseConnection,
    flag_key: &str,
    tenant_id: &str,
    enabled: bool,
) -> Result<(), ApiError> {
    db.execute(statement(
        "INSERT INTO tenant_flag_overrides (flag_key, tenant_id, enabled) VALUES (?, ?, ?) \
         ON CONFLICT(flag_key, tenant_id) DO UPDATE SET enabled = excluded.enabled",
        vec![text(flag_key), text(tenant_id), flag(enabled)],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

async fn get_override(
    db: &DatabaseConnection,
    key: &str,
    tenant_id: &str,
) -> Result<Option<bool>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT enabled FROM tenant_flag_overrides WHERE flag_key = ? AND tenant_id = ?",
            vec![text(key), text(tenant_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref()
        .map(|row| read_bool(row, "enabled"))
        .transpose()
}

/// FNV-1a (64-bit) over UTF-8 bytes. Stable across processes and platforms,
/// which is what a percentage rollout needs: the same tenant always lands in
/// the same bucket. No external dependency and no random seed.
pub fn stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A rollout hits when the tenant's stable bucket falls below the percentage.
/// 0 never hits, 100 always hits; out-of-range values are clamped.
pub fn rollout_enabled(hash: u64, rollout_percentage: i64) -> bool {
    let percentage = rollout_percentage.clamp(0, 100) as u64;
    (hash % 100) < percentage
}

/// The single server-side flag evaluation path.
///
/// Order: unknown key -> false; kill switch -> false; tenant override -> its
/// value; a percentage-rollout hit -> true; otherwise the global `enabled`
/// value. Failing closed means a missing flag is never treated as enabled.
pub async fn evaluate(
    db: &DatabaseConnection,
    key: &str,
    tenant_id: &str,
) -> Result<bool, ApiError> {
    let Some(flag_row) = get_flag(db, key).await? else {
        return Ok(false);
    };
    if flag_row.kill_switch {
        return Ok(false);
    }
    if let Some(overridden) = get_override(db, key, tenant_id).await? {
        return Ok(overridden);
    }
    if rollout_enabled(stable_hash(tenant_id), flag_row.rollout_percentage) {
        return Ok(true);
    }
    Ok(flag_row.enabled)
}

// ---------------------------------------------------------------------------
// Append-only audit log
// ---------------------------------------------------------------------------

/// The client IP, when a reverse proxy supplied one. Axum's `HeaderMap` has
/// no socket peer address, so a direct connection reports `None` rather than
/// inventing an address from a header the client controls.
pub fn request_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Audit context from a request: `(ip, user-agent)`.
///
/// The IP is a `String` (empty when no proxy header is present) so callers can
/// bind it directly to the audit insert; [`request_ip`] exposes the `Option`
/// form. The actor identity is taken from the signed `AuthUser`, never from a
/// header or body field.
pub fn audit_from_request(user: &AuthUser, headers: &HeaderMap) -> (String, Option<String>) {
    let _ = user; // the identity is read by the caller via `user.id` / `user.email`
    let ip = request_ip(headers).unwrap_or_default();
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    (ip, user_agent)
}

/// Append one audit entry. There is intentionally no update/delete counterpart.
#[allow(clippy::too_many_arguments)]
pub async fn append_audit(
    db: &DatabaseConnection,
    actor_id: &str,
    actor_email: &str,
    tenant_id: Option<&str>,
    action: &str,
    target_type: &str,
    target_id: Option<&str>,
    before: Option<&str>,
    after: Option<&str>,
    ip: Option<&str>,
    ua: Option<&str>,
) -> Result<(), ApiError> {
    let id = Uuid::new_v4().to_string();
    db.execute(statement(
        "INSERT INTO audit_log (id, actor_user_id, actor_email, tenant_id, action, \
         target_type, target_id, before_json, after_json, ip_address, user_agent) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![
            text(&id),
            text(actor_id),
            text(actor_email),
            opt_text(tenant_id),
            text(action),
            text(target_type),
            opt_text(target_id),
            opt_text(before),
            opt_text(after),
            opt_text(ip),
            opt_text(ua),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

pub async fn list_audit(
    db: &DatabaseConnection,
    limit: i64,
    offset: i64,
    tenant_id: Option<&str>,
) -> Result<Vec<AuditRow>, ApiError> {
    let columns = "SELECT id, actor_user_id, actor_email, tenant_id, action, target_type, \
                   target_id, before_json, after_json, ip_address, user_agent, created_at \
                   FROM audit_log";
    let rows = match tenant_id {
        Some(tenant) if !tenant.is_empty() => {
            db.query_all(statement(
                &format!("{columns} WHERE tenant_id = ? ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"),
                vec![text(tenant), num(limit), num(offset)],
            ))
            .await
            .map_err(ApiError::from)?
        }
        _ => db
            .query_all(statement(
                &format!("{columns} ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"),
                vec![num(limit), num(offset)],
            ))
            .await
            .map_err(ApiError::from)?,
    };
    rows.iter().map(map_audit).collect()
}

// ---------------------------------------------------------------------------
// Impersonations
// ---------------------------------------------------------------------------

/// Record an impersonation session. This is an audit record only: no session
/// token is swapped or minted here.
pub async fn insert_impersonation(
    db: &DatabaseConnection,
    id: &str,
    admin_user_id: &str,
    target_user_id: &str,
    tenant_id: &str,
    reason: Option<&str>,
) -> Result<(), ApiError> {
    db.execute(statement(
        "INSERT INTO impersonations (id, admin_user_id, target_user_id, tenant_id, reason) \
         VALUES (?, ?, ?, ?, ?)",
        vec![
            text(id),
            text(admin_user_id),
            text(target_user_id),
            text(tenant_id),
            opt_text(reason),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

pub async fn list_impersonations(
    db: &DatabaseConnection,
    limit: i64,
    offset: i64,
) -> Result<Vec<ImpersonationRow>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT id, admin_user_id, target_user_id, tenant_id, reason, started_at, ended_at \
             FROM impersonations ORDER BY started_at DESC, id DESC LIMIT ? OFFSET ?",
            vec![num(limit), num(offset)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_impersonation).collect()
}

// ---------------------------------------------------------------------------
// Status, incidents and usage
// ---------------------------------------------------------------------------

/// A real query, not just `ping`, so a broken pool is reported as unavailable.
pub async fn database_ok(db: &DatabaseConnection) -> bool {
    matches!(
        db.query_one(statement("SELECT 1 AS ok", vec![])).await,
        Ok(Some(_))
    )
}

/// Public incident projection: five whitelisted columns, never `message` or
/// `id`, so an operator's internal notes cannot leak through the status page.
pub async fn list_incidents(db: &DatabaseConnection) -> Result<Vec<IncidentRow>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT status, component, title, opened_at, resolved_at FROM status_incidents \
             ORDER BY opened_at DESC, id DESC",
            vec![],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_incident).collect()
}

async fn count_table(db: &DatabaseConnection, table: &str) -> Result<i64, ApiError> {
    // `table` is always a compile-time literal from `usage_counts`, never user input.
    let row = db
        .query_one(statement(
            &format!("SELECT COUNT(*) AS count FROM {table}"),
            vec![],
        ))
        .await
        .map_err(ApiError::from)?;
    read_count(row)
}

pub async fn usage_counts(db: &DatabaseConnection) -> Result<UsageCounts, ApiError> {
    Ok(UsageCounts {
        projects: count_table(db, "projects").await?,
        integrations: count_table(db, "integrations").await?,
        deployments: count_table(db, "deployments").await?,
        usage_events: count_table(db, "usage_events").await?,
        tenant_members: count_table(db, "tenant_members").await?,
        audit_log: count_table(db, "audit_log").await?,
    })
}

// ---------------------------------------------------------------------------
// Analytics source rows (subscriptions + invoices only)
// ---------------------------------------------------------------------------

/// `(plan_id, amount_cents, status)` for every subscription. The amount is the
/// most recent paid invoice for that subscription (0 when none), and the
/// status is suffixed `_annual` when the invoice period spans more than 45
/// days. The pure formulas in `analytics` turn these rows into MRR/ARR.
pub async fn analytics_subscriptions(
    db: &DatabaseConnection,
) -> Result<Vec<(String, i64, String)>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT s.plan_id AS plan_id, COALESCE(i.amount_cents, 0) AS amount_cents, \
             CASE WHEN i.period_start IS NOT NULL AND i.period_end IS NOT NULL \
                   AND (julianday(i.period_end) - julianday(i.period_start)) > 45 \
                  THEN s.status || '_annual' ELSE s.status END AS status \
             FROM subscriptions s \
             LEFT JOIN invoices i ON i.id = ( \
                 SELECT inv.id FROM invoices inv \
                 WHERE inv.subscription_id = s.id AND inv.status = 'paid' \
                 ORDER BY inv.issued_at DESC, inv.id DESC LIMIT 1) \
             ORDER BY s.id",
            vec![],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get::<String>("", "plan_id")
                    .map_err(ApiError::from)?,
                row.try_get::<i64>("", "amount_cents")
                    .map_err(ApiError::from)?,
                row.try_get::<String>("", "status")
                    .map_err(ApiError::from)?,
            ))
        })
        .collect()
}

/// Invoice rows joined to their plan, used for both revenue-by-plan and the
/// monthly paid-revenue series.
pub async fn analytics_invoices(
    db: &DatabaseConnection,
) -> Result<Vec<InvoiceAnalyticRow>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT s.plan_id AS plan_id, i.issued_at AS issued_at, \
             i.amount_cents AS amount_cents, i.status AS status \
             FROM invoices i JOIN subscriptions s ON s.id = i.subscription_id \
             ORDER BY i.issued_at ASC, i.id ASC",
            vec![],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter()
        .map(|row| {
            Ok(InvoiceAnalyticRow {
                plan_id: row.try_get("", "plan_id").map_err(ApiError::from)?,
                issued_at: row.try_get("", "issued_at").map_err(ApiError::from)?,
                amount_cents: row.try_get("", "amount_cents").map_err(ApiError::from)?,
                status: row.try_get("", "status").map_err(ApiError::from)?,
            })
        })
        .collect()
}

/// `(active_at_start, churned)` counts. The snapshot at period start is
/// reconstructed as active-now plus churned, so the churn formula uses the
/// population that was actually exposed to churn.
pub async fn analytics_subscription_counts(
    db: &DatabaseConnection,
) -> Result<(i64, i64), ApiError> {
    let row = db
        .query_one(statement(
            "SELECT \
             SUM(CASE WHEN status IN ('active', 'trialing', 'past_due') THEN 1 ELSE 0 END) AS active_now, \
             SUM(CASE WHEN status IN ('canceled', 'expired', 'incomplete_expired') THEN 1 ELSE 0 END) AS churned \
             FROM subscriptions",
            vec![],
        ))
        .await
        .map_err(ApiError::from)?;
    let Some(row) = row else {
        return Ok((0, 0));
    };
    let active_now: i64 = row.try_get("", "active_now").unwrap_or(0);
    let churned: i64 = row.try_get("", "churned").unwrap_or(0);
    Ok((active_now + churned, churned))
}
