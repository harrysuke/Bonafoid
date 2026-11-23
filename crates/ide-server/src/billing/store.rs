//! Raw-SQL persistence for plans, subscriptions and usage.
//!
//! Every tenant-owned table is queried with `tenant_id = ?`; there is no
//! overload that accepts a tenant id from a request body. Money is stored as
//! integer cents. Entitlements use the projection shared with the pure rules
//! in [`super::engine`].

use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, Value,
};
use serde::Serialize;

use crate::error::ApiError;

use super::engine::Entitlement;

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn text(value: &str) -> Value {
    Value::from(value.to_string())
}

fn opt_text(value: Option<&str>) -> Value {
    Value::String(value.map(|v| Box::new(v.to_string())))
}

fn integer(value: i64) -> Value {
    Value::from(value)
}

fn flag(value: bool) -> Value {
    integer(if value { 1 } else { 0 })
}

/// A sellable plan. `external_id` is the optional provider-side price/product
/// identifier; the core tables never require it.
#[derive(Debug, Clone, Serialize)]
pub struct PlanRow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub currency: String,
    pub interval: String,
    pub amount_cents: i64,
    pub is_active: bool,
    pub external_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PriceRow {
    pub id: String,
    pub plan_id: String,
    pub amount_cents: i64,
    pub currency: String,
    pub interval: String,
    pub external_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionRow {
    pub id: String,
    pub tenant_id: String,
    pub plan_id: String,
    pub price_id: String,
    pub status: String,
    pub seat_count: i64,
    pub period_start: String,
    pub period_end: String,
    pub cancel_at_period_end: bool,
    pub external_customer_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InvoiceRow {
    pub id: String,
    pub tenant_id: String,
    pub subscription_id: String,
    pub amount_cents: i64,
    pub currency: String,
    pub status: String,
    pub issued_at: String,
    pub paid_at: Option<String>,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageMeterRow {
    pub tenant_id: String,
    pub meter_key: String,
    pub quantity: i64,
    pub period: String,
    pub updated_at: String,
}

fn map_plan(row: &QueryResult) -> Result<PlanRow, ApiError> {
    let active: i64 = row.try_get("", "is_active").map_err(ApiError::from)?;
    Ok(PlanRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        name: row.try_get("", "name").map_err(ApiError::from)?,
        description: row.try_get("", "description").map_err(ApiError::from)?,
        currency: row.try_get("", "currency").map_err(ApiError::from)?,
        interval: row.try_get("", "interval").map_err(ApiError::from)?,
        amount_cents: row.try_get("", "amount_cents").map_err(ApiError::from)?,
        is_active: active != 0,
        external_id: row.try_get("", "external_id").map_err(ApiError::from)?,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
        updated_at: row.try_get("", "updated_at").map_err(ApiError::from)?,
    })
}

fn map_subscription(row: &QueryResult) -> Result<SubscriptionRow, ApiError> {
    let cancel: i64 = row
        .try_get("", "cancel_at_period_end")
        .map_err(ApiError::from)?;
    Ok(SubscriptionRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        tenant_id: row.try_get("", "tenant_id").map_err(ApiError::from)?,
        plan_id: row.try_get("", "plan_id").map_err(ApiError::from)?,
        price_id: row.try_get("", "price_id").map_err(ApiError::from)?,
        status: row.try_get("", "status").map_err(ApiError::from)?,
        seat_count: row.try_get("", "seat_count").map_err(ApiError::from)?,
        period_start: row.try_get("", "period_start").map_err(ApiError::from)?,
        period_end: row.try_get("", "period_end").map_err(ApiError::from)?,
        cancel_at_period_end: cancel != 0,
        external_customer_id: row
            .try_get("", "external_customer_id")
            .map_err(ApiError::from)?,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
        updated_at: row.try_get("", "updated_at").map_err(ApiError::from)?,
    })
}

fn map_invoice(row: &QueryResult) -> Result<InvoiceRow, ApiError> {
    Ok(InvoiceRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        tenant_id: row.try_get("", "tenant_id").map_err(ApiError::from)?,
        subscription_id: row.try_get("", "subscription_id").map_err(ApiError::from)?,
        amount_cents: row.try_get("", "amount_cents").map_err(ApiError::from)?,
        currency: row.try_get("", "currency").map_err(ApiError::from)?,
        status: row.try_get("", "status").map_err(ApiError::from)?,
        issued_at: row.try_get("", "issued_at").map_err(ApiError::from)?,
        paid_at: row.try_get("", "paid_at").map_err(ApiError::from)?,
        period_start: row.try_get("", "period_start").map_err(ApiError::from)?,
        period_end: row.try_get("", "period_end").map_err(ApiError::from)?,
        description: row.try_get("", "description").map_err(ApiError::from)?,
    })
}

