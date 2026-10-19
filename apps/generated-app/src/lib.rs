//! Generated No-Code application.

pub mod config;
pub mod db;
pub mod handlers;
pub mod middleware;
pub mod models;
pub mod routes;
pub mod services;
pub mod utils;

/// Start the application HTTP server.
///
/// The listen port is read from the `PORT` environment variable (default 8080).
/// This blocks until the server stops.
pub fn start() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let port = nocode_runtime::port_from_env(8080);
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("failed to start tokio runtime: {err}");
            return;
        }
    };

    let result =
        runtime.block_on(async move { nocode_runtime::RuntimeServer::new(port).serve().await });

    if let Err(err) = result {
        eprintln!("runtime server error: {err:#}");
    }
}
