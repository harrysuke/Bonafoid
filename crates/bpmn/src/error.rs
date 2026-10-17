//! Error types for the BPMN engine and NAPI bridge.

use thiserror::Error;

/// Result alias used throughout the BPMN crate.
pub type Result<T> = std::result::Result<T, BpmnError>;

/// Errors produced by validation, execution, and BPMN parsing.
#[derive(Debug, Error)]
pub enum BpmnError {
    #[error("invalid workflow: {0}")]
    InvalidWorkflow(String),

    #[error("execution error: {0}")]
    Execution(String),

    #[error("process instance `{0}` is not running")]
    InstanceNotRunning(String),

    #[error("task `{0}` not found")]
    TaskNotFound(String),

    #[error("task `{0}` has already been completed")]
    TaskAlreadyCompleted(String),

    #[error("expression error: {0}")]
    Expression(String),

    #[error("invalid BPMN XML: {0}")]
    Xml(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
