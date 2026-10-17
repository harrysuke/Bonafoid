//! Code generation pipeline for the No-Code IDE

pub mod backend;
pub mod frontend;
pub mod database;
pub mod templates;
pub mod view;
pub mod project;
pub mod targets;

pub use view::*;
pub use backend::*;
pub use frontend::*;
pub use database::*;
pub use project::*;
pub use targets::*;