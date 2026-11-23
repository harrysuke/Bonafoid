//! HTTP handlers and DTOs for the platform administration panel.
//!
//! Every admin route is guarded by [`SuperAdmin`], a thin `FromRequestParts`
//! wrapper around `RequireRole::super_admin`. The signed platform role is the
//! only authorization input; a body or query field cannot escalate a caller.
//! The two status routes are deliberately public and expose no tenant data.

use axum::{
    extract::{FromRequestParts, Path, Query, State},
    http::{request::Parts, HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

use super::{analytics, store};
use crate::{
    auth::{AuthUser, RequireRole},
    error::{require_csrf, ApiError},
    server::AppState,
};

/// Platform super-admin extractor. `AuthError` converts to `ApiError`, so a
/// tenant admin or ordinary member is rejected with 403 before the handler
/// reads a body or checks CSRF.
#[derive(Debug)]
pub struct SuperAdmin(pub AuthUser);

#[async_trait::async_trait]
impl FromRequestParts<AppState> for SuperAdmin {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let role = RequireRole::super_admin(parts, state).await?;
        Ok(Self(role.0))
    }
}

// ---------------------------------------------------------------------------
// Query and body DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub search: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl ListQuery {
    fn limit(&self) -> i64 {
        store::clamp_limit(self.limit)
    }

    fn offset(&self) -> i64 {
        store::clamp_offset(self.offset)
    }
}

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub tenant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EvaluateQuery {
    pub key: String,
}

#[derive(Debug, Deserialize)]
pub struct TenantStatusRequest {
    pub status: String,
}

/// A JSON `null` for an `Option<T>` field is always deserialized as `None` by
/// serde, which would make "clear the platform role" indistinguishable from
/// "field omitted". This type preserves all three states with a custom
/// deserializer and is used with `#[serde(default)]` on the patch body.
#[derive(Debug, Clone)]
pub enum PlatformRoleField {
    Missing,
    Null,
    Role(String),
    Invalid,
}

impl Default for PlatformRoleField {
    fn default() -> Self {
        Self::Missing
    }
}

impl<'de> Deserialize<'de> for PlatformRoleField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Accept every JSON value here and validate in the handler so a wrong
        // type is a 400 from this API rather than axum's 422 JSON rejection.
        match Value::deserialize(deserializer)? {
            Value::Null => Ok(Self::Null),
            Value::String(role) => Ok(Self::Role(role)),
            _ => Ok(Self::Invalid),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub is_verified: Option<bool>,
    #[serde(default)]
    pub platform_role: PlatformRoleField,
}

