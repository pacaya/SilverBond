use std::{
    collections::HashSet,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::sleep,
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow};
use futures::future::BoxFuture;
use regex::Regex;
use serde_json::{Value, json};
use tmux_tools_core::{
    TmuxInvocation,
    idle::{
        DEFAULT_READY_SCAN_LINES, DEFAULT_READY_STABLE_SECONDS, IdleConfig, IdleReason,
        wait_for_idle,
    },
    names, target, tmux,
};

use crate::{
    driver::{self, AccessMode, AgentConfig, InteractionKind, NodeOutcome},
    model::{
        CaptureConfig, KillConfig, NodeKind, RunAgentConfig, RunAsConfig, SendConfig, SpawnConfig,
        WaitConfig, WaitMode, WorkflowNode,
    },
    pty_output::strip_ansi,
    runtime::{
        AgentExecutionMetadata, NodeResult, NodeRunner, RuntimeContext, active_pane_key,
        register_tmux_session,
    },
    storage::Database,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct TmuxNodeRunner;

const IDLE_ESCALATE_SECS: f64 = 30.0;
const TMUX_LOOKUP_SENTINEL: &str = "SBTMUX:";
const TMUX_LOOKUP_COMMAND: &str = r#"print -r -- "SBTMUX:$(command -v tmux)""#;

impl TmuxNodeRunner {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct PaneCleanupTarget {
    pub(crate) pane_id: String,
    pub(crate) session_name: Option<String>,
}

impl PaneCleanupTarget {
    pub(crate) fn new(pane_id: String, session_name: Option<String>) -> Self {
        Self {
            pane_id,
            session_name,
        }
    }
}

pub fn build_tmux_invocation(run_as: &RunAsConfig, run_id: &str) -> TmuxInvocation {
    let prefix = if let Some(command) = &run_as.command {
        command.clone()
    } else if let Some(user) = &run_as.user {
        vec![
            "sudo".to_string(),
            "-u".to_string(),
            user.clone(),
            "-H".to_string(),
            "--".to_string(),
        ]
    } else {
        Vec::new()
    };

    let socket = run_as
        .socket
        .clone()
        .unwrap_or_else(|| format!("silverbond-{run_id}"));
    let tmux_bin = resolve_tmux_bin(&prefix);

    TmuxInvocation {
        prefix,
        socket: Some(socket),
        tmux_bin,
    }
}

fn resolve_tmux_bin(prefix: &[String]) -> String {
    resolve_tmux_bin_with_timeout(prefix, tmux::DEFAULT_TMUX_COMMAND_TIMEOUT)
}

fn resolve_tmux_bin_with_timeout(prefix: &[String], timeout: Duration) -> String {
    let mut command = if let Some((program, args)) = prefix.split_first() {
        let mut command = Command::new(program);
        command.args(args);
        command
    } else {
        Command::new("zsh")
    };

    if prefix.is_empty() {
        // Keep `-i`: zsh only sources .zshrc for interactive shells, and
        // operator tooling PATHs often live there. The sentinel parse below
        // keeps rc stdout banners from corrupting the resolved path.
        command.args(["-lic", TMUX_LOOKUP_COMMAND]);
    } else {
        // Keep `-i`: zsh only sources .zshrc for interactive shells, and
        // run_as tooling PATHs often live there. The sentinel parse below
        // keeps rc stdout banners from corrupting the resolved path.
        command.args(["zsh", "-lic", TMUX_LOOKUP_COMMAND]);
    }

    let Ok(output) = tmux::command_output_with_timeout(command, timeout) else {
        return "tmux".to_string();
    };
    if !output.status.success() {
        return "tmux".to_string();
    }

    parse_tmux_lookup_output(&output.stdout).unwrap_or_else(|| "tmux".to_string())
}

fn parse_tmux_lookup_output(stdout: &[u8]) -> Option<String> {
    let output = String::from_utf8_lossy(stdout);
    let mut matches = output.lines().filter_map(|line| {
        line.strip_prefix(TMUX_LOOKUP_SENTINEL)
            .map(str::trim)
            .filter(|resolved| !resolved.is_empty())
    });
    let resolved = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(resolved.to_string())
}

impl NodeRunner for TmuxNodeRunner {
    fn run(
        &self,
        agent: String,
        prompt: String,
        cwd: String,
        timeout_secs: Option<u64>,
        config: Option<AgentConfig>,
    ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                run_agent_interactive(
                    agent,
                    prompt,
                    cwd,
                    timeout_secs,
                    None,
                    config.as_ref(),
                    None,
                    None,
                    None,
                )
            })
            .await
            .context("tmux runner task panicked")?
        })
    }

    fn run_with_interaction(
        &self,
        agent: String,
        prompt: String,
        cwd: String,
        timeout_secs: Option<u64>,
        config: Option<AgentConfig>,
        ctx: RuntimeContext,
        run_id: String,
    ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
        Box::pin(async move {
            let inv = ctx.run_invocation.clone();
            let abort_flag = ctx
                .registry
                .abort_signal(&run_id)
                .await
                .map(|(abort_flag, _)| abort_flag);
            let interaction = InteractionEscalation::new(ctx, run_id, abort_flag);
            tokio::task::spawn_blocking(move || {
                if let Some(inv) = inv {
                    tmux_tools_core::with_invocation(inv, || {
                        run_agent_interactive(
                            agent,
                            prompt,
                            cwd,
                            timeout_secs,
                            None,
                            config.as_ref(),
                            None,
                            None,
                            Some(&interaction),
                        )
                    })
                } else {
                    run_agent_interactive(
                        agent,
                        prompt,
                        cwd,
                        timeout_secs,
                        None,
                        config.as_ref(),
                        None,
                        None,
                        Some(&interaction),
                    )
                }
            })
            .await
            .context("tmux runner task panicked")?
        })
    }

    fn run_node_with_interaction(
        &self,
        node: WorkflowNode,
        agent: String,
        prompt: String,
        cwd: String,
        timeout_secs: Option<u64>,
        config: Option<AgentConfig>,
        previous_output: String,
        ctx: RuntimeContext,
        run_id: String,
        cursor_id: String,
    ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
        Box::pin(async move {
            let inv = ctx.run_invocation.clone();
            let abort_flag = ctx
                .registry
                .abort_signal(&run_id)
                .await
                .map(|(abort_flag, _)| abort_flag);
            let active_pane =
                ActivePaneRegistration::new(&ctx, run_id.clone(), cursor_id, node.id.clone());
            let interaction = InteractionEscalation::new(ctx, run_id, abort_flag);
            tokio::task::spawn_blocking(move || {
                if let Some(inv) = inv {
                    tmux_tools_core::with_invocation(inv, || {
                        run_tmux_node(
                            node,
                            agent,
                            prompt,
                            cwd,
                            timeout_secs,
                            config.as_ref(),
                            previous_output,
                            Some(active_pane),
                            Some(interaction),
                        )
                    })
                } else {
                    run_tmux_node(
                        node,
                        agent,
                        prompt,
                        cwd,
                        timeout_secs,
                        config.as_ref(),
                        previous_output,
                        Some(active_pane),
                        Some(interaction),
                    )
                }
            })
            .await
            .context("tmux node runner task panicked")?
        })
    }
}

fn run_tmux_node(
    node: WorkflowNode,
    agent: String,
    prompt: String,
    cwd: String,
    timeout_secs: Option<u64>,
    config: Option<&AgentConfig>,
    previous_output: String,
    active_pane: Option<ActivePaneRegistration>,
    interaction: Option<InteractionEscalation>,
) -> anyhow::Result<NodeResult> {
    let continue_session_from = node.continue_session_from.clone();
    match &node.kind {
        NodeKind::Task { .. } => run_agent_interactive(
            agent,
            prompt,
            cwd,
            timeout_secs,
            None,
            config,
            continue_session_from.as_deref(),
            active_pane.as_ref(),
            interaction.as_ref(),
        ),
        NodeKind::RunAgent {
            run_agent_config, ..
        } => run_agent_interactive(
            agent,
            prompt,
            cwd,
            timeout_secs,
            Some(run_agent_config),
            config,
            continue_session_from.as_deref(),
            active_pane.as_ref(),
            interaction.as_ref(),
        ),
        NodeKind::Spawn { spawn_config } => {
            execute_spawn(&node, spawn_config, &agent, &cwd, active_pane.as_ref())
        }
        NodeKind::Send { send_config } => execute_send(
            &node,
            send_config,
            &prompt,
            &previous_output,
            active_pane.as_ref(),
        ),
        NodeKind::Wait { wait_config } => execute_wait(
            &node,
            wait_config,
            timeout_secs,
            &previous_output,
            active_pane.as_ref(),
        ),
        NodeKind::Capture { capture_config } => execute_capture(
            &node,
            capture_config,
            &previous_output,
            active_pane.as_ref(),
        ),
        NodeKind::Kill { kill_config } => {
            execute_kill(&node, kill_config, &previous_output, active_pane.as_ref())
        }
        NodeKind::Approval
        | NodeKind::Split
        | NodeKind::Collector
        | NodeKind::Decide { .. }
        | NodeKind::ParallelBatch { .. }
        | NodeKind::Subflow { .. }
        | NodeKind::Call { .. } => Err(anyhow!(
            "tmux runner cannot execute {} nodes directly",
            node.node_type_str()
        )),
    }
}

#[derive(Debug, Clone)]
struct SpawnedPane {
    pane_id: String,
    session_name: String,
    agent: Option<String>,
    command: String,
}

struct SessionGuard {
    session_name: Option<String>,
}

impl SessionGuard {
    fn new(session_name: String) -> Self {
        Self {
            session_name: Some(session_name),
        }
    }

    fn disarm(&mut self) {
        self.session_name = None;
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if let Some(ref session) = self.session_name {
            let _ = tmux::run(&["kill-session", "-t", session]);
        }
    }
}

/// Returns true when called from within a Tokio async task (not a `spawn_blocking` thread).
fn in_current_task() -> bool {
    tokio::task::try_id().is_some()
}

/// Registers active tmux panes for a workflow run.
///
/// # Invariant
/// Only ever entered from a `tokio::task::spawn_blocking` blocking-pool thread where
/// `Handle::block_on` is safe. `Handle::current()` is captured on the async runtime
/// thread in `new()`, then `block_on` bridges back to async registry/DB calls from the
/// blocking pool.
#[derive(Clone)]
struct ActivePaneRegistration {
    registry: crate::runtime::RunRegistry,
    db: Database,
    run_id: String,
    cursor_id: String,
    key: String,
    handle: tokio::runtime::Handle,
}

impl ActivePaneRegistration {
    fn new(ctx: &RuntimeContext, run_id: String, cursor_id: String, node_id: String) -> Self {
        let key = active_pane_key(&cursor_id, &node_id);
        Self {
            registry: ctx.registry.clone(),
            db: ctx.db.clone(),
            run_id,
            cursor_id,
            key,
            handle: tokio::runtime::Handle::current(),
        }
    }

    fn set(&self, target: &str) {
        self.set_with_session(target, None);
    }

    fn set_with_session(&self, target: &str, session_name: Option<&str>) {
        let registry = self.registry.clone();
        let db = self.db.clone();
        let run_id = self.run_id.clone();
        let key = self.key.clone();
        let target = target.to_string();
        let session_name = session_name.map(str::to_string);
        debug_assert!(
            !in_current_task(),
            "Handle::block_on may only be called from spawn_blocking thread"
        );
        self.handle.block_on(async move {
            registry
                .set_active_pane_with_session(&run_id, &key, &target, session_name.clone())
                .await;
            if let Some(session_name) = session_name {
                let _ = register_tmux_session(&db, &run_id, &session_name).await;
            }
        });
    }

    fn clear_key(&self) {
        let registry = self.registry.clone();
        let run_id = self.run_id.clone();
        let key = self.key.clone();
        debug_assert!(
            !in_current_task(),
            "Handle::block_on may only be called from spawn_blocking thread"
        );
        self.handle.block_on(async move {
            registry.clear_active_pane(&run_id, &key).await;
        });
    }

    fn clear_target(&self, target: &str) {
        let registry = self.registry.clone();
        let run_id = self.run_id.clone();
        let target = target.to_string();
        debug_assert!(
            !in_current_task(),
            "Handle::block_on may only be called from spawn_blocking thread"
        );
        self.handle.block_on(async move {
            registry.clear_active_pane_target(&run_id, &target).await;
        });
    }

