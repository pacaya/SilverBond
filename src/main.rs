use std::net::SocketAddr;

use silverbond::{
    app::ApplicationConfig,
    host::{ApplicationHost, HostConfig},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "silverbond=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let host = ApplicationHost::start(HostConfig {
        application: ApplicationConfig::from_cli_environment()?,
        bind_addr: SocketAddr::from(([127, 0, 0, 1], 3333)),
    })
    .await?;

    tracing::info!("SilverBond -> {}", host.local_url());
    tracing::info!("Workflows: {}", host.paths().workflows_dir.display());
    tracing::info!("Templates: {}", host.paths().templates_dir.display());
    tracing::info!("Database:  {}", host.paths().database_path.display());

    tokio::signal::ctrl_c().await?;
    host.shutdown().await?;

    Ok(())
}
