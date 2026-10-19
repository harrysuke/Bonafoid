//! No-Code IDE runtime library.

pub mod auth;
pub mod server;
pub mod storage;
pub mod workflow;

pub use server::RuntimeServer;

/// Resolve the listen port from the `PORT` environment variable, falling back
/// to `default_port` when unset or invalid.
pub fn port_from_env(default_port: u16) -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(default_port)
}