    fn resolve(&self, key: &str) -> Option<String> {
        let registry = self.registry.clone();
        let run_id = self.run_id.clone();
        let same_cursor_key = active_pane_key(&self.cursor_id, key);
        let fallback_key = key.to_string();
        debug_assert!(
            !in_current_task(),
            "Handle::block_on may only be called from spawn_blocking thread"
        );
        self.handle.block_on(async move {
            if let Some(target) = registry
                .resolve_active_pane(&run_id, &same_cursor_key)
                .await
            {
                Some(target)
            } else {
                registry.resolve_active_pane(&run_id, &fallback_key).await
            }
        })
    }
}

/// Escalates agent interactions back to the async runtime.
///
/// # Invariant
/// Only ever entered from a `tokio::task::spawn_blocking` blocking-pool thread where
/// `Handle::block_on` is safe.
#[derive(Clone)]
pub(crate) struct InteractionEscalation {
    ctx: RuntimeContext,
    run_id: String,
    handle: tokio::runtime::Handle,
    abort_flag: Option<Arc<AtomicBool>>,
}

impl InteractionEscalation {
    pub(crate) fn new(ctx: RuntimeContext, run_id: String, abort_flag: Option<Arc<AtomicBool>>) -> Self {
        Self {
            ctx,
            run_id,
            handle: tokio::runtime::Handle::current(),
            abort_flag,
        }
    }

    fn abort_flag(&self) -> Option<&AtomicBool> {
        self.abort_flag.as_deref()
    }

    fn is_aborted(&self) -> bool {
        abort_requested(self.abort_flag())
    }

    fn request(
        &self,
        session_id: &str,
        interaction_type: &str,
        description: &str,
        output_so_far: &str,
    ) -> anyhow::Result<String> {
        let ctx = self.ctx.clone();
        let run_id = self.run_id.clone();
        let session_id = session_id.to_string();
        let interaction_type = interaction_type.to_string();
        let description = description.to_string();
        let output_so_far = output_so_far.to_string();
        let abort_flag = self.abort_flag.clone();
        debug_assert!(
            !in_current_task(),
            "Handle::block_on may only be called from spawn_blocking thread"
        );
        self.handle.block_on(async move {
            if abort_requested(abort_flag.as_deref()) {
                anyhow::bail!("Interaction aborted");
            }
            crate::runtime::escalate_agent_interaction(
                &ctx,
                &run_id,
                &session_id,
                &interaction_type,
                &description,
                &output_so_far,
            )
            .await
        })
    }
}

fn execute_spawn(
    node: &WorkflowNode,
    cfg: &SpawnConfig,
    default_agent: &str,
    default_cwd: &str,
    active_pane: Option<&ActivePaneRegistration>,
) -> anyhow::Result<NodeResult> {
    let start = Instant::now();
    let has_command = cfg
        .command
        .as_deref()
        .is_some_and(|command| !command.trim().is_empty());
    let agent = cfg
        .agent
        .clone()
        .or_else(|| node.agent.clone())
        .or_else(|| (!has_command).then(|| default_agent.to_owned()));
    let cwd = cfg
        .cwd
        .as_deref()
        .or(node.cwd.as_deref())
        .unwrap_or(default_cwd);
    let spawned = spawn_pane(Some(cfg), agent.as_deref(), cwd, None, None, None)?;
    if let Some(active_pane) = active_pane {
        active_pane.set_with_session(&spawned.pane_id, Some(&spawned.session_name));
    }
    let parsed = json!({
        "paneId": spawned.pane_id,
        "sessionName": spawned.session_name,
        "agent": spawned.agent,
        "command": spawned.command,
    });
    Ok(json_result(
        true,
        parsed,
        "tmux",
        node.prompt.clone(),
        start.elapsed(),
    ))
}

fn execute_send(
    node: &WorkflowNode,
    cfg: &SendConfig,
    resolved_prompt: &str,
    previous_output: &str,
    active_pane: Option<&ActivePaneRegistration>,
) -> anyhow::Result<NodeResult> {
    let start = Instant::now();
    let pane = resolve_pane_target(cfg.target.as_deref(), previous_output)?;
    if let Some(active_pane) = active_pane {
        active_pane.set(&pane);
    }
    let text = if !resolved_prompt.is_empty() {
        resolved_prompt
    } else if !cfg.text.is_empty() {
        cfg.text.as_str()
    } else {
        node.prompt.as_str()
    };
    send_text(&pane, text, cfg.enter)?;
    let parsed = json!({
        "paneId": pane,
        "sent": text,
        "enter": cfg.enter,
    });
    Ok(json_result(
        true,
        parsed,
        "tmux",
        text.to_owned(),
        start.elapsed(),
    ))
}

fn execute_wait(
    node: &WorkflowNode,
    cfg: &WaitConfig,
    timeout_secs: Option<u64>,
    previous_output: &str,
    active_pane: Option<&ActivePaneRegistration>,
) -> anyhow::Result<NodeResult> {
    let start = Instant::now();
    let pane = resolve_pane_target(cfg.target.as_deref(), previous_output)?;
    if let Some(active_pane) = active_pane {
        active_pane.set(&pane);
    }
    let outcome = wait_for_mode(&pane, &cfg, timeout_secs)?;
    let success = outcome.reason != IdleReason::TimedOut;
    let parsed = json!({
        "paneId": pane,
        "reason": outcome.reason.as_str(),
        "durationMs": outcome.duration.as_millis(),
        "finalCapture": outcome.final_capture,
    });
    let mut result = json_result(
        success,
        parsed,
        "tmux",
        node.prompt.clone(),
        start.elapsed(),
    );
    if !success {
        result.stderr = "Timed out waiting for tmux pane".to_owned();
        result.exit_code = -2;
        result.metadata.outcome = Some(NodeOutcome::ErrorTimeout);
        result.metadata.error_type = Some("timeout".to_owned());
    }
    Ok(result)
}

fn execute_capture(
    node: &WorkflowNode,
    cfg: &CaptureConfig,
    previous_output: &str,
    active_pane: Option<&ActivePaneRegistration>,
) -> anyhow::Result<NodeResult> {
    let start = Instant::now();
    let pane = resolve_pane_target(cfg.target.as_deref(), previous_output)?;
    if let Some(active_pane) = active_pane {
        active_pane.set(&pane);
    }
    let output = capture_pane(&pane, &cfg)?;
    let mut result = NodeResult {
        success: true,
        output: output.clone(),
        stderr: String::new(),
        exit_code: 0,
        duration: duration_string(start.elapsed()),
        agent: "tmux".to_owned(),
        prompt: node.prompt.clone(),
        raw_output: Some(output),
        parsed_output: Some(json!({
            "paneId": pane,
            "lines": cfg.lines,
            "all": cfg.all,
            "ansi": cfg.ansi,
        })),
        metadata: AgentExecutionMetadata {
            outcome: Some(NodeOutcome::Success),
            ..Default::default()
        },
        ..Default::default()
    };
    result.resolved_prompt = Some(node.prompt.clone());
    Ok(result)
}

fn execute_kill(
    node: &WorkflowNode,
    cfg: &KillConfig,
    previous_output: &str,
    active_pane: Option<&ActivePaneRegistration>,
) -> anyhow::Result<NodeResult> {
    let start = Instant::now();
    let previous = parse_previous(previous_output);
    let session = cfg
        .session_name
        .as_deref()
        .or_else(|| previous_value(&previous, &["sessionName", "session_name"]));
    let target = cfg
        .target
        .as_deref()
        .or_else(|| previous_value(&previous, &["paneId", "pane_id", "target"]));

    if let Some(session) = session.filter(|s| !s.trim().is_empty()) {
        let session = stable_session_name(session);
        tmux::run_checked(&["kill-session", "-t", &session])?;
        if let Some(active_pane) = active_pane {
            active_pane.clear_key();
        }
        let parsed = json!({ "sessionName": session, "killed": true });
        return Ok(json_result(
            true,
            parsed,
            "tmux",
            node.prompt.clone(),
            start.elapsed(),
        ));
    }

    let pane = resolve_pane_target(target, previous_output)?;
    tmux::run_checked(&["kill-pane", "-t", &pane])?;
    if let Some(active_pane) = active_pane {
        active_pane.clear_target(&pane);
        active_pane.clear_key();
    }
    let parsed = json!({ "paneId": pane, "killed": true });
    Ok(json_result(
        true,
        parsed,
        "tmux",
        node.prompt.clone(),
        start.elapsed(),
    ))
}

struct CompiledInteractionPattern {
    regex: Regex,
    kind: InteractionKind,
    description: String,
    send_enter: bool,
}

struct InteractiveCapture {
    output: String,
    final_capture: String,
}

enum InteractiveReadyResult {
    Ready(IdleReason),
    Aborted,
}

enum InteractivePollResult {
    Completed(InteractiveCapture),
    TimedOut(InteractiveCapture),
    Aborted(InteractiveCapture),
}

struct PromptCaptureMarkers {
    prompt_end: String,
    response_end: String,
}

impl PromptCaptureMarkers {
    fn new() -> Self {
        let token = uuid::Uuid::now_v7().simple().to_string();
        Self {
            prompt_end: format!("SB_PROMPT_END_{token}"),
            response_end: format!("SB_RESPONSE_DONE_{token}"),
        }
    }
}

fn wrap_prompt_for_capture_markers(prompt: &str, markers: &PromptCaptureMarkers) -> String {
    let (response_prefix, response_suffix) = split_marker_for_prompt(&markers.response_end);
    format!(
        "{prompt}\n\nWhen your answer is complete, print the completion marker formed by joining `{response_prefix}` and `{response_suffix}` with no spaces on its own final line. Do not print either marker anywhere else.\n{}",
        markers.prompt_end
    )
}

fn split_marker_for_prompt(marker: &str) -> (&str, &str) {
    let midpoint = marker.len() / 2;
    marker.split_at(midpoint)
}

