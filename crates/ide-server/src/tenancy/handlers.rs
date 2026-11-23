//! Tenant CRUD, membership and the session tenant switch.
//!
//! Every mutation checks CSRF first. Tenant admin actions additionally check
//! the role from the SIGNED claim, never a body/query field.

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::store;
use crate::{
    auth::{self, cookie_header, jwt, AuthUser, COOKIE_ACCESS, COOKIE_REFRESH, CSRF_HEADER},
    error::{require_csrf, ApiError},
    server::AppState,
};

#[derive(Debug, Serialize)]
pub struct TenantResponse {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub plan_id: Option<String>,
    pub status: String,
    pub role: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl TenantResponse {
    fn from_row(row: store::TenantRow, role: Option<String>) -> Self {
        Self {
            id: row.id,
            name: row.name,
            slug: row.slug,
            plan_id: row.plan_id,
            status: row.status,
            role,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateTenantRequest {
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTenantRequest {
    pub name: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct MemberRequest {
    pub user_id: String,
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct MemberResponse {
    pub tenant_id: String,
    pub user_id: String,
    pub email: String,
    pub role: String,
    pub created_at: String,
}

fn normalize_slug(raw: &str) -> Result<String, ApiError> {
    let slug: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    if slug.is_empty() || slug.len() > 63 || !slug.bytes().any(|b| b.is_ascii_alphanumeric()) {
        return Err(ApiError::BadRequest("invalid tenant slug".to_string()));
    }
    Ok(slug)
}

fn validate_role(role: &str) -> Result<(), ApiError> {
    if matches!(role, "owner" | "admin" | "member") {
        Ok(())
    } else {
        Err(ApiError::BadRequest(
            "role must be owner, admin or member".to_string(),
        ))
    }
}

/// The tenants the caller belongs to.
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<TenantResponse>>, ApiError> {
    let rows = store::list_for_user(&state.db, &user.id).await?;
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let role = store::role_in(&state.db, &row.id, &user.id).await?;
        result.push(TenantResponse::from_row(row, role));
    }
    Ok(Json(result))
}

/// Create a tenant and make the creator its owner.
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Json(body): Json<CreateTenantRequest>,
) -> Result<Json<TenantResponse>, ApiError> {
    require_csrf(&headers)?;
    // Creating a tenant grants the creator `owner`, so an ordinary member
    // must not be able to do it: otherwise every member could mint a tenant
    // in which they outrank their administrator.
    if !user.is_tenant_admin() {
        return Err(ApiError::Forbidden("tenant admin required".to_string()));
    }
    let name = body.name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(ApiError::BadRequest("invalid tenant name".to_string()));
    }
    let slug = normalize_slug(&body.slug)?;
    let id = Uuid::new_v4().to_string();
    let row = store::TenantRow {
        id: id.clone(),
        name: name.to_string(),
        slug,
        plan_id: None,
        status: "active".to_string(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    store::insert(&state.db, &row).await?;
    store::set_member(&state.db, &id, &user.id, "owner").await?;
    let stored = store::get(&state.db, &id)
        .await?
        .ok_or(ApiError::Internal)?;
    Ok(Json(TenantResponse::from_row(
        stored,
        Some("owner".to_string()),
    )))
}

/// Update a tenant. Only an owner/admin of THAT tenant may do so.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateTenantRequest>,
) -> Result<Json<TenantResponse>, ApiError> {
    require_csrf(&headers)?;
    let role = store::role_in(&state.db, &id, &user.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if role != "owner" && role != "admin" {
        return Err(ApiError::Forbidden("tenant admin required".to_string()));
    }
    if body.name.trim().is_empty() || body.name.len() > 128 {
        return Err(ApiError::BadRequest("invalid tenant name".to_string()));
    }
    if !matches!(body.status.as_str(), "active" | "suspended") {
        return Err(ApiError::BadRequest("invalid tenant status".to_string()));
    }
    let written = store::update(&state.db, &id, body.name.trim(), &body.status).await?;
    if !written {
        return Err(ApiError::NotFound);
    }
    let row = store::get(&state.db, &id)
        .await?
        .ok_or(ApiError::Internal)?;
    Ok(Json(TenantResponse::from_row(row, Some(role))))
}

/// Switch the session's active tenant. Membership is verified server-side
/// before new signed cookies are issued; a client cannot choose a tenant it
/// does not belong to.
pub async fn switch(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<axum::response::Response, ApiError> {
    require_csrf(&headers)?;
    let role = store::role_in(&state.db, &id, &user.id)
        .await?
        .ok_or(ApiError::Forbidden(
            "not a member of this tenant".to_string(),
        ))?;
    let tenant = store::get(&state.db, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if tenant.status != "active" {
        return Err(ApiError::Forbidden("tenant is suspended".to_string()));
    }
    store::set_active_tenant(&state.db, &user.id, &id).await?;

    let now = jwt::now_secs();
    let roles = vec![role];
    let access = state.keys.encode(&jwt::Claims::access_for(
        &user.id,
        &user.email,
        id,
        roles,
        user.platform_role.clone(),
        &Uuid::new_v4().to_string(),
        now,
    ))?;
    let refresh = state.keys.encode(&jwt::Claims::refresh(
        &user.id,
        &user.email,
        &Uuid::new_v4().to_string(),
        now,
    ))?;

    let csrf = headers
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .unwrap_or_default();
    let mut response =
        Json(serde_json::json!({ "tenant_id": tenant.id, "csrf_token": csrf })).into_response();
    let out = response.headers_mut();
    let access_cookie = cookie_header(COOKIE_ACCESS, &access, auth::jwt::ACCESS_TTL_SECS, true);
    HeaderValue::from_str(&access_cookie)
        .map(|value| out.append(header::SET_COOKIE, value))
        .map_err(|_| ApiError::Internal)?;
    let refresh_cookie = cookie_header(COOKIE_REFRESH, &refresh, auth::jwt::REFRESH_TTL_SECS, true);
    HeaderValue::from_str(&refresh_cookie)
        .map(|value| out.append(header::SET_COOKIE, value))
        .map_err(|_| ApiError::Internal)?;
    // The CSRF cookie is unchanged: the client already holds that token.
    Ok(response)
}

/// List members of a tenant the caller administers.
pub async fn members(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<MemberResponse>>, ApiError> {
    let role = store::role_in(&state.db, &id, &user.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if role != "owner" && role != "admin" {
        return Err(ApiError::Forbidden("tenant admin required".to_string()));
    }
    let rows = store::list_members(&state.db, &id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| MemberResponse {
                tenant_id: row.tenant_id,
                user_id: row.user_id,
                email: row.email,
                role: row.role,
                created_at: row.created_at,
            })
            .collect(),
    ))
}

/// Invite (add or update) a member. Requires a tenant admin role.
pub async fn add_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<MemberRequest>,
) -> Result<Json<MemberResponse>, ApiError> {
    require_csrf(&headers)?;
    let role = store::role_in(&state.db, &id, &user.id)
        .await?
        .ok_or(ApiError::Forbidden("tenant admin required".to_string()))?;
    if role != "owner" && role != "admin" {
        return Err(ApiError::Forbidden("tenant admin required".to_string()));
    }
    validate_role(&body.role)?;
    // An admin cannot grant the owner role; only owners can.
    if body.role == "owner" && role != "owner" {
        return Err(ApiError::Forbidden(
            "only an owner can grant owner".to_string(),
        ));
    }
    let written = store::set_member(&state.db, &id, &body.user_id, &body.role).await?;
    if !written {
        return Err(ApiError::NotFound);
    }
    let members = store::list_members(&state.db, &id).await?;
    let found = members
        .into_iter()
        .find(|row| row.user_id == body.user_id)
        .ok_or(ApiError::Internal)?;
    Ok(Json(MemberResponse {
        tenant_id: found.tenant_id,
        user_id: found.user_id,
        email: found.email,
        role: found.role,
        created_at: found.created_at,
    }))
}

/// Remove a member. The last owner is protected by the store.
pub async fn remove_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_csrf(&headers)?;
    let role = store::role_in(&state.db, &id, &user.id)
        .await?
        .ok_or(ApiError::Forbidden("tenant admin required".to_string()))?;
    if role != "owner" && role != "admin" {
        return Err(ApiError::Forbidden("tenant admin required".to_string()));
    }
    let target = store::role_in(&state.db, &id, &user_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if target == "owner" && role != "owner" {
        return Err(ApiError::Forbidden(
            "only an owner can remove an owner".to_string(),
        ));
    }
    if !store::remove_member(&state.db, &id, &user_id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}
