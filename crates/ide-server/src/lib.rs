//! No-Code IDE web server.
//!
//! Axum-based backend that exposes the IDE service over REST, serves the built
//! web IDE static assets, and persists IDE state in SQLite via SeaORM.

pub mod auth;
pub mod bpmn;
pub mod db;
pub mod error;
pub mod generation;
pub mod migration;
pub mod plugins;
pub mod preview;
pub mod projects;
pub mod server;

pub use auth::JwtKeys;
pub use plugins::{PluginRegistry, PluginSummary};
pub use preview::{BuildManager, BuildRecord, BuildState, BuildTarget, PreviewConfig};
pub use server::{build_router, serve, AppState, ServerConfig};
