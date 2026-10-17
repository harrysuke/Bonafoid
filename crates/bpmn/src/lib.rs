//! No-Code IDE BPMN integration.
//!
//! - [`engine`] implements a token-based process engine over
//!   [`nocode_model::Workflow`].
//! - [`expr`] evaluates gateway condition expressions.
//! - [`xml`] imports/exports BPMN 2.0 XML for `bpmn-js`.
//! - [`napi_bridge`] exposes the engine to the TypeScript frontend via
//!   NAPI-RS (enable the `node` feature).

pub mod engine;
pub mod error;
pub mod expr;
#[cfg(feature = "node")]
pub mod napi_bridge;
pub mod xml;

pub use engine::{
    BpmnEngine, HistoryEvent, InstanceStatus, ProcessInstance, TaskInstance, TaskStatus, Token,
    TokenStatus, ValidationIssue, ValidationResult, Severity,
};
pub use error::{BpmnError, Result};
