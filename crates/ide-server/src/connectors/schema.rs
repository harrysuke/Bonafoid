//! Connector registry schema (Phase 10, migration 0008).
//!
//! Every instance belongs to exactly one tenant. Secrets live ONLY in the
//! sealed columns; `config_json` must contain redacted configuration.

/// Per-tenant connector instances with an encrypted credential blob.
pub const CONNECTORS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS connector_instances (
    id                TEXT NOT NULL,
    tenant_id         TEXT NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('http_rest', 'webhook', 'oauth2', 'sql', 'message_queue')),
    name              TEXT NOT NULL,
    config_json       TEXT NOT NULL,
    sealed_nonce      TEXT,
    sealed_ciphertext TEXT,
    enabled           INTEGER NOT NULL DEFAULT 1,
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_connectors_tenant ON connector_instances(tenant_id);
"#;
