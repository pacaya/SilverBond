use std::{collections::HashMap, path::PathBuf, sync::Arc};

use axum::{Router, routing::get};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, broadcast};

use crate::{
    api, frontend,
    runtime::RuntimeContext,
    storage::{Database, TemplateStore, WorkflowStore, seed_bundled_templates},
    util::ensure_dir,
};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub workflows_dir: PathBuf,
    pub templates_dir: PathBuf,
    pub database_path: PathBuf,
}

impl AppPaths {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            workflows_dir: root.join("workflows"),
            templates_dir: root.join("templates"),
            database_path: root.join(".silverbond").join("silverbond.sqlite"),
            root,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApplicationConfig {
    pub paths: AppPaths,
    pub seed_bundled_templates: bool,
    pub security: SecurityConfig,
}

impl ApplicationConfig {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self {
            paths: AppPaths::from_root(root),
            seed_bundled_templates: false,
            security: SecurityConfig::default(),
        }
    }

    pub fn from_cli_environment() -> anyhow::Result<Self> {
        let root = std::env::var_os("SILVERBOND_ROOT")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?);
        let mut config = Self::from_root(root);
        config.security = SecurityConfig::from_environment();
        Ok(config)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecurityConfig {
    pub agent_user: Option<String>,
    pub unlock_password_hash: Option<String>,
}

impl SecurityConfig {
    pub fn from_environment() -> Self {
        Self {
            agent_user: env_config_value("SILVERBOND_AGENT_USER"),
            unlock_password_hash: env_config_value("SILVERBOND_UNLOCK_PASSWORD_HASH"),
        }
    }

    pub fn verify_unlock_secret(&self, secret: Option<&str>) -> bool {
        let (Some(secret), Some(hash)) = (secret, self.unlock_password_hash.as_deref()) else {
            return false;
        };
        verify_sha256_unlock_hash(secret, hash)
    }
}

pub fn sha256_unlock_password_hash(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    format!("sha256:{}", hex_lower(&digest))
}

fn env_config_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn verify_sha256_unlock_hash(secret: &str, hash: &str) -> bool {
    let Some(expected_hex) = hash.strip_prefix("sha256:") else {
        return false;
    };
    if expected_hex.len() != 64 || !expected_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    constant_time_eq(
        sha256_unlock_password_hash(secret).as_bytes(),
        format!("sha256:{}", expected_hex.to_ascii_lowercase()).as_bytes(),
    )
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut diff = left.len() ^ right.len();
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= usize::from(*a ^ *b);
    }
    diff == 0
}

#[derive(Clone)]
pub struct AppState {
    pub paths: AppPaths,
    pub workflows: WorkflowStore,
    pub templates: TemplateStore,
    pub runtime: RuntimeContext,
    pub pane_streams: PaneStreamRegistry,
    pub security: SecurityConfig,
}

#[derive(Clone)]
pub struct PaneStreamRegistry {
    pub(crate) inner: Arc<Mutex<HashMap<String, PaneStreamEntry>>>,
    pub(crate) max_subscribers_per_pane: usize,
}

#[derive(Clone)]
pub(crate) struct PaneStreamEntry {
    pub(crate) sender: broadcast::Sender<Vec<u8>>,
    pub(crate) refcount: usize,
}

impl Default for PaneStreamRegistry {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            max_subscribers_per_pane: 32,
        }
    }
}

impl PaneStreamRegistry {
    pub(crate) async fn unsubscribe(&self, target: &str) {
        let mut inner = self.inner.lock().await;
        let Some(entry) = inner.get_mut(target) else {
            return;
        };
        if entry.refcount <= 1 {
            inner.remove(target);
        } else {
            entry.refcount -= 1;
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn remove_if_sender(&self, target: &str, sender: &broadcast::Sender<Vec<u8>>) {
        let mut inner = self.inner.lock().await;
        let should_remove = inner
            .get(target)
            .map(|entry| entry.refcount == 0 && entry.sender.same_channel(sender))
            .unwrap_or(false);
        if should_remove {
            inner.remove(target);
        }
    }

    pub(crate) async fn remove_terminal_sender(
        &self,
        target: &str,
        sender: &broadcast::Sender<Vec<u8>>,
    ) {
        let mut inner = self.inner.lock().await;
        let should_remove = inner
            .get(target)
            .map(|entry| entry.sender.same_channel(sender))
            .unwrap_or(false);
        if should_remove {
            inner.remove(target);
        }
    }
}

pub struct Application {
    state: AppState,
    pub paths: AppPaths,
}

impl Application {
    pub async fn boot(config: ApplicationConfig) -> anyhow::Result<Self> {
        let paths = config.paths;
        ensure_dir(&paths.workflows_dir)?;
        ensure_dir(&paths.templates_dir)?;
        ensure_dir(
            paths
                .database_path
                .parent()
                .expect("database parent exists"),
        )?;
        if config.seed_bundled_templates {
            seed_bundled_templates(&paths.templates_dir)?;
        }

        let workflows = WorkflowStore::new(paths.workflows_dir.clone());
        let templates = TemplateStore::new(paths.templates_dir.clone());
        let db = Database::new(paths.database_path.clone());
        db.init().await?;
        let runtime = RuntimeContext::new(db);
        crate::runtime::reap_stale_tmux_sessions(&runtime).await?;

        let state = AppState {
            paths: paths.clone(),
            workflows,
            templates,
            runtime,
            pane_streams: PaneStreamRegistry::default(),
            security: config.security,
        };

        Ok(Self { state, paths })
    }

    pub fn router(&self) -> Router {
        api::router(self.state.clone())
            .route(
                "/",
                get(|| async { frontend::serve("/".to_string()).await }),
            )
            .route(
                "/{*path}",
                get(
                    |axum::extract::Path(path): axum::extract::Path<String>| async move {
                        frontend::serve(path).await
                    },
                ),
            )
    }
}
