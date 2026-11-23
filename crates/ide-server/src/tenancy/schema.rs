//! SaaS tenancy schema (Phase 7, migrations 0004 and 0005).
//!
//! Migrations are append-only: 0004 adds nullable columns and the new tables,
//! 0005 backfills existing rows into the default tenant. SQLite cannot
//! retrofit NOT NULL onto an existing column, so the application layer
//! enforces tenant_id presence on every write; the backfill makes every
//! pre-existing row owned by the default tenant.

/// New tables and nullable columns added for tenancy.
pub const TENANCY_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tenants (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL UNIQUE,
    plan_id     TEXT,
    status      TEXT NOT NULL DEFAULT 'active',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS tenant_members (
    tenant_id  TEXT NOT NULL,
    user_id    TEXT NOT NULL,
    role       TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (tenant_id, user_id),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

ALTER TABLE users ADD COLUMN platform_role TEXT;
ALTER TABLE users ADD COLUMN tenant_id TEXT;
ALTER TABLE projects ADD COLUMN tenant_id TEXT;
ALTER TABLE plugins ADD COLUMN tenant_id TEXT;
ALTER TABLE integrations ADD COLUMN tenant_id TEXT;
ALTER TABLE deployments ADD COLUMN tenant_id TEXT;
"#;

/// Second step: backfill rows created before tenancy existed into the default
/// tenant, then add the indexes the isolation queries rely on.
pub const TENANT_BACKFILL: &str = r#"
INSERT OR IGNORE INTO tenants (id, name, slug, status)
    VALUES ('default-tenant', 'Default', 'default', 'active');

UPDATE users SET tenant_id = 'default-tenant' WHERE tenant_id IS NULL OR tenant_id = '';
UPDATE projects SET tenant_id = 'default-tenant' WHERE tenant_id IS NULL OR tenant_id = '';
UPDATE plugins SET tenant_id = 'default-tenant' WHERE tenant_id IS NULL OR tenant_id = '';
UPDATE integrations SET tenant_id = 'default-tenant' WHERE tenant_id IS NULL OR tenant_id = '';
UPDATE deployments SET tenant_id = 'default-tenant' WHERE tenant_id IS NULL OR tenant_id = '';

INSERT OR IGNORE INTO tenant_members (tenant_id, user_id, role)
    SELECT 'default-tenant', id, 'owner' FROM users;

CREATE INDEX IF NOT EXISTS idx_tenants_slug ON tenants(slug);
CREATE INDEX IF NOT EXISTS idx_tenant_members_user ON tenant_members(user_id);
CREATE INDEX IF NOT EXISTS idx_projects_tenant ON projects(tenant_id, user_id);
CREATE INDEX IF NOT EXISTS idx_plugins_tenant ON plugins(tenant_id, user_id);
CREATE INDEX IF NOT EXISTS idx_integrations_tenant ON integrations(tenant_id, user_id);
CREATE INDEX IF NOT EXISTS idx_deployments_tenant ON deployments(tenant_id, user_id);
"#;

/// The tenant every pre-tenancy row is migrated into.
pub const DEFAULT_TENANT_ID: &str = "default-tenant";
