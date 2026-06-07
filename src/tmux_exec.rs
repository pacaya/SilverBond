use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use futures::future::BoxFuture;
use regex::Regex;
use serde_json::{Value, json};
use tmux_tools_core::{
    agents,
    idle::{
        DEFAULT_READY_SCAN_LINES, DEFAULT_READY_STABLE_SECONDS, IdleConfig, IdleReason,
        wait_for_idle,
    },
    names, target, tmux,
};

use crate::{
    driver::{AccessMode, AgentConfig, NodeOutcome},
    model::{
        CaptureConfig, RunAgentConfig, SpawnConfig, WaitConfig, WaitMode, WorkflowNode,
        WorkflowNodeType,
    },
    runtime::{AgentExecutionMetadata, NodeResult, NodeRunner, RuntimeContext},
};

#[derive(Debug, Clone, Default)]
pub(crate) struct TmuxNodeRunner;

impl TmuxNodeRunner {
    pub(crate) fn new() -> Self {
        Self
    }
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
                run_agent_sequence(
                    agent,
                    prompt,
                    cwd,
                    timeout_secs,
                    None,
                    config.as_ref(),
                    None,
                )
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
    ) -> BoxFuture<'static, anyhow::Result<NodeResult>> {
        let active_pane = ActivePaneRegistration::new(&ctx, run_id, node.id.clone());
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                run_tmux_node(
                    node,
                    agent,
                    prompt,
                    cwd,
                    timeout_secs,
                    config.as_ref(),
                    previous_output,
                    Some(active_pane),
                )
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
) -> anyhow::Result<NodeResult> {
    match node.node_type {
        WorkflowNodeType::Task | WorkflowNodeType::RunAgent => run_agent_sequence(
            agent,
            prompt,
            cwd,
            timeout_secs,
            node.run_agent_config.as_ref(),
            config,
            active_pane.as_ref(),
        ),
        WorkflowNodeType::Spawn => execute_spawn(&node, &agent, &cwd, active_pane.as_ref()),
        WorkflowNodeType::Send => {
            execute_send(&node, &prompt, &previous_output, active_pane.as_ref())
        }
        WorkflowNodeType::Wait => {
            execute_wait(&node, timeout_secs, &previous_output, active_pane.as_ref())
        }
        WorkflowNodeType::Capture => execute_capture(&node, &previous_output, active_pane.as_ref()),
        WorkflowNodeType::Kill => execute_kill(&node, &previous_output, active_pane.as_ref()),
        WorkflowNodeType::Approval
        | WorkflowNodeType::Split
        | WorkflowNodeType::Collector
        | WorkflowNodeType::Decide
        | WorkflowNodeType::ParallelBatch
        | WorkflowNodeType::Subflow
        | WorkflowNodeType::Call => Err(anyhow!(
            "tmux runner cannot execute {} nodes directly",
            node.node_type.as_str()
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

#[derive(Clone)]
struct ActivePaneRegistration {
    registry: crate::runtime::RunRegistry,
    run_id: String,
    key: String,
    handle: tokio::runtime::Handle,
}

impl ActivePaneRegistration {
    fn new(ctx: &RuntimeContext, run_id: String, key: String) -> Self {
        Self {
            registry: ctx.registry.clone(),
            run_id,
            key,
            handle: tokio::runtime::Handle::current(),
        }
    }

    fn set(&self, target: &str) {
        let registry = self.registry.clone();
        let run_id = self.run_id.clone();
        let key = self.key.clone();
        let target = target.to_string();
        self.handle.block_on(async move {
            registry.set_active_pane(&run_id, &key, &target).await;
        });
    }

    fn clear_key(&self) {
        let registry = self.registry.clone();
        let run_id = self.run_id.clone();
        let key = self.key.clone();
        self.handle.block_on(async move {
            registry.clear_active_pane(&run_id, &key).await;
        });
    }

    fn clear_target(&self, target: &str) {
        let registry = self.registry.clone();
        let run_id = self.run_id.clone();
        let target = target.to_string();
        self.handle.block_on(async move {
            registry.clear_active_pane_target(&run_id, &target).await;
        });
    }
}

fn execute_spawn(
    node: &WorkflowNode,
    default_agent: &str,
    default_cwd: &str,
    active_pane: Option<&ActivePaneRegistration>,
) -> anyhow::Result<NodeResult> {
    let start = Instant::now();
    let cfg = node.spawn_config.as_ref();
    let has_command = cfg
        .and_then(|cfg| cfg.command.as_deref())
        .is_some_and(|command| !command.trim().is_empty());
    let agent = cfg
        .and_then(|cfg| cfg.agent.clone())
        .or_else(|| node.agent.clone())
        .or_else(|| (!has_command).then(|| default_agent.to_owned()));
    let cwd = cfg
        .and_then(|cfg| cfg.cwd.as_deref())
        .or(node.cwd.as_deref())
        .unwrap_or(default_cwd);
    let spawned = spawn_pane(cfg, agent.as_deref(), cwd, None, None, None)?;
    if let Some(active_pane) = active_pane {
        active_pane.set(&spawned.pane_id);
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
    resolved_prompt: &str,
    previous_output: &str,
    active_pane: Option<&ActivePaneRegistration>,
) -> anyhow::Result<NodeResult> {
    let start = Instant::now();
    let cfg = node.send_config.clone().unwrap_or_default();
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
    timeout_secs: Option<u64>,
    previous_output: &str,
    active_pane: Option<&ActivePaneRegistration>,
) -> anyhow::Result<NodeResult> {
    let start = Instant::now();
    let cfg = node.wait_config.clone().unwrap_or_default();
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
    previous_output: &str,
    active_pane: Option<&ActivePaneRegistration>,
) -> anyhow::Result<NodeResult> {
    let start = Instant::now();
    let cfg = node.capture_config.clone().unwrap_or_default();
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
    previous_output: &str,
    active_pane: Option<&ActivePaneRegistration>,
) -> anyhow::Result<NodeResult> {
    let start = Instant::now();
    let cfg = node.kill_config.clone().unwrap_or_default();
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
        tmux::run_checked(&["kill-session", "-t", session])?;
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

fn run_agent_sequence(
    agent: String,
    prompt: String,
    cwd: String,
    timeout_secs: Option<u64>,
    cfg: Option<&RunAgentConfig>,
    config: Option<&AgentConfig>,
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

    let spawned = spawn_pane(
        Some(&spawn_cfg),
        Some(&effective_agent),
        &effective_cwd,
        config,
        None,
        None,
    )?;
    if let Some(active_pane) = active_pane {
        active_pane.set(&spawned.pane_id);
    }

    let result = (|| {
        if !echo_mode {
            let ready_cfg = WaitConfig {
                mode: WaitMode::Ready,
                timeout,
                idle_seconds: cfg.and_then(|cfg| cfg.idle_seconds),
                ready_stable_seconds: cfg.and_then(|cfg| cfg.ready_stable_seconds),
                ..Default::default()
            };
            let ready = wait_for_mode(&spawned.pane_id, &ready_cfg, timeout)?;
            if ready.reason == IdleReason::TimedOut {
                return Ok(failed_result(
                    "Timed out waiting for tmux agent readiness",
                    -2,
                    &effective_agent,
                    &effective_prompt,
                    start.elapsed(),
                    Some(spawned.pane_id.clone()),
                    Some(NodeOutcome::ErrorTimeout),
                ));
            }

            let before = capture_visible_stripped(&spawned.pane_id)?;
            send_text(&spawned.pane_id, &effective_prompt, true)?;
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
            let waited = wait_for_mode(&spawned.pane_id, &wait_cfg, timeout)?;
            if waited.reason == IdleReason::TimedOut {
                return Ok(failed_result(
                    "Timed out waiting for tmux agent response",
                    -2,
                    &effective_agent,
                    &effective_prompt,
                    start.elapsed(),
                    Some(spawned.pane_id.clone()),
                    Some(NodeOutcome::ErrorTimeout),
                ));
            }
            let after = capture_visible_stripped(&spawned.pane_id)?;
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
                    agent_session_id: Some(spawned.pane_id.clone()),
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
        let waited = wait_for_mode(&spawned.pane_id, &wait_cfg, timeout)?;
        if waited.reason == IdleReason::TimedOut {
            return Ok(failed_result(
                "Timed out waiting for tmux command",
                -2,
                &effective_agent,
                &effective_prompt,
                start.elapsed(),
                Some(spawned.pane_id.clone()),
                Some(NodeOutcome::ErrorTimeout),
            ));
        }
        let output = capture_visible_stripped(&spawned.pane_id)?;
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
                agent_session_id: Some(spawned.pane_id.clone()),
                ..Default::default()
            },
            ..Default::default()
        })
    })();

    if cfg.map(|cfg| cfg.kill_after).unwrap_or(true) {
        let _ = tmux::run(&["kill-session", "-t", &spawned.session_name]);
        if let Some(active_pane) = active_pane {
            active_pane.clear_key();
        }
    }

    result
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
    let command = command_override
        .or_else(|| cfg.and_then(|cfg| cfg.command.clone()))
        .map(Ok)
        .unwrap_or_else(|| {
            let agent = agent
                .as_deref()
                .ok_or_else(|| anyhow!("spawn requires an agent or command"))?;
            build_agent_command(agent, cfg, agent_config)
        })?;
    let session_name = session_override
        .or_else(|| cfg.and_then(|cfg| cfg.session_name.clone()))
        .unwrap_or_else(|| unique_session_name(cfg.and_then(|cfg| cfg.name.as_deref())));
    let work_dir = cfg
        .and_then(|cfg| cfg.cwd.as_deref())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(cwd);
    let command = wrap_keep_open(&command);

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
    args.push(command.clone());

    let pane_id = tmux::run_checked_owned(&args)?.trim().to_owned();
    if pane_id.is_empty() {
        return Err(anyhow!("tmux new-session returned an empty pane id"));
    }

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
) -> anyhow::Result<String> {
    let extra_args = cfg.map(|cfg| cfg.extra_args.as_slice()).unwrap_or(&[]);
    let explicit_access = cfg.and_then(|cfg| cfg.access.as_deref());
    let registry = agents::Registry::load()?;
    let profile = explicit_access
        .map(str::to_owned)
        .or_else(|| access_profile_from_config(agent_config));

    let argv = match registry.launch_argv(agent, profile.as_deref()) {
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

    Ok(argv
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" "))
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
    let registry = agents::Registry::load()?;
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
    tmux::run_checked(&["send-keys", "-t", pane, "-l", text])?;
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
    tmux::run_checked(&["capture-pane", "-t", pane, "-p"])
}

fn resolve_pane_target(configured: Option<&str>, previous_output: &str) -> anyhow::Result<String> {
    let previous = parse_previous(previous_output);
    let raw = configured
        .or_else(|| previous_value(&previous, &["paneId", "pane_id", "target"]))
        .or_else(|| {
            let trimmed = previous_output.trim();
            (!trimmed.is_empty()).then_some(trimmed)
        })
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

fn duration_string(duration: Duration) -> String {
    format!("{:.1}", duration.as_secs_f64())
}

fn unique_session_name(name: Option<&str>) -> String {
    let suffix = uuid::Uuid::now_v7().simple().to_string();
    let base = name
        .map(sanitize_tmux_name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "run".to_owned());
    format!("silverbond-{base}-{}", &suffix[..8])
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

fn wrap_keep_open(cmd: &str) -> String {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".to_owned());
    format!("({cmd}); exec {}", shell_quote(&shell))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn extract_after_prompt(before: &str, after: &str, prompt_text: &str) -> String {
    let before_lines = before.lines().collect::<std::collections::HashSet<_>>();
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
