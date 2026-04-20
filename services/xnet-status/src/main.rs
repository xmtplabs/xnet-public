mod config;
mod cutover;
mod events;
mod health;
mod migration;
mod phase;
mod prometheus;
mod routes;
mod state;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "xnet-status", about = "xnet status page and API")]
struct Cli {
    #[arg(long, short)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "xnet_status=info".into()),
        )
        .init();

    let cli = Cli::parse();
    tracing::info!("loading config from {:?}", cli.config);

    let cfg = config::StatusConfig::load(&cli.config)?;
    let listen_addr = cfg.status.listen.clone();

    let app_state = state::AppState::new(cfg.status);
    state::spawn_background_tasks(app_state.clone());

    let router = routes::build_router(app_state);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("listening on {}", listen_addr);

    axum::serve(listener, router).await?;

    Ok(())
}
