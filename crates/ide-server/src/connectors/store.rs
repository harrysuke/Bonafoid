//! Tenant-scoped raw-SQL persistence for connector instances.
//!
//! # Isolation
//! Every statement carries `tenant_id` in its `WHERE` clause (or in its
//! `INSERT`). An instance id alone is never sufficient to read, change or
//! delete a row, so a forgotten ownership check cannot cross tenants.
//!
//! # Secret discipline
//! `config_json` contains only the recursively redacted configuration. The
//! plaintext secret is sealed before it reaches this module and is stored
//! exclusively in `sealed_nonce` / `sealed_ciphertext`. `list` does not select
//! those columns at all; `get` does, but only into this private row type,
//! never into a response DTO.

use nocode_deploy::SealedCredential;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, Value as DbValue,
};
use serde_json::Value;

use crate::error::ApiError;

pub use super::runtime::redact_config;

const RESPONSE_COLUMNS: &str = "id, tenant_id, kind, name, config_json, enabled, \
                                (sealed_ciphertext IS NOT NULL) AS has_secret, created_at, updated_at";
const ALL_COLUMNS: &str =
    "id, tenant_id, kind, name, config_json, sealed_nonce, sealed_ciphertext, enabled, created_at, updated_at";

fn statement(sql: &str, values: Vec<DbValue>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn text(value: &str) -> DbValue {
    DbValue::from(value.to_string())
}

fn opt_text(value: Option<&str>) -> DbValue {
    DbValue::from(value)
}

/// Internal row type. The sealed columns are ciphertext, never a token.
#[derive(Debug, Clone)]
pub struct ConnectorRow {
    pub id: String,
    pub tenant_id: String,
    pub kind: String,
    pub name: String,
    pub config_json: String,
    pub sealed_nonce: Option<String>,
    pub sealed_ciphertext: Option<String>,
    /// Whether a sealed secret exists (selected without exposing the blob).
    pub has_secret: bool,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl ConnectorRow {
    /// Reassemble the stored blob, if this instance has a secret.
    pub fn sealed(&self) -> Option<SealedCredential> {
        Some(SealedCredential {
            nonce: self.sealed_nonce.clone()?,
            ciphertext: self.sealed_ciphertext.clone()?,
        })
    }
}

fn map_row_inner(row: &QueryResult, with_sealed: bool) -> Result<ConnectorRow, ApiError> {
    let sealed_nonce = if with_sealed {
        row.try_get::<Option<String>>("", "sealed_nonce")
            .map_err(ApiError::from)?
    } else {
        None
    };
    let sealed_ciphertext = if with_sealed {
        row.try_get::<Option<String>>("", "sealed_ciphertext")
            .map_err(ApiError::from)?
    } else {
        None
    };
    let has_secret = if with_sealed {
        sealed_nonce.is_some() && sealed_ciphertext.is_some()
    } else {
        row.try_get::<i64>("", "has_secret")
            .map_err(ApiError::from)?
            != 0
    };
    Ok(ConnectorRow {
        id: row.try_get("", "id").map_err(ApiError::from)?,
        tenant_id: row.try_get("", "tenant_id").map_err(ApiError::from)?,
        kind: row.try_get("", "kind").map_err(ApiError::from)?,
        name: row.try_get("", "name").map_err(ApiError::from)?,
        config_json: row.try_get("", "config_json").map_err(ApiError::from)?,
        sealed_nonce,
        sealed_ciphertext,
        has_secret,
        enabled: row.try_get::<i64>("", "enabled").map_err(ApiError::from)? != 0,
        created_at: row.try_get("", "created_at").map_err(ApiError::from)?,
        updated_at: row.try_get("", "updated_at").map_err(ApiError::from)?,
    })
}

fn map_row(row: &QueryResult) -> Result<ConnectorRow, ApiError> {
    map_row_inner(row, true)
}

/// `list` uses a projection without sealed columns, so a response DTO cannot
/// accidentally serialise a credential blob.
fn map_public_row(row: &QueryResult) -> Result<ConnectorRow, ApiError> {
    map_row_inner(row, false)
}

/// Parse stored, already-redacted config. Corrupt data is a server fault, not
/// a caller error.
pub fn parse_config(json: &str) -> Result<Value, ApiError> {
    serde_json::from_str(json).map_err(|error| {
        tracing::error!(%error, "stored connector config is corrupt");
        ApiError::Internal
    })
}

/// Insert a new instance. `config_json` must already be redacted and `sealed`
/// must already be encrypted by the caller.
pub async fn create(
    db: &DatabaseConnection,
    tenant_id: &str,
    id: &str,
    kind: &str,
    name: &str,
    config_json: &str,
    sealed: Option<&SealedCredential>,
) -> Result<(), ApiError> {
    db.execute(statement(
        "INSERT INTO connector_instances \
         (id, tenant_id, kind, name, config_json, sealed_nonce, sealed_ciphertext) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        vec![
            text(id),
            text(tenant_id),
            text(kind),
            text(name),
            text(config_json),
            opt_text(sealed.map(|s| s.nonce.as_str())),
            opt_text(sealed.map(|s| s.ciphertext.as_str())),
        ],
    ))
    .await
    .map_err(ApiError::from)?;
    Ok(())
}

/// List a tenant's instances. Sealed columns are deliberately not selected.
pub async fn list(db: &DatabaseConnection, tenant_id: &str) -> Result<Vec<ConnectorRow>, ApiError> {
    let rows = db
        .query_all(statement(
            &format!(
                "SELECT {RESPONSE_COLUMNS} FROM connector_instances \
                 WHERE tenant_id = ? ORDER BY created_at DESC, id DESC"
            ),
            vec![text(tenant_id)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.iter().map(map_public_row).collect()
}

/// Fetch one instance by tenant AND id.
pub async fn get(
    db: &DatabaseConnection,
    tenant_id: &str,
    id: &str,
) -> Result<Option<ConnectorRow>, ApiError> {
    let rows = db
        .query_all(statement(
            &format!(
                "SELECT {ALL_COLUMNS} FROM connector_instances \
                 WHERE tenant_id = ? AND id = ? LIMIT 1"
            ),
            vec![text(tenant_id), text(id)],
        ))
        .await
        .map_err(ApiError::from)?;
    rows.first().map(map_row).transpose()
}

/// Update selected fields of one tenant's instance.
///
/// `None` means "leave unchanged" for every field. Fragments are fixed
/// strings; no caller text is interpolated into SQL.
pub async fn update(
    db: &DatabaseConnection,
    tenant_id: &str,
    id: &str,
    name: Option<&str>,
    config_json: Option<&str>,
    sealed: Option<&SealedCredential>,
) -> Result<bool, ApiError> {
    let mut assignments = Vec::new();
    let mut values = Vec::new();
    if let Some(name) = name {
        assignments.push("name = ?");
        values.push(text(name));
    }
    if let Some(config_json) = config_json {
        assignments.push("config_json = ?");
        values.push(text(config_json));
    }
    if let Some(sealed) = sealed {
        assignments.push("sealed_nonce = ?");
        assignments.push("sealed_ciphertext = ?");
        values.push(opt_text(Some(sealed.nonce.as_str())));
        values.push(opt_text(Some(sealed.ciphertext.as_str())));
    }
    if assignments.is_empty() {
        return Ok(false);
    }
    assignments.push("updated_at = datetime('now')");
    values.push(text(tenant_id));
    values.push(text(id));
    let sql = format!(
        "UPDATE connector_instances SET {} WHERE tenant_id = ? AND id = ?",
        assignments.join(", ")
    );
    let result = db
        .execute(statement(&sql, values))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() > 0)
}

/// Delete one tenant's instance. Returns whether a row was actually removed.
pub async fn remove(db: &DatabaseConnection, tenant_id: &str, id: &str) -> Result<bool, ApiError> {
    let result = db
        .execute(statement(
            "DELETE FROM connector_instances WHERE tenant_id = ? AND id = ?",
            vec![text(tenant_id), text(id)],
        ))
        .await
        .map_err(ApiError::from)?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_replaces_secret_values_even_in_nested_arrays() {
        let input = serde_json::json!({
            "api_key": "KEY-1",
            "nested": [{"token": "KEY-2", "url": "http://x"}]
        });
        let output = redact_config(&input);
        assert_eq!(
            output["api_key"],
            serde_json::json!(nocode_deploy::REDACTED)
        );
        assert_eq!(
            output["nested"][0]["token"],
            serde_json::json!(nocode_deploy::REDACTED)
        );
        assert_eq!(output["nested"][0]["url"], serde_json::json!("http://x"));
    }

    #[test]
    fn parse_config_reports_corrupt_stored_json() {
        assert!(parse_config("{\"ok\": true}").is_ok());
        assert!(parse_config("not json").is_err());
    }
}
