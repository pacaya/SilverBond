//! PTY session manager — spawns and manages interactive agent CLI sessions.
//!
//! Each [`ManagedSession`] wraps a pseudo-terminal running an agent CLI (claude,
//! codex, gemini) in interactive mode using `expectrl`. Prompts are sent via
//! PTY stdin, and responses are captured using sentinel-based completion detection.

use std::{
    collections::HashMap,
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::driver::InteractionKind;
use crate::pty_output::{ParsedResponse, strip_ansi};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Role in a session conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Agent,
}

/// A single entry in the session conversation log.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub role: Role,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    #[serde(skip)]
    pub raw_bytes: Vec<u8>,
}

/// Runtime state of a managed session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Processing,
    WaitingInteraction,
    Completed,
    Error,
}

/// Summary info returned by `list_sessions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub agent_name: String,
    pub state: SessionState,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub entry_count: usize,
}

// ---------------------------------------------------------------------------
// Internal session wrapper
// ---------------------------------------------------------------------------

struct ManagedSession {
    id: String,
    agent_name: String,
    session: expectrl::Session,
    state: SessionState,
    output_history: Vec<SessionEntry>,
    created_at: DateTime<Utc>,
    last_activity: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Session Manager
// ---------------------------------------------------------------------------

/// Manages the lifecycle of PTY-backed agent sessions.
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, ManagedSession>>>,
    pub artifact_base_dir: PathBuf,
}

