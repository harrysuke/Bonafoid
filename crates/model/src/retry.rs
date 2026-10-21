//! The retry policy shared by everything that can retry a request.
//!
//! Three modules used to define their own `RetryConfig`: `page` (data
//! sources), `workflow` (HTTP service tasks) and `project` (webhooks). All
//! three were glob re-exported from the crate root, so `nocode_model::
//! RetryConfig` was ambiguous and `cargo clippy -- -D warnings` failed on
//! it. `page` and `workflow` were byte-identical apart from the name of
//! the last field, so they are now one type here.
//!
//! The wire name is `retryable_codes`, which is what the page module used
//! and what the frontend's `listModel` writes. The workflow module's
//! `retryable_status_codes` is kept as a serde alias, so projects saved
//! before the unification still load.
//!
//! `#[serde(default)]` on the list is what lets the old webhook config —
//! which had no retryable list at all — keep loading.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How many times to retry, and how long to wait between attempts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct RetryConfig {
    /// Total attempts, including the first one.
    pub max_attempts: u32,
    /// Delay before the first retry, in milliseconds.
    pub initial_interval_ms: u64,
    /// Ceiling for the backoff delay, in milliseconds.
    pub max_interval_ms: u64,
    /// Factor the delay is multiplied by after each attempt.
    pub multiplier: f64,
    /// Response status codes that should be retried.
    #[serde(rename = "retryable_codes", alias = "retryable_status_codes", default)]
    pub retryable_codes: Vec<u16>,
}

#[cfg(test)]
mod tests {
    use super::RetryConfig;
    use serde_json::json;

    #[test]
    fn retry_config_reads_the_old_workflow_field_name() {
        let saved = json!({
            "max_attempts": 3,
            "initial_interval_ms": 500,
            "max_interval_ms": 5000,
            "multiplier": 2.0,
            "retryable_status_codes": [429, 500, 502, 503, 504],
        });

        let parsed: RetryConfig = serde_json::from_value(saved).expect("old workflow shape loads");

        assert_eq!(parsed.retryable_codes, vec![429, 500, 502, 503, 504]);
    }

    #[test]
    fn retry_config_reads_the_old_page_field_name() {
        let saved = json!({
            "max_attempts": 3,
            "initial_interval_ms": 500,
            "max_interval_ms": 5000,
            "multiplier": 2.0,
            "retryable_codes": [429, 500],
        });

        let parsed: RetryConfig = serde_json::from_value(saved).expect("page shape loads");

        assert_eq!(parsed.retryable_codes, vec![429, 500]);
    }

    #[test]
    fn retry_config_reads_the_old_webhook_shape_without_a_list() {
        let saved = json!({
            "max_attempts": 3,
            "initial_interval_ms": 500,
            "max_interval_ms": 5000,
            "multiplier": 2.0,
        });

        let parsed: RetryConfig = serde_json::from_value(saved).expect("webhook shape loads");

        assert!(parsed.retryable_codes.is_empty());
    }

    #[test]
    fn retry_config_writes_the_canonical_field_name() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_interval_ms: 500,
            max_interval_ms: 5000,
            multiplier: 2.0,
            retryable_codes: vec![429],
        };

        let written = serde_json::to_value(&config).expect("serialises");

        assert_eq!(written["retryable_codes"], json!([429]));
        assert!(
            written.get("retryable_status_codes").is_none(),
            "the old field name must not be written: {written}"
        );
    }

    #[test]
    fn the_crate_root_re_exports_are_unambiguous() {
        // This is the whole point of the cleanup: `nocode_model::RetryConfig`
        // used to be an ambiguous glob re-export, and `nocode_model::FieldId`
        // was ambiguous twice over.
        let config: crate::RetryConfig = RetryConfig::default();
        assert_eq!(config.max_attempts, 0);

        let entity: crate::EntityFieldId = crate::entity::EntityFieldId::new();
        let form: crate::FormFieldId = crate::form::FormFieldId::new();
        assert_ne!(entity.0, form.0);
    }
}