fn map_usage(row: &QueryResult) -> Result<UsageMeterRow, ApiError> {
    Ok(UsageMeterRow {
        tenant_id: row.try_get("", "tenant_id").map_err(ApiError::from)?,
        meter_key: row.try_get("", "meter_key").map_err(ApiError::from)?,
        quantity: row.try_get("", "quantity").map_err(ApiError::from)?,
        period: row.try_get("", "period").map_err(ApiError::from)?,
        updated_at: row.try_get("", "updated_at").map_err(ApiError::from)?,
    })
}

/// Active plans, cheapest first, for the public pricing list.
pub async fn list_active_plans(db: &DatabaseConnection) -> Result<Vec<PlanRow>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT id, name, description, currency, interval, amount_cents, is_active, \
             external_id, created_at, updated_at FROM plans \
             WHERE is_active = 1 ORDER BY amount_cents ASC, name ASC",
            vec![],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_plan).collect()
}

/// Fetch one plan by id, active or not (administrators may inspect retired ones).
pub async fn get_plan(db: &DatabaseConnection, id: &str) -> Result<Option<PlanRow>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT id, name, description, currency, interval, amount_cents, is_active, \
             external_id, created_at, updated_at FROM plans WHERE id = ?",
            vec![text(id)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_plan).transpose()
}