fn run_agent_interactive(
    agent: String,
    prompt: String,
    cwd: String,
    timeout_secs: Option<u64>,
    cfg: Option<&RunAgentConfig>,
    config: Option<&AgentConfig>,
    continue_session_from: Option<&str>,
    active_pane: Option<&ActivePaneRegistration>,
    interaction: Option<&InteractionEscalation>,
) -> anyhow::Result<NodeResult> {
    let effective_agent = cfg.and_then(|cfg| cfg.agent.clone()).unwrap_or(agent);

    // Preserve the lightweight command path used by tests and echo nodes.
    if effective_agent == "echo" {
        return run_agent_sequence(
            effective_agent,
            prompt,
            cwd,
            timeout_secs,
            cfg,
            config,
            continue_session_from,
            active_pane,
        );
    }

    let start = Instant::now();
    let effective_prompt = prompt;
    let abort_flag = interaction.and_then(InteractionEscalation::abort_flag);
    let effective_cwd = cfg
        .and_then(|cfg| cfg.cwd.clone())
        .unwrap_or_else(|| cwd.clone());
    let timeout = cfg.and_then(|cfg| cfg.timeout).or(timeout_secs);
    let timeout_duration = timeout
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(300));
    let idle_seconds = cfg.and_then(|cfg| cfg.idle_seconds).unwrap_or(2.0);
    let ready_stable_seconds = cfg
        .and_then(|cfg| cfg.ready_stable_seconds)
        .unwrap_or(DEFAULT_READY_STABLE_SECONDS);

    let default_config = AgentConfig::default();
    let agent_cfg = config.unwrap_or(&default_config);
    let drv = driver::get_driver(&effective_agent)
        .with_context(|| format!("No driver for agent: {}", effective_agent))?;
    anyhow::ensure!(
        drv.capabilities().worker_execution,
        "Agent {} cannot execute workflow nodes",
        effective_agent
    );
    if abort_requested(abort_flag) {
        return Ok(aborted_result(
            &effective_agent,
            &effective_prompt,
            start.elapsed(),
            None,
            None,
        ));
    }

    let interaction_patterns = drv
        .interaction_patterns()
        .into_iter()
        .filter_map(|pattern| {
            Regex::new(&pattern.pattern)
                .ok()
                .map(|regex| CompiledInteractionPattern {
                    regex,
                    kind: pattern.kind,
                    description: pattern.description,
                    send_enter: pattern.send_enter,
                })
        })
        .collect::<Vec<_>>();
    let destructive_regexes = drv
        .destructive_blocklist()
        .iter()
        .filter_map(|pattern| Regex::new(pattern).ok())
        .collect::<Vec<_>>();

    let spawn_cfg = SpawnConfig {
        agent: Some(effective_agent.clone()),
        cwd: Some(effective_cwd.clone()),
        access: cfg
            .and_then(|cfg| cfg.access.clone())
            .or_else(|| access_profile_from_config(config)),
        extra_args: cfg.map(|cfg| cfg.extra_args.clone()).unwrap_or_default(),
        name: cfg.and_then(|cfg| cfg.name.clone()),
        ..Default::default()
    };

    let reused_pane = resolve_reused_pane(active_pane, continue_session_from)?;
    let (pane_id, session_name, reused) = if let Some(pane_id) = reused_pane {
        if let Some(active_pane) = active_pane {
            active_pane.set(&pane_id);
        }
        (pane_id, None, true)
    } else {
        let spawned = spawn_pane(
            Some(&spawn_cfg),
            Some(&effective_agent),
            &effective_cwd,
            config,
            None,
            None,
        )?;
        if let Some(active_pane) = active_pane {
            active_pane.set_with_session(&spawned.pane_id, Some(&spawned.session_name));
        }
        (spawned.pane_id, Some(spawned.session_name), false)
    };

    let subagent_timeout = agent_cfg
        .orchestrator
        .as_ref()
        .and_then(|orchestrator| orchestrator.subagent_timeout_secs)
        .map(|seconds| Duration::from_secs(seconds as u64))
        .unwrap_or(Duration::from_secs(600));
    let stale_timeout_secs = agent_cfg
        .orchestrator
        .as_ref()
        .and_then(|orchestrator| orchestrator.stale_timeout_secs);

    let result = (|| {
        let ready = wait_for_agent_ready_interactive(
            &pane_id,
            timeout_duration,
            idle_seconds,
            ready_stable_seconds,
            false,
            subagent_timeout,
            &interaction_patterns,
            &destructive_regexes,
            interaction,
            abort_flag,
        )?;
        match ready {
            InteractiveReadyResult::Ready(IdleReason::TimedOut) => {
                return Ok(failed_result(
                    "Timed out waiting for tmux agent readiness",
                    -2,
                    &effective_agent,
                    &effective_prompt,
                    start.elapsed(),
                    Some(pane_id.clone()),
                    Some(NodeOutcome::ErrorTimeout),
                ));
            }
            InteractiveReadyResult::Aborted => {
                return Ok(aborted_result(
                    &effective_agent,
                    &effective_prompt,
                    start.elapsed(),
                    Some(pane_id.clone()),
                    None,
                ));
            }
            InteractiveReadyResult::Ready(_) => {}
        }

        let capture_markers = PromptCaptureMarkers::new();
        let prompt_to_send = wrap_prompt_for_capture_markers(&effective_prompt, &capture_markers);
        let before = capture_visible_stripped(&pane_id)?;
        send_text(&pane_id, &prompt_to_send, true)?;

        let polled = poll_agent_interactive(
            &pane_id,
            &before,
            &effective_prompt,
            timeout_duration,
            idle_seconds,
            ready_stable_seconds,
            cfg.and_then(|cfg| cfg.until.as_deref()),
            Some(&capture_markers),
            agent_cfg.auto_approve,
            subagent_timeout,
            stale_timeout_secs,
            &interaction_patterns,
            &destructive_regexes,
            interaction,
            abort_flag,
        )?;

        let response = match polled {
            InteractivePollResult::Completed(response) => response,
            InteractivePollResult::TimedOut(response) => {
                let mut result = failed_result(
                    "Timeout waiting for agent response",
                    -1,
                    &effective_agent,
                    &effective_prompt,
                    start.elapsed(),
                    Some(pane_id.clone()),
                    Some(NodeOutcome::ErrorTimeout),
                );
                result.output = response.output;
                result.raw_output = Some(response.final_capture);
                result.metadata.error_type = Some("timeout".to_owned());
                return Ok(result);
            }
            InteractivePollResult::Aborted(response) => {
                return Ok(aborted_result(
                    &effective_agent,
                    &effective_prompt,
                    start.elapsed(),
                    Some(pane_id.clone()),
                    Some(response),
                ));
            }
        };

        let cost = drv
            .cost_command()
            .and_then(|command| query_agent_command(&pane_id, command).ok())
            .and_then(|output| drv.parse_cost_response(&output));
        let context_pct = drv
            .context_command()
            .and_then(|command| query_agent_command(&pane_id, command).ok())
            .and_then(|output| drv.parse_context_response(&output))
            .and_then(|info| info.used_percentage);

        Ok(NodeResult {
            success: true,
            output: response.output.clone(),
            stderr: String::new(),
            exit_code: 0,
            duration: duration_string(start.elapsed()),
            agent: effective_agent.clone(),
            prompt: effective_prompt.clone(),
            raw_output: Some(response.final_capture),
            metadata: AgentExecutionMetadata {
                outcome: Some(NodeOutcome::Success),
                agent_session_id: Some(pane_id.clone()),
                cost_usd: cost.as_ref().and_then(|info| info.total_cost_usd),
                input_tokens: cost.as_ref().and_then(|info| info.input_tokens),
                output_tokens: cost.as_ref().and_then(|info| info.output_tokens),
                thinking_tokens: cost.as_ref().and_then(|info| info.thinking_tokens),
                cache_read_tokens: cost.as_ref().and_then(|info| info.cache_read_tokens),
                cache_write_tokens: cost.as_ref().and_then(|info| info.cache_write_tokens),
                context_used_pct: context_pct,
                ..Default::default()
            },
            ..Default::default()
        })
    })();

    if should_kill_after(cfg, config, reused) {
        if let Some(session_name) = session_name.as_deref() {
            let _ = tmux::run(&["kill-session", "-t", session_name]);
        } else {
            let _ = tmux::run(&["kill-pane", "-t", &pane_id]);
        }
        if let Some(active_pane) = active_pane {
            active_pane.clear_key();
        }
    }

    result
}

/// Run a single classifier/one-shot prompt through an interactive tmux agent pane.
///
/// Spawns a fresh pane, sends the prompt, captures the response, kills the pane.
/// This is heavier per call than `--print` but sandboxed and observable.
pub(crate) fn run_tmux_oneshot(
    agent: &str,
    prompt: &str,
    cwd: &str,
    config: Option<&AgentConfig>,
    inv: Option<TmuxInvocation>,
    interaction: Option<&InteractionEscalation>,
) -> anyhow::Result<NodeResult> {
    let run = || {
        run_agent_interactive(
            agent.to_string(),
            prompt.to_string(),
            cwd.to_string(),
            None,
            None,
            config,
            None,
            None,
            interaction,
        )
    };

    if let Some(inv) = inv {
        tmux_tools_core::with_invocation(inv, run)
    } else {
        run()
    }
}

pub(crate) fn list_silverbond_tmux_sessions() -> anyhow::Result<Vec<String>> {
    let output = match tmux::run_checked(&["list-sessions", "-F", "#{session_name}"]) {
        Ok(output) => output,
        Err(_) => return Ok(Vec::new()),
    };
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.starts_with(SILVERBOND_SESSION_PREFIX))
        .map(str::to_string)
        .collect())
}

pub(crate) fn kill_tmux_session(session_name: &str) {
    let _ = tmux::run(&["kill-session", "-t", session_name]);
}

pub(crate) fn cleanup_panes(targets: &[PaneCleanupTarget]) -> anyhow::Result<()> {
    let mut seen_sessions = HashSet::new();
    let mut seen_panes = HashSet::new();
    for target in targets {
        if let Some(session_name) = target.session_name.as_deref() {
            if seen_sessions.insert(session_name.to_string()) {
                let _ = tmux::run(&["kill-session", "-t", session_name]);
            }
        } else if seen_panes.insert(target.pane_id.clone()) {
            let _ = tmux::run(&["kill-pane", "-t", &target.pane_id]);
        }
    }
    Ok(())
}

fn run_agent_sequence(
    agent: String,
    prompt: String,
    cwd: String,
    timeout_secs: Option<u64>,
    cfg: Option<&RunAgentConfig>,
    config: Option<&AgentConfig>,
    continue_session_from: Option<&str>,
    active_pane: Option<&ActivePaneRegistration>,
) -> anyhow::Result<NodeResult> {
    let start = Instant::now();
    let effective_agent = cfg.and_then(|cfg| cfg.agent.clone()).unwrap_or(agent);
    let effective_prompt = prompt;
    let effective_cwd = cfg
        .and_then(|cfg| cfg.cwd.clone())
        .unwrap_or_else(|| cwd.clone());
    let timeout = cfg.and_then(|cfg| cfg.timeout).or(timeout_secs);

    let mut spawn_cfg = SpawnConfig {
        agent: Some(effective_agent.clone()),
        cwd: Some(effective_cwd.clone()),
        access: cfg
            .and_then(|cfg| cfg.access.clone())
            .or_else(|| access_profile_from_config(config)),
        extra_args: cfg.map(|cfg| cfg.extra_args.clone()).unwrap_or_default(),
        name: cfg.and_then(|cfg| cfg.name.clone()),
        ..Default::default()
    };

    let echo_mode = effective_agent == "echo";
    if echo_mode {
        spawn_cfg.command = Some(format!("printf '%s\\n' {}", shell_quote(&effective_prompt)));
    }

    let reused_pane = resolve_reused_pane(active_pane, continue_session_from)?;
    let (pane_id, session_name, reused) = if let Some(pane_id) = reused_pane {
        if let Some(active_pane) = active_pane {
            active_pane.set(&pane_id);
        }
        (pane_id, None, true)
    } else {
        let spawned = spawn_pane(
            Some(&spawn_cfg),
            Some(&effective_agent),
            &effective_cwd,
            config,
            None,
            None,
        )?;
        if let Some(active_pane) = active_pane {
            active_pane.set_with_session(&spawned.pane_id, Some(&spawned.session_name));
        }
        (spawned.pane_id, Some(spawned.session_name), false)
    };

    let result = (|| {
        if !echo_mode {
            let ready_cfg = WaitConfig {
                mode: WaitMode::Ready,
                timeout,
                idle_seconds: cfg.and_then(|cfg| cfg.idle_seconds),
                ready_stable_seconds: cfg.and_then(|cfg| cfg.ready_stable_seconds),
                ..Default::default()
            };
            let ready = wait_for_mode(&pane_id, &ready_cfg, timeout)?;
            if ready.reason == IdleReason::TimedOut {
                return Ok(failed_result(
                    "Timed out waiting for tmux agent readiness",
                    -2,
                    &effective_agent,
                    &effective_prompt,
                    start.elapsed(),
                    Some(pane_id.clone()),
                    Some(NodeOutcome::ErrorTimeout),
                ));
            }

            let before = capture_visible_stripped(&pane_id)?;
            send_text(&pane_id, &effective_prompt, true)?;
            let wait_cfg = WaitConfig {
                mode: if cfg.and_then(|cfg| cfg.until.as_ref()).is_some() {
                    WaitMode::Until
                } else {
                    WaitMode::Idle
                },
                marker: cfg.and_then(|cfg| cfg.until.clone()),
                timeout,
                idle_seconds: cfg.and_then(|cfg| cfg.idle_seconds),
                ready_stable_seconds: cfg.and_then(|cfg| cfg.ready_stable_seconds),
                ..Default::default()
            };
            let waited = wait_for_mode(&pane_id, &wait_cfg, timeout)?;
            if waited.reason == IdleReason::TimedOut {
                return Ok(failed_result(
                    "Timed out waiting for tmux agent response",
                    -2,
                    &effective_agent,
                    &effective_prompt,
                    start.elapsed(),
                    Some(pane_id.clone()),
                    Some(NodeOutcome::ErrorTimeout),
                ));
            }
            let after = capture_visible_stripped(&pane_id)?;
            let output = extract_after_prompt(&before, &after, &effective_prompt);
            return Ok(NodeResult {
                success: true,
                output: output.clone(),
                stderr: String::new(),
                exit_code: 0,
                duration: duration_string(start.elapsed()),
                agent: effective_agent.clone(),
                prompt: effective_prompt.clone(),
                raw_output: Some(after),
                metadata: AgentExecutionMetadata {
                    outcome: Some(NodeOutcome::Success),
                    agent_session_id: Some(pane_id.clone()),
                    ..Default::default()
                },
                ..Default::default()
            });
        }

        let wait_cfg = WaitConfig {
            mode: WaitMode::Idle,
            timeout,
            idle_seconds: cfg.and_then(|cfg| cfg.idle_seconds),
            ready_stable_seconds: cfg.and_then(|cfg| cfg.ready_stable_seconds),
            ..Default::default()
        };
        let waited = wait_for_mode(&pane_id, &wait_cfg, timeout)?;
        if waited.reason == IdleReason::TimedOut {
            return Ok(failed_result(
                "Timed out waiting for tmux command",
                -2,
                &effective_agent,
                &effective_prompt,
                start.elapsed(),
                Some(pane_id.clone()),
                Some(NodeOutcome::ErrorTimeout),
            ));
        }
        let output = capture_visible_stripped(&pane_id)?;
        Ok(NodeResult {
            success: true,
            output: output.clone(),
            stderr: String::new(),
            exit_code: 0,
            duration: duration_string(start.elapsed()),
            agent: effective_agent.clone(),
            prompt: effective_prompt.clone(),
            raw_output: Some(output),
            metadata: AgentExecutionMetadata {
                outcome: Some(NodeOutcome::Success),
                agent_session_id: Some(pane_id.clone()),
                ..Default::default()
            },
            ..Default::default()
        })
    })();

    if should_kill_after(cfg, config, reused) {
        if let Some(session_name) = session_name.as_deref() {
            let _ = tmux::run(&["kill-session", "-t", session_name]);
        } else {
            let _ = tmux::run(&["kill-pane", "-t", &pane_id]);
        }
        if let Some(active_pane) = active_pane {
            active_pane.clear_key();
        }
    }

    result
}

