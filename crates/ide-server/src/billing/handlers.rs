//! HTTP handlers for plans, subscriptions, invoices, entitlements, usage and
//! Stripe webhooks.
//!
//! Every mutation checks CSRF first, except the Stripe webhook: Stripe sends
//! neither a session cookie nor an `x-csrf-token`, and authenticates with a
//! signed body instead. Tenant identity always comes from the signed JWT.

use axum::{
    body::Bytes,
    extract::{FromRequestParts, State},
    http::{request::Parts, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Duration, Utc};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth::{self, AuthUser, RequireRole},
    error::{require_csrf, ApiError},
    server::AppState,
};

use super::{engine, store, stripe};

/// Platform super-admin guard, built from the async role helper so a tenant
/// admin can never create plans.
pub struct SuperAdmin(pub AuthUser);

#[async_trait::async_trait]
impl FromRequestParts<AppState> for SuperAdmin {
    type Rejection = auth::AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let role = RequireRole::super_admin(parts, state).await?;
        Ok(Self(role.0))
    }
}

/// Tenant owner/admin guard for billing mutations.
pub struct TenantAdmin(pub AuthUser);

#[async_trait::async_trait]
impl FromRequestParts<AppState> for TenantAdmin {
    type Rejection = auth::AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let role = RequireRole::admin(parts, state).await?;
        Ok(Self(role.0))
    }
}

#[derive(Debug, Deserialize)]
pub struct CreatePlanRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub currency: String,
    pub interval: String,
    pub amount_cents: i64,
    #[serde(default)]
    pub external_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SubscribeRequest {
    pub plan_id: String,
    #[serde(default)]
    pub seat_count: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct RecordUsageRequest {
    pub meter_key: String,
    pub quantity: i64,
}

#[derive(Debug, Deserialize)]
pub struct CheckoutRequest {
    pub plan_id: String,
    pub success_url: String,
    pub cancel_url: String,
}

#[derive(Debug, Deserialize)]
pub struct PortalRequest {
    pub return_url: String,
}

#[derive(Debug, Serialize)]
pub struct CancelResponse {
    pub subscription_id: String,
    pub cancel_at_period_end: bool,
}

#[derive(Debug, Serialize)]
pub struct UsageMeterResponse {
    pub tenant_id: String,
    pub meter_key: String,
    pub quantity: i64,
    pub period: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct WebhookAck {
    pub received: bool,
    pub duplicate: bool,
}

fn required(value: &str, field: &str) -> Result<String, ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err(ApiError::BadRequest(format!("{field} is invalid")));
    }
    Ok(trimmed.to_string())
}

fn validate_currency(value: &str) -> Result<String, ApiError> {
    let currency = value.trim().to_ascii_uppercase();
    if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Err(ApiError::BadRequest(
            "currency must be a three-letter ISO-4217 code".to_string(),
        ));
    }
    Ok(currency)
}

fn validate_interval(value: &str) -> Result<String, ApiError> {
    let interval = value.trim().to_ascii_lowercase();
    if interval != "month" && interval != "year" {
        return Err(ApiError::BadRequest(
            "interval must be `month` or `year`".to_string(),
        ));
    }
    Ok(interval)
}

fn validate_url(value: &str, field: &str) -> Result<String, ApiError> {
    let url = value.trim();
    if url.is_empty()
        || url.len() > 2048
        || !(url.to_ascii_lowercase().starts_with("https://")
            || url.to_ascii_lowercase().starts_with("http://"))
    {
        return Err(ApiError::BadRequest(format!(
            "{field} must be an absolute http(s) URL"
        )));
    }
    Ok(url.to_string())
}

fn validate_meter_key(value: &str) -> Result<String, ApiError> {
    let key = value.trim();
    if key.is_empty()
        || key.len() > 64
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        return Err(ApiError::BadRequest(
            "meter_key may only contain letters, digits, `_`, `.` and `-`".to_string(),
        ));
    }
    Ok(key.to_string())
}

/// The plans table intentionally carries no feature column. Until plan packs
/// are modeled, every subscription receives this documented baseline; the seat
/// count is the only per-subscription numeric limit.
fn baseline_entitlements(seats: i64) -> Vec<engine::Entitlement> {
    vec![
        engine::Entitlement {
            feature_key: "api_access".to_string(),
            enabled: true,
            limit_value: None,
        },
        engine::Entitlement {
            feature_key: "deployments".to_string(),
            enabled: true,
            limit_value: None,
        },
        engine::Entitlement {
            feature_key: "seats".to_string(),
            enabled: true,
            limit_value: Some(seats),
        },
    ]
}

