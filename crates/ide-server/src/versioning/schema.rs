//! Version control and environments schema (Phase 11, migration 0009).
//!
//! Revisions are append-only: a commit is never edited or deleted. Branches
//! point at a head revision; environments point at the revision promoted to
//! them and can be rolled back by moving that pointer.

/// Commit graph, branches, environments, secrets and deployment records.
pub const VERSIONING_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS project_revisions (
    id          TEXT PRIMARY KEY NOT NULL,
    tenant_id   TEXT NOT NULL,
    project_id  TEXT NOT NULL,
    parent_id   TEXT,
    branch      TEXT NOT NULL,
    message     TEXT NOT NULL,
    author_id   TEXT NOT NULL,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_revisions_project ON project_revisions(tenant_id, project_id, created_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_revisions_parent ON project_revisions(parent_id) WHERE parent_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS project_branches (
    tenant_id         TEXT NOT NULL,
    project_id        TEXT NOT NULL,
    name              TEXT NOT NULL,
    head_revision_id  TEXT NOT NULL,
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (tenant_id, project_id, name)
);

CREATE TABLE IF NOT EXISTS project_environments (
    tenant_id    TEXT NOT NULL,
    project_id   TEXT NOT NULL,
    name         TEXT NOT NULL,
    config_json  TEXT NOT NULL,
    revision_id  TEXT,
    status       TEXT NOT NULL DEFAULT 'idle',
    updated_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (tenant_id, project_id, name),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS environment_secrets (
    tenant_id         TEXT NOT NULL,
    project_id        TEXT NOT NULL,
    environment_name  TEXT NOT NULL,
    key               TEXT NOT NULL,
    sealed_nonce      TEXT NOT NULL,
    sealed_ciphertext TEXT NOT NULL,
    PRIMARY KEY (tenant_id, project_id, environment_name, key)
);

CREATE TABLE IF NOT EXISTS environment_deployments (
    id               TEXT PRIMARY KEY NOT NULL,
    tenant_id        TEXT NOT NULL,
    project_id       TEXT NOT NULL,
    environment_name TEXT NOT NULL,
    revision_id      TEXT NOT NULL,
    status           TEXT NOT NULL,
    deployed_at      TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_env_deployments
    ON environment_deployments(tenant_id, project_id, environment_name, deployed_at);
"#;