/// Insert a plan and its single default price in one call.
///
/// The schema has no per-plan feature column, so entitlements are granted from
/// a documented baseline in the handler rather than from hidden plan data.
#[allow(clippy::too_many_arguments)]
pub async fn insert_plan(
    db: &DatabaseConnection,
    id: &str,
    name: &str,
    description: Option<&str>,
    currency: &str,
    interval: &str,
    amount_cents: i64,
    external_id: Option<&str>,
    created_at: &str,
    updated_at: &str,
) -> Result<(), ApiError> {
    db.execute(statement(
        "INSERT INTO plans \
         (id, name, description, currency, interval, amount_cents, is_active, external_id, \
          created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
        vec![
            text(id),
            text(name),
            opt_text(description),
            text(currency),
            text(interval),
            integer(amount_cents),
            opt_text(external_id),
            text(created_at),
            text(updated_at),
        ],
    ))
    .await
    .map_err(ApiError::from)?;

    db.execute(statement(
        "INSERT INTO prices \
         (id, plan_id, amount_cents, currency, interval, external_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        vec![
            text(&format!("price_{id}")),
            text(id),
            integer(amount_cents),
            text(currency),
            text(interval),
            opt_text(external_id),
            text(created_at),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// The default price attached to a plan.
pub async fn get_price_for_plan(
    db: &DatabaseConnection,
    plan_id: &str,
) -> Result<Option<PriceRow>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT id, plan_id, amount_cents, currency, interval, external_id, created_at \
             FROM prices WHERE plan_id = ? ORDER BY created_at ASC LIMIT 1",
            vec![text(plan_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(PriceRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        plan_id: row.try_get("", "plan_id").map_err(ApiError::from)?,
        amount_cents: row.try_get("", "amount_cents").map_err(ApiError::from)?,
        currency: row.try_get("", "currency").map_err(ApiError::from)?,
        interval: row.try_get("", "interval").map_err(ApiError::from)?,
        external_id: row.try_get("", "external_id").map_err(ApiError::from)?,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
    }))
}

/// The subscription a customer-facing endpoint should display. Past-due rows
/// are excluded here on purpose: a payment problem is resolved in billing, not
/// hidden behind the normal "current plan" view.
pub async fn get_current_subscription(
    db: &DatabaseConnection,
    tenant_id: &str,
) -> Result<Option<SubscriptionRow>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT id, tenant_id, plan_id, price_id, status, seat_count, period_start, \
             period_end, cancel_at_period_end, external_customer_id, created_at, updated_at \
             FROM subscriptions \
             WHERE tenant_id = ? AND status IN ('trialing', 'active') \
             ORDER BY created_at DESC LIMIT 1",
            vec![text(tenant_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_subscription).transpose()
}

/// Any live billing relationship, including an overdue invoice.
pub async fn get_billable_subscription(
    db: &DatabaseConnection,
    tenant_id: &str,
) -> Result<Option<SubscriptionRow>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT id, tenant_id, plan_id, price_id, status, seat_count, period_start, \
             period_end, cancel_at_period_end, external_customer_id, created_at, updated_at \
             FROM subscriptions \
             WHERE tenant_id = ? AND status IN ('trialing', 'active', 'past_due') \
             ORDER BY created_at DESC LIMIT 1",
            vec![text(tenant_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_subscription).transpose()
}

/// Fetch a subscription by id *within* a tenant. Even a caller holding a
/// primary key cannot read another tenant's row.
pub async fn get_subscription(
    db: &DatabaseConnection,
    tenant_id: &str,
    id: &str,
) -> Result<Option<SubscriptionRow>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT id, tenant_id, plan_id, price_id, status, seat_count, period_start, \
             period_end, cancel_at_period_end, external_customer_id, created_at, updated_at \
             FROM subscriptions WHERE id = ? AND tenant_id = ?",
            vec![text(id), text(tenant_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_subscription).transpose()
}

/// Resolve a live subscription from a provider customer id (webhook path).
pub async fn find_subscription_by_customer(
    db: &DatabaseConnection,
    external_customer_id: &str,
) -> Result<Option<SubscriptionRow>, ApiError> {
    let row = db
        .query_one(statement(
            "SELECT id, tenant_id, plan_id, price_id, status, seat_count, period_start, \
             period_end, cancel_at_period_end, external_customer_id, created_at, updated_at \
             FROM subscriptions \
             WHERE external_customer_id = ? AND status NOT IN ('cancelled', 'expired') \
             ORDER BY created_at DESC LIMIT 1",
            vec![text(external_customer_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    row.as_ref().map(map_subscription).transpose()
}

/// Create or replace a subscription row. The caller owns the status transition
/// check via [`super::engine::legal_transition`].
#[allow(clippy::too_many_arguments)]
pub async fn upsert_subscription(
    db: &DatabaseConnection,
    id: &str,
    tenant_id: &str,
    plan_id: &str,
    price_id: &str,
    status: &str,
    seat_count: i64,
    period_start: &str,
    period_end: &str,
    created_at: &str,
    updated_at: &str,
) -> Result<(), ApiError> {
    let sql = "INSERT INTO subscriptions \
               (id, tenant_id, plan_id, price_id, status, seat_count, period_start, \
                period_end, cancel_at_period_end, created_at, updated_at) \
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?) \
               ON CONFLICT(id) DO UPDATE SET \
                 plan_id = excluded.plan_id, price_id = excluded.price_id, \
                 status = excluded.status, seat_count = excluded.seat_count, \
                 period_start = excluded.period_start, period_end = excluded.period_end, \
                 cancel_at_period_end = 0, updated_at = excluded.updated_at";
    db.execute(statement(
        sql,
        vec![
            text(id),
            text(tenant_id),
            text(plan_id),
            text(price_id),
            text(status),
            integer(seat_count),
            text(period_start),
            text(period_end),
            text(created_at),
            text(updated_at),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Set `cancel_at_period_end` on the tenant's live subscription.
pub async fn cancel_subscription(
    db: &DatabaseConnection,
    tenant_id: &str,
    updated_at: &str,
) -> Result<bool, ApiError> {
    let result = db
        .execute(statement(
            "UPDATE subscriptions SET cancel_at_period_end = 1, updated_at = ? \
             WHERE tenant_id = ? AND status IN ('trialing', 'active', 'past_due') \
             AND cancel_at_period_end = 0",
            vec![text(updated_at), text(tenant_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

/// Attach the provider customer id (idempotent webhook step).
pub async fn set_external_customer(
    db: &DatabaseConnection,
    subscription_id: &str,
    external_customer_id: &str,
) -> Result<(), ApiError> {
    db.execute(statement(
        "UPDATE subscriptions SET external_customer_id = ? WHERE id = ?",
        vec![text(external_customer_id), text(subscription_id)],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Apply a provider-driven status change. The caller has already validated the
/// transition against the state machine.
pub async fn update_subscription_status(
    db: &DatabaseConnection,
    id: &str,
    status: &str,
    updated_at: &str,
) -> Result<bool, ApiError> {
    let result = db
        .execute(statement(
            "UPDATE subscriptions SET status = ?, updated_at = ? WHERE id = ? \
             AND status <> ?",
            vec![text(status), text(updated_at), text(id), text(status)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

/// Invoices owned by a tenant, newest first.
pub async fn list_invoices(
    db: &DatabaseConnection,
    tenant_id: &str,
) -> Result<Vec<InvoiceRow>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT id, tenant_id, subscription_id, amount_cents, currency, status, \
             issued_at, paid_at, period_start, period_end, description \
             FROM invoices WHERE tenant_id = ? ORDER BY issued_at DESC, id DESC",
            vec![text(tenant_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_invoice).collect()
}

/// Insert an invoice once; a webhook replay with the same provider invoice id
/// is a no-op even if the event was claimed out of order.
#[allow(clippy::too_many_arguments)]
pub async fn insert_invoice(
    db: &DatabaseConnection,
    id: &str,
    tenant_id: &str,
    subscription_id: &str,
    amount_cents: i64,
    currency: &str,
    status: &str,
    issued_at: &str,
    paid_at: Option<&str>,
    period_start: Option<&str>,
    period_end: Option<&str>,
    description: Option<&str>,
) -> Result<(), ApiError> {
    db.execute(statement(
        "INSERT OR IGNORE INTO invoices \
         (id, tenant_id, subscription_id, amount_cents, currency, status, issued_at, \
          paid_at, period_start, period_end, description) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![
            text(id),
            text(tenant_id),
            text(subscription_id),
            integer(amount_cents),
            text(currency),
            text(status),
            text(issued_at),
            opt_text(paid_at),
            opt_text(period_start),
            opt_text(period_end),
            opt_text(description),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Grants owned by a tenant.
pub async fn list_entitlements(
    db: &DatabaseConnection,
    tenant_id: &str,
) -> Result<Vec<Entitlement>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT feature_key, enabled, limit_value FROM entitlements \
             WHERE tenant_id = ? ORDER BY feature_key ASC",
            vec![text(tenant_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter()
        .map(|row| {
            let enabled: i64 = row.try_get("", "enabled").map_err(ApiError::from)?;
            Ok(Entitlement {
                feature_key: row.try_get("", "feature_key").map_err(ApiError::from)?,
                enabled: enabled != 0,
                limit_value: row.try_get("", "limit_value").map_err(ApiError::from)?,
            })
        })
        .collect()
}

/// Replace a tenant's grants; the delete is tenant-scoped so one tenant can
/// never clear another's entitlements.
pub async fn replace_entitlements(
    db: &DatabaseConnection,
    tenant_id: &str,
    grants: &[Entitlement],
) -> Result<(), ApiError> {
    db.execute(statement(
        "DELETE FROM entitlements WHERE tenant_id = ?",
        vec![text(tenant_id)],
    ))
    .await
    .map_err(ApiError::from)?;
    for grant in grants {
        db.execute(statement(
            "INSERT INTO entitlements (tenant_id, feature_key, enabled, limit_value) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(tenant_id, feature_key) DO UPDATE SET \
               enabled = excluded.enabled, limit_value = excluded.limit_value, \
               updated_at = datetime('now')",
            vec![
                text(tenant_id),
                text(&grant.feature_key),
                flag(grant.enabled),
                grant
                    .limit_value
                    .map(integer)
                    .unwrap_or_else(|| Value::String(None)),
            ],
        ))
        .await
        .map_err(ApiError::from)?;
    }
    Ok(())
}

/// Usage meters for a tenant in the current and prior periods.
pub async fn list_usage(
    db: &DatabaseConnection,
    tenant_id: &str,
) -> Result<Vec<UsageMeterRow>, ApiError> {
    let rows = db
        .query_all(statement(
            "SELECT tenant_id, meter_key, quantity, period, updated_at FROM usage_meters \
             WHERE tenant_id = ? ORDER BY period DESC, meter_key ASC",
            vec![text(tenant_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_usage).collect()
}

/// Upsert one counter for the tenant/meter/period triple.
pub async fn upsert_usage(
    db: &DatabaseConnection,
    tenant_id: &str,
    meter_key: &str,
    quantity: i64,
    period: &str,
    updated_at: &str,
) -> Result<(), ApiError> {
    db.execute(statement(
        "INSERT INTO usage_meters (tenant_id, meter_key, quantity, period, updated_at) \
         VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT(tenant_id, meter_key, period) DO UPDATE SET \
           quantity = excluded.quantity, updated_at = excluded.updated_at",
        vec![
            text(tenant_id),
            text(meter_key),
            integer(quantity),
            text(period),
            text(updated_at),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// Atomically claim a provider event. Returns `true` only for the first claim;
/// every replay returns `false` and must be a no-op.
pub async fn claim_event(
    db: &DatabaseConnection,
    provider: &str,
    event_id: &str,
) -> Result<bool, ApiError> {
    let result = db
        .execute(statement(
            "INSERT INTO webhook_events (provider, event_id) VALUES (?, ?) \
             ON CONFLICT(provider, event_id) DO NOTHING",
            vec![text(provider), text(event_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() == 1)
}

/// Record that a claimed event has been fully processed.
pub async fn mark_event_processed(
    db: &DatabaseConnection,
    provider: &str,
    event_id: &str,
) -> Result<(), ApiError> {
    db.execute(statement(
        "UPDATE webhook_events SET processed_at = datetime('now') \
         WHERE provider = ? AND event_id = ?",
        vec![text(provider), text(event_id)],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_sqlite_memory;
    use sea_orm::Statement;

    async fn seed_tenants(db: &DatabaseConnection) {
        for id in ["tenant-a", "tenant-b"] {
            db.execute(statement(
                "INSERT OR IGNORE INTO tenants (id, name, slug) VALUES (?, ?, ?)",
                vec![text(id), text(id), text(id)],
            ))
            .await
            .unwrap();
        }
    }

    async fn seed_plan(db: &DatabaseConnection) {
        insert_plan(
            db,
            "plan-pro",
            "Pro",
            None,
            "USD",
            "month",
            1900,
            None,
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
        )
        .await
        .unwrap();
    }

    async fn seed_subscription(db: &DatabaseConnection, tenant: &str, id: &str) {
        upsert_subscription(
            db,
            id,
            tenant,
            "plan-pro",
            "price_plan-pro",
            "active",
            3,
            "2024-01-01T00:00:00Z",
            "2024-02-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
        )
        .await
        .unwrap();
    }

    fn grants(seats: i64) -> Vec<Entitlement> {
        vec![
            Entitlement {
                feature_key: "deploy".into(),
                enabled: true,
                limit_value: None,
            },
            Entitlement {
                feature_key: "seats".into(),
                enabled: true,
                limit_value: Some(seats),
            },
        ]
    }

    #[tokio::test]
    async fn claim_event_is_idempotent() {
        let db = connect_sqlite_memory().await.unwrap();
        assert!(claim_event(&db, "stripe", "evt_1").await.unwrap());
        assert!(!claim_event(&db, "stripe", "evt_1").await.unwrap());
        // Provider is part of the identity.
        assert!(claim_event(&db, "other", "evt_1").await.unwrap());
        mark_event_processed(&db, "stripe", "evt_1").await.unwrap();
        let processed: i64 = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT processed_at IS NOT NULL AS done FROM webhook_events \
                 WHERE provider = ? AND event_id = ?",
                vec![text("stripe"), text("evt_1")],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "done")
            .unwrap();
        assert_eq!(processed, 1);
    }

    #[tokio::test]
    async fn tenant_scoping_holds_for_subscriptions_invoices_and_entitlements() {
        let db = connect_sqlite_memory().await.unwrap();
        seed_tenants(&db).await;
        seed_plan(&db).await;
        seed_subscription(&db, "tenant-a", "sub-a").await;
        seed_subscription(&db, "tenant-b", "sub-b").await;

        let a = get_current_subscription(&db, "tenant-a")
            .await
            .unwrap()
            .unwrap();
        let b = get_current_subscription(&db, "tenant-b")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(a.id, "sub-a");
        assert_eq!(b.id, "sub-b");

        insert_invoice(
            &db,
            "inv-a",
            "tenant-a",
            "sub-a",
            1900,
            "USD",
            "paid",
            "2024-01-02T00:00:00Z",
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(list_invoices(&db, "tenant-a").await.unwrap().len(), 1);
        assert!(list_invoices(&db, "tenant-b").await.unwrap().is_empty());

        replace_entitlements(&db, "tenant-a", &grants(3))
            .await
            .unwrap();
        assert_eq!(list_entitlements(&db, "tenant-a").await.unwrap().len(), 2);
        assert!(list_entitlements(&db, "tenant-b").await.unwrap().is_empty());
        // Replacing A's grants must not touch B's.
        replace_entitlements(&db, "tenant-b", &grants(99))
            .await
            .unwrap();
        let a_grants = list_entitlements(&db, "tenant-a").await.unwrap();
        assert_eq!(
            a_grants
                .iter()
                .find(|g| g.feature_key == "seats")
                .and_then(|g| g.limit_value),
            Some(3)
        );

        upsert_usage(&db, "tenant-a", "api_calls", 7, "2024-01", "now")
            .await
            .unwrap();
        assert_eq!(list_usage(&db, "tenant-a").await.unwrap().len(), 1);
        assert!(list_usage(&db, "tenant-b").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancel_and_status_updates_are_tenant_scoped() {
        let db = connect_sqlite_memory().await.unwrap();
        seed_tenants(&db).await;
        seed_plan(&db).await;
        seed_subscription(&db, "tenant-a", "sub-a").await;
        seed_subscription(&db, "tenant-b", "sub-b").await;

        assert!(cancel_subscription(&db, "tenant-a", "now").await.unwrap());
        assert!(!cancel_subscription(&db, "tenant-a", "now").await.unwrap());
        let a = get_subscription(&db, "tenant-a", "sub-a")
            .await
            .unwrap()
            .unwrap();
        let b = get_subscription(&db, "tenant-b", "sub-b")
            .await
            .unwrap()
            .unwrap();
        assert!(a.cancel_at_period_end);
        assert!(!b.cancel_at_period_end);

        assert!(update_subscription_status(&db, "sub-a", "past_due", "now")
            .await
            .unwrap());
        assert_eq!(
            get_subscription(&db, "tenant-a", "sub-a")
                .await
                .unwrap()
                .unwrap()
                .status,
            "past_due"
        );
        assert_eq!(
            get_subscription(&db, "tenant-b", "sub-b")
                .await
                .unwrap()
                .unwrap()
                .status,
            "active"
        );
    }
}
