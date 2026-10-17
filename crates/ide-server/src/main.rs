use anyhow::Result;
use nocode_ide_server::{db, serve, AppState, JwtKeys, ServerConfig};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nocode_ide_server=info,tower_http=info".into()),
        )
        .init();

    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8081);

    let config = ServerConfig {
        port,
        ..ServerConfig::default()
    };

    let database_url = db::default_url();
    tracing::info!(%database_url, "connecting to database");
    let db = db::connect(&database_url).await?;

    let keys_dir =
        PathBuf::from(std::env::var("JWT_KEYS_DIR").unwrap_or_else(|_| "jwt_keys".into()));
    let keys = Arc::new(JwtKeys::load_or_generate(&keys_dir)?);
    let state = AppState::new(db, keys);

    serve(config, state).await
}