#[derive(Debug, Deserialize)]
pub struct ImpersonateRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFlagRequest {
    pub key: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub rollout_percentage: i64,
    #[serde(default)]
    pub kill_switch: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateFlagRequest {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub rollout_percentage: Option<i64>,
    #[serde(default)]
    pub kill_switch: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct OverrideRequest {
    pub flag_key: String,
    pub tenant_id: String,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Response DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct TenantResponse {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub plan_id: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<store::TenantRow> for TenantResponse {
    fn from(row: store::TenantRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            slug: row.slug,
            plan_id: row.plan_id,
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub is_active: bool,
    pub is_verified: bool,
    pub platform_role: Option<String>,
    pub tenant_id: Option<String>,
    pub last_login_at: Option<String>,
    pub created_at: String,
}

impl From<store::UserRow> for UserResponse {
    fn from(row: store::UserRow) -> Self {
        Self {
            id: row.id,
            email: row.email,
            is_active: row.is_active,
            is_verified: row.is_verified,
            platform_role: row.platform_role,
            tenant_id: row.tenant_id,
            last_login_at: row.last_login_at,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FlagResponse {
    pub key: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub rollout_percentage: i64,
    pub kill_switch: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<store::FlagRow> for FlagResponse {
    fn from(row: store::FlagRow) -> Self {
        Self {
            key: row.key,
            description: row.description,
            enabled: row.enabled,
            rollout_percentage: row.rollout_percentage,
            kill_switch: row.kill_switch,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
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

impl From<store::AuditRow> for AuditEntry {
    fn from(row: store::AuditRow) -> Self {
        Self {
            id: row.id,
            actor_user_id: row.actor_user_id,
            actor_email: row.actor_email,
            tenant_id: row.tenant_id,
            action: row.action,
            target_type: row.target_type,
            target_id: row.target_id,
            before_json: row.before_json,
            after_json: row.after_json,
            ip_address: row.ip_address,
            user_agent: row.user_agent,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpersonationEntry {
    pub id: String,
    pub admin_user_id: String,
    pub target_user_id: String,
    pub tenant_id: String,
    pub reason: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

impl From<store::ImpersonationRow> for ImpersonationEntry {
    fn from(row: store::ImpersonationRow) -> Self {
        Self {
            id: row.id,
            admin_user_id: row.admin_user_id,
            target_user_id: row.target_user_id,
            tenant_id: row.tenant_id,
            reason: row.reason,
            started_at: row.started_at,
            ended_at: row.ended_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageResponse {
    pub projects: i64,
    pub integrations: i64,
    pub deployments: i64,
    pub usage_events: i64,
    pub tenant_members: i64,
    pub audit_log: i64,
}

impl From<store::UsageCounts> for UsageResponse {
    fn from(counts: store::UsageCounts) -> Self {
        Self {
            projects: counts.projects,
            integrations: counts.integrations,
            deployments: counts.deployments,
            usage_events: counts.usage_events,
            tenant_members: counts.tenant_members,
            audit_log: counts.audit_log,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MonthlyPoint {
    pub month: String,
    pub revenue_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalyticsResponse {
    pub mrr_cents: i64,
    pub arr_cents: i64,
    pub churn_rate: f64,
    pub revenue_by_plan: BTreeMap<String, i64>,
    pub monthly_series: Vec<MonthlyPoint>,
    pub active_subscriptions: i64,
    pub churned_subscriptions: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentStatus {
    pub database: &'static str,
    pub plugin_host: &'static str,
    pub preview_toolchain: &'static str,
    pub storage: &'static str,
    pub queue_depth: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusResponse {
    pub status: &'static str,
    pub components: ComponentStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct IncidentEntry {
    pub status: String,
    pub component: Option<String>,
    pub title: String,
    pub opened_at: String,
    pub resolved_at: Option<String>,
}

impl From<store::IncidentRow> for IncidentEntry {
    fn from(row: store::IncidentRow) -> Self {
        Self {
            status: row.status,
            component: row.component,
            title: row.title,
            opened_at: row.opened_at,
            resolved_at: row.resolved_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

fn validate_flag_key(raw: &str) -> Result<String, ApiError> {
    let key = raw.trim().to_string();
    let valid = !key.is_empty()
        && key.len() <= 128
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'));
    if valid {
        Ok(key)
    } else {
        Err(ApiError::BadRequest("invalid flag key".to_string()))
    }
}

fn validate_rollout(raw: i64) -> Result<i64, ApiError> {
    if (0..=100).contains(&raw) {
        Ok(raw)
    } else {
        Err(ApiError::BadRequest(
            "rollout_percentage must be between 0 and 100".to_string(),
        ))
    }
}

/// Interpret the explicit-null-aware `platform_role` field.
fn parse_platform_role(field: PlatformRoleField) -> Result<Option<Option<String>>, ApiError> {
    match field {
        PlatformRoleField::Missing => Ok(None),
        PlatformRoleField::Null => Ok(Some(None)),
        PlatformRoleField::Role(role) if role == "super_admin" => Ok(Some(Some(role))),
        PlatformRoleField::Role(_) | PlatformRoleField::Invalid => Err(ApiError::BadRequest(
            "platform_role must be null or \"super_admin\"".to_string(),
        )),
    }
}

fn peer_ip(ip: &str) -> Option<&str> {
    if ip.is_empty() {
        None
    } else {
        Some(ip)
    }
}

// ---------------------------------------------------------------------------
// Tenants and users
// ---------------------------------------------------------------------------

/// `GET /api/v1/admin/tenants`
pub async fn list_tenants(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
    _admin: SuperAdmin,
) -> Result<Json<Vec<TenantResponse>>, ApiError> {
    let rows = store::list_tenants(
        &state.db,
        query.search.as_deref(),
        query.limit(),
        query.offset(),
    )
    .await?;
    Ok(Json(rows.into_iter().map(TenantResponse::from).collect()))
}

/// `PATCH /api/v1/admin/tenants/:id/status`
pub async fn set_tenant_status(
    State(state): State<AppState>,
    _admin: SuperAdmin,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TenantStatusRequest>,
) -> Result<Json<TenantResponse>, ApiError> {
    require_csrf(&headers)?;
    if !matches!(body.status.as_str(), "active" | "suspended") {
        return Err(ApiError::BadRequest(
            "status must be active or suspended".to_string(),
        ));
    }
    let Some(existing) = store::get_tenant(&state.db, &id).await? else {
        return Err(ApiError::NotFound);
    };
    if !store::set_tenant_status(&state.db, &id, &body.status).await? {
        return Err(ApiError::NotFound);
    }
    let updated = store::get_tenant(&state.db, &id)
        .await?
        .ok_or(ApiError::Internal)?;

    let before = serde_json::json!({ "status": existing.status }).to_string();
    let after = serde_json::json!({ "status": body.status }).to_string();
    let (ip, ua) = store::audit_from_request(&_admin.0, &headers);
    store::append_audit(
        &state.db,
        &_admin.0.id,
        &_admin.0.email,
        Some(&id),
        "tenant_status_update",
        "tenant",
        Some(&id),
        Some(&before),
        Some(&after),
        peer_ip(&ip),
        ua.as_deref(),
    )
    .await?;

    Ok(Json(TenantResponse::from(updated)))
}

/// `GET /api/v1/admin/users`
pub async fn list_users(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
    _admin: SuperAdmin,
) -> Result<Json<Vec<UserResponse>>, ApiError> {
    let rows = store::list_users(
        &state.db,
        query.search.as_deref(),
        query.limit(),
        query.offset(),
    )
    .await?;
    Ok(Json(rows.into_iter().map(UserResponse::from).collect()))
}

/// `PATCH /api/v1/admin/users/:id`
pub async fn update_user(
    State(state): State<AppState>,
    admin: SuperAdmin,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    require_csrf(&headers)?;
    let platform_role = parse_platform_role(body.platform_role)?;
    let Some(existing) = store::get_user(&state.db, &id).await? else {
        return Err(ApiError::NotFound);
    };
    // Demoting the last super admin would leave the platform unmanageable.
    if matches!(platform_role.as_ref(), Some(None))
        && existing.platform_role.as_deref() == Some("super_admin")
        && store::count_super_admins(&state.db).await? <= 1
    {
        return Err(ApiError::Conflict(
            "cannot demote the last super admin".to_string(),
        ));
    }

    let written = store::update_user(
        &state.db,
        &id,
        body.is_active,
        body.is_verified,
        platform_role,
    )
    .await?;
    if !written {
        return Err(ApiError::NotFound);
    }
    let updated = store::get_user(&state.db, &id)
        .await?
        .ok_or(ApiError::Internal)?;

    let before = serde_json::json!({
        "is_active": existing.is_active,
        "is_verified": existing.is_verified,
        "platform_role": existing.platform_role,
    })
    .to_string();
    let after = serde_json::json!({
        "is_active": updated.is_active,
        "is_verified": updated.is_verified,
        "platform_role": updated.platform_role,
    })
    .to_string();
    let (ip, ua) = store::audit_from_request(&admin.0, &headers);
    store::append_audit(
        &state.db,
        &admin.0.id,
        &admin.0.email,
        updated.tenant_id.as_deref(),
        "user_update",
        "user",
        Some(&id),
        Some(&before),
        Some(&after),
        peer_ip(&ip),
        ua.as_deref(),
    )
    .await?;

    Ok(Json(UserResponse::from(updated)))
}

/// `POST /api/v1/admin/users/:id/impersonate`
///
/// Records an audit entry and an impersonation row. No session token is
/// swapped: turning this into a real login-as feature would require a
/// separately authorized, expiring token flow and is intentionally out of
/// scope for this administration panel.
pub async fn impersonate(
    State(state): State<AppState>,
    admin: SuperAdmin,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ImpersonateRequest>,
) -> Result<Json<Value>, ApiError> {
    require_csrf(&headers)?;
    let Some(target) = store::get_user(&state.db, &id).await? else {
        return Err(ApiError::NotFound);
    };
    let impersonation_id = Uuid::new_v4().to_string();
    let tenant_id = target
        .tenant_id
        .clone()
        .unwrap_or_else(|| admin.0.tenant_id.clone());
    store::insert_impersonation(
        &state.db,
        &impersonation_id,
        &admin.0.id,
        &id,
        &tenant_id,
        body.reason.as_deref(),
    )
    .await?;

    let after = serde_json::json!({
        "target_user_id": id,
        "tenant_id": tenant_id,
        "reason": body.reason,
    })
    .to_string();
    let (ip, ua) = store::audit_from_request(&admin.0, &headers);
    store::append_audit(
        &state.db,
        &admin.0.id,
        &admin.0.email,
        Some(&tenant_id),
        "impersonate",
        "user",
        Some(&id),
        None,
        Some(&after),
        peer_ip(&ip),
        ua.as_deref(),
    )
    .await?;

    Ok(Json(serde_json::json!({
        "impersonation_id": impersonation_id,
        "note": "Audit-only impersonation record; no session token was swapped.",
    })))
}

// ---------------------------------------------------------------------------
// Analytics and usage
// ---------------------------------------------------------------------------

/// `GET /api/v1/admin/analytics`
pub async fn analytics(
    State(state): State<AppState>,
    _admin: SuperAdmin,
) -> Result<Json<AnalyticsResponse>, ApiError> {
    let subs = store::analytics_subscriptions(&state.db).await?;
    let invoices = store::analytics_invoices(&state.db).await?;
    let (active_start, churned) = store::analytics_subscription_counts(&state.db).await?;

    let mrr = analytics::mrr_cents(&subs);
    let by_plan: Vec<(String, i64, String)> = invoices
        .iter()
        .map(|row| (row.plan_id.clone(), row.amount_cents, row.status.clone()))
        .collect();
    let by_month: Vec<(String, i64, String)> = invoices
        .iter()
        .map(|row| (row.issued_at.clone(), row.amount_cents, row.status.clone()))
        .collect();
    let revenue_by_plan = analytics::revenue_by_plan(&by_plan);
    let monthly_series = analytics::monthly_series(&by_month)
        .into_iter()
        .map(|(month, revenue_cents)| MonthlyPoint {
            month,
            revenue_cents,
        })
        .collect();

    Ok(Json(AnalyticsResponse {
        mrr_cents: mrr,
        arr_cents: analytics::arr_cents(mrr),
        churn_rate: analytics::churn_rate(active_start, churned),
        revenue_by_plan,
        monthly_series,
        active_subscriptions: (active_start - churned).max(0),
        churned_subscriptions: churned,
    }))
}

/// `GET /api/v1/admin/usage`
pub async fn usage(
    State(state): State<AppState>,
    _admin: SuperAdmin,
) -> Result<Json<UsageResponse>, ApiError> {
    Ok(Json(UsageResponse::from(
        store::usage_counts(&state.db).await?,
    )))
}

// ---------------------------------------------------------------------------
// Platform flags
// ---------------------------------------------------------------------------

/// `GET /api/v1/admin/flags`
pub async fn list_flags(
    State(state): State<AppState>,
    _admin: SuperAdmin,
) -> Result<Json<Vec<FlagResponse>>, ApiError> {
    let rows = store::list_flags(&state.db).await?;
    Ok(Json(rows.into_iter().map(FlagResponse::from).collect()))
}

/// `POST /api/v1/admin/flags`
pub async fn create_flag(
    State(state): State<AppState>,
    _admin: SuperAdmin,
    headers: HeaderMap,
    Json(body): Json<CreateFlagRequest>,
) -> Result<Json<FlagResponse>, ApiError> {
    require_csrf(&headers)?;
    let key = validate_flag_key(&body.key)?;
    let rollout = validate_rollout(body.rollout_percentage)?;
    if store::get_flag(&state.db, &key).await?.is_some() {
        return Err(ApiError::Conflict(format!("flag `{key}` already exists")));
    }
    store::insert_flag(
        &state.db,
        &key,
        body.description
            .as_deref()
            .filter(|value| !value.is_empty()),
        body.enabled,
        rollout,
        body.kill_switch,
    )
    .await?;

    let after = serde_json::json!({
        "key": key,
        "enabled": body.enabled,
        "rollout_percentage": rollout,
        "kill_switch": body.kill_switch,
    })
    .to_string();
    let (ip, ua) = store::audit_from_request(&_admin.0, &headers);
    store::append_audit(
        &state.db,
        &_admin.0.id,
        &_admin.0.email,
        None,
        "flag_create",
        "flag",
        Some(&key),
        None,
        Some(&after),
        peer_ip(&ip),
        ua.as_deref(),
    )
    .await?;

    let row = store::get_flag(&state.db, &key)
        .await?
        .ok_or(ApiError::Internal)?;
    Ok(Json(FlagResponse::from(row)))
}

/// `PATCH /api/v1/admin/flags/:key`
pub async fn update_flag(
    State(state): State<AppState>,
    _admin: SuperAdmin,
    headers: HeaderMap,
    Path(key): Path<String>,
    Json(body): Json<UpdateFlagRequest>,
) -> Result<Json<FlagResponse>, ApiError> {
    require_csrf(&headers)?;
    let rollout = match body.rollout_percentage {
        Some(value) => Some(validate_rollout(value)?),
        None => None,
    };
    let Some(existing) = store::get_flag(&state.db, &key).await? else {
        return Err(ApiError::NotFound);
    };
    store::update_flag(
        &state.db,
        &key,
        body.description.map(Some),
        body.enabled,
        rollout,
        body.kill_switch,
    )
    .await?;
    let updated = store::get_flag(&state.db, &key)
        .await?
        .ok_or(ApiError::Internal)?;

    let before = serde_json::json!({
        "enabled": existing.enabled,
        "rollout_percentage": existing.rollout_percentage,
        "kill_switch": existing.kill_switch,
    })
    .to_string();
    let after = serde_json::json!({
        "enabled": updated.enabled,
        "rollout_percentage": updated.rollout_percentage,
        "kill_switch": updated.kill_switch,
    })
    .to_string();
    let (ip, ua) = store::audit_from_request(&_admin.0, &headers);
    store::append_audit(
        &state.db,
        &_admin.0.id,
        &_admin.0.email,
        None,
        "flag_update",
        "flag",
        Some(&key),
        Some(&before),
        Some(&after),
        peer_ip(&ip),
        ua.as_deref(),
    )
    .await?;

    Ok(Json(FlagResponse::from(updated)))
}

/// `DELETE /api/v1/admin/flags/:key`
pub async fn delete_flag(
    State(state): State<AppState>,
    _admin: SuperAdmin,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_csrf(&headers)?;
    let Some(existing) = store::get_flag(&state.db, &key).await? else {
        return Err(ApiError::NotFound);
    };
    if !store::delete_flag(&state.db, &key).await? {
        return Err(ApiError::NotFound);
    }

    let before = serde_json::json!({
        "key": existing.key,
        "enabled": existing.enabled,
    })
    .to_string();
    let (ip, ua) = store::audit_from_request(&_admin.0, &headers);
    store::append_audit(
        &state.db,
        &_admin.0.id,
        &_admin.0.email,
        None,
        "flag_delete",
        "flag",
        Some(&key),
        Some(&before),
        None,
        peer_ip(&ip),
        ua.as_deref(),
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/admin/flags/overrides`
pub async fn set_override(
    State(state): State<AppState>,
    admin: SuperAdmin,
    headers: HeaderMap,
    Json(body): Json<OverrideRequest>,
) -> Result<Json<Value>, ApiError> {
    require_csrf(&headers)?;
    if store::get_flag(&state.db, &body.flag_key).await?.is_none() {
        return Err(ApiError::NotFound);
    }
    store::set_override(&state.db, &body.flag_key, &body.tenant_id, body.enabled).await?;

    let response = serde_json::json!({
        "flag_key": body.flag_key,
        "tenant_id": body.tenant_id,
        "enabled": body.enabled,
    });
    let after = response.to_string();
    let (ip, ua) = store::audit_from_request(&admin.0, &headers);
    store::append_audit(
        &state.db,
        &admin.0.id,
        &admin.0.email,
        Some(&body.tenant_id),
        "flag_override_set",
        "flag",
        Some(&body.flag_key),
        None,
        Some(&after),
        peer_ip(&ip),
        ua.as_deref(),
    )
    .await?;

    Ok(Json(response))
}

/// `GET /api/v1/admin/flags/evaluate?key=...`
///
/// Authenticated tenant user, not just a super admin: the tenant id comes from
/// the signed claim, so a caller cannot evaluate another tenant's override.
pub async fn evaluate_flag(
    State(state): State<AppState>,
    Query(query): Query<EvaluateQuery>,
    user: AuthUser,
) -> Result<Json<Value>, ApiError> {
    let key = query.key.trim();
    if key.is_empty() {
        return Err(ApiError::BadRequest("key is required".to_string()));
    }
    let enabled = store::evaluate(&state.db, key, &user.tenant_id).await?;
    Ok(Json(serde_json::json!({ "key": key, "enabled": enabled })))
}

// ---------------------------------------------------------------------------
// Audit and impersonation listings
// ---------------------------------------------------------------------------

/// `GET /api/v1/admin/audit`
pub async fn audit(
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
    _admin: SuperAdmin,
) -> Result<Json<Vec<AuditEntry>>, ApiError> {
    let rows = store::list_audit(
        &state.db,
        store::clamp_limit(query.limit),
        store::clamp_offset(query.offset),
        query.tenant_id.as_deref().filter(|value| !value.is_empty()),
    )
    .await?;
    Ok(Json(rows.into_iter().map(AuditEntry::from).collect()))
}

/// `GET /api/v1/admin/impersonations`
pub async fn impersonations(
    State(state): State<AppState>,
    _admin: SuperAdmin,
) -> Result<Json<Vec<ImpersonationEntry>>, ApiError> {
    let rows = store::list_impersonations(
        &state.db,
        store::clamp_limit(None),
        store::clamp_offset(None),
    )
    .await?;
    Ok(Json(
        rows.into_iter().map(ImpersonationEntry::from).collect(),
    ))
}

// ---------------------------------------------------------------------------
// Public status endpoints (no authentication, no tenant data)
// ---------------------------------------------------------------------------

fn toolchain_status() -> &'static str {
    match std::process::Command::new("cargo")
        .arg("--version")
        .output()
    {
        Ok(output) if output.status.success() => "ok",
        _ => "unavailable",
    }
}

/// The plugin storage root exists or can be created. The path itself is never
/// put in the response: it can reveal deployment layout to an anonymous caller.
fn storage_status() -> &'static str {
    let root = crate::plugins::plugins_root();
    if root.exists() || std::fs::create_dir_all(&root).is_ok() {
        "ok"
    } else {
        "unavailable"
    }
}

/// `GET /api/v1/status`
pub async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    let database = if store::database_ok(&state.db).await {
        "ok"
    } else {
        "unavailable"
    };
    // Touching the registry's configuration proves the host manager is
    // accessible; constructing a per-user WASM host here would be wasteful.
    let plugin_host = if state.plugins.root().as_os_str().is_empty() {
        "unavailable"
    } else {
        "ok"
    };
    let preview_toolchain = toolchain_status();
    let storage = storage_status();
    // `RunManager` exposes per-user lookups but no global count, so there is
    // no queue depth to report without inventing one. Zero means "none known".
    let queue_depth = 0usize;
    let healthy =
        database == "ok" && plugin_host == "ok" && preview_toolchain == "ok" && storage == "ok";

    Json(StatusResponse {
        status: if healthy { "operational" } else { "degraded" },
        components: ComponentStatus {
            database,
            plugin_host,
            preview_toolchain,
            storage,
            queue_depth,
        },
    })
}

/// `GET /api/v1/status/incidents`
pub async fn incidents(
    State(state): State<AppState>,
) -> Result<Json<Vec<IncidentEntry>>, ApiError> {
    let rows = store::list_incidents(&state.db).await?;
    Ok(Json(rows.into_iter().map(IncidentEntry::from).collect()))
}
