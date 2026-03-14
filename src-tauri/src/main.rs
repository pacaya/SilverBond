use std::{
    io,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use silverbond::{
    app::{AppPaths, ApplicationConfig},
    host::{ApplicationHost, HostConfig},
};
use anyhow::Context;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "silverbond=info,tauri=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let host_state = Arc::new(Mutex::new(None::<ApplicationHost>));
    let shutdown_state = Arc::clone(&host_state);

    let app = tauri::Builder::default()
        .setup(move |app| {
            let host_state = Arc::clone(&host_state);
            tauri::async_runtime::block_on(async move {
                let app_data_dir = app
                    .path()
                    .app_data_dir()
                    .context("Failed to resolve SilverBond app data directory")?;
                let host = ApplicationHost::start(HostConfig {
                    application: ApplicationConfig {
                        paths: AppPaths::from_root(app_data_dir),
                        seed_bundled_templates: true,
                    },
                    bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                })
                .await?;

                if let Err(error) = host.wait_for_health(Duration::from_secs(5)).await {
                    let _ = host.shutdown().await;
                    return Err(error);
                }

                let window_url = host
                    .local_url()
                    .parse()
                    .context("Failed to build SilverBond localhost URL")?;
                if let Err(error) = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(window_url))
                    .title("SilverBond")
                    .inner_size(1440.0, 960.0)
                    .min_inner_size(1100.0, 700.0)
                    .resizable(true)
                    .build()
                {
                    let _ = host.shutdown().await;
                    return Err(anyhow::Error::from(error));
                }

                *host_state.lock().expect("host state lock poisoned") = Some(host);
                Ok::<(), anyhow::Error>(())
            })
            .map_err(|error| Box::new(io::Error::other(error.to_string())) as Box<dyn std::error::Error>)
        })
        .build(tauri::generate_context!())
        .expect("failed to build SilverBond Tauri shell");

    app.run(move |app, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            api.prevent_exit();
            let app_handle = app.clone();
            let host_state = Arc::clone(&shutdown_state);
            tauri::async_runtime::spawn(async move {
                let host = host_state.lock().expect("host state lock poisoned").take();
                if let Some(host) = host {
                    let _ = host.shutdown().await;
                }
                app_handle.exit(0);
            });
        }
    });
}
