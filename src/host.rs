use std::{net::SocketAddr, time::{Duration, Instant}};

use anyhow::Context;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
    time::sleep,
};

use crate::app::{AppPaths, Application, ApplicationConfig};

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub application: ApplicationConfig,
    pub bind_addr: SocketAddr,
}

pub struct ApplicationHost {
    local_addr: SocketAddr,
    paths: AppPaths,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: JoinHandle<anyhow::Result<()>>,
}

impl ApplicationHost {
    pub async fn start(config: HostConfig) -> anyhow::Result<Self> {
        let application = Application::boot(config.application).await?;
        let listener = TcpListener::bind(config.bind_addr).await?;
        let local_addr = listener.local_addr()?;
        let paths = application.paths.clone();
        let router = application.router();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .map_err(anyhow::Error::from)
        });

        Ok(Self {
            local_addr,
            paths,
            shutdown_tx: Some(shutdown_tx),
            task,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn local_url(&self) -> String {
        format!("http://{}", self.local_addr)
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub async fn wait_for_health(&self, timeout: Duration) -> anyhow::Result<()> {
        wait_for_health(self.local_addr, timeout).await
    }

    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        self.task.await.context("server task join failed")??;
        Ok(())
    }
}

async fn wait_for_health(local_addr: SocketAddr, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match check_health(local_addr).await {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(error) if Instant::now() < deadline => {
                tracing::debug!("health probe failed while waiting for startup: {error}");
            }
            Err(error) => return Err(error),
        }
        if Instant::now() >= deadline {
            anyhow::bail!("Timed out waiting for SilverBond health endpoint");
        }
        sleep(Duration::from_millis(50)).await;
    }
}

async fn check_health(local_addr: SocketAddr) -> anyhow::Result<bool> {
    let mut stream = match TcpStream::connect(local_addr).await {
        Ok(stream) => stream,
        Err(error) => {
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::NotConnected
            ) {
                return Ok(false);
            }
            return Err(error.into());
        }
    };
    stream
        .write_all(
            b"GET /api/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        )
        .await?;
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await?;
    Ok(buffer.starts_with(b"HTTP/1.1 200") || buffer.starts_with(b"HTTP/1.0 200"))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::app::{AppPaths, ApplicationConfig};

    #[tokio::test]
    async fn starts_host_on_ephemeral_port_and_serves_health() {
        let temp = TempDir::new().unwrap();
        let host = ApplicationHost::start(HostConfig {
            application: ApplicationConfig {
                paths: AppPaths::from_root(temp.path()),
                seed_bundled_templates: true,
            },
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        })
        .await
        .unwrap();

        assert_ne!(host.local_addr().port(), 0);
        host.wait_for_health(Duration::from_secs(2)).await.unwrap();
        assert!(host.paths().templates_dir.exists());

        host.shutdown().await.unwrap();
    }
}