/// `GET /api/v1/billing/plans`
pub async fn list_plans(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Result<Json<Vec<store::PlanRow>>, ApiError> {
    Ok(Json(store::list_active_plans(&state.db).await?))
}

/// `POST /api/v1/billing/plans` (platform super admin)
pub async fn create_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    _admin: SuperAdmin,
    Json(body): Json<CreatePlanRequest>,
) -> Result<Response, ApiError> {
    require_csrf(&headers)?;

    let name = required(&body.name, "name")?;
    if name.len() > 120 {
        return Err(ApiError::BadRequest("name is too long".to_string()));
    }
    let currency = validate_currency(&body.currency)?;
    let interval = validate_interval(&body.interval)?;
    if body.amount_cents <= 0 {
        return Err(ApiError::BadRequest(
            "amount_cents must be positive".to_string(),
        ));
    }
    let external_id = body
        .external_id
        .as_deref()
        .map(|value| required(value, "external_id"))
        .transpose()?;

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    store::insert_plan(
        &state.db,
        &id,
        &name,
        body.description.as_deref(),
        &currency,
        &interval,
        body.amount_cents,
        external_id.as_deref(),
        &now,
        &now,
    )
    .await?;
    let plan = store::get_plan(&state.db, &id)
        .await?
        .ok_or(ApiError::Internal)?;
    Ok((StatusCode::CREATED, Json(plan)).into_response())
}

