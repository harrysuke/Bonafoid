//! Billing schema (Phase 8, migration 0006).
//!
//! Provider-agnostic: no Stripe-specific identifier is stored on the core
//! tables except the optional external customer id. Money is always integer
//! minor units (cents) with an ISO-4217 currency code.

/// Plans, prices, subscriptions, invoices, entitlements, usage and webhooks.
pub const BILLING_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS plans (
    id           TEXT PRIMARY KEY NOT NULL,
    name         TEXT NOT NULL,
    description  TEXT,
    currency     TEXT NOT NULL DEFAULT 'USD',
    interval     TEXT NOT NULL CHECK (interval IN ('month', 'year')),
    amount_cents INTEGER NOT NULL,
    is_active    INTEGER NOT NULL DEFAULT 1,
    external_id  TEXT,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS prices (
    id          TEXT PRIMARY KEY NOT NULL,
    plan_id     TEXT NOT NULL,
    amount_cents INTEGER NOT NULL,
    currency    TEXT NOT NULL,
    interval    TEXT NOT NULL,
    external_id TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_prices_plan ON prices(plan_id);

CREATE TABLE IF NOT EXISTS subscriptions (
    id                    TEXT PRIMARY KEY NOT NULL,
    tenant_id             TEXT NOT NULL,
    plan_id               TEXT NOT NULL,
    price_id              TEXT NOT NULL,
    status                TEXT NOT NULL,
    seat_count            INTEGER NOT NULL DEFAULT 1,
    period_start          TEXT NOT NULL,
    period_end            TEXT NOT NULL,
    cancel_at_period_end  INTEGER NOT NULL DEFAULT 0,
    external_customer_id  TEXT,
    created_at            TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at            TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE,
    FOREIGN KEY (plan_id) REFERENCES plans(id)
);
CREATE INDEX IF NOT EXISTS idx_subscriptions_tenant ON subscriptions(tenant_id);

CREATE TABLE IF NOT EXISTS invoices (
    id               TEXT PRIMARY KEY NOT NULL,
    tenant_id        TEXT NOT NULL,
    subscription_id  TEXT NOT NULL,
    amount_cents     INTEGER NOT NULL,
    currency         TEXT NOT NULL,
    status           TEXT NOT NULL,
    issued_at        TEXT NOT NULL,
    paid_at          TEXT,
    period_start     TEXT,
    period_end       TEXT,
    description      TEXT,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE,
    FOREIGN KEY (subscription_id) REFERENCES subscriptions(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_invoices_tenant ON invoices(tenant_id, issued_at);

CREATE TABLE IF NOT EXISTS entitlements (
    tenant_id    TEXT NOT NULL,
    feature_key  TEXT NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 0,
    limit_value  INTEGER,
    updated_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (tenant_id, feature_key),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS usage_meters (
    tenant_id    TEXT NOT NULL,
    meter_key    TEXT NOT NULL,
    quantity     INTEGER NOT NULL DEFAULT 0,
    period       TEXT NOT NULL,
    updated_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (tenant_id, meter_key, period),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS webhook_events (
    provider      TEXT NOT NULL,
    event_id      TEXT NOT NULL,
    received_at   TEXT NOT NULL DEFAULT (datetime('now')),
    processed_at  TEXT,
    PRIMARY KEY (provider, event_id)
);
"#;