fn resolve_reused_pane(
    active_pane: Option<&ActivePaneRegistration>,
    continue_session_from: Option<&str>,
) -> anyhow::Result<Option<String>> {
    let Some(source_id) = continue_session_from
        .map(str::trim)
        .filter(|source_id| !source_id.is_empty())
    else {
        return Ok(None);
    };
    let registration = active_pane
        .ok_or_else(|| anyhow!("continueSessionFrom requires a run-scoped tmux pane registry"))?;
    let pane_id = registration.resolve(source_id).ok_or_else(|| {
        anyhow!(
            "continueSessionFrom source node \"{}\" does not have an active tmux pane",
            source_id
        )
    })?;
    Ok(Some(pane_id))
}

fn should_kill_after(
    cfg: Option<&RunAgentConfig>,
    config: Option<&AgentConfig>,
    reused: bool,
) -> bool {
    !reused
        && cfg.map(|cfg| cfg.kill_after).unwrap_or(true)
        && config
            .map(|config| config.ephemeral_session)
            .unwrap_or(true)
}

#[allow(clippy::too_many_arguments)]
fn wait_for_agent_ready_interactive(
    pane: &str,
    timeout: Duration,
    idle_seconds: f64,
    ready_stable_seconds: f64,
    auto_approve: bool,
    subagent_timeout: Duration,
    interaction_patterns: &[CompiledInteractionPattern],
    destructive_regexes: &[Regex],
    interaction: Option<&InteractionEscalation>,
    abort_flag: Option<&AtomicBool>,
) -> anyhow::Result<InteractiveReadyResult> {
    let ready_signal = ready_signal_for_pane(pane)?;
    let start = Instant::now();
    let mut deadline = start + timeout;
    let max_total_timeout = timeout.max(subagent_timeout * 10);
    let absolute_deadline = start + max_total_timeout;
    let mut capture = capture_visible_stripped(pane).unwrap_or_default();
    let mut last_seen = capture.clone();
    let mut progress = CaptureProgress::new(&capture, start);
    let mut handled_matches = Vec::new();
    let mut handled_destructive_matches = HandledDestructiveMatches::default();
    let mut first_poll = true;

    loop {
        if abort_requested(abort_flag) {
            return Ok(InteractiveReadyResult::Aborted);
        }
        let now = Instant::now();
        capture = match capture_visible_stripped(pane) {
            Ok(capture) => capture,
            Err(_err) if abort_requested(abort_flag) => {
                return Ok(InteractiveReadyResult::Aborted);
            }
            Err(err) => return Err(err),
        };
        let new_text = capture_delta(&last_seen, &capture);
        let has_new_text = !new_text.is_empty();
        if has_new_text {
            last_seen = capture.clone();
        }

        if let Some(destructive_match) = next_unhandled_destructive_match(
            destructive_regexes,
            &capture,
            &handled_destructive_matches,
        ) {
            handled_destructive_matches.record(destructive_match);
            let reply = escalate_or_fallback(
                interaction,
                pane,
                InteractionKind::DestructiveWarning.event_type(),
                "Potentially destructive command detected",
                &capture,
                "n",
            )?;
            send_reply_if_present(pane, &reply, true)?;
            continue;
        }

        if (first_poll || has_new_text)
            && let Some((idx, key)) =
                next_unhandled_pattern_match(interaction_patterns, &capture, &handled_matches)
        {
            handled_matches.push(key);
            let pattern = &interaction_patterns[idx];
            match &pattern.kind {
                InteractionKind::AutoRespond { response } => {
                    send_text(pane, response, pattern.send_enter)?;
                    continue;
                }
                InteractionKind::SubagentActive => {
                    let reset_at = Instant::now();
                    deadline = reset_at + subagent_timeout;
                    if reset_at < absolute_deadline {
                        continue;
                    }
                }
                InteractionKind::PermissionRequest => {
                    let is_destructive = destructive_regexes
                        .iter()
                        .any(|regex| regex.is_match(&capture));
                    let reply = permission_request_reply(
                        interaction,
                        pane,
                        &pattern.description,
                        &capture,
                        is_destructive,
                        auto_approve,
                    )?;
                    send_reply_if_present(pane, &reply, pattern.send_enter)?;
                    continue;
                }
                InteractionKind::DestructiveWarning => {
                    let reply = escalate_or_fallback(
                        interaction,
                        pane,
                        InteractionKind::DestructiveWarning.event_type(),
                        &pattern.description,
                        &capture,
                        "n",
                    )?;
                    send_reply_if_present(pane, &reply, pattern.send_enter)?;
                    continue;
                }
            }
        }

        if let Some(reason) = progress.observe(
            &capture,
            now,
            idle_seconds,
            &ready_signal.regex,
            ready_signal.scan_lines,
            ready_stable_seconds,
            &[],
        ) {
            return Ok(InteractiveReadyResult::Ready(reason));
        }

        if now >= deadline || now >= absolute_deadline {
            return Ok(InteractiveReadyResult::Ready(IdleReason::TimedOut));
        }

        first_poll = false;
        sleep(Duration::from_millis(250));
    }
}

#[allow(clippy::too_many_arguments)]
fn poll_agent_interactive(
    pane: &str,
    before: &str,
    prompt: &str,
    timeout: Duration,
    idle_seconds: f64,
    ready_stable_seconds: f64,
    until: Option<&str>,
    capture_markers: Option<&PromptCaptureMarkers>,
    auto_approve: bool,
    subagent_timeout: Duration,
    stale_timeout_secs: Option<u32>,
    interaction_patterns: &[CompiledInteractionPattern],
    destructive_regexes: &[Regex],
    interaction: Option<&InteractionEscalation>,
    abort_flag: Option<&AtomicBool>,
) -> anyhow::Result<InteractivePollResult> {
    let ready_signal = ready_signal_for_pane(pane)?;
    let until_regexes = compile_until_regexes(until, capture_markers)?;
    let stale_threshold = stale_timeout_secs
        .map(|seconds| seconds as f64)
        .unwrap_or(IDLE_ESCALATE_SECS);
    let start = Instant::now();
    let mut deadline = start + timeout;
    let max_total_timeout = timeout.max(subagent_timeout * 10);
    let absolute_deadline = start + max_total_timeout;
    let mut last_seen = before.to_owned();
    let mut progress = CaptureProgress::new(before, start);
    let mut handled_matches = Vec::new();
    let mut handled_destructive_matches = HandledDestructiveMatches::default();
    let mut idle_since: Option<Instant> = None;
    let mut escalated_idle = false;

    loop {
        if abort_requested(abort_flag) {
            let output_so_far =
                extract_after_prompt_with_markers(before, &last_seen, prompt, capture_markers);
            return Ok(InteractivePollResult::Aborted(InteractiveCapture {
                output: output_so_far,
                final_capture: last_seen,
            }));
        }
        let now = Instant::now();
        let capture = match capture_visible_stripped(pane) {
            Ok(capture) => capture,
            Err(_err) if abort_requested(abort_flag) => {
                let output_so_far =
                    extract_after_prompt_with_markers(before, &last_seen, prompt, capture_markers);
                return Ok(InteractivePollResult::Aborted(InteractiveCapture {
                    output: output_so_far,
                    final_capture: last_seen,
                }));
            }
            Err(err) => return Err(err),
        };
        let new_text = capture_delta(&last_seen, &capture);
        let has_new_text = !new_text.is_empty();
        if has_new_text {
            last_seen = capture.clone();
            idle_since = None;
        }
        let output_so_far =
            extract_after_prompt_with_markers(before, &capture, prompt, capture_markers);

        // Cumulative over the visible pane; a line that scrolls fully off-screen is still out of scope.
        if let Some(destructive_match) = next_unhandled_destructive_match(
            destructive_regexes,
            &output_so_far,
            &handled_destructive_matches,
        ) {
            handled_destructive_matches.record(destructive_match);
            let reply = escalate_or_fallback(
                interaction,
                pane,
                InteractionKind::DestructiveWarning.event_type(),
                "Potentially destructive command detected",
                &output_so_far,
                "n",
            )?;
            send_reply_if_present(pane, &reply, true)?;
            continue;
        }

        if has_new_text
            && let Some((idx, key)) =
                next_unhandled_pattern_match(interaction_patterns, &output_so_far, &handled_matches)
        {
            handled_matches.push(key);
            let pattern = &interaction_patterns[idx];
            match &pattern.kind {
                InteractionKind::AutoRespond { response } => {
                    send_text(pane, response, pattern.send_enter)?;
                    continue;
                }
                InteractionKind::SubagentActive => {
                    let reset_at = Instant::now();
                    deadline = reset_at + subagent_timeout;
                    if reset_at < absolute_deadline {
                        continue;
                    }
                }
                InteractionKind::PermissionRequest => {
                    let is_destructive = destructive_regexes
                        .iter()
                        .any(|regex| regex.is_match(&output_so_far));
                    let reply = permission_request_reply(
                        interaction,
                        pane,
                        &pattern.description,
                        &output_so_far,
                        is_destructive,
                        auto_approve,
                    )?;
                    send_reply_if_present(pane, &reply, pattern.send_enter)?;
                    continue;
                }
                InteractionKind::DestructiveWarning => {
                    let reply = escalate_or_fallback(
                        interaction,
                        pane,
                        InteractionKind::DestructiveWarning.event_type(),
                        &pattern.description,
                        &output_so_far,
                        "n",
                    )?;
                    send_reply_if_present(pane, &reply, pattern.send_enter)?;
                    continue;
                }
            }
        }

        if let Some(reason) = progress.observe(
            &capture,
            now,
            idle_seconds,
            &ready_signal.regex,
            ready_signal.scan_lines,
            ready_stable_seconds,
            &until_regexes,
        ) {
            if is_interactive_completion_reason(reason) {
                return Ok(InteractivePollResult::Completed(InteractiveCapture {
                    output: output_so_far,
                    final_capture: capture,
                }));
            }

            if matches!(reason, IdleReason::Idle) {
                let idle_started = *idle_since.get_or_insert(now);
                if !escalated_idle && (now - idle_started).as_secs_f64() >= stale_threshold {
                    let reply = escalate_or_fallback(
                        interaction,
                        pane,
                        "agent_idle",
                        "Agent output appears stale — it may be waiting for input",
                        &output_so_far,
                        "",
                    )?;
                    send_reply_if_present(pane, &reply, true)?;
                    escalated_idle = true;
                    continue;
                }
            }
        }

        if now >= deadline || now >= absolute_deadline {
            return Ok(InteractivePollResult::TimedOut(InteractiveCapture {
                output: output_so_far,
                final_capture: capture,
            }));
        }

        sleep(Duration::from_millis(250));
    }
}

fn compile_until_regexes(
    until: Option<&str>,
    capture_markers: Option<&PromptCaptureMarkers>,
) -> anyhow::Result<Vec<Regex>> {
    let mut regexes = Vec::new();
    if let Some(pattern) = until {
        regexes.push(Regex::new(pattern).context("invalid wait marker regex")?);
    }
    if let Some(markers) = capture_markers {
        regexes.push(
            Regex::new(&regex::escape(&markers.response_end))
                .context("invalid response sentinel regex")?,
        );
    }
    Ok(regexes)
}

fn is_interactive_completion_reason(reason: IdleReason) -> bool {
    match reason {
        IdleReason::ReadyMatched | IdleReason::UntilMatched => true,
        IdleReason::Idle | IdleReason::TimedOut => false,
    }
}

fn query_agent_command(pane: &str, command: &str) -> anyhow::Result<String> {
    const QUERY_IDLE_SECONDS: f64 = 0.25;
    const QUERY_TIMEOUT: Duration = Duration::from_secs(3);

    let before = capture_visible_stripped(pane)?;
    send_text(pane, command, true)?;

    let start = Instant::now();
    let mut capture = before.clone();
    let mut progress = CaptureProgress::new(&capture, start);
    let deadline = start + QUERY_TIMEOUT;

    loop {
        let now = Instant::now();
        capture = capture_visible_stripped(pane)?;

        if let Some(IdleReason::Idle) = progress.observe(
            &capture,
            now,
            QUERY_IDLE_SECONDS,
            &None,
            DEFAULT_READY_SCAN_LINES,
            QUERY_IDLE_SECONDS,
            &[],
        ) {
            break;
        }

        if now >= deadline {
            break;
        }

        sleep(Duration::from_millis(250));
    }

    let delta = capture_delta(&before, &capture);
    if !delta.trim().is_empty() {
        return Ok(delta);
    }
    Ok(extract_after_prompt(&before, &capture, command))
}

fn escalate_or_fallback(
    interaction: Option<&InteractionEscalation>,
    session_id: &str,
    interaction_type: &str,
    description: &str,
    output_so_far: &str,
    fallback: &str,
) -> anyhow::Result<String> {
    if let Some(interaction) = interaction {
        match interaction.request(session_id, interaction_type, description, output_so_far) {
            Ok(reply) => Ok(reply),
            Err(_err) if interaction.is_aborted() => Ok(String::new()),
            Err(err) => Err(err),
        }
    } else {
        if interaction_type == InteractionKind::PermissionRequest.event_type()
            && fallback.eq_ignore_ascii_case("n")
        {
            tracing::warn!(
                session_id,
                interaction_type,
                description,
                "permission_denied: no interaction channel is available"
            );
        }
        Ok(fallback.to_owned())
    }
}

fn permission_request_reply(
    interaction: Option<&InteractionEscalation>,
    pane: &str,
    description: &str,
    output_so_far: &str,
    is_destructive: bool,
    auto_approve: bool,
) -> anyhow::Result<String> {
    if is_destructive {
        return escalate_or_fallback(
            interaction,
            pane,
            InteractionKind::DestructiveWarning.event_type(),
            description,
            output_so_far,
            "n",
        );
    }

    if auto_approve {
        return Ok("y".to_owned());
    }

    escalate_or_fallback(
        interaction,
        pane,
        InteractionKind::PermissionRequest.event_type(),
        description,
        output_so_far,
        "n",
    )
}

fn send_reply_if_present(pane: &str, reply: &str, send_enter: bool) -> anyhow::Result<()> {
    if !reply.is_empty() {
        send_text(pane, reply, send_enter)?;
    }
    Ok(())
}

fn abort_requested(abort_flag: Option<&AtomicBool>) -> bool {
    abort_flag.is_some_and(|flag| flag.load(Ordering::SeqCst))
}

#[derive(Default)]
struct HandledDestructiveMatches {
    spans: HashSet<String>,
    lines: HashSet<String>,
}

impl HandledDestructiveMatches {
    fn is_handled(&self, matched: &DestructiveMatch) -> bool {
        self.spans.contains(&matched.span_key) || self.lines.contains(&matched.line_key)
    }

    fn record(&mut self, matched: DestructiveMatch) {
        self.spans.insert(matched.span_key);
        self.lines.insert(matched.line_key);
    }
}

struct DestructiveMatch {
    span_key: String,
    line_key: String,
}

struct BuiltCommand {
    command: String,
    env: Vec<(String, String)>,
}

fn next_unhandled_destructive_match(
    regexes: &[Regex],
    text: &str,
    handled_matches: &HandledDestructiveMatches,
) -> Option<DestructiveMatch> {
    for regex in regexes {
        for matched in regex.find_iter(text) {
            let line_start = text[..matched.start()]
                .rfind('\n')
                .map(|idx| idx + 1)
                .unwrap_or(0);
            let line_end = text[matched.end()..]
                .find('\n')
                .map(|idx| matched.end() + idx)
                .unwrap_or(text.len());
            let line = text[line_start..line_end].trim();
            let candidate = DestructiveMatch {
                span_key: format!("{}:{}:{}", regex.as_str(), matched.start(), matched.end()),
                line_key: format!("{}:{}", regex.as_str(), line),
            };
            if !handled_matches.is_handled(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn next_unhandled_pattern_match(
    patterns: &[CompiledInteractionPattern],
    text: &str,
    handled_matches: &[String],
) -> Option<(usize, String)> {
    patterns.iter().enumerate().find_map(|(idx, pattern)| {
        let matched = pattern.regex.find(text)?;
        let key = format!(
            "{}:{}:{}:{}",
            pattern.kind.event_type(),
            pattern.description,
            matched.start(),
            matched.end()
        );
        (!handled_matches.iter().any(|handled| handled == &key)).then_some((idx, key))
    })
}

struct CaptureProgress {
    previous_capture: String,
    last_change: Instant,
    capture_count: u64,
    ready_debounce: MatchDebounce,
    until_debounce: MatchDebounce,
}

impl CaptureProgress {
    fn new(initial_capture: &str, now: Instant) -> Self {
        Self {
            previous_capture: initial_capture.to_owned(),
            last_change: now,
            capture_count: 0,
            ready_debounce: MatchDebounce::default(),
            until_debounce: MatchDebounce::default(),
        }
    }

    fn observe(
        &mut self,
        stripped: &str,
        now: Instant,
        idle_seconds: f64,
        ready_regex: &Option<Regex>,
        ready_scan_lines: usize,
        ready_stable_seconds: f64,
        until_regexes: &[Regex],
    ) -> Option<IdleReason> {
        if self.previous_capture != stripped {
            self.last_change = now;
            self.previous_capture = stripped.to_owned();
        }
        self.capture_count += 1;

        let until_now = until_regexes.iter().any(|regex| regex.is_match(stripped));
        if self
            .until_debounce
            .observe(until_now, now, ready_stable_seconds)
        {
            return Some(IdleReason::UntilMatched);
        }

        let ready_now = ready_matches(stripped, ready_regex, ready_scan_lines);
        if self
            .ready_debounce
            .observe(ready_now, now, ready_stable_seconds)
        {
            return Some(IdleReason::ReadyMatched);
        }

        if !ready_now
            && !until_now
            && self.capture_count >= 2
            && (now - self.last_change).as_secs_f64() >= idle_seconds
        {
            return Some(IdleReason::Idle);
        }

        None
    }
}

#[derive(Default)]
struct MatchDebounce {
    since: Option<Instant>,
}

impl MatchDebounce {
    fn observe(&mut self, matched: bool, now: Instant, stable_seconds: f64) -> bool {
        if !matched {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        (now - since).as_secs_f64() >= stable_seconds
    }
}

fn ready_matches(stripped: &str, ready_regex: &Option<Regex>, ready_scan_lines: usize) -> bool {
    ready_regex.as_ref().is_some_and(|regex| {
        bottom_non_blank_lines(stripped, ready_scan_lines)
            .iter()
            .any(|line| regex.is_match(line))
    })
}

fn bottom_non_blank_lines(stripped: &str, count: usize) -> Vec<&str> {
    let count = count.max(1);
    let mut lines: Vec<&str> = stripped
        .split('\n')
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(count)
        .collect();
    lines.reverse();
    lines
}

fn capture_delta(previous: &str, current: &str) -> String {
    if current == previous {
        return String::new();
    }
    if let Some(suffix) = current.strip_prefix(previous) {
        return suffix.to_owned();
    }

    let mut boundary = 0usize;
    for ((prev_idx, prev_ch), (current_idx, current_ch)) in
        previous.char_indices().zip(current.char_indices())
    {
        if prev_ch != current_ch || prev_idx != current_idx {
            break;
        }
        boundary = current_idx + current_ch.len_utf8();
    }
    current[boundary..].to_owned()
}

fn spawn_pane(
    cfg: Option<&SpawnConfig>,
    fallback_agent: Option<&str>,
    cwd: &str,
    agent_config: Option<&AgentConfig>,
    command_override: Option<String>,
    session_override: Option<String>,
) -> anyhow::Result<SpawnedPane> {
    let agent = cfg
        .and_then(|cfg| cfg.agent.as_deref())
        .or(fallback_agent)
        .map(str::to_owned);
    let built_command = command_override
        .or_else(|| cfg.and_then(|cfg| cfg.command.clone()))
        .map(|command| {
            Ok(BuiltCommand {
                command,
                env: Vec::new(),
            })
        })
        .unwrap_or_else(|| {
            let agent = agent
                .as_deref()
                .ok_or_else(|| anyhow!("spawn requires an agent or command"))?;
            build_agent_command(agent, cfg, agent_config)
        })?;
    let session_name = session_override
        .as_deref()
        .or_else(|| cfg.and_then(|cfg| cfg.session_name.as_deref()))
        .map(stable_session_name)
        .unwrap_or_else(|| unique_session_name(cfg.and_then(|cfg| cfg.name.as_deref())));
    let work_dir = cfg
        .and_then(|cfg| cfg.cwd.as_deref())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(cwd);
    let command = wrap_keep_open(&built_command.command, work_dir);

    let mut args = vec![
        "new-session".to_owned(),
        "-d".to_owned(),
        "-s".to_owned(),
        session_name.clone(),
        "-P".to_owned(),
        "-F".to_owned(),
        "#{pane_id}".to_owned(),
    ];
    if !work_dir.trim().is_empty() {
        args.push("-c".to_owned());
        args.push(work_dir.to_owned());
    }
    for (key, value) in &built_command.env {
        args.push("-e".to_owned());
        args.push(format!("{key}={value}"));
    }
    args.push(command.clone());

    let pane_id = tmux::run_checked_owned(&args)?.trim().to_owned();
    if pane_id.is_empty() {
        return Err(anyhow!("tmux new-session returned an empty pane id"));
    }
    let mut session_guard = SessionGuard::new(session_name.clone());

    if let Some(agent) = &agent {
        names::set(&pane_id, names::KEY_AGENT, agent)?;
    }
    if let Some(access) = cfg.and_then(|cfg| cfg.access.as_deref()) {
        names::set(&pane_id, names::KEY_ACCESS, access)?;
    }
    if let Some(name) = cfg.and_then(|cfg| cfg.name.as_deref()) {
        names::set(&pane_id, names::KEY_NAME, name)?;
    }
    if !work_dir.trim().is_empty() {
        names::set(&pane_id, names::KEY_CWD, work_dir)?;
    }

    session_guard.disarm();
    Ok(SpawnedPane {
        pane_id,
        session_name,
        agent,
        command,
    })
}

fn build_agent_command(
    agent: &str,
    cfg: Option<&SpawnConfig>,
    agent_config: Option<&AgentConfig>,
) -> anyhow::Result<BuiltCommand> {
    let extra_args = cfg.map(|cfg| cfg.extra_args.as_slice()).unwrap_or(&[]);
    let registry = driver::load_agent_registry()?;

    let (argv, env) = if let Some(agent_config) = agent_config {
        let drv =
            driver::get_driver(agent).ok_or_else(|| anyhow!("No driver for agent: {agent}"))?;
        let session_args = drv.build_session_args(agent_config)?;
        let binary = registry
            .get(agent)
            .map(|spec| spec.binary.clone())
            .or_else(|| driver::agent_binary(agent))
            .unwrap_or_else(|| agent.to_owned());
        (
            std::iter::once(binary)
                .chain(session_args.args)
                .chain(extra_args.iter().cloned())
                .collect::<Vec<_>>(),
            session_args.env,
        )
    } else {
        let explicit_access = cfg.and_then(|cfg| cfg.access.as_deref());
        let argv = match registry.launch_argv(agent, explicit_access) {
            Ok((binary, args)) => std::iter::once(binary)
                .chain(args)
                .chain(extra_args.iter().cloned())
                .collect::<Vec<_>>(),
            Err(error) => {
                if registry.get(agent).is_some() {
                    return Err(error);
                }
                std::iter::once(agent.to_owned())
                    .chain(extra_args.iter().cloned())
                    .collect::<Vec<_>>()
            }
        };
        (argv, Vec::new())
    };

    Ok(BuiltCommand {
        command: argv
            .iter()
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" "),
        env,
    })
}

fn access_profile_from_config(config: Option<&AgentConfig>) -> Option<String> {
    let config = config?;
    Some(
        match config.access_mode {
            AccessMode::ReadOnly => "read-only",
            AccessMode::Edit | AccessMode::Execute => "workspace-write",
            AccessMode::Unrestricted => "full-access",
        }
        .to_owned(),
    )
}

fn wait_for_mode(
    pane: &str,
    cfg: &WaitConfig,
    timeout_secs: Option<u64>,
) -> anyhow::Result<tmux_tools_core::idle::IdleOutcome> {
    let mut ready_regex = None;
    let mut ready_scan_lines = DEFAULT_READY_SCAN_LINES;
    let mut until_regex = None;

    match cfg.mode {
        WaitMode::Idle => {}
        WaitMode::Ready => {
            let ready = ready_signal_for_pane(pane)?;
            ready_regex = ready.regex;
            ready_scan_lines = ready.scan_lines;
        }
        WaitMode::Until => {
            let marker = cfg
                .marker
                .as_deref()
                .ok_or_else(|| anyhow!("wait until mode requires marker"))?;
            until_regex = Some(Regex::new(marker).context("invalid wait marker regex")?);
        }
    }

    let timeout = cfg
        .timeout
        .or(timeout_secs)
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(120));
    let idle_seconds = cfg.idle_seconds.unwrap_or(2.0);
    let ready_stable_seconds = cfg
        .ready_stable_seconds
        .unwrap_or(DEFAULT_READY_STABLE_SECONDS);

    wait_for_idle(
        pane,
        &IdleConfig {
            idle_seconds,
            poll_interval: Duration::from_millis(250),
            timeout,
            ready_regex,
            ready_scan_lines,
            ready_stable_seconds,
            until_regex,
        },
    )
}

struct ReadySignal {
    regex: Option<Regex>,
    scan_lines: usize,
}

fn ready_signal_for_pane(pane: &str) -> anyhow::Result<ReadySignal> {
    let registered = names::read(pane)?;
    let Some(agent_name) = registered.agent else {
        return Ok(ReadySignal {
            regex: None,
            scan_lines: DEFAULT_READY_SCAN_LINES,
        });
    };
    let registry = driver::load_agent_registry()?;
    let Some(agent) = registry.get(&agent_name) else {
        return Ok(ReadySignal {
            regex: None,
            scan_lines: DEFAULT_READY_SCAN_LINES,
        });
    };
    let scan_lines = agent.ready_lines.unwrap_or(DEFAULT_READY_SCAN_LINES).max(1);
    let Some(pattern) = agent.ready_regex.as_deref() else {
        return Ok(ReadySignal {
            regex: None,
            scan_lines,
        });
    };
    Ok(ReadySignal {
        regex: Some(
            Regex::new(pattern).with_context(|| {
                format!("invalid ready_regex for agent {agent_name}: {pattern}")
            })?,
        ),
        scan_lines,
    })
}

fn send_text(pane: &str, text: &str, enter: bool) -> anyhow::Result<()> {
    tmux::run_checked(&["send-keys", "-t", pane, "-l", "--", text])?;
    if enter {
        tmux::run_checked(&["send-keys", "-t", pane, "Enter"])?;
    }
    Ok(())
}

fn capture_pane(pane: &str, cfg: &CaptureConfig) -> anyhow::Result<String> {
    if cfg.ansi {
        if cfg.all {
            return tmux::run_checked(&["capture-pane", "-e", "-p", "-t", pane, "-S", "-"]);
        }
        return tmux_tools_core::stream::capture_ansi(
            pane,
            tmux_tools_core::stream::CaptureAnsiOpts {
                start: cfg.lines.map(|lines| -(i64::from(lines))),
                end: None,
            },
        );
    }

    let mut args = vec![
        "capture-pane".to_owned(),
        "-t".to_owned(),
        pane.to_owned(),
        "-p".to_owned(),
    ];
    if cfg.all {
        args.push("-S".to_owned());
        args.push("-".to_owned());
    } else if let Some(lines) = cfg.lines {
        args.push("-S".to_owned());
        args.push(format!("-{lines}"));
        args.push("-E".to_owned());
        args.push("-".to_owned());
    }
    tmux::run_checked_owned(&args)
}

fn capture_visible_stripped(pane: &str) -> anyhow::Result<String> {
    let raw = tmux::run_checked(&["capture-pane", "-e", "-p", "-t", pane])?;
    Ok(strip_ansi(raw.as_bytes()))
}

fn resolve_pane_target(configured: Option<&str>, previous_output: &str) -> anyhow::Result<String> {
    let previous = parse_previous(previous_output);
    let raw = configured
        .or_else(|| previous_value(&previous, &["paneId", "pane_id", "target"]))
        .ok_or_else(|| anyhow!("tmux pane target is required"))?;
    target::resolve(&target::parse(raw), None, None)
}

fn parse_previous(previous_output: &str) -> Option<Value> {
    serde_json::from_str(previous_output.trim()).ok()
}

fn previous_value<'a>(value: &'a Option<Value>, names: &[&str]) -> Option<&'a str> {
    let value = value.as_ref()?;
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
}

fn json_result(
    success: bool,
    parsed: Value,
    agent: &str,
    prompt: String,
    duration: Duration,
) -> NodeResult {
    NodeResult {
        success,
        output: parsed.to_string(),
        stderr: String::new(),
        exit_code: if success { 0 } else { 1 },
        duration: duration_string(duration),
        agent: agent.to_owned(),
        prompt,
        raw_output: Some(parsed.to_string()),
        parsed_output: Some(parsed),
        metadata: AgentExecutionMetadata {
            outcome: Some(if success {
                NodeOutcome::Success
            } else {
                NodeOutcome::ErrorExecution
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn failed_result(
    stderr: &str,
    exit_code: i32,
    agent: &str,
    prompt: &str,
    duration: Duration,
    pane_id: Option<String>,
    outcome: Option<NodeOutcome>,
) -> NodeResult {
    NodeResult {
        success: false,
        output: String::new(),
        stderr: stderr.to_owned(),
        exit_code,
        duration: duration_string(duration),
        agent: agent.to_owned(),
        prompt: prompt.to_owned(),
        metadata: AgentExecutionMetadata {
            outcome,
            error_type: Some("tmux".to_owned()),
            agent_session_id: pane_id,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn aborted_result(
    agent: &str,
    prompt: &str,
    duration: Duration,
    pane_id: Option<String>,
    capture: Option<InteractiveCapture>,
) -> NodeResult {
    let mut result = failed_result(
        "Agent run aborted",
        -15,
        agent,
        prompt,
        duration,
        pane_id,
        Some(NodeOutcome::ErrorExecution),
    );
    result.metadata.error_type = Some("aborted".to_owned());
    if let Some(capture) = capture {
        result.output = capture.output;
        result.raw_output = Some(capture.final_capture);
    }
    result
}

fn duration_string(duration: Duration) -> String {
    format!("{:.1}", duration.as_secs_f64())
}

const SILVERBOND_SESSION_PREFIX: &str = "silverbond-";

fn stable_session_name(name: &str) -> String {
    normalized_session_name(Some(name), None)
}

fn unique_session_name(name: Option<&str>) -> String {
    let suffix = uuid::Uuid::now_v7().simple().to_string();
    normalized_session_name(name, Some(&suffix))
}

fn normalized_session_name(name: Option<&str>, suffix: Option<&str>) -> String {
    let base = name
        .map(sanitize_tmux_name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "run".to_owned());
    let prefixed = if base.starts_with(SILVERBOND_SESSION_PREFIX) {
        base
    } else {
        format!("{SILVERBOND_SESSION_PREFIX}{base}")
    };
    if let Some(suffix) = suffix.filter(|value| !value.is_empty()) {
        format!("{prefixed}-{suffix}")
    } else {
        prefixed
    }
}

fn sanitize_tmux_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

fn wrap_keep_open(cmd: &str, cwd: &str) -> String {
    let inner = format!("cd {} && {{ {}; }}; exec zsh -li", shell_quote(cwd), cmd);
    format!("zsh -lic {}", shell_quote(&inner))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub(crate) fn extract_after_prompt(before: &str, after: &str, prompt_text: &str) -> String {
    extract_after_prompt_with_sentinels(before, after, prompt_text, None, None)
}

fn extract_after_prompt_with_markers(
    before: &str,
    after: &str,
    prompt_text: &str,
    markers: Option<&PromptCaptureMarkers>,
) -> String {
    extract_after_prompt_with_sentinels(
        before,
        after,
        prompt_text,
        markers.map(|markers| markers.prompt_end.as_str()),
        markers.map(|markers| markers.response_end.as_str()),
    )
}

pub(crate) fn extract_after_prompt_with_sentinels(
    before: &str,
    after: &str,
    prompt_text: &str,
    prompt_end_marker: Option<&str>,
    response_end_marker: Option<&str>,
) -> String {
    if let Some(prompt_end_marker) = prompt_end_marker {
        if let Some(start) = marker_line_end(after, prompt_end_marker) {
            let answer_region = &after[start..];
            let answer_region = if let Some(response_end_marker) = response_end_marker {
                trim_before_last_marker_line(answer_region, response_end_marker)
                    .unwrap_or(answer_region)
            } else {
                answer_region
            };
            return answer_region.to_owned();
        }
    }

    if let Some(response_end_marker) = response_end_marker {
        if let Some(end) = marker_line_start(after, response_end_marker) {
            return after[..end].to_owned();
        }
    }

    let before_lines = before.lines().collect::<HashSet<_>>();
    let mut first_new_match_end = None;
    let mut last_match_end = None;
    let mut offset = 0usize;

    for line in after.split_inclusive('\n') {
        if line.contains(prompt_text) {
            let end = offset + line.len();
            last_match_end = Some(end);
            let trimmed = line.strip_suffix('\n').unwrap_or(line);
            if first_new_match_end.is_none() && !before_lines.contains(trimmed) {
                first_new_match_end = Some(end);
            }
        }
        offset += line.len();
    }

    first_new_match_end
        .or(last_match_end)
        .map(|index| after[index..].to_owned())
        .unwrap_or_else(|| after.to_owned())
}

fn marker_line_end(text: &str, marker: &str) -> Option<usize> {
    let marker_start = text.find(marker)?;
    Some(
        text[marker_start..]
            .find('\n')
            .map(|idx| marker_start + idx + 1)
            .unwrap_or(text.len()),
    )
}

fn marker_line_start(text: &str, marker: &str) -> Option<usize> {
    let marker_start = text.rfind(marker)?;
    Some(
        text[..marker_start]
            .rfind('\n')
            .map(|idx| idx + 1)
            .unwrap_or(0),
    )
}

fn trim_before_last_marker_line<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    marker_line_start(text, marker).map(|line_start| &text[..line_start])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RunAsConfig;

    /// build_tmux_invocation: no explicit socket → socket defaults to the run id.
    #[test]
    fn build_invocation_defaults_to_run_scoped_socket() {
        let cfg = RunAsConfig::default();
        let inv = build_tmux_invocation(&cfg, "run_123");
        assert!(
            inv.prefix.is_empty(),
            "expected empty prefix for default RunAsConfig, got {:?}",
            inv.prefix
        );
        assert_eq!(
            inv.socket.as_deref(),
            Some("silverbond-run_123"),
            "socket should default to a run-scoped silverbond socket"
        );
    }

    /// build_tmux_invocation: `command` field propagates verbatim as prefix.
    #[test]
    fn build_invocation_command_prefix_is_verbatim() {
        let cfg = RunAsConfig {
            command: Some(vec![
                "docker".to_string(),
                "exec".to_string(),
                "-it".to_string(),
                "sandbox".to_string(),
            ]),
            user: None,
            socket: None,
        };
        let inv = build_tmux_invocation(&cfg, "run_command");
        assert_eq!(
            inv.prefix,
            vec!["docker", "exec", "-it", "sandbox"],
            "command should be used verbatim as prefix"
        );
        assert_eq!(inv.socket.as_deref(), Some("silverbond-run_command"));
    }

    /// build_tmux_invocation: `user` field synthesizes `["sudo", "-u", <user>, "-H", "--"]`.
    #[test]
    fn build_invocation_user_synthesizes_sudo_prefix() {
        let cfg = RunAsConfig {
            user: Some("agent".to_string()),
            command: None,
            socket: None,
        };
        let inv = build_tmux_invocation(&cfg, "run_user");
        assert_eq!(
            inv.prefix,
            vec!["sudo", "-u", "agent", "-H", "--"],
            "user should synthesize sudo prefix"
        );
        assert_eq!(inv.socket.as_deref(), Some("silverbond-run_user"));
    }

    /// build_tmux_invocation: explicit socket overrides the default.
    #[test]
    fn build_invocation_explicit_socket_is_preserved() {
        let cfg = RunAsConfig {
            user: None,
            command: None,
            socket: Some("my-socket".to_string()),
        };
        let inv = build_tmux_invocation(&cfg, "run_explicit");
        assert_eq!(inv.socket.as_deref(), Some("my-socket"));
    }

    /// build_tmux_invocation: `command` takes precedence over `user` when both are set.
    #[test]
    fn build_invocation_command_takes_precedence_over_user() {
        let cfg = RunAsConfig {
            command: Some(vec!["custom".to_string()]),
            user: Some("agent".to_string()),
            socket: None,
        };
        let inv = build_tmux_invocation(&cfg, "run_precedence");
        assert_eq!(
            inv.prefix,
            vec!["custom"],
            "command should take precedence over user"
        );
    }

    #[test]
    fn resolve_tmux_bin_sentinel_parse_ignores_stdout_banners() {
        let output = b"Welcome from zshrc\nSBTMUX:/opt/homebrew/bin/tmux\n";

        assert_eq!(
            parse_tmux_lookup_output(output).as_deref(),
            Some("/opt/homebrew/bin/tmux")
        );
    }

    #[test]
    fn resolve_tmux_bin_sentinel_parse_treats_empty_lookup_as_not_found() {
        let output = b"Welcome from zshrc\nSBTMUX:\n";

        assert_eq!(parse_tmux_lookup_output(output), None);
    }

    #[cfg(unix)]
    #[test]
    fn resolve_tmux_bin_timeout_falls_back_to_tmux() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let script = temp.path().join("blocking-tmux-lookup.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let started = Instant::now();
        let resolved = resolve_tmux_bin_with_timeout(
            &[script.to_string_lossy().into_owned()],
            Duration::from_millis(50),
        );

        assert_eq!(resolved, "tmux");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "blocking tmux lookup should return promptly"
        );
    }

    #[test]
    fn unique_session_name_uses_full_uuid_suffix() {
        let session_name = unique_session_name(Some("Reviewer A!"));
        let prefix = "silverbond-Reviewer-A-";
        assert!(
            session_name.starts_with(prefix),
            "session name should include sanitized base; got {session_name}"
        );

        let suffix = &session_name[prefix.len()..];
        assert_eq!(
            suffix.len(),
            32,
            "session suffix should keep the full compact UUID"
        );
        assert!(
            suffix.chars().all(|ch| ch.is_ascii_hexdigit()),
            "session suffix should be compact hex UUID; got {suffix}"
        );
    }

    #[test]
    fn unique_session_name_does_not_collide_in_bursts() {
        let mut names = std::collections::HashSet::new();
        for _ in 0..32 {
            assert!(
                names.insert(unique_session_name(Some("parallel-batch"))),
                "burst-generated unnamed tmux sessions must be unique"
            );
        }
    }

    #[test]
    fn stable_session_name_sanitizes_prefixes_and_is_idempotent() {
        assert_eq!(stable_session_name("Reviewer A!"), "silverbond-Reviewer-A");
        assert_eq!(
            stable_session_name("silverbond-Reviewer-A"),
            "silverbond-Reviewer-A"
        );
        assert_eq!(stable_session_name("!!!"), "silverbond-run");
    }

    #[cfg(unix)]
    #[test]
    fn spawn_pane_normalizes_explicit_session_name() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let script = temp.path().join("tmux-spawn-prefix.sh");
        let log = temp.path().join("tmux-spawn-args.log");
        std::fs::write(
            &script,
            "#!/bin/sh\nlog=\"$1\"\nshift\nprintf '%s\\n' \"$@\" >> \"$log\"\nif [ \"$1\" = \"tmux\" ]; then shift; fi\nif [ \"$1\" = \"-L\" ]; then shift 2; fi\nif [ \"$1\" = \"new-session\" ]; then printf '%%spawned-pane\\n'; fi\nexit 0\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let invocation = tmux_tools_core::TmuxInvocation {
            prefix: vec![
                script.to_string_lossy().into_owned(),
                log.to_string_lossy().into_owned(),
            ],
            socket: Some("spawn-test-socket".to_string()),
            tmux_bin: "tmux".to_string(),
        };
        let cfg = SpawnConfig {
            command: Some("true".to_string()),
            session_name: Some("Reviewer A!".to_string()),
            ..Default::default()
        };

        let spawned = tmux_tools_core::with_invocation(invocation, || {
            spawn_pane(Some(&cfg), None, "", None, None, None)
        })
        .unwrap();

        assert_eq!(spawned.session_name, "silverbond-Reviewer-A");
        let recorded =
            std::fs::read_to_string(log).expect("fake tmux prefix should record spawn args");
        let args = recorded.lines().collect::<Vec<_>>();
        assert!(
            args.windows(5)
                .any(|window| window == ["new-session", "-d", "-s", "silverbond-Reviewer-A", "-P"]),
            "explicit session name should be normalized before tmux spawn; args={args:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn send_text_terminates_options_before_literal_text() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let script = temp.path().join("tmux-send-prefix.sh");
        let log = temp.path().join("tmux-send-args.log");
        std::fs::write(
            &script,
            "#!/bin/sh\nlog=\"$1\"\nshift\nprintf '%s\\n' \"$@\" >> \"$log\"\nexit 0\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let invocation = tmux_tools_core::TmuxInvocation {
            prefix: vec![
                script.to_string_lossy().into_owned(),
                log.to_string_lossy().into_owned(),
            ],
            socket: Some("send-test-socket".to_string()),
            tmux_bin: "tmux".to_string(),
        };
        tmux_tools_core::with_invocation(invocation, || send_text("%pane-1", "-N", false)).unwrap();

        let recorded =
            std::fs::read_to_string(log).expect("fake tmux prefix should record send args");
        let args = recorded.lines().collect::<Vec<_>>();
        assert!(
            args.windows(6)
                .any(|window| window == ["send-keys", "-t", "%pane-1", "-l", "--", "-N"]),
            "literal text that starts with '-' must appear after an end-of-options marker; args={args:?}"
        );
    }

    #[test]
    fn build_agent_command_appends_resolved_session_safety_args() {
        let spawn_cfg = SpawnConfig {
            extra_args: vec!["--tail-flag".to_string()],
            ..Default::default()
        };
        let agent_config = AgentConfig {
            model: Some("claude-test-model".to_string()),
            allowed_tools: Some(vec!["Read".to_string(), "Grep".to_string()]),
            disallowed_tools: Some(vec!["Bash(rm *)".to_string()]),
            ..AgentConfig::default()
        };

        let built = build_agent_command("claude", Some(&spawn_cfg), Some(&agent_config)).unwrap();

        assert!(built.command.contains("'--model' 'claude-test-model'"));
        assert!(built.command.contains("'--allowedTools' 'Read'"));
        assert!(built.command.contains("'--allowedTools' 'Grep'"));
        assert!(built.command.contains("'--disallowedTools' 'Bash(rm *)'"));
        assert!(
            !built.command.contains("'--permission-mode'"),
            "fine-grained tool control should replace the access-mode shorthand"
        );
        assert!(
            built.command.find("'--model'").unwrap() < built.command.find("'--tail-flag'").unwrap(),
            "driver-rendered session args should precede caller extra args"
        );
        assert!(built.env.is_empty());
    }

    #[test]
    fn build_agent_command_uses_driver_access_args_without_registry_duplicate() {
        let agent_config = AgentConfig {
            access_mode: AccessMode::ReadOnly,
            ..AgentConfig::default()
        };

        let built = build_agent_command("codex", None, Some(&agent_config)).unwrap();

        assert!(!built.command.contains("'--model'"));
        assert!(built.command.contains("'exec'"));
        assert!(built.command.contains("'--sandbox' 'read-only'"));
        assert_eq!(
            built.command.matches("'--sandbox'").count(),
            1,
            "registry launch args and driver session args must not both emit access flags"
        );
    }

    #[test]
    fn wrap_keep_open_runs_follow_up_shell_after_short_lived_command() {
        if !zsh_available() {
            return;
        }
        let temp = tempfile::TempDir::new().unwrap();
        for file in [".zshenv", ".zprofile", ".zshrc", ".zlogin"] {
            std::fs::write(temp.path().join(file), "").unwrap();
        }
        let cwd = temp.path().to_string_lossy();
        let wrapped = wrap_keep_open("true", &cwd);
        let mut command = std::process::Command::new("zsh");
        command
            .arg("-lc")
            .arg(wrapped)
            .env("ZDOTDIR", temp.path())
            .env("PS1", "")
            .env("PROMPT", "")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        // Detach from any controlling terminal so the interactive follow-up shell reads
        // commands from the stdin pipe instead of attempting tty/job-control reads. Without
        // this, running the suite from a tmux pane (where the test process isn't the tty's
        // foreground process group) makes zsh's tty read fail with EIO. setsid() on the forked,
        // non-leader child always succeeds and reproduces the clean headless condition.
        unsafe {
            use std::os::unix::process::CommandExt;
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();

        let mut stdin = child.stdin.take().unwrap();
        use std::io::Write;
        stdin
            .write_all(b"printf '%s\\n' keep-open-proof\nexit\n")
            .unwrap();
        drop(stdin);

        let output = wait_for_child_output(child, Duration::from_secs(3));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stdout.contains("keep-open-proof"),
            "short-lived command should return to the keep-open shell; stdout={stdout:?} stderr={stderr:?}"
        );
    }

    fn zsh_available() -> bool {
        std::process::Command::new("zsh")
            .arg("-c")
            .arg("true")
            .status()
            .is_ok_and(|status| status.success())
    }

    fn tmux_available() -> bool {
        tmux::run(&["list-sessions"]).is_ok()
    }

    fn tmux_session_exists(session_name: &str) -> bool {
        tmux::run(&["has-session", "-t", session_name]).is_ok_and(|output| output.exit_code == 0)
    }

    #[test]
    fn session_guard_kills_session_on_drop_without_disarm() {
        if !tmux_available() {
            return;
        }
        let session_name = format!("silverbond-test-guard-{}", uuid::Uuid::now_v7().simple());
        tmux::run_checked(&["new-session", "-d", "-s", &session_name, "sleep", "600"]).unwrap();
        assert!(tmux_session_exists(&session_name));

        {
            let _guard = SessionGuard::new(session_name.clone());
        }

        assert!(
            !tmux_session_exists(&session_name),
            "SessionGuard should kill the session on drop when not disarmed"
        );
    }

    #[test]
    fn session_guard_disarm_prevents_kill_on_drop() {
        if !tmux_available() {
            return;
        }
        let session_name = format!("silverbond-test-disarm-{}", uuid::Uuid::now_v7().simple());
        tmux::run_checked(&["new-session", "-d", "-s", &session_name, "sleep", "600"]).unwrap();

        {
            let mut guard = SessionGuard::new(session_name.clone());
            guard.disarm();
        }

        assert!(
            tmux_session_exists(&session_name),
            "disarmed SessionGuard must not kill the session on drop"
        );
        let _ = tmux::run(&["kill-session", "-t", &session_name]);
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_panes_kills_registered_sessions_and_pane_fallbacks() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let script = temp.path().join("tmux-cleanup-prefix.sh");
        let log = temp.path().join("tmux-cleanup-args.log");
        std::fs::write(
            &script,
            "#!/bin/sh\nlog=\"$1\"\nshift\nprintf '%s\\n' \"$@\" >> \"$log\"\nexit 0\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let invocation = tmux_tools_core::TmuxInvocation {
            prefix: vec![
                script.to_string_lossy().into_owned(),
                log.to_string_lossy().into_owned(),
            ],
            socket: Some("cleanup-test-socket".to_string()),
            tmux_bin: "tmux".to_string(),
        };
        tmux_tools_core::with_invocation(invocation, || {
            cleanup_panes(&[
                PaneCleanupTarget::new(
                    "%ephemeral-pane".to_string(),
                    Some("ephemeral-session".to_string()),
                ),
                PaneCleanupTarget::new("%external-pane".to_string(), None),
            ])
        })
        .unwrap();

        let recorded =
            std::fs::read_to_string(log).expect("fake tmux prefix should record cleanup args");
        let args = recorded.lines().collect::<Vec<_>>();
        assert!(
            args.windows(3)
                .any(|window| window == ["kill-session", "-t", "ephemeral-session"]),
            "ephemeral sessions should be killed by session name; args={args:?}"
        );
        assert!(
            args.windows(3)
                .any(|window| window == ["kill-pane", "-t", "%external-pane"]),
            "external/reused panes should fall back to kill-pane; args={args:?}"
        );
    }

    fn wait_for_child_output(
        mut child: std::process::Child,
        timeout: Duration,
    ) -> std::process::Output {
        let started = Instant::now();
        loop {
            if child.try_wait().unwrap().is_some() {
                return child.wait_with_output().unwrap();
            }
            if started.elapsed() >= timeout {
                let _ = child.kill();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "timed out waiting for keep-open shell; stdout={:?} stderr={:?}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn permission_request_without_interaction_denies_by_default() {
        let reply = permission_request_reply(
            None,
            "%1",
            "Tool permission prompt",
            "Allow this action? [y/n]",
            false,
            false,
        )
        .unwrap();
        assert_eq!(reply, "n");
    }

    #[test]
    fn permission_request_auto_approve_still_allows_non_destructive_prompt() {
        let reply = permission_request_reply(
            None,
            "%1",
            "Tool permission prompt",
            "Allow this action? [y/n]",
            false,
            true,
        )
        .unwrap();
        assert_eq!(reply, "y");
    }

    #[cfg(unix)]
    #[test]
    fn continuous_subagent_markers_are_bounded_by_absolute_deadline() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let script = temp.path().join("tmux-subagent-timeout-prefix.sh");
        let state = temp.path().join("capture-count");
        std::fs::write(
            &script,
            r#"#!/bin/sh
state="$1"
shift
if [ "$1" = "tmux" ]; then shift; fi
if [ "$1" = "-L" ]; then shift 2; fi
cmd="$1"

case "$cmd" in
  display-message)
    printf '\037\037\037\037\n'
    ;;
  capture-pane)
    count=0
    if [ -f "$state" ]; then count=$(cat "$state"); fi
    count=$((count + 1))
    printf '%s\n' "$count" > "$state"
    suffix=""
    i=0
    while [ "$i" -lt "$count" ]; do
      suffix="${suffix}x"
      i=$((i + 1))
    done
    printf 'Task prompt\nSubagent active %s\n' "$suffix"
    ;;
  send-keys)
    ;;
esac
exit 0
"#,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let invocation = tmux_tools_core::TmuxInvocation {
            prefix: vec![
                script.to_string_lossy().into_owned(),
                state.to_string_lossy().into_owned(),
            ],
            socket: Some("subagent-timeout-test-socket".to_string()),
            tmux_bin: "tmux".to_string(),
        };
        let interaction_patterns = vec![CompiledInteractionPattern {
            regex: Regex::new(r"Subagent active x+").unwrap(),
            kind: InteractionKind::SubagentActive,
            description: "Subagent active".to_string(),
            send_enter: true,
        }];
        let abort = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let abort_worker = Arc::clone(&abort);
        let finished_worker = Arc::clone(&finished);
        let abort_thread = std::thread::spawn(move || {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(5)
                && !finished_worker.load(Ordering::SeqCst)
            {
                sleep(Duration::from_millis(10));
            }
            if !finished_worker.load(Ordering::SeqCst) {
                abort_worker.store(true, Ordering::SeqCst);
            }
        });

        let started = Instant::now();
        let result = tmux_tools_core::with_invocation(invocation, || {
            poll_agent_interactive(
                "%subagent-test",
                "Task prompt\n",
                "Task prompt",
                Duration::from_millis(10),
                60.0,
                0.0,
                None,
                None,
                false,
                Duration::from_millis(5),
                None,
                &interaction_patterns,
                &[],
                None,
                Some(&abort),
            )
        })
        .unwrap();
        finished.store(true, Ordering::SeqCst);
        abort_thread.join().unwrap();

        assert!(
            matches!(result, InteractivePollResult::TimedOut(_)),
            "continuous SubagentActive matches should hit the absolute deadline before the abort guard"
        );
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "absolute deadline should bound total polling time"
        );
    }

    #[cfg(unix)]
    #[test]
    fn auto_approve_is_only_used_after_workflow_prompt_is_sent() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().unwrap();
        let script = temp.path().join("tmux-auto-approve-prefix.sh");
        let log = temp.path().join("send-log");
        let phase = temp.path().join("phase");
        let prompt = temp.path().join("prompt");
        std::fs::write(
            &script,
            r#"#!/bin/sh
log="$1"
phase="$2"
prompt_file="$3"
shift 3
if [ "$1" = "tmux" ]; then shift; fi
if [ "$1" = "-L" ]; then shift 2; fi
cmd="$1"

case "$cmd" in
  new-session)
    printf '%%auto-approve-test\n'
    ;;
  display-message)
    printf '\037codex\037\037\037\n'
    ;;
  set-option|kill-session|kill-pane)
    ;;
  capture-pane)
    if [ -f "$phase" ]; then
      cat "$prompt_file"
      printf '\nAllow this action now [y/n]\nDONE\n'
    else
      printf 'Startup banner\nAllow this action [y/n]\n'
    fi
    ;;
  send-keys)
    shift
    literal=""
    previous=""
    for arg in "$@"; do
      if [ "$previous" = "--" ]; then
        literal="$arg"
        break
      fi
      previous="$arg"
    done
    if [ -n "$literal" ]; then
      printf '%s\n' "$literal" >> "$log"
      case "$literal" in
        y|n) ;;
        *)
          printf '%s\n' "$literal" > "$prompt_file"
          printf 'sent\n' > "$phase"
          ;;
      esac
    else
      printf 'ENTER\n' >> "$log"
    fi
    ;;