/// `GET /api/v1/billing/subscription`
pub async fn current_subscription(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Option<store::SubscriptionRow>>, ApiError> {
    Ok(Json(
        store::get_current_subscription(&state.db, &user.tenant_id).await?,
    ))
}

/// `POST /api/v1/billing/subscribe`
pub async fn subscribe(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: TenantAdmin,
    Json(body): Json<SubscribeRequest>,
) -> Result<Json<store::SubscriptionRow>, ApiError> {
    require_csrf(&headers)?;

    let plan_id = required(&body.plan_id, "plan_id")?;
    let plan = store::get_plan(&state.db, &plan_id)
        .await?
        .filter(|plan| plan.is_active)
        .ok_or_else(|| ApiError::BadRequest("unknown or inactive plan".to_string()))?;
    let price = store::get_price_for_plan(&state.db, &plan.id)
        .await?
        .ok_or(ApiError::Internal)?;

    let seats = body.seat_count.unwrap_or(1);
    if !(1..=10_000).contains(&seats) {
        return Err(ApiError::BadRequest(
            "seat_count must be between 1 and 10000".to_string(),
        ));
    }

    let existing = store::get_billable_subscription(&state.db, &user.tenant_id).await?;
    let status = match existing.as_ref() {
        None => engine::SubscriptionStatus::Trialing.as_str().to_string(),
        Some(row) => {
            let current = engine::SubscriptionStatus::parse(&row.status)
                .unwrap_or(engine::SubscriptionStatus::Expired);
            // Paying an overdue invoice restores service: PastDue -> Active is
            // a legal edge; every other live status is left untouched.
            if current == engine::SubscriptionStatus::PastDue {
                engine::SubscriptionStatus::Active.as_str().to_string()
            } else {
                row.status.clone()
            }
        }
    };
    let id = existing
        .map(|row| row.id)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let now = Utc::now();
    let duration = if plan.interval == "year" {
        Duration::days(365)
    } else {
        Duration::days(30)
    };
    let period_start = now.to_rfc3339();
    let period_end = (now + duration).to_rfc3339();

    store::upsert_subscription(
        &state.db,
        &id,
        &user.tenant_id,
        &plan.id,
        &price.id,
        &status,
        seats,
        &period_start,
        &period_end,
        &now.to_rfc3339(),
        &now.to_rfc3339(),
    )
    .await?;
    store::replace_entitlements(&state.db, &user.tenant_id, &baseline_entitlements(seats)).await?;

    let subscription = store::get_subscription(&state.db, &user.tenant_id, &id)
        .await?
        .ok_or(ApiError::Internal)?;
    Ok(Json(subscription))
}

/// `POST /api/v1/billing/cancel`
pub async fn cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: TenantAdmin,
) -> Result<Json<CancelResponse>, ApiError> {
    require_csrf(&headers)?;

    let subscription = store::get_billable_subscription(&state.db, &user.tenant_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let updated =
        store::cancel_subscription(&state.db, &user.tenant_id, &Utc::now().to_rfc3339()).await?;
    if !updated {
        return Err(ApiError::Conflict(
            "subscription is already set to cancel at period end".to_string(),
        ));
    }
    Ok(Json(CancelResponse {
        subscription_id: subscription.id,
        cancel_at_period_end: true,
    }))
}

/// `GET /api/v1/billing/invoices`
pub async fn invoices(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<store::InvoiceRow>>, ApiError> {
    Ok(Json(
        store::list_invoices(&state.db, &user.tenant_id).await?,
    ))
}

/// `GET /api/v1/billing/entitlements`
pub async fn entitlements(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<engine::Entitlement>>, ApiError> {
    Ok(Json(
        store::list_entitlements(&state.db, &user.tenant_id).await?,
    ))
}

/// `GET /api/v1/billing/usage`
pub async fn usage(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<store::UsageMeterRow>>, ApiError> {
    Ok(Json(store::list_usage(&state.db, &user.tenant_id).await?))
}

/// `POST /api/v1/billing/record-usage`
pub async fn record_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: TenantAdmin,
    Json(body): Json<RecordUsageRequest>,
) -> Result<Json<UsageMeterResponse>, ApiError> {
    require_csrf(&headers)?;

    let meter_key = validate_meter_key(&body.meter_key)?;
    if body.quantity < 0 {
        return Err(ApiError::BadRequest(
            "quantity must not be negative".to_string(),
        ));
    }
    let period = Utc::now().format("%Y-%m").to_string();
    let now = Utc::now().to_rfc3339();
    store::upsert_usage(
        &state.db,
        &user.tenant_id,
        &meter_key,
        body.quantity,
        &period,
        &now,
    )
    .await?;
    Ok(Json(UsageMeterResponse {
        tenant_id: user.tenant_id.clone(),
        meter_key,
        quantity: body.quantity,
        period,
        updated_at: now,
    }))
}

/// `POST /api/v1/billing/checkout`
pub async fn checkout(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: TenantAdmin,
    Json(body): Json<CheckoutRequest>,
) -> Result<Json<stripe::CheckoutSession>, ApiError> {
    require_csrf(&headers)?;

    let plan_id = required(&body.plan_id, "plan_id")?;
    let success_url = validate_url(&body.success_url, "success_url")?;
    let cancel_url = validate_url(&body.cancel_url, "cancel_url")?;
    let plan = store::get_plan(&state.db, &plan_id)
        .await?
        .filter(|plan| plan.is_active)
        .ok_or_else(|| ApiError::BadRequest("unknown or inactive plan".to_string()))?;
    let price = store::get_price_for_plan(&state.db, &plan.id)
        .await?
        .ok_or(ApiError::Internal)?;
    let seats = store::get_billable_subscription(&state.db, &user.tenant_id)
        .await?
        .map(|row| row.seat_count)
        .unwrap_or(1);

    let provider = stripe::StripeProvider::from_env()?;
    let session = provider
        .create_checkout_session(stripe::CheckoutParams {
            plan_id: plan.id,
            price_id: price.id,
            seat_count: seats,
            tenant_id: user.tenant_id.clone(),
            user_id: user.id.clone(),
            email: user.email.clone(),
            success_url,
            cancel_url,
        })
        .await?;
    Ok(Json(session))
}

/// `POST /api/v1/billing/portal`
pub async fn portal(
    headers: HeaderMap,
    _user: TenantAdmin,
    Json(body): Json<PortalRequest>,
) -> Result<Json<stripe::PortalSession>, ApiError> {
    require_csrf(&headers)?;
    let return_url = validate_url(&body.return_url, "return_url")?;
    let provider = stripe::StripeProvider::from_env()?;
    let session = provider
        .create_portal_session(stripe::PortalParams { return_url })
        .await?;
    Ok(Json(session))
}

/// `POST /api/v1/billing/webhooks/stripe`
///
/// Raw body, no authentication and no CSRF: the HMAC signature is the
/// authentication. The secret comes from `STRIPE_WEBHOOK_SECRET`.
pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<WebhookAck>, ApiError> {
    let secret = std::env::var("STRIPE_WEBHOOK_SECRET").map_err(|_| {
        ApiError::ServiceUnavailable(
            "Stripe webhook intake is not configured. Set STRIPE_WEBHOOK_SECRET to receive events."
                .to_string(),
        )
    })?;
    let signature = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    handle_webhook(&state.db, &secret, signature, &body).await
}

/// Event object of a Stripe envelope; tolerate test envelopes that omit
/// `data.object` by falling back to the root value.
fn event_object(value: &Value) -> &Value {
    value
        .get("data")
        .and_then(|data| data.get("object"))
        .unwrap_or(value)
}

/// Verify, claim and apply a webhook. Public so tests can exercise the whole
/// flow with an injected secret instead of mutating process environment.
pub async fn handle_webhook(
    db: &DatabaseConnection,
    secret: &str,
    signature: &str,
    body: &[u8],
) -> Result<Json<WebhookAck>, ApiError> {
    stripe::verify_signature(secret, signature, body, auth::jwt::now_secs())?;

    let value: Value = serde_json::from_slice(body)
        .map_err(|_| ApiError::BadRequest("webhook body is not valid JSON".to_string()))?;
    let event_id = value.get("id").and_then(Value::as_str).unwrap_or("");
    let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");
    if event_id.is_empty() {
        return Err(ApiError::BadRequest("webhook event has no id".to_string()));
    }

    // Claim before doing any work: a concurrent/replayed delivery must be a
    // no-op even if the first attempt is still being processed.
    if !store::claim_event(db, "stripe", event_id).await? {
        return Ok(Json(WebhookAck {
            received: true,
            duplicate: true,
        }));
    }

    let applied = match event_type {
        "checkout.session.completed" => apply_checkout_completed(db, &value).await,
        "customer.subscription.updated" => apply_subscription_updated(db, &value).await,
        "customer.subscription.deleted" => {
            apply_status_change(db, &value, engine::SubscriptionStatus::Cancelled).await
        }
        "invoice.paid" => apply_invoice_paid(db, &value).await,
        _ => {
            tracing::info!(event_type, "ignoring unknown Stripe event type");
            Ok(())
        }
    };
    if let Err(error) = applied {
        // Deliberately best-effort: never retry an already-claimed event into a
        // partial duplicate application. The claim row is still marked done.
        tracing::warn!(event_id, event_type, error = %error, "webhook application failed");
    }
    store::mark_event_processed(db, "stripe", event_id).await?;
    Ok(Json(WebhookAck {
        received: true,
        duplicate: false,
    }))
}

async fn apply_checkout_completed(db: &DatabaseConnection, value: &Value) -> Result<(), ApiError> {
    let object = event_object(value);
    let customer = object.get("customer").and_then(Value::as_str);
    let tenant_id = object
        .get("client_reference_id")
        .and_then(Value::as_str)
        .or_else(|| {
            object
                .get("metadata")
                .and_then(|metadata| metadata.get("tenant_id"))
                .and_then(Value::as_str)
        });
    if let (Some(customer), Some(tenant_id)) = (customer, tenant_id) {
        if let Some(subscription) = store::get_current_subscription(db, tenant_id).await? {
            store::set_external_customer(db, &subscription.id, customer).await?;
        }
    }
    Ok(())
}

async fn apply_subscription_updated(
    db: &DatabaseConnection,
    value: &Value,
) -> Result<(), ApiError> {
    let Some(status) = event_object(value)
        .get("status")
        .and_then(Value::as_str)
        .and_then(engine::SubscriptionStatus::parse)
    else {
        return Ok(());
    };
    apply_status_change(db, value, status).await
}

async fn apply_status_change(
    db: &DatabaseConnection,
    value: &Value,
    target: engine::SubscriptionStatus,
) -> Result<(), ApiError> {
    let Some(customer) = event_object(value).get("customer").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(subscription) = store::find_subscription_by_customer(db, customer).await? else {
        return Ok(());
    };
    let Some(current) = engine::SubscriptionStatus::parse(&subscription.status) else {
        return Ok(());
    };
    if current == target {
        return Ok(());
    }
    if !engine::legal_transition(&current, &target) {
        tracing::warn!(%current, %target, "ignoring illegal subscription transition");
        return Ok(());
    }
    store::update_subscription_status(
        db,
        &subscription.id,
        target.as_str(),
        &Utc::now().to_rfc3339(),
    )
    .await?;
    Ok(())
}

async fn apply_invoice_paid(db: &DatabaseConnection, value: &Value) -> Result<(), ApiError> {
    let object = event_object(value);
    let Some(invoice_id) = object.get("id").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(customer) = object.get("customer").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(subscription) = store::find_subscription_by_customer(db, customer).await? else {
        return Ok(());
    };
    let amount = object
        .get("amount_paid")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let currency = object
        .get("currency")
        .and_then(Value::as_str)
        .unwrap_or("usd")
        .to_ascii_uppercase();
    let now = Utc::now().to_rfc3339();
    store::insert_invoice(
        db,
        invoice_id,
        &subscription.tenant_id,
        &subscription.id,
        amount,
        &currency,
        "paid",
        &now,
        Some(&now),
        None,
        None,
        Some("Stripe invoice"),
    )
    .await
}
