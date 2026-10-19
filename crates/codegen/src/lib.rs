//! Code generation pipeline for the No-Code IDE

pub mod backend;
pub mod database;
pub mod frontend;
pub mod project;
pub mod targets;
pub mod templates;
pub mod view;

pub use backend::*;
pub use database::*;
pub use frontend::*;
pub use project::*;
pub use targets::*;
pub use view::*;
