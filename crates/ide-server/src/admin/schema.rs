//! Platform administration schema (Phase 9, migration 0007).
//!
//! These tables are platform-scoped and are never queried by the customer IDE.
//! `platform_flags` is deliberately NOT the per-project `FeatureFlags` model.

/// Platform flags, audit trail, impersonation, telemetry and incidents.
pub const ADMIN_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS platform_flags (
    key                TEXT PRIMARY KEY NOT NULL,
    description        TEXT,
    enabled            INTEGER NOT NULL DEFAULT 0,
    rollout_percentage INTEGER NOT NULL DEFAULT 0 CHECK (rollout_percentage BETWEEN 0 AND 100),
    kill_switch        INTEGER NOT NULL DEFAULT 0,
    created_at         TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS tenant_flag_overrides (
    flag_key  TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    enabled   INTEGER NOT NULL,
    PRIMARY KEY (flag_key, tenant_id),
    FOREIGN KEY (flag_key) REFERENCES platform_flags(key) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS audit_log (
    id              TEXT PRIMARY KEY NOT NULL,
    actor_user_id   TEXT NOT NULL,
    actor_email     TEXT,
    tenant_id       TEXT,
    action          TEXT NOT NULL,
    target_type     TEXT NOT NULL,
    target_id       TEXT,
    before_json     TEXT,
    after_json      TEXT,
    ip_address      TEXT,
    user_agent      TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_audit_log_created ON audit_log(created_at);
CREATE INDEX IF NOT EXISTS idx_audit_log_tenant ON audit_log(tenant_id);

CREATE TABLE IF NOT EXISTS impersonations (
    id              TEXT PRIMARY KEY NOT NULL,
    admin_user_id   TEXT NOT NULL,
    target_user_id  TEXT NOT NULL,
    tenant_id       TEXT NOT NULL,
    reason          TEXT,
    started_at      TEXT NOT NULL DEFAULT (datetime('now')),
    ended_at        TEXT
);
CREATE INDEX IF NOT EXISTS idx_impersonations_target ON impersonations(target_user_id);

CREATE TABLE IF NOT EXISTS usage_events (
    id         TEXT PRIMARY KEY NOT NULL,
    tenant_id  TEXT NOT NULL,
    kind       TEXT NOT NULL,
    quantity   INTEGER NOT NULL DEFAULT 1,
    occurred_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_usage_events_tenant ON usage_events(tenant_id, kind);

CREATE TABLE IF NOT EXISTS status_incidents (
    id          TEXT PRIMARY KEY NOT NULL,
    title       TEXT NOT NULL,
    status      TEXT NOT NULL CHECK (status IN ('investigating', 'identified', 'monitoring', 'resolved')),
    message     TEXT,
    component   TEXT,
    opened_at   TEXT NOT NULL DEFAULT (datetime('now')),
    resolved_at TEXT
);
"#;