esac
exit 0
"#,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let invocation = tmux_tools_core::TmuxInvocation {
            prefix: vec![
                script.to_string_lossy().into_owned(),
                log.to_string_lossy().into_owned(),
                phase.to_string_lossy().into_owned(),
                prompt.to_string_lossy().into_owned(),
            ],
            socket: Some("auto-approve-phase-test-socket".to_string()),
            tmux_bin: "tmux".to_string(),
        };
        let run_cfg = RunAgentConfig {
            timeout: Some(2),
            idle_seconds: Some(0.01),
            ready_stable_seconds: Some(0.0),
            until: Some("DONE".to_string()),
            ..RunAgentConfig::default()
        };
        let agent_cfg = AgentConfig {
            auto_approve: true,
            ..AgentConfig::default()
        };

        let result = tmux_tools_core::with_invocation(invocation, || {
            run_agent_interactive(
                "codex".to_string(),
                "Workflow prompt".to_string(),
                temp.path().to_string_lossy().into_owned(),
                Some(2),
                Some(&run_cfg),
                Some(&agent_cfg),
                None,
                None,
                None,
            )
        })
        .unwrap();
        assert!(
            result.success,
            "fake post-prompt DONE marker should complete"
        );

        let sent = std::fs::read_to_string(log).unwrap();
        let decisions = sent
            .lines()
            .filter(|line| *line == "y" || *line == "n")
            .collect::<Vec<_>>();
        assert_eq!(
            decisions,
            vec!["n", "y"],
            "startup permission prompts should not use auto-approve, but post-prompt prompts should"
        );
    }

    #[test]
    fn cumulative_destructive_scan_catches_split_command_once() {
        let regexes = vec![Regex::new(r"rm\s+-rf\s+\S+").unwrap()];
        let mut handled = HandledDestructiveMatches::default();

        assert!(
            next_unhandled_destructive_match(&regexes, "rm -r", &handled).is_none(),
            "partial split command should not match yet"
        );

        let matched = next_unhandled_destructive_match(&regexes, "rm -rf /tmp/project", &handled)
            .expect("complete cumulative output should match");
        handled.record(matched);

        assert!(
            next_unhandled_destructive_match(&regexes, "rm -rf /tmp/project", &handled).is_none(),
            "same destructive line should escalate once"
        );
    }

    #[test]
    fn cumulative_destructive_scan_catches_repainted_completion() {
        let regexes = vec![Regex::new(r"rm\s+-rf\s+\S+").unwrap()];
        let previous = "rm -r\npermission UI";
        let current = "rm -rf /tmp/project\npermission UI";
        let delta = capture_delta(previous, current);
        assert!(
            !regexes[0].is_match(&delta),
            "delta-only scan should miss a destructive command completed before the suffix"
        );

        let mut handled = HandledDestructiveMatches::default();
        let matched = next_unhandled_destructive_match(&regexes, current, &handled)
            .expect("cumulative visible output should match");
        handled.record(matched);
        assert!(
            next_unhandled_destructive_match(&regexes, current, &handled).is_none(),
            "repainted destructive line should not escalate twice"
        );
    }

    #[test]
    fn markerless_visual_idle_is_not_interactive_completion() {
        assert!(!is_interactive_completion_reason(IdleReason::Idle));
    }

    #[test]
    fn only_affirmative_signals_complete_interactive_polling() {
        assert!(is_interactive_completion_reason(IdleReason::ReadyMatched));
        assert!(is_interactive_completion_reason(IdleReason::UntilMatched));
        assert!(!is_interactive_completion_reason(IdleReason::Idle));
        assert!(!is_interactive_completion_reason(IdleReason::TimedOut));
    }

    #[test]
    fn idle_does_not_auto_complete_and_requires_escalation() {
        let now = Instant::now();
        let mut progress = CaptureProgress::new("frozen shell prompt", now);
        let first_reason = progress.observe(
            "frozen shell prompt",
            now,
            2.0,
            &None,
            DEFAULT_READY_SCAN_LINES,
            0.0,
            &[],
        );
        let reason = progress.observe(
            "frozen shell prompt",
            now + Duration::from_secs(3),
            2.0,
            &None,
            DEFAULT_READY_SCAN_LINES,
            0.0,
            &[],
        );

        assert_eq!(first_reason, None);
        assert!(matches!(reason, Some(IdleReason::Idle)));
        assert!(!is_interactive_completion_reason(reason.unwrap()));
    }

    #[test]
    fn capture_progress_reports_until_marker_match() {
        let now = Instant::now();
        let mut progress = CaptureProgress::new("working", now);
        let until_regexes = vec![Regex::new("DONE").unwrap()];
        let reason = progress.observe(
            "answer\nDONE",
            now,
            2.0,
            &None,
            DEFAULT_READY_SCAN_LINES,
            0.0,
            &until_regexes,
        );
        assert_eq!(reason, Some(IdleReason::UntilMatched));
    }

    #[test]
    fn prompt_capture_wrapper_does_not_echo_response_marker_literal() {
        let markers = PromptCaptureMarkers {
            prompt_end: "SB_PROMPT_END_test".to_string(),
            response_end: "SB_RESPONSE_DONE_test".to_string(),
        };
        let wrapped = wrap_prompt_for_capture_markers("Choose an outcome", &markers);
        assert!(wrapped.contains(&markers.prompt_end));
        assert!(
            !wrapped.contains(&markers.response_end),
            "the exact response sentinel must not appear in the echoed prompt"
        );
    }

    #[test]
    fn extract_after_prompt_uses_sentinels_for_multiline_prompt() {
        let before = "ready\n";
        let prompt = "Choose one:\n- approve\n- reject";
        let after = concat!(
            "ready\n",
            "Choose one:\n",
            "- approve\n",
            "- reject\n",
            "SB_PROMPT_END_test\n",
            "reject\n",
            "SB_RESPONSE_DONE_test\n"
        );

        let output = extract_after_prompt_with_sentinels(
            before,
            after,
            prompt,
            Some("SB_PROMPT_END_test"),
            Some("SB_RESPONSE_DONE_test"),
        );

        assert_eq!(output.trim(), "reject");
        assert!(!output.contains("- approve"));
    }

    #[test]
    fn extract_after_prompt_uses_sentinels_for_wrapped_tui_prompt() {
        let before = "ready\n";
        let prompt = "Choose one:\n- approve\n- reject";
        let after = concat!(
            "ready\n",
            "Choose one:\n",
            "- appro\n",
            "ve\n",
            "- reje\n",
            "ct\n",
            "SB_PROMPT_END_test\n",
            "approve\n",
            "SB_RESPONSE_DONE_test\n"
        );

        let output = extract_after_prompt_with_sentinels(
            before,
            after,
            prompt,
            Some("SB_PROMPT_END_test"),
            Some("SB_RESPONSE_DONE_test"),
        );

        assert_eq!(output.trim(), "approve");
        assert!(!output.contains("reject"));
    }
}
