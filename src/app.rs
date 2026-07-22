use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{Router, routing::get};
use sha2::{Digest, Sha256};
use tmux_tools_core::TmuxInvocation;
use tokio::sync::{Mutex, broadcast, watch};
use tokio_util::sync::CancellationToken;

use crate::{
    api, frontend,
    runtime::RuntimeContext,
    storage::{Database, TemplateStore, WorkflowStore, seed_bundled_templates},
    util::{constant_time_eq, ensure_dir},
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

// Threat model: the unlock hash is loaded only into the server process environment/config;
// SecurityConfig is not serializable and is never logged, returned, or persisted. Contained-agent
// launches use `sudo -u <user> -H --` without `-E`, so sudo's env_reset does not forward the hash.
// Disclosure therefore already requires the server uid that unlocking grants; global failed-attempt
// throttling, rather than a KDF, is the control for guesses through the HTTP unlock path.
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

const INITIAL_UNLOCK_RETRY_DELAY: Duration = Duration::from_secs(5);
const MAX_UNLOCK_RETRY_DELAY: Duration = Duration::from_secs(30);

#[derive(Clone, Default)]
pub struct UnlockThrottle {
    inner: Arc<Mutex<UnlockThrottleState>>,
}

#[derive(Default)]
struct UnlockThrottleState {
    failed_attempts: u32,
    blocked_until: Option<Instant>,
}

impl UnlockThrottle {
    pub(crate) async fn verify_unlock_secret(
        &self,
        security: &SecurityConfig,
        secret: &str,
    ) -> Result<bool, Duration> {
        let mut state = self.inner.lock().await;
        let now = Instant::now();
        if let Some(blocked_until) = state.blocked_until
            && blocked_until > now
        {
            return Err(blocked_until.duration_since(now));
        }

        if security.verify_unlock_secret(Some(secret)) {
            *state = UnlockThrottleState::default();
            return Ok(true);
        }

        let multiplier = 1_u32 << state.failed_attempts.min(3);
        let retry_delay = INITIAL_UNLOCK_RETRY_DELAY
            .saturating_mul(multiplier)
            .min(MAX_UNLOCK_RETRY_DELAY);
        state.failed_attempts = state.failed_attempts.saturating_add(1);
        state.blocked_until = Some(now + retry_delay);
        Ok(false)
    }
}

#[derive(Clone)]
pub struct AppState {
    pub paths: AppPaths,
    pub workflows: WorkflowStore,
    pub templates: TemplateStore,
    pub runtime: RuntimeContext,
    pub pane_streams: PaneStreamRegistry,
    pub security: SecurityConfig,
    pub unlock_throttle: UnlockThrottle,
}

#[derive(Clone)]
pub struct PaneStreamRegistry {
    pub(crate) inner: Arc<Mutex<HashMap<PaneStreamKey, PaneStreamEntry>>>,
    pub(crate) max_subscribers_per_pane: usize,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PaneStreamKey {
    prefix: Vec<String>,
    socket: Option<String>,
    tmux_bin: String,
    pane_target: String,
}

impl PaneStreamKey {
    pub(crate) fn new(invocation: &TmuxInvocation, pane_target: &str) -> Self {
        Self {
            prefix: invocation.prefix.clone(),
            socket: invocation.socket.clone(),
            tmux_bin: invocation.tmux_bin.clone(),
            pane_target: pane_target.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneStreamLifecycle {
    Active,
    Draining,
    Terminating,
}

#[derive(Clone)]
pub(crate) struct PaneStreamEntry {
    pub(crate) sender: broadcast::Sender<Vec<u8>>,
    pub(crate) refcount: usize,
    lifecycle: PaneStreamLifecycle,
    drain_signal: watch::Sender<bool>,
    owner_done: CancellationToken,
}

impl PaneStreamEntry {
    pub(crate) fn new(sender: broadcast::Sender<Vec<u8>>, refcount: usize) -> Self {
        let draining = refcount == 0;
        let (drain_signal, _) = watch::channel(draining);
        Self {
            sender,
            refcount,
            lifecycle: if draining {
                PaneStreamLifecycle::Draining
            } else {
                PaneStreamLifecycle::Active
            },
            drain_signal,
            owner_done: CancellationToken::new(),
        }
    }

    pub(crate) fn is_draining(&self) -> bool {
        self.lifecycle == PaneStreamLifecycle::Draining
    }

    pub(crate) fn is_terminating(&self) -> bool {
        self.lifecycle == PaneStreamLifecycle::Terminating
    }

    pub(crate) fn mark_draining(&mut self) {
        if self.lifecycle == PaneStreamLifecycle::Terminating {
            return;
        }
        self.lifecycle = PaneStreamLifecycle::Draining;
        self.drain_signal.send_replace(true);
    }

    pub(crate) fn revive(&mut self) {
        self.lifecycle = PaneStreamLifecycle::Active;
        self.drain_signal.send_replace(false);
    }

    pub(crate) fn begin_termination(&mut self) {
        self.lifecycle = PaneStreamLifecycle::Terminating;
    }

    pub(crate) fn drain_receiver(&self) -> watch::Receiver<bool> {
        self.drain_signal.subscribe()
    }

    pub(crate) fn owner_done(&self) -> CancellationToken {
        self.owner_done.clone()
    }
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
    pub(crate) async fn unsubscribe(
        &self,
        key: &PaneStreamKey,
        sender: &broadcast::Sender<Vec<u8>>,
    ) {
        let mut inner = self.inner.lock().await;
        let Some(entry) = inner.get_mut(key) else {
            return;
        };
        if !entry.sender.same_channel(sender) {
            return;
        }
        if entry.refcount <= 1 {
            entry.refcount = 0;
            entry.mark_draining();
        } else {
            entry.refcount -= 1;
        }
    }

    pub(crate) async fn remove_terminal_sender(
        &self,
        key: &PaneStreamKey,
        sender: &broadcast::Sender<Vec<u8>>,
    ) {
        let mut inner = self.inner.lock().await;
        let should_remove = inner
            .get(key)
            .map(|entry| entry.sender.same_channel(sender))
            .unwrap_or(false);
        if should_remove {
            inner.remove(key);
        }
    }

    /// Removes a stuck terminating entry and signals waiters (bounded-wait timeout path).
    pub(crate) async fn force_clear_stuck_terminating_owner(&self, key: &PaneStreamKey) {
        let owner_done = {
            let mut inner = self.inner.lock().await;
            let Some(entry) = inner.get(key) else {
                return;
            };
            if !entry.is_terminating() {
                return;
            }
            let owner_done = entry.owner_done();
            inner.remove(key);
            owner_done
        };
        owner_done.cancel();
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
        crate::runtime::reap_stale_tmux_sessions(&runtime).await;

        let state = AppState {
            paths: paths.clone(),
            workflows,
            templates,
            runtime,
            pane_streams: PaneStreamRegistry::default(),
            security: config.security,
            unlock_throttle: UnlockThrottle::default(),
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

#[cfg(test)]
mod tests {
    use super::verify_sha256_unlock_hash;

    const PASSWORD_HASH: &str =
        "sha256:5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8";

    #[test]
    fn unlock_hash_rejects_missing_prefix() {
        assert!(!verify_sha256_unlock_hash(
            "password",
            "5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8"
        ));
    }

    #[test]
    fn unlock_hash_rejects_wrong_length_and_non_hex_digest() {
        assert!(!verify_sha256_unlock_hash("password", "sha256:1234"));
        assert!(!verify_sha256_unlock_hash(
            "password",
            "sha256:ge884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8"
        ));
    }

    #[test]
    fn unlock_hash_accepts_uppercase_hex_digest() {
        assert!(verify_sha256_unlock_hash(
            "password",
            "sha256:5E884898DA28047151D0E56F8DC6292773603D0D6AABBDD62A11EF721D1542D8"
        ));
    }

    #[test]
    fn unlock_hash_rejects_wrong_secret() {
        assert!(!verify_sha256_unlock_hash("wrong", PASSWORD_HASH));
    }
}