impl SessionManager {
    /// Create a new session manager with the given base directory for artifacts.
    pub fn new(artifact_base_dir: PathBuf) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            artifact_base_dir,
        }
    }

    /// Spawn a new interactive PTY session for the given agent CLI.
    ///
    /// Returns the session ID (a UUID).
    pub async fn create_session(
        &self,
        agent_name: &str,
        executable: &str,
        args: Vec<String>,
        env: Vec<(String, String)>,
        cwd: &str,
    ) -> anyhow::Result<String> {
        let session_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        let mut cmd = Command::new(executable);
        cmd.args(&args);
        if !cwd.is_empty() {
            cmd.current_dir(cwd);
        }
        // Inherit HOME and PATH from the current process
        if let Ok(home) = std::env::var("HOME") {
            cmd.env("HOME", home);
        }
        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", path);
        }
        // Apply extra env vars from the driver
        for (key, value) in &env {
            cmd.env(key, value);
        }
        // Force color/interactive mode hints
        cmd.env("TERM", "xterm-256color");

        let expectrl_session = expectrl::Session::spawn(cmd)
            .context("Failed to spawn agent process via expectrl")?;

        let session = ManagedSession {
            id: session_id.clone(),
            agent_name: agent_name.to_string(),
            session: expectrl_session,
            state: SessionState::Idle,
            output_history: Vec::new(),
            created_at: now,
            last_activity: now,
        };

        self.sessions.lock().await.insert(session_id.clone(), session);
        Ok(session_id)
    }

    /// Send a prompt to an existing session and wait for the agent's response.
    ///
    /// The prompt should already be wrapped with sentinel instructions by the caller.
    /// The sentinel is a unique marker the agent prints when done responding.
    pub async fn send_prompt(
        &self,
        session_id: &str,
        prompt: &str,
        sentinel: &str,
        timeout_duration: Duration,
    ) -> anyhow::Result<ParsedResponse> {
        // Write the prompt — must be done under the lock briefly.
        {
            let mut sessions = self.sessions.lock().await;
            let session = sessions
                .get_mut(session_id)
                .context("Session not found")?;
            session.state = SessionState::Processing;
            session.last_activity = Utc::now();
            session.output_history.push(SessionEntry {
                role: Role::User,
                content: prompt.to_string(),
                timestamp: Utc::now(),
                raw_bytes: Vec::new(),
            });
            session
                .session
                .send_line(prompt)
                .context("Failed to write prompt to PTY")?;
        }

        // Read output in a blocking task (PTY read is blocking I/O)
        let sessions_ref = self.sessions.clone();
        let sid = session_id.to_string();
        let sentinel_owned = sentinel.to_string();

        let response = tokio::task::spawn_blocking(move || {
            let mut sessions_guard = sessions_ref.blocking_lock();
            let session = sessions_guard
                .get_mut(&sid)
                .context("Session disappeared during read")?;

            session.session.set_expect_timeout(Some(timeout_duration));

            // Use regex with whitespace tolerance for sentinel matching
            let sentinel_pattern = expectrl::Regex(
                format!(r"\s*{}\s*", regex::escape(&sentinel_owned))
            );

            let captures = session
                .session
                .expect(sentinel_pattern)
                .context("Failed to read response (timeout or sentinel not found)")?;

            // Everything before the sentinel match is the agent's response
            let before_bytes = captures.before().to_vec();
            let text = strip_ansi(&before_bytes);

            // Strip the echoed prompt from the beginning of the response
            let cleaned_text = text.trim().to_string();

            Ok::<ParsedResponse, anyhow::Error>(ParsedResponse {
                text: cleaned_text,
                raw: before_bytes,
            })
        })
        .await
        .context("PTY read task panicked")??;

        // Record the response in history
        {
            let mut sessions = self.sessions.lock().await;
            if let Some(session) = sessions.get_mut(session_id) {
                session.state = SessionState::Idle;
                session.last_activity = Utc::now();
                session.output_history.push(SessionEntry {
                    role: Role::Agent,
                    content: response.text.clone(),
                    timestamp: Utc::now(),
                    raw_bytes: response.raw.clone(),
                });
            }
        }

        Ok(response)
    }

    /// Send a slash command (e.g. `/cost`) and capture the output.
    ///
    /// Slash commands produce short, immediate responses — uses a brief
    /// fixed timeout and reads whatever is available.
    pub async fn send_command(
        &self,
        session_id: &str,
        command: &str,
    ) -> anyhow::Result<String> {
        let sessions_ref = self.sessions.clone();
        let sid = session_id.to_string();
        let cmd = command.to_string();

        let output = tokio::task::spawn_blocking(move || {
            let mut sessions_guard = sessions_ref.blocking_lock();
            let session = sessions_guard
                .get_mut(&sid)
                .context("Session not found")?;

            session.session.send_line(&cmd)
                .context("Failed to send command to PTY")?;

            // Brief sleep to let the command output arrive
            std::thread::sleep(Duration::from_millis(500));

            // Read whatever is available in the buffer
            let mut buf = vec![0u8; 8192];
            let mut collected = Vec::new();

            loop {
                match session.session.try_read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => collected.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }

            Ok::<String, anyhow::Error>(strip_ansi(&collected))
        })
        .await
        .context("Command read task panicked")??;

        Ok(output)
    }

    /// Return the conversation history for a session (read-only).
    pub async fn get_history(&self, session_id: &str) -> anyhow::Result<Vec<SessionEntry>> {
        let sessions = self.sessions.lock().await;
        let session = sessions.get(session_id).context("Session not found")?;
        Ok(session.output_history.clone())
    }

    /// List all active sessions with summary info.
    pub async fn list_sessions(&self) -> Vec<SessionSummary> {
        let sessions = self.sessions.lock().await;
        sessions
            .values()
            .map(|s| SessionSummary {
                id: s.id.clone(),
                agent_name: s.agent_name.clone(),
                state: s.state,
                created_at: s.created_at,
                last_activity: s.last_activity,
                entry_count: s.output_history.len(),
            })
            .collect()
    }

    /// Check if a session's agent process is still running.
    pub async fn is_alive(&self, session_id: &str) -> bool {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.session.is_alive().unwrap_or(false)
        } else {
            false
        }
    }

    /// Close and clean up a single session.
    pub async fn close_session(&self, session_id: &str) -> anyhow::Result<()> {
        let mut sessions = self.sessions.lock().await;
        if let Some(mut session) = sessions.remove(session_id) {
            // Try to send exit command gracefully
            let _ = session.session.send_line("/exit");
            // Give the process a moment, then drop (which cleans up the PTY)
            std::thread::sleep(Duration::from_millis(200));
            session.state = SessionState::Completed;
        }
        Ok(())
    }

    /// Close all active sessions. Called on workflow completion.
    pub async fn close_all(&self) -> anyhow::Result<()> {
        let mut sessions = self.sessions.lock().await;
        for (_, mut session) in sessions.drain() {
            let _ = session.session.send_line("/exit");
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Interactive prompt handling
// ---------------------------------------------------------------------------

/// Events produced by `send_prompt_interactive` during the sentinel wait loop.
#[derive(Debug)]
pub enum PromptEvent {
    /// Sentinel was found — the agent completed its response.
    Completed(ParsedResponse),
    /// An interactive prompt was detected that needs handling.
    InteractionRequired {
        kind: InteractionKind,
        description: String,
        output_so_far: String,
    },
    /// No new output for the intermediate timeout while the process is alive.
    StaleDetected {
        output_so_far: String,
    },
    /// The total timeout was exceeded.
    Timeout {
        output_so_far: String,
    },
}

impl SessionManager {
    /// Run a warmup phase after session creation, auto-responding to known prompts.
    ///
    /// This handles trust-folder prompts, welcome screens, and other startup
    /// interactions before the first real prompt is sent.
    pub async fn warmup_session(
        &self,
        session_id: &str,
        auto_patterns: Vec<(regex::Regex, String)>,
        warmup_timeout: Duration,
    ) -> anyhow::Result<()> {
        let sessions_ref = self.sessions.clone();
        let sid = session_id.to_string();

        tokio::task::spawn_blocking(move || {
            let mut sessions_guard = sessions_ref.blocking_lock();
            let session = sessions_guard
                .get_mut(&sid)
                .context("Session not found for warmup")?;

            let deadline = std::time::Instant::now() + warmup_timeout;
            let chunk_timeout = Duration::from_millis(500);

            while std::time::Instant::now() < deadline {
                session.session.set_expect_timeout(Some(chunk_timeout));

                // Try to read whatever is in the buffer
                let mut buf = vec![0u8; 8192];
                let mut collected = Vec::new();

                loop {
                    match session.session.try_read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => collected.extend_from_slice(&buf[..n]),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    }
                }

                if collected.is_empty() {
                    // Brief sleep before retrying
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }

                let text = strip_ansi(&collected);

                // Check against auto-respond patterns
                let mut matched = false;
                for (pattern, response) in &auto_patterns {
                    if pattern.is_match(&text) {
                        let _ = session.session.send_line(response);
                        session.last_activity = Utc::now();
                        matched = true;
                        // Brief wait for the response to be processed
                        std::thread::sleep(Duration::from_millis(300));
                        break;
                    }
                }

                if !matched {
                    // No pattern matched — continue reading to drain buffer
                    continue;
                }
            }

            Ok::<(), anyhow::Error>(())
        })
        .await
        .context("Warmup task panicked")??;

        Ok(())
    }

    /// Send a prompt and wait for the sentinel, watching for interactive prompts.
    ///
    /// Unlike `send_prompt`, this method returns `PromptEvent` variants that
    /// indicate whether the agent completed, needs interaction, or went stale.
    pub async fn send_prompt_interactive(
        &self,
        session_id: &str,
        prompt: &str,
        sentinel: &str,
        interaction_patterns: Vec<(regex::Regex, InteractionKind, String)>,
        intermediate_timeout: Duration,
        total_timeout: Duration,
    ) -> anyhow::Result<PromptEvent> {
        // Send the prompt
        {
            let mut sessions = self.sessions.lock().await;
            let session = sessions
                .get_mut(session_id)
                .context("Session not found")?;
            session.state = SessionState::Processing;
            session.last_activity = Utc::now();
            session.output_history.push(SessionEntry {
                role: Role::User,
                content: prompt.to_string(),
                timestamp: Utc::now(),
                raw_bytes: Vec::new(),
            });
            session
                .session
                .send_line(prompt)
                .context("Failed to write prompt to PTY")?;
        }

        let sessions_ref = self.sessions.clone();
        let sid = session_id.to_string();
        let sentinel_owned = sentinel.to_string();

        let event = tokio::task::spawn_blocking(move || -> anyhow::Result<PromptEvent> {
            let mut sessions_guard = sessions_ref.blocking_lock();
            let session = sessions_guard
                .get_mut(&sid)
                .context("Session disappeared during interactive read")?;

            let total_deadline = std::time::Instant::now() + total_timeout;
            let sentinel_regex = regex::Regex::new(
                &format!(r"\s*{}\s*", regex::escape(&sentinel_owned))
            ).context("Failed to compile sentinel regex")?;

            let mut accumulated_output = Vec::new();
            let mut accumulated_text = String::new();

            loop {
                let remaining = total_deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Ok(PromptEvent::Timeout {
                        output_so_far: accumulated_text,
                    });
                }

                let this_timeout = intermediate_timeout.min(remaining);
                session.session.set_expect_timeout(Some(this_timeout));

                // Try reading with the sentinel pattern
                let sentinel_pattern = expectrl::Regex(
                    format!(r"\s*{}\s*", regex::escape(&sentinel_owned))
                );

                match session.session.expect(sentinel_pattern) {
                    Ok(captures) => {
                        let before_bytes = captures.before().to_vec();
                        let text = strip_ansi(&before_bytes);
                        accumulated_output.extend_from_slice(&before_bytes);
                        accumulated_text.push_str(&text);

                        session.state = SessionState::Idle;
                        session.last_activity = Utc::now();
                        session.output_history.push(SessionEntry {
                            role: Role::Agent,
                            content: accumulated_text.trim().to_string(),
                            timestamp: Utc::now(),
                            raw_bytes: accumulated_output.clone(),
                        });

                        return Ok(PromptEvent::Completed(ParsedResponse {
                            text: accumulated_text.trim().to_string(),
                            raw: accumulated_output,
                        }));
                    }
                    Err(_) => {
                        // Timeout on sentinel — read whatever is in the buffer
                        let mut buf = vec![0u8; 16384];
                        let mut new_output = Vec::new();
                        loop {
                            match session.session.try_read(&mut buf) {
                                Ok(0) => break,
                                Ok(n) => new_output.extend_from_slice(&buf[..n]),
                                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                                Err(_) => break,
                            }
                        }

                        if new_output.is_empty() {
                            // No new output — check if stale
                            if !accumulated_text.is_empty() || !accumulated_output.is_empty() {
                                // Check for interaction patterns in accumulated text
                                for (pattern, kind, desc) in &interaction_patterns {
                                    if pattern.is_match(&accumulated_text) {
                                        session.state = SessionState::WaitingInteraction;
                                        return Ok(PromptEvent::InteractionRequired {
                                            kind: kind.clone(),
                                            description: desc.clone(),
                                            output_so_far: accumulated_text,
                                        });
                                    }
                                }
                            }
                            return Ok(PromptEvent::StaleDetected {
                                output_so_far: accumulated_text,
                            });
                        }

                        let new_text = strip_ansi(&new_output);
                        accumulated_output.extend_from_slice(&new_output);
                        accumulated_text.push_str(&new_text);

                        // Check sentinel in accumulated text
                        if sentinel_regex.is_match(&accumulated_text) {
                            let parts: Vec<&str> = accumulated_text
                                .splitn(2, &sentinel_owned)
                                .collect();
                            let response_text = parts[0].trim().to_string();

                            session.state = SessionState::Idle;
                            session.last_activity = Utc::now();
                            session.output_history.push(SessionEntry {
                                role: Role::Agent,
                                content: response_text.clone(),
                                timestamp: Utc::now(),
                                raw_bytes: accumulated_output.clone(),
                            });

                            return Ok(PromptEvent::Completed(ParsedResponse {
                                text: response_text,
                                raw: accumulated_output,
                            }));
                        }

                        // Check for interaction patterns in the new output
                        for (pattern, kind, desc) in &interaction_patterns {
                            if pattern.is_match(&new_text) || pattern.is_match(&accumulated_text) {
                                session.state = SessionState::WaitingInteraction;
                                return Ok(PromptEvent::InteractionRequired {
                                    kind: kind.clone(),
                                    description: desc.clone(),
                                    output_so_far: accumulated_text,
                                });
                            }
                        }

                        // Output is still flowing — continue the loop
                    }
                }
            }
        })
        .await
        .context("Interactive PTY read task panicked")??;

        Ok(event)
    }

    /// Send a response to an interactive prompt (e.g., "y" to approve).
    pub async fn respond_to_interaction(
        &self,
        session_id: &str,
        response: &str,
    ) -> anyhow::Result<()> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .context("Session not found")?;
        session
            .session
            .send_line(response)
            .context("Failed to send interaction response to PTY")?;
        session.state = SessionState::Processing;
        session.last_activity = Utc::now();
        session.output_history.push(SessionEntry {
            role: Role::User,
            content: format!("[interaction response] {}", response),
            timestamp: Utc::now(),
            raw_bytes: Vec::new(),
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_entry_serialization() {
        let entry = SessionEntry {
            role: Role::User,
            content: "Hello".to_string(),
            timestamp: Utc::now(),
            raw_bytes: vec![1, 2, 3],
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(r#""role":"user"#));
        assert!(json.contains(r#""content":"Hello"#));
        // raw_bytes should be skipped
        assert!(!json.contains("raw_bytes"));
    }

    #[test]
    fn session_state_serialization() {
        let json = serde_json::to_string(&SessionState::Idle).unwrap();
        assert_eq!(json, r#""idle""#);
        let json = serde_json::to_string(&SessionState::Processing).unwrap();
        assert_eq!(json, r#""processing""#);
    }

    #[test]
    fn session_summary_serialization() {
        let summary = SessionSummary {
            id: "test-id".to_string(),
            agent_name: "claude".to_string(),
            state: SessionState::Idle,
            created_at: Utc::now(),
            last_activity: Utc::now(),
            entry_count: 5,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains(r#""agentName":"claude"#));
        assert!(json.contains(r#""entryCount":5"#));
    }

    #[tokio::test]
    async fn session_manager_creation() {
        let sm = SessionManager::new(PathBuf::from("/tmp/test-artifacts"));
        assert_eq!(sm.artifact_base_dir, PathBuf::from("/tmp/test-artifacts"));
        assert!(sm.list_sessions().await.is_empty());
    }

    #[tokio::test]
    async fn create_and_close_session() {
        let sm = SessionManager::new(PathBuf::from("/tmp/test-artifacts"));

        // Spawn a simple shell session to verify lifecycle
        let session_id = sm
            .create_session(
                "test",
                "/bin/sh",
                vec![],
                vec![],
                "/tmp",
            )
            .await
            .unwrap();

        assert!(!session_id.is_empty());
        assert!(sm.is_alive(&session_id).await);

        let sessions = sm.list_sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].agent_name, "test");
        assert_eq!(sessions[0].state, SessionState::Idle);

        sm.close_session(&session_id).await.unwrap();
        assert!(sm.list_sessions().await.is_empty());
    }

    #[tokio::test]
    async fn close_all_sessions() {
        let sm = SessionManager::new(PathBuf::from("/tmp/test-artifacts"));

        let _s1 = sm
            .create_session("a", "/bin/sh", vec![], vec![], "/tmp")
            .await
            .unwrap();
        let _s2 = sm
            .create_session("b", "/bin/sh", vec![], vec![], "/tmp")
            .await
            .unwrap();

        assert_eq!(sm.list_sessions().await.len(), 2);
        sm.close_all().await.unwrap();
        assert!(sm.list_sessions().await.is_empty());
    }

    #[tokio::test]
    async fn get_history_empty() {
        let sm = SessionManager::new(PathBuf::from("/tmp/test-artifacts"));
        let session_id = sm
            .create_session("test", "/bin/sh", vec![], vec![], "/tmp")
            .await
            .unwrap();

        let history = sm.get_history(&session_id).await.unwrap();
        assert!(history.is_empty());

        sm.close_session(&session_id).await.unwrap();
    }

    #[tokio::test]
    async fn get_history_missing_session() {
        let sm = SessionManager::new(PathBuf::from("/tmp/test-artifacts"));
        let result = sm.get_history("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn is_alive_nonexistent() {
        let sm = SessionManager::new(PathBuf::from("/tmp/test-artifacts"));
        assert!(!sm.is_alive("nonexistent").await);
    }
}
