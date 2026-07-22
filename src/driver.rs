//! Agent Abstraction Layer — driver trait and implementations for CLI-based agent backends.
//!
//! Each agent backend (Claude, Codex, etc.) is represented by an [`AgentDriver`] implementation
//! that knows how to build CLI arguments for interactive PTY sessions and parse agent output.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tmux_tools_core::agents;

use crate::pty_output::{ContextInfo, CostInfo};

// ---------------------------------------------------------------------------
// Interaction types (for interactive prompt handling)
// ---------------------------------------------------------------------------

/// The kind of interactive prompt detected from PTY output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionKind {
    /// Known prompt with a deterministic response — auto-send immediately.
    AutoRespond { response: String },
    /// Agent is requesting permission to perform an action.
    PermissionRequest,
    /// Agent has spawned a subagent; silence is expected.
    SubagentActive,
    /// A destructive pattern was detected — always escalate to human.
    DestructiveWarning,
}

impl InteractionKind {
    /// Event type string for serialization to the frontend.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::AutoRespond { .. } => "auto_respond",
            Self::PermissionRequest => "permission",
            Self::SubagentActive => "subagent_active",
            Self::DestructiveWarning => "destructive_warning",
        }
    }
}

/// A regex pattern that matches interactive prompts in PTY output.
#[derive(Debug, Clone)]
pub struct InteractionPattern {
    pub kind: InteractionKind,
    pub pattern: String,
    pub description: String,
    /// When false, reply keystrokes are sent without a trailing Enter (menu-style prompts).
    pub send_enter: bool,
}

impl InteractionPattern {
    pub fn new(
        kind: InteractionKind,
        pattern: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            pattern: pattern.into(),
            description: description.into(),
            send_enter: true,
        }
    }
}

/// Shared destructive patterns common to all agent backends.
pub fn shared_destructive_patterns() -> &'static [&'static str] {
    &[
        r"rm\s+-rf",
        r"(?i)drop\s+(table|database)",
        r"(?i)force[- ]push",
        r"(?i)git\s+push\s+--force",
        r"(?i)delete\s+\d+\s+files",
        r"(?i)chmod\s+777",
        r"(?i)truncate\s+",
    ]
}

// ---------------------------------------------------------------------------
// Capability descriptor
// ---------------------------------------------------------------------------

/// Declares what a given agent backend supports.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    pub worker_execution: bool,
    pub prompt_refinement: bool,
    pub branch_choice: bool,
    pub loop_verdict: bool,
    pub structured_output: bool,
    pub session_reuse: bool,
    pub native_json_schema: bool,
    pub model_selection: bool,
    pub reasoning_config: bool,
    pub system_prompt: bool,
    pub budget_limit: bool,
    pub turn_limit: bool,
    pub cost_reporting: bool,
    pub tool_allowlist: bool,
    pub web_search: bool,
}

fn capabilities_from_registry(name: &str) -> Option<AgentCapabilities> {
    let registry = load_agent_registry().ok()?;
    registry.get(name).map(|spec| {
        let caps = &spec.capabilities;
        AgentCapabilities {
            worker_execution: caps.worker_execution,
            prompt_refinement: caps.prompt_refinement,
            branch_choice: caps.branch_choice,
            loop_verdict: caps.loop_verdict,
            structured_output: caps.structured_output,
            session_reuse: caps.session_reuse,
            native_json_schema: caps.native_json_schema,
            model_selection: caps.model_selection,
            reasoning_config: caps.reasoning_config,
            system_prompt: caps.system_prompt,
            budget_limit: caps.budget_limit,
            turn_limit: caps.turn_limit,
            cost_reporting: caps.cost_reporting,
            tool_allowlist: caps.tool_allowlist,
            web_search: caps.web_search,
        }
    })
}

fn registry_capabilities_or(name: &str, fallback: AgentCapabilities) -> AgentCapabilities {
    capabilities_from_registry(name).unwrap_or(fallback)
}

/// Fingerprint of the user `agents.toml` used to invalidate the registry cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AgentsConfigFingerprint {
    Missing,
    Present {
        mtime: SystemTime,
        len: u64,
        #[cfg(unix)]
        inode: u64,
        #[cfg(unix)]
        ctime: i64,
        #[cfg(unix)]
        ctime_nsec: i64,
    },
}

fn agents_config_path() -> Option<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
        })?;

    Some(config_home.join("tmux-tools").join("agents.toml"))
}

fn agents_config_fingerprint(path: &Path) -> std::io::Result<AgentsConfigFingerprint> {
    match std::fs::metadata(path) {
        Ok(meta) => Ok(AgentsConfigFingerprint::Present {
            mtime: meta.modified()?,
            len: meta.len(),
            #[cfg(unix)]
            inode: meta.ino(),
            #[cfg(unix)]
            ctime: meta.ctime(),
            #[cfg(unix)]
            ctime_nsec: meta.ctime_nsec(),
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(AgentsConfigFingerprint::Missing)
        }
        Err(err) => Err(err),
    }
}

fn current_agents_config_fingerprint() -> anyhow::Result<AgentsConfigFingerprint> {
    match agents_config_path() {
        Some(path) => Ok(agents_config_fingerprint(&path)?),
        None => Ok(AgentsConfigFingerprint::Missing),
    }
}

static AGENT_REGISTRY_CACHE: OnceLock<Mutex<Option<(AgentsConfigFingerprint, agents::Registry)>>> =
    OnceLock::new();

fn load_agent_registry_cached(
    fingerprint: AgentsConfigFingerprint,
    cache: &Mutex<Option<(AgentsConfigFingerprint, agents::Registry)>>,
    load: impl FnOnce() -> anyhow::Result<(agents::Registry, Vec<agents::LoadWarning>)>,
) -> anyhow::Result<agents::Registry> {
    if let Ok(guard) = cache.lock() {
        if let Some((cached_fingerprint, cached_registry)) = guard.as_ref() {
            if *cached_fingerprint == fingerprint {
                return Ok(cached_registry.clone());
            }
        }
    }

    let (registry, warnings) = load()?;
    for warning in warnings {
        tracing::warn!(
            "[tmux-tools] agent registry warning [{}]: {}",
            warning.agent,
            warning.detail
        );
    }

    if let Ok(mut guard) = cache.lock() {
        *guard = Some((fingerprint, registry.clone()));
    }

    Ok(registry)
}

pub(crate) fn load_agent_registry() -> anyhow::Result<agents::Registry> {
    let fingerprint = current_agents_config_fingerprint()?;
    let cache = AGENT_REGISTRY_CACHE.get_or_init(|| Mutex::new(None));
    load_agent_registry_cached(fingerprint, cache, agents::Registry::load)
}

fn registry_interaction_pattern(
    pattern: &agents::InteractionPatternSpec,
) -> Option<InteractionPattern> {
    let kind = match &pattern.kind {
        agents::InteractionKind::Permission => InteractionKind::PermissionRequest,
        agents::InteractionKind::AutoRespond => InteractionKind::AutoRespond {
            response: pattern.response.clone().unwrap_or_default(),
        },
        agents::InteractionKind::SubagentActive => InteractionKind::SubagentActive,
        agents::InteractionKind::DestructiveWarning => InteractionKind::DestructiveWarning,
        agents::InteractionKind::Unknown => return None,
    };

    Some(InteractionPattern {
        kind,
        pattern: pattern.pattern.clone(),
        description: pattern.description.clone(),
        send_enter: pattern.send_enter,
    })
}

pub fn agent_binary(name: &str) -> Option<String> {
    load_agent_registry()
        .ok()
        .and_then(|registry| registry.get(name).map(|spec| spec.binary.clone()))
        .or_else(|| match name {
            "claude" | "codex" => Some(name.to_string()),
            _ => None,
        })
}

fn access_mode_profile_name(mode: &AccessMode) -> &'static str {
    match mode {
        AccessMode::ReadOnly => "read-only",
        AccessMode::Edit | AccessMode::Execute => "workspace-write",
        AccessMode::Unrestricted => "full-access",
    }
}

/// Ordered privilege carried by an agent access profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AccessPrivilege {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

impl AccessMode {
    pub(crate) fn privilege(&self) -> AccessPrivilege {
        match self {
            Self::ReadOnly => AccessPrivilege::ReadOnly,
            Self::Edit | Self::Execute => AccessPrivilege::WorkspaceWrite,
            Self::Unrestricted => AccessPrivilege::FullAccess,
        }
    }
}

fn declared_profile_privilege(
    spec: &agents::AgentSpec,
    profile_name: &str,
) -> Option<AccessPrivilege> {
    let declared = match profile_name {
        "read-only" => Some(AccessPrivilege::ReadOnly),
        "workspace-write" => Some(AccessPrivilege::WorkspaceWrite),
        "full-access" => Some(AccessPrivilege::FullAccess),
        _ => None,
    };
    if declared.is_some() {
        return declared;
    }

    if spec.name == "cursor"
        && profile_name == "plan"
        && spec
            .access_profiles
            .get(profile_name)
            .is_some_and(|profile| profile.args == ["--mode", "plan"])
    {
        return Some(AccessPrivilege::ReadOnly);
    }

    // The pinned core crate does not yet attach privilege metadata to AccessProfile.
    // For non-canonical names, accept only an exact shape already declared by this
    // agent under a canonical ranked profile. This is a closed set comparison, not
    // inference from free-form argv.
    let profile = spec.access_profiles.get(profile_name)?;
    let mut matched = None;
    for (canonical_name, privilege) in [
        ("read-only", AccessPrivilege::ReadOnly),
        ("workspace-write", AccessPrivilege::WorkspaceWrite),
        ("full-access", AccessPrivilege::FullAccess),
    ] {
        if spec.access_profiles.get(canonical_name) == Some(profile) {
            if matched.is_some_and(|previous| previous != privilege) {
                return None;
            }
            matched = Some(privilege);
        }
    }
    matched
}

fn require_privilege(spec: &agents::AgentSpec, profile: &str) -> anyhow::Result<AccessPrivilege> {
    declared_profile_privilege(spec, profile).ok_or_else(|| {
        anyhow::anyhow!(
            "agent {} access profile {profile} has no declared privilege rank",
            spec.name
        )
    })
}

pub(crate) fn access_profile_privilege(
    agent: &str,
    profile_name: &str,
) -> anyhow::Result<AccessPrivilege> {
    let registry = load_agent_registry()?;
    let spec = registry
        .get(agent)
        .ok_or_else(|| anyhow::anyhow!("unknown agent {agent}"))?;
    require_privilege(spec, profile_name)
}

pub(crate) fn resolved_access_profile(
    agent: &str,
    config: &AgentConfig,
) -> anyhow::Result<(String, AccessPrivilege)> {
    let registry = load_agent_registry()?;
    let profile_name = resolve_registry_access_profile(&registry, agent, config)?;
    let spec = registry
        .get(agent)
        .ok_or_else(|| anyhow::anyhow!("unknown agent {agent}"))?;
    let privilege = require_privilege(spec, &profile_name)?;
    Ok((profile_name, privilege))
}

/// Access-profile keys declared for an agent in the tmux-tools registry.
pub fn agent_access_profile_names(name: &str) -> Vec<String> {
    let Ok(registry) = load_agent_registry() else {
        return Vec::new();
    };
    let Some(spec) = registry.get(name) else {
        return Vec::new();
    };
    let mut names: Vec<String> = spec.access_profiles.keys().cloned().collect();
    names.sort_unstable();
    names
}

fn resolve_registry_access_profile(
    registry: &agents::Registry,
    agent: &str,
    config: &AgentConfig,
) -> anyhow::Result<String> {
    let spec = registry
        .get(agent)
        .ok_or_else(|| anyhow::anyhow!("unknown agent {agent}"))?;
    let mapped = access_mode_profile_name(&config.access_mode);
    let selected = if let Some(profile) = config.access_profile_override.as_deref() {
        if !spec.access_profiles.contains_key(profile) {
            anyhow::bail!("agent {agent} has no access profile {profile}");
        }
        profile
    } else {
        mapped
    };
    if spec.access_profiles.contains_key(selected) {
        let profile_privilege = require_privilege(spec, selected)?;
        if profile_privilege > config.access_mode.privilege() {
            anyhow::bail!(
                "access profile {selected} would widen {:?} access mode for agent {agent}",
                config.access_mode
            );
        }
        return Ok(selected.to_string());
    }

    // Read-only never falls back: an explicitly ranked read-only profile is required.
    if config.access_mode == AccessMode::ReadOnly {
        anyhow::bail!("agent {agent} has no read-only access profile");
    }
    if spec.access_profiles.contains_key("default") {
        let default_privilege = require_privilege(spec, "default")?;
        if default_privilege <= config.access_mode.privilege() {
            return Ok("default".to_string());
        }
        anyhow::bail!(
            "agent {agent} default access profile would widen {:?} access mode",
            config.access_mode
        );
    }

    anyhow::bail!("agent {agent} has no access profile {mapped} and no default profile");
}

fn resolve_builtin_access_profile(
    agent: &str,
    config: &AgentConfig,
) -> anyhow::Result<(String, Option<Vec<String>>)> {
    let registry = load_agent_registry()?;
    let profile = resolve_registry_access_profile(&registry, agent, config)?;
    let override_args = if config.access_profile_override.is_some() {
        let (_binary, args) = registry.launch_argv(agent, Some(&profile))?;
        Some(args)
    } else {
        None
    };
    Ok((profile, override_args))
}

fn validate_registry_config_fields(
    config: &AgentConfig,
    caps: &AgentCapabilities,
) -> anyhow::Result<()> {
    let AgentConfig {
        model,
        reasoning_level,
        system_prompt,
        max_turns,
        max_budget_usd,
        resume_session_id: _,
        ephemeral_session: _,
        json_schema: _,
        access_mode: _,
        access_profile_override: _,
        tool_toggles,
        allowed_tools,
        disallowed_tools,
        cwd: _,
        auto_approve: _,
        orchestrator: _,
    } = config;

    if model.is_some() && !caps.model_selection {
        anyhow::bail!("agent does not support model selection");
    }
    if reasoning_level.is_some() && !caps.reasoning_config {
        anyhow::bail!("agent does not support reasoning config");
    }
    if system_prompt.is_some() && !caps.system_prompt {
        anyhow::bail!("agent does not support system prompt");
    }
    if max_budget_usd.is_some() && !caps.budget_limit {
        anyhow::bail!("agent does not support budget limit");
    }
    if max_turns.is_some() && !caps.turn_limit {
        anyhow::bail!("agent does not support turn limit");
    }
    if tool_toggles.web_search.is_some() && !caps.web_search {
        anyhow::bail!("agent does not support web search toggle");
    }
    if allowed_tools.is_some() && !caps.tool_allowlist {
        anyhow::bail!("agent does not support tool allowlist");
    }
    if disallowed_tools.is_some() && !caps.tool_allowlist {
        anyhow::bail!("agent does not support tool denylist");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Configuration types
// ---------------------------------------------------------------------------

/// Reasoning effort level (supported by some backends such as Codex).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningLevel {
    Low,
    Medium,
    High,
}

/// Filesystem / tool access level the agent is granted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    ReadOnly,
    Edit,
    #[default]
    Execute,
    Unrestricted,
}

/// Per-tool feature toggles that override the default tool set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolToggles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_search: Option<bool>,
}

/// Fully-resolved configuration passed to a driver's `build_session_args` method.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub model: Option<String>,
    pub reasoning_level: Option<ReasoningLevel>,
    pub system_prompt: Option<String>,
    pub max_turns: Option<u32>,
    pub max_budget_usd: Option<f64>,
    pub resume_session_id: Option<String>,
    pub ephemeral_session: bool,
    pub json_schema: Option<Value>,
    pub access_mode: AccessMode,
    pub access_profile_override: Option<String>,
    pub tool_toggles: ToolToggles,
    pub allowed_tools: Option<Vec<String>>,
    pub disallowed_tools: Option<Vec<String>>,
    pub cwd: String,
    pub auto_approve: bool,
    pub orchestrator: Option<crate::model::OrchestratorConfig>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: None,
            reasoning_level: None,
            system_prompt: None,
            max_turns: None,
            max_budget_usd: None,
            resume_session_id: None,
            ephemeral_session: true,
            json_schema: None,
            access_mode: AccessMode::Execute,
            access_profile_override: None,
            tool_toggles: ToolToggles::default(),
            allowed_tools: None,
            disallowed_tools: None,
            cwd: String::new(),
            auto_approve: false,
            orchestrator: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// Normalized outcome of a single agent invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeOutcome {
    Success,
    ErrorExecution,
    ErrorMaxTurns,
    ErrorMaxBudget,
    ErrorSchemaValidation,
    ErrorTimeout,
    ErrorNotFound,
}

impl NodeOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, NodeOutcome::Success)
    }
}

/// Normalized result produced by any agent backend.
#[derive(Debug, Clone)]
pub struct AgentOutput {
    pub response_text: String,
    pub session_id: Option<String>,
    pub cost_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub thinking_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub model_used: Option<String>,
    pub num_turns: Option<u32>,
    pub duration_api_ms: Option<u64>,
    pub structured_output: Option<Value>,
    pub outcome: NodeOutcome,
    pub error_message: Option<String>,
}

// ---------------------------------------------------------------------------
// Command args returned by build_session_args
// ---------------------------------------------------------------------------

/// CLI arguments produced by [`AgentDriver::build_session_args`].
#[derive(Debug, Clone)]
pub struct CommandArgs {
    pub args: Vec<String>,
    /// Registry profile used to render the command's access arguments.
    pub access_profile: String,
    /// Temporary directory to clean up after the agent process exits.
    pub temp_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Driver trait
// ---------------------------------------------------------------------------

/// A backend-specific driver that knows how to invoke an agent CLI in interactive mode.
pub trait AgentDriver: Send + Sync {
    /// CLI executable name (e.g. `"claude"`, `"codex"`).
    fn name(&self) -> &str;

    /// Capability flags for this backend.
    fn capabilities(&self) -> AgentCapabilities;

    /// Build CLI args for interactive session (no --print, no --output-format json).
    /// Unlike the old build_args, this does NOT take a prompt — prompts are sent via PTY stdin.
    fn build_session_args(&self, config: &AgentConfig) -> anyhow::Result<CommandArgs>;

    /// Wrap a user prompt with instructions for the agent to print a sentinel
    /// marker when it has finished responding.
    fn wrap_prompt_with_sentinel(&self, prompt: &str, sentinel: &str) -> String {
        format!(
            "{}\n\n[After completing your full response, print exactly this marker on its own line: {}]",
            prompt, sentinel
        )
    }

    /// Command to query cost (e.g., "/cost"), if supported.
    fn cost_command(&self) -> Option<&str>;

    /// Command to query context usage (e.g., "/context"), if supported.
    fn context_command(&self) -> Option<&str>;

    /// Command to exit the agent session.
    fn exit_command(&self) -> &str;

    /// Parse cost command output into structured data.
    fn parse_cost_response(&self, output: &str) -> Option<CostInfo>;

    /// Parse context command output into structured data.
    fn parse_context_response(&self, output: &str) -> Option<ContextInfo>;

    /// Return patterns that match interactive prompts for this agent backend.
    fn interaction_patterns(&self) -> Vec<InteractionPattern> {
        vec![]
    }

    /// Return regex patterns for destructive commands that should always
    /// escalate to human approval, even when auto-approve is on.
    fn destructive_blocklist(&self) -> &[&str] {
        shared_destructive_patterns()
    }
}

// ===========================================================================
// Claude driver
// ===========================================================================

/// Driver for the Anthropic **Claude Code** CLI.
pub struct ClaudeDriver;

fn append_claude_tool_flags(args: &mut Vec<String>, config: &AgentConfig) {
    if let Some(allowed) = &config.allowed_tools {
        for tool in allowed {
            args.push("--allowedTools".to_string());
            args.push(tool.clone());
        }
    }
    if let Some(disallowed) = &config.disallowed_tools {
        for tool in disallowed {
            args.push("--disallowedTools".to_string());
            args.push(tool.clone());
        }
    }
}

fn resolve_claude_access(config: &AgentConfig) -> anyhow::Result<(String, Vec<String>)> {
    let (access_profile, access_profile_override_args) =
        resolve_builtin_access_profile("claude", config)?;
    let mut args = if let Some(access_args) = access_profile_override_args {
        access_args
    } else {
        match config.access_mode {
            AccessMode::ReadOnly => vec!["--permission-mode".to_string(), "plan".to_string()],
            AccessMode::Edit if config.allowed_tools.is_none() => {
                let mut args = Vec::new();
                for tool in &["Read", "Edit", "Write", "Glob", "Grep", "Bash(git *)"] {
                    args.push("--allowedTools".to_string());
                    args.push(tool.to_string());
                }
                args
            }
            AccessMode::Edit => Vec::new(),
            AccessMode::Execute | AccessMode::Unrestricted => {
                vec!["--dangerously-skip-permissions".to_string()]
            }
        }
    };

    append_claude_tool_flags(&mut args, config);
    Ok((access_profile, args))
}

impl AgentDriver for ClaudeDriver {
    fn name(&self) -> &str {
        "claude"
    }

    fn capabilities(&self) -> AgentCapabilities {
        registry_capabilities_or(
            self.name(),
            AgentCapabilities {
                worker_execution: true,
                prompt_refinement: true,
                branch_choice: true,
                loop_verdict: true,
                structured_output: true,
                session_reuse: true,
                native_json_schema: true,
                model_selection: true,
                reasoning_config: false,
                system_prompt: true,
                budget_limit: true,
                turn_limit: true,
                cost_reporting: true,
                tool_allowlist: true,
                web_search: true,
            },
        )
    }

    fn build_session_args(&self, config: &AgentConfig) -> anyhow::Result<CommandArgs> {
        let mut args = Vec::new();
        let (access_profile, access_args) = resolve_claude_access(config)?;

        // Model selection
        if let Some(model) = &config.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }

        // System prompt
        if let Some(sys_prompt) = &config.system_prompt {
            args.push("--append-system-prompt".to_string());
            args.push(sys_prompt.clone());
        }

        // Budget limits
        if let Some(budget) = config.max_budget_usd {
            args.push("--max-budget-usd".to_string());
            args.push(budget.to_string());
        }
        if let Some(turns) = config.max_turns {
            args.push("--max-turns".to_string());
            args.push(turns.to_string());
        }

        // Session handling
        if let Some(session_id) = &config.resume_session_id {
            args.push("--resume".to_string());
            args.push(session_id.clone());
        } else if config.ephemeral_session {
            args.push("--no-session-persistence".to_string());
        }

        // Access mode is the ceiling; explicit tool lists only refine it.
        args.extend(access_args);

        // Web search toggle
        if let Some(false) = config.tool_toggles.web_search {
            args.push("--disallowedTools".to_string());
            args.push("WebSearch".to_string());
            args.push("--disallowedTools".to_string());
            args.push("WebFetch".to_string());
        }

        Ok(CommandArgs {
            args,
            access_profile,
            temp_dir: None,
        })
    }

    fn cost_command(&self) -> Option<&str> {
        Some("/cost")
    }
    fn context_command(&self) -> Option<&str> {
        Some("/context")
    }
    fn exit_command(&self) -> &str {
        "/exit"
    }

    fn parse_cost_response(&self, output: &str) -> Option<CostInfo> {
        crate::pty_output::parse_claude_cost(output)
    }

    fn parse_context_response(&self, output: &str) -> Option<ContextInfo> {
        crate::pty_output::parse_claude_context(output)
    }

    fn interaction_patterns(&self) -> Vec<InteractionPattern> {
        vec![
            InteractionPattern::new(
                InteractionKind::AutoRespond {
                    response: "y".to_string(),
                },
                r"(?i)do you trust.*\?\s*$",
                "Trust folder prompt",
            ),
            InteractionPattern::new(
                InteractionKind::PermissionRequest,
                r"(?i)(allow|wants to use)\s+\w+.*\?\s*\(y/n\)",
                "Tool permission prompt",
            ),
            InteractionPattern::new(
                InteractionKind::SubagentActive,
                r"(?i)(launching|spawning)\s+agent",
                "Agent spawning a subagent",
            ),
            InteractionPattern::new(
                InteractionKind::SubagentActive,
                r"(?i)agent.*running.*background",
                "Subagent running in background",
            ),
        ]
    }
}

// ===========================================================================
// Codex driver
// ===========================================================================

/// Driver for the OpenAI **Codex** CLI.
pub struct CodexDriver;

impl AgentDriver for CodexDriver {
    fn name(&self) -> &str {
        "codex"
    }

    fn capabilities(&self) -> AgentCapabilities {
        registry_capabilities_or(
            self.name(),
            AgentCapabilities {
                worker_execution: true,
                prompt_refinement: false,
                branch_choice: false,
                loop_verdict: false,
                structured_output: true,
                session_reuse: true,
                native_json_schema: true,
                model_selection: true,
                reasoning_config: true,
                system_prompt: false,
                budget_limit: false,
                turn_limit: false,
                cost_reporting: false,
                tool_allowlist: false,
                web_search: true,
            },
        )
    }

    fn build_session_args(&self, config: &AgentConfig) -> anyhow::Result<CommandArgs> {
        let mut args = Vec::new();
        let (access_profile, access_profile_override_args) =
            resolve_builtin_access_profile(self.name(), config)?;

        // Sub-command: `exec resume <id>` or plain `exec`
        if let Some(session_id) = &config.resume_session_id {
            args.push("exec".to_string());
            args.push("resume".to_string());
            args.push(session_id.clone());
        } else {
            args.push("exec".to_string());
        }

        // Model selection
        if let Some(model) = &config.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }

        // Reasoning config
        if let Some(level) = &config.reasoning_level {
            let level_str = match level {
                ReasoningLevel::Low => "low",
                ReasoningLevel::Medium => "medium",
                ReasoningLevel::High => "high",
            };
            args.push("-c".to_string());
            args.push(format!("model_reasoning_effort={level_str}"));
        }

        // Session persistence
        if config.resume_session_id.is_none() && config.ephemeral_session {
            args.push("--ephemeral".to_string());
        }

        // Access mode
        if let Some(access_args) = access_profile_override_args {
            args.extend(access_args);
        } else {
            match config.access_mode {
                AccessMode::ReadOnly => {
                    args.push("--sandbox".to_string());
                    args.push("read-only".to_string());
                    args.push("-a".to_string());
                    args.push("never".to_string());
                }
                AccessMode::Edit => {
                    args.push("--sandbox".to_string());
                    args.push("workspace-write".to_string());
                    args.push("-a".to_string());
                    args.push("untrusted".to_string());
                }
                AccessMode::Execute => {
                    args.push("--full-auto".to_string());
                }
                AccessMode::Unrestricted => {
                    args.push("--sandbox".to_string());
                    args.push("danger-full-access".to_string());
                    args.push("-a".to_string());
                    args.push("never".to_string());
                }
            }
        }

        // Web search
        if let Some(true) = config.tool_toggles.web_search {
            args.push("--search".to_string());
        }

        Ok(CommandArgs {
            args,
            access_profile,
            temp_dir: None,
        })
    }

    fn cost_command(&self) -> Option<&str> {
        None
    }
    fn context_command(&self) -> Option<&str> {
        None
    }
    fn exit_command(&self) -> &str {
        "/exit"
    }
    fn parse_cost_response(&self, _output: &str) -> Option<CostInfo> {
        None
    }
    fn parse_context_response(&self, _output: &str) -> Option<ContextInfo> {
        None
    }

    fn interaction_patterns(&self) -> Vec<InteractionPattern> {
        vec![InteractionPattern::new(
            InteractionKind::PermissionRequest,
            r"(?i)allow this action.*\[y/n\]",
            "Action approval prompt",
        )]
    }
}

// ===========================================================================
// Structured-output helpers
// ===========================================================================

/// Generate a human-readable schema description for prompt injection (structured output fallback).
pub fn schema_to_prompt_hint(schema: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        let required: Vec<&str> = schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        for (name, prop) in props {
            let ty = prop.get("type").and_then(|v| v.as_str()).unwrap_or("any");
            let desc = prop
                .get("description")
                .and_then(|v| v.as_str())
                .map(|d| format!(" — {d}"))
                .unwrap_or_default();
            let req = if required.contains(&name.as_str()) {
                ", required"
            } else {
                ""
            };
            parts.push(format!("  - {name} ({ty}{req}){desc}"));
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    format!(
        "\nThe JSON object should have these fields:\n{}",
        parts.join("\n")
    )
}

// ===========================================================================
// Generic tmux-tools registry profile driver
// ===========================================================================

/// Driver for user-defined tmux-tools registry profiles.
pub struct RegistryProfileDriver {
    name: String,
}

impl RegistryProfileDriver {
    fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl AgentDriver for RegistryProfileDriver {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> AgentCapabilities {
        let mut capabilities =
            capabilities_from_registry(&self.name).unwrap_or(AgentCapabilities {
                worker_execution: true,
                prompt_refinement: false,
                branch_choice: false,
                loop_verdict: false,
                structured_output: false,
                session_reuse: false,
                native_json_schema: false,
                model_selection: false,
                reasoning_config: false,
                system_prompt: false,
                budget_limit: false,
                turn_limit: false,
                cost_reporting: false,
                tool_allowlist: false,
                web_search: false,
            });

        // Registry profiles provide only static access-profile argv. Do not
        // advertise tuning controls this driver cannot render dynamically,
        // even when a hand-authored registry spec claims CLI-level support.
        capabilities.model_selection = false;
        capabilities.reasoning_config = false;
        capabilities.system_prompt = false;
        capabilities.budget_limit = false;
        capabilities.turn_limit = false;
        capabilities.tool_allowlist = false;
        capabilities.web_search = false;
        capabilities
    }

    fn build_session_args(&self, config: &AgentConfig) -> anyhow::Result<CommandArgs> {
        let registry = load_agent_registry()?;
        validate_registry_config_fields(config, &self.capabilities())?;
        let profile = resolve_registry_access_profile(&registry, &self.name, config)?;
        let (_binary, args) = registry.launch_argv(&self.name, Some(&profile))?;
        Ok(CommandArgs {
            args,
            access_profile: profile,
            temp_dir: None,
        })
    }

    fn cost_command(&self) -> Option<&str> {
        None
    }
    fn context_command(&self) -> Option<&str> {
        None
    }
    fn exit_command(&self) -> &str {
        "/exit"
    }
    fn parse_cost_response(&self, _output: &str) -> Option<CostInfo> {
        None
    }
    fn parse_context_response(&self, _output: &str) -> Option<ContextInfo> {
        None
    }

    fn interaction_patterns(&self) -> Vec<InteractionPattern> {
        let Ok(registry) = load_agent_registry() else {
            return vec![];
        };
        let Some(spec) = registry.get(&self.name) else {
            return vec![];
        };

        spec.interaction_patterns
            .iter()
            .filter_map(registry_interaction_pattern)
            .collect()
    }
}

// ===========================================================================
// Driver registry
// ===========================================================================

/// Look up a driver by CLI executable name.
pub fn get_driver(name: &str) -> Option<Box<dyn AgentDriver>> {
    match name {
        "claude" => Some(Box::new(ClaudeDriver)),
        "codex" => Some(Box::new(CodexDriver)),
        _ => load_agent_registry().ok().and_then(|registry| {
            registry
                .get(name)
                .map(|_| Box::new(RegistryProfileDriver::new(name)) as Box<dyn AgentDriver>)
        }),
    }
}

/// Return every registered driver.
pub fn all_drivers() -> Vec<Box<dyn AgentDriver>> {
    let Ok(registry) = load_agent_registry() else {
        return vec![Box::new(ClaudeDriver), Box::new(CodexDriver)];
    };

    registry
        .agents()
        .keys()
        .filter_map(|name| get_driver(name))
        .collect()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(unix)]
    fn restore_file_times(path: &Path, metadata: &std::fs::Metadata) {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::MetadataExt;

        let path = CString::new(path.as_os_str().as_bytes()).unwrap();
        let times = [
            libc::timespec {
                tv_sec: metadata.atime(),
                tv_nsec: metadata.atime_nsec() as _,
            },
            libc::timespec {
                tv_sec: metadata.mtime(),
                tv_nsec: metadata.mtime_nsec() as _,
            },
        ];
        // SAFETY: `path` is a valid NUL-terminated path and `times` contains
        // exactly the two timespec values required by utimensat.
        let result = unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) };
        assert_eq!(result, 0, "failed to restore file times");
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn default_config() -> AgentConfig {
        AgentConfig::default()
    }

    #[cfg(unix)]
    #[test]
    fn registry_cache_reloads_after_equal_length_rewrite_with_restored_mtime() {
        const FIRST_CONFIG: &str = r#"[cache_probe]
binary = "agent-one"
"#;
        const SECOND_CONFIG: &str = r#"[cache_probe]
binary = "agent-two"
"#;
        assert_eq!(FIRST_CONFIG.len(), SECOND_CONFIG.len());

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("agents.toml");
        std::fs::write(&path, FIRST_CONFIG).unwrap();
        let original_metadata = std::fs::metadata(&path).unwrap();
        let first_fingerprint = agents_config_fingerprint(&path).unwrap();
        let cache = Mutex::new(None);

        let first_registry = load_agent_registry_cached(first_fingerprint, &cache, || {
            agents::Registry::load_with_user_path(Some(&path))
        })
        .unwrap();
        assert_eq!(
            first_registry.get("cache_probe").unwrap().binary,
            "agent-one"
        );

        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&path, SECOND_CONFIG).unwrap();
        restore_file_times(&path, &original_metadata);
        let rewritten_metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(rewritten_metadata.len(), original_metadata.len());
        assert_eq!(
            rewritten_metadata.modified().unwrap(),
            original_metadata.modified().unwrap()
        );

        let second_fingerprint = agents_config_fingerprint(&path).unwrap();
        let second_registry = load_agent_registry_cached(second_fingerprint, &cache, || {
            agents::Registry::load_with_user_path(Some(&path))
        })
        .unwrap();
        assert_eq!(
            second_registry.get("cache_probe").unwrap().binary,
            "agent-two"
        );
    }

    fn args_contain(args: &[String], needle: &str) -> bool {
        args.iter().any(|a| a == needle)
    }

    fn arg_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.windows(2).find_map(|w| {
            if w[0] == flag {
                Some(w[1].as_str())
            } else {
                None
            }
        })
    }

    // -----------------------------------------------------------------------
    // ClaudeDriver::build_session_args
    // -----------------------------------------------------------------------

    #[test]
    fn claude_default_args() {
        let driver = ClaudeDriver;
        let cmd = driver.build_session_args(&default_config()).unwrap();
        // Interactive mode: no --print, no --output-format json
        assert!(!args_contain(&cmd.args, "--print"));
        assert!(!args_contain(&cmd.args, "--output-format"));
        assert!(!args_contain(&cmd.args, "--json-schema"));
        assert!(args_contain(&cmd.args, "--no-session-persistence"));
        assert!(args_contain(&cmd.args, "--dangerously-skip-permissions"));
    }

    #[test]
    fn claude_model_selection() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            model: Some("claude-sonnet-4-20250514".into()),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(
            arg_after(&cmd.args, "--model"),
            Some("claude-sonnet-4-20250514")
        );
    }

    #[test]
    fn claude_budget_and_turns() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            max_budget_usd: Some(1.5),
            max_turns: Some(5),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--max-budget-usd"), Some("1.5"));
        assert_eq!(arg_after(&cmd.args, "--max-turns"), Some("5"));
    }

    #[test]
    fn claude_system_prompt() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            system_prompt: Some("Be concise.".into()),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(
            arg_after(&cmd.args, "--append-system-prompt"),
            Some("Be concise.")
        );
    }

    #[test]
    fn claude_session_resume() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            resume_session_id: Some("sess-42".into()),
            ephemeral_session: false,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--resume"), Some("sess-42"));
        assert!(!args_contain(&cmd.args, "--no-session-persistence"));
    }

    #[test]
    fn claude_access_mode_read_only() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            access_mode: AccessMode::ReadOnly,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--permission-mode"), Some("plan"));
        assert!(!args_contain(&cmd.args, "--dangerously-skip-permissions"));
    }

    #[test]
    fn claude_read_only_with_disallowed_tools_retains_plan_mode() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            access_mode: AccessMode::ReadOnly,
            disallowed_tools: Some(vec!["Bash".into()]),
            ..default_config()
        };

        let cmd = driver.build_session_args(&config).unwrap();

        assert_eq!(arg_after(&cmd.args, "--permission-mode"), Some("plan"));
        assert_eq!(arg_after(&cmd.args, "--disallowedTools"), Some("Bash"));
        assert_eq!(cmd.access_profile, "read-only");
    }

    #[test]
    fn claude_access_mode_edit() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            access_mode: AccessMode::Edit,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert!(args_contain(&cmd.args, "--allowedTools"));
        assert!(!args_contain(&cmd.args, "--dangerously-skip-permissions"));
        // Should include known edit tools
        let allowed: Vec<&str> = cmd
            .args
            .windows(2)
            .filter_map(|w| {
                if w[0] == "--allowedTools" {
                    Some(w[1].as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(allowed.contains(&"Read"));
        assert!(allowed.contains(&"Edit"));
        assert!(allowed.contains(&"Write"));
    }

    #[test]
    fn claude_tool_toggles_disable_web() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            tool_toggles: ToolToggles {
                web_search: Some(false),
            },
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        let disallowed: Vec<&str> = cmd
            .args
            .windows(2)
            .filter_map(|w| {
                if w[0] == "--disallowedTools" {
                    Some(w[1].as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(disallowed.contains(&"WebSearch"));
        assert!(disallowed.contains(&"WebFetch"));
    }

    #[test]
    fn claude_allowed_tools_refine_read_only_access_mode() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            access_mode: AccessMode::ReadOnly,
            allowed_tools: Some(vec!["Read".into(), "Grep".into()]),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--permission-mode"), Some("plan"));
        let allowed: Vec<&str> = cmd
            .args
            .windows(2)
            .filter_map(|w| {
                if w[0] == "--allowedTools" {
                    Some(w[1].as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(allowed, vec!["Read", "Grep"]);
        assert_eq!(cmd.access_profile, "read-only");
    }

    #[test]
    fn claude_allowed_tools_retain_execute_permission_mode() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            access_mode: AccessMode::Execute,
            allowed_tools: Some(vec!["Read".into(), "Bash".into()]),
            ..default_config()
        };

        let cmd = driver.build_session_args(&config).unwrap();

        assert!(args_contain(&cmd.args, "--dangerously-skip-permissions"));
        let allowed: Vec<&str> = cmd
            .args
            .windows(2)
            .filter_map(|w| {
                if w[0] == "--allowedTools" {
                    Some(w[1].as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(allowed, vec!["Read", "Bash"]);
        assert_eq!(cmd.access_profile, "workspace-write");
    }

    // -----------------------------------------------------------------------
    // ClaudeDriver PTY methods
    // -----------------------------------------------------------------------

    #[test]
    fn claude_wrap_prompt_with_sentinel() {
        let driver = ClaudeDriver;
        let wrapped = driver.wrap_prompt_with_sentinel("Hello", "SILVERBOND_DONE_abc123");
        assert!(wrapped.contains("Hello"));
        assert!(wrapped.contains("SILVERBOND_DONE_abc123"));
    }

    #[test]
    fn claude_cost_command() {
        let driver = ClaudeDriver;
        assert_eq!(driver.cost_command(), Some("/cost"));
    }

    #[test]
    fn claude_context_command() {
        let driver = ClaudeDriver;
        assert_eq!(driver.context_command(), Some("/context"));
    }

    #[test]
    fn claude_exit_command() {
        let driver = ClaudeDriver;
        assert_eq!(driver.exit_command(), "/exit");
    }

    // -----------------------------------------------------------------------
    // CodexDriver::build_session_args
    // -----------------------------------------------------------------------

    #[test]
    fn codex_default_args() {
        let driver = CodexDriver;
        let cmd = driver.build_session_args(&default_config()).unwrap();
        assert_eq!(cmd.args[0], "exec");
        // Interactive mode: no --json, no --output-schema
        assert!(!args_contain(&cmd.args, "--json"));
        assert!(!args_contain(&cmd.args, "--output-schema"));
        assert!(args_contain(&cmd.args, "--ephemeral"));
        assert!(args_contain(&cmd.args, "--full-auto"));
    }

    #[test]
    fn codex_model_selection() {
        let driver = CodexDriver;
        let config = AgentConfig {
            model: Some("o3-mini".into()),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--model"), Some("o3-mini"));
    }

    #[test]
    fn codex_reasoning_level() {
        let driver = CodexDriver;
        let config = AgentConfig {
            reasoning_level: Some(ReasoningLevel::High),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(
            arg_after(&cmd.args, "-c"),
            Some("model_reasoning_effort=high")
        );
    }

    #[test]
    fn codex_session_resume() {
        let driver = CodexDriver;
        let config = AgentConfig {
            resume_session_id: Some("thread-42".into()),
            ephemeral_session: false,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(cmd.args[0], "exec");
        assert_eq!(cmd.args[1], "resume");
        assert_eq!(cmd.args[2], "thread-42");
        assert!(!args_contain(&cmd.args, "--ephemeral"));
    }

    #[test]
    fn codex_access_mode_read_only() {
        let driver = CodexDriver;
        let config = AgentConfig {
            access_mode: AccessMode::ReadOnly,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--sandbox"), Some("read-only"));
        assert_eq!(arg_after(&cmd.args, "-a"), Some("never"));
    }

    #[test]
    fn codex_access_mode_edit() {
        let driver = CodexDriver;
        let config = AgentConfig {
            access_mode: AccessMode::Edit,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--sandbox"), Some("workspace-write"));
        assert_eq!(arg_after(&cmd.args, "-a"), Some("untrusted"));
    }

    #[test]
    fn codex_access_mode_unrestricted() {
        let driver = CodexDriver;
        let config = AgentConfig {
            access_mode: AccessMode::Unrestricted,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(
            arg_after(&cmd.args, "--sandbox"),
            Some("danger-full-access")
        );
    }

    #[test]
    fn codex_web_search_enabled() {
        let driver = CodexDriver;
        let config = AgentConfig {
            tool_toggles: ToolToggles {
                web_search: Some(true),
            },
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert!(args_contain(&cmd.args, "--search"));
    }

    // -----------------------------------------------------------------------
    // CodexDriver PTY methods
    // -----------------------------------------------------------------------

    #[test]
    fn codex_cost_command() {
        let driver = CodexDriver;
        assert_eq!(driver.cost_command(), None);
    }

    #[test]
    fn codex_exit_command() {
        let driver = CodexDriver;
        assert_eq!(driver.exit_command(), "/exit");
    }

    // -----------------------------------------------------------------------
    // Driver registry
    // -----------------------------------------------------------------------

    #[test]
    fn registry_get_claude() {
        let driver = get_driver("claude").expect("claude driver should exist");
        assert_eq!(driver.name(), "claude");
    }

    #[test]
    fn registry_get_codex() {
        let driver = get_driver("codex").expect("codex driver should exist");
        assert_eq!(driver.name(), "codex");
    }

    #[test]
    fn registry_unknown_returns_none() {
        assert!(get_driver("gpt4-cli").is_none());
    }

    #[test]
    fn registry_all_drivers() {
        let drivers = all_drivers();
        let names: Vec<&str> = drivers.iter().map(|d| d.name()).collect();
        assert!(names.contains(&"claude"));
        assert!(names.contains(&"codex"));
        // The Gemini driver was removed; ensure it no longer appears.
        assert!(!names.contains(&"gemini"));
    }

    // -----------------------------------------------------------------------
    // NodeOutcome helpers
    // -----------------------------------------------------------------------

    #[test]
    fn node_outcome_is_success() {
        assert!(NodeOutcome::Success.is_success());
        assert!(!NodeOutcome::ErrorExecution.is_success());
        assert!(!NodeOutcome::ErrorMaxTurns.is_success());
        assert!(!NodeOutcome::ErrorMaxBudget.is_success());
        assert!(!NodeOutcome::ErrorSchemaValidation.is_success());
        assert!(!NodeOutcome::ErrorTimeout.is_success());
        assert!(!NodeOutcome::ErrorNotFound.is_success());
    }

    // -----------------------------------------------------------------------
    // AgentConfig default
    // -----------------------------------------------------------------------

    #[test]
    fn agent_config_default() {
        let config = AgentConfig::default();
        assert!(config.ephemeral_session);
        assert_eq!(config.access_mode, AccessMode::Execute);
        assert!(config.model.is_none());
        assert!(config.reasoning_level.is_none());
        assert!(config.system_prompt.is_none());
        assert!(config.max_turns.is_none());
        assert!(config.max_budget_usd.is_none());
        assert!(config.resume_session_id.is_none());
        assert!(config.json_schema.is_none());
        assert!(config.allowed_tools.is_none());
        assert!(config.disallowed_tools.is_none());
        assert!(config.cwd.is_empty());
    }

    // -----------------------------------------------------------------------
    // Serde round-trips
    // -----------------------------------------------------------------------

    #[test]
    fn reasoning_level_serde() {
        let json = serde_json::to_string(&ReasoningLevel::High).unwrap();
        assert_eq!(json, "\"high\"");
        let parsed: ReasoningLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ReasoningLevel::High);
    }

    #[test]
    fn access_mode_serde() {
        let json = serde_json::to_string(&AccessMode::ReadOnly).unwrap();
        assert_eq!(json, "\"read_only\"");
        let parsed: AccessMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, AccessMode::ReadOnly);
    }

    #[test]
    fn node_outcome_serde() {
        let json = serde_json::to_string(&NodeOutcome::ErrorMaxTurns).unwrap();
        assert_eq!(json, "\"error_max_turns\"");
        let parsed: NodeOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, NodeOutcome::ErrorMaxTurns);
    }

    #[test]
    fn tool_toggles_serde_skip_none() {
        let toggles = ToolToggles::default();
        let json = serde_json::to_string(&toggles).unwrap();
        assert_eq!(json, "{}");

        let toggles = ToolToggles {
            web_search: Some(true),
        };
        let json = serde_json::to_string(&toggles).unwrap();
        assert!(json.contains("webSearch"));
    }

    #[test]
    fn schema_to_prompt_hint_empty() {
        let schema = json!({"type": "object"});
        let hint = schema_to_prompt_hint(&schema);
        assert!(hint.is_empty());
    }

    // -----------------------------------------------------------------------
    // Capability gating — unsupported options silently ignored
    // -----------------------------------------------------------------------

    #[test]
    fn claude_ignores_reasoning_level() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            reasoning_level: Some(ReasoningLevel::High),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        // Claude doesn't support reasoning_config — no flag should appear
        assert!(!args_contain(&cmd.args, "-c"));
        assert!(!cmd.args.iter().any(|a| a.contains("reasoning")));
    }

    #[test]
    fn codex_ignores_system_prompt() {
        let driver = CodexDriver;
        let config = AgentConfig {
            system_prompt: Some("Be concise.".into()),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert!(!args_contain(&cmd.args, "--append-system-prompt"));
        assert!(!cmd.args.iter().any(|a| a.contains("Be concise")));
    }

    #[test]
    fn codex_ignores_budget_limit() {
        let driver = CodexDriver;
        let config = AgentConfig {
            max_budget_usd: Some(5.0),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert!(!args_contain(&cmd.args, "--max-budget-usd"));
    }

    #[test]
    fn codex_ignores_tool_allowlist() {
        let driver = CodexDriver;
        let config = AgentConfig {
            allowed_tools: Some(vec!["Read".into(), "Edit".into()]),
            disallowed_tools: Some(vec!["Bash".into()]),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert!(!args_contain(&cmd.args, "--allowedTools"));
        assert!(!args_contain(&cmd.args, "--disallowedTools"));
    }

    // -----------------------------------------------------------------------
    // Full config combination tests (mixed agent scenarios)
    // -----------------------------------------------------------------------

    #[test]
    fn claude_full_config_combination() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            model: Some("claude-opus-4-20250514".into()),
            system_prompt: Some("You are helpful.".into()),
            max_turns: Some(10),
            max_budget_usd: Some(2.0),
            access_mode: AccessMode::Edit,
            tool_toggles: ToolToggles {
                web_search: Some(false),
            },
            ephemeral_session: true,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(
            arg_after(&cmd.args, "--model"),
            Some("claude-opus-4-20250514")
        );
        assert_eq!(
            arg_after(&cmd.args, "--append-system-prompt"),
            Some("You are helpful.")
        );
        assert_eq!(arg_after(&cmd.args, "--max-turns"), Some("10"));
        assert_eq!(arg_after(&cmd.args, "--max-budget-usd"), Some("2"));
        assert!(!args_contain(&cmd.args, "--json-schema"));
        assert!(args_contain(&cmd.args, "--allowedTools"));
        assert!(args_contain(&cmd.args, "--no-session-persistence"));
    }

    #[test]
    fn codex_full_config_combination() {
        let driver = CodexDriver;
        let config = AgentConfig {
            model: Some("o3-mini".into()),
            reasoning_level: Some(ReasoningLevel::Medium),
            access_mode: AccessMode::Edit,
            tool_toggles: ToolToggles {
                web_search: Some(true),
            },
            ephemeral_session: true,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--model"), Some("o3-mini"));
        assert_eq!(
            arg_after(&cmd.args, "-c"),
            Some("model_reasoning_effort=medium")
        );
        assert_eq!(arg_after(&cmd.args, "--sandbox"), Some("workspace-write"));
        assert!(args_contain(&cmd.args, "--search"));
        assert!(args_contain(&cmd.args, "--ephemeral"));
    }

    // -----------------------------------------------------------------------
    // Capabilities
    // -----------------------------------------------------------------------

    #[test]
    fn claude_capabilities() {
        let driver = ClaudeDriver;
        let caps = driver.capabilities();
        assert!(caps.worker_execution);
        assert!(caps.structured_output);
        assert!(caps.native_json_schema);
        assert!(caps.session_reuse);
        assert!(caps.model_selection);
        assert!(!caps.reasoning_config); // Claude doesn't support reasoning config
        assert!(caps.system_prompt);
        assert!(caps.budget_limit);
        assert!(caps.turn_limit);
        assert!(caps.cost_reporting);
        assert!(caps.tool_allowlist);
        assert!(caps.web_search);
    }

    #[test]
    fn codex_capabilities() {
        let driver = CodexDriver;
        let caps = driver.capabilities();
        assert!(caps.worker_execution);
        assert!(caps.structured_output);
        assert!(caps.native_json_schema);
        assert!(caps.session_reuse);
        assert!(caps.model_selection);
        assert!(caps.reasoning_config);
        assert!(!caps.system_prompt);
        assert!(!caps.budget_limit);
        assert!(!caps.turn_limit);
        assert!(!caps.cost_reporting);
        assert!(!caps.tool_allowlist);
        assert!(caps.web_search);
    }

    #[test]
    fn registry_profile_driver_capabilities_default_when_absent() {
        // A registry profile that isn't in the registry falls back to the
        // worker-execution-only capability set.
        let driver = RegistryProfileDriver::new("definitely-not-a-real-agent");
        let caps = driver.capabilities();
        assert!(caps.worker_execution);
        assert!(!caps.structured_output);
        assert!(!caps.session_reuse);
        assert!(!caps.reasoning_config);
    }

    #[test]
    fn registry_profile_driver_hides_unrendered_registry_capabilities() {
        let claude_caps = RegistryProfileDriver::new("claude").capabilities();

        assert!(claude_caps.worker_execution);
        assert!(claude_caps.structured_output);
        assert!(claude_caps.session_reuse);
        assert!(claude_caps.native_json_schema);
        assert!(claude_caps.cost_reporting);
        assert!(!claude_caps.model_selection);
        assert!(!claude_caps.system_prompt);
        assert!(!claude_caps.budget_limit);
        assert!(!claude_caps.turn_limit);
        assert!(!claude_caps.tool_allowlist);
        assert!(!claude_caps.web_search);

        let codex_caps = RegistryProfileDriver::new("codex").capabilities();
        assert!(!codex_caps.reasoning_config);
    }

    #[test]
    fn agent_access_profile_names_includes_known_agents() {
        let profiles = agent_access_profile_names("agy");
        assert!(profiles.contains(&"default".to_string()));
        assert!(profiles.contains(&"workspace-write".to_string()));
        assert!(!profiles.contains(&"read-only".to_string()));
    }

    #[test]
    fn registry_profile_agy_rejects_read_only_without_a_read_only_profile() {
        let driver = RegistryProfileDriver::new("agy");
        let config = AgentConfig {
            access_mode: AccessMode::ReadOnly,
            ..default_config()
        };
        let err = driver.build_session_args(&config).unwrap_err();
        assert_eq!(err.to_string(), "agent agy has no read-only access profile");
    }

    #[test]
    fn registry_profile_agy_unrestricted_uses_full_access_profile() {
        let driver = RegistryProfileDriver::new("agy");
        let config = AgentConfig {
            access_mode: AccessMode::Unrestricted,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert!(args_contain(&cmd.args, "--dangerously-skip-permissions"));
    }

    #[test]
    fn registry_edit_rejects_unranked_default_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("agents.toml");
        std::fs::write(
            &path,
            r#"
[custom]
binary = "custom-agent"

[custom.access.default]
args = ["--dangerously-write-anywhere"]
"#,
        )
        .unwrap();
        let (registry, warnings) = agents::Registry::load_with_user_path(Some(&path)).unwrap();
        assert!(warnings.is_empty());
        let config = AgentConfig {
            access_mode: AccessMode::Edit,
            ..default_config()
        };

        let error = resolve_registry_access_profile(&registry, "custom", &config).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("access profile default has no declared privilege rank"),
            "unexpected error: {error}"
        );
    }

    static REGISTRY_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn install_temp_agent_registry(toml: &str) -> (tempfile::TempDir, Option<std::ffi::OsString>) {
        let temp = tempfile::tempdir().unwrap();
        let agents_dir = temp.path().join("tmux-tools");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(agents_dir.join("agents.toml"), toml).unwrap();
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        // SAFETY: `REGISTRY_ENV_LOCK` serializes all tests that mutate `XDG_CONFIG_HOME`.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", temp.path());
        }
        (temp, previous)
    }

    fn restore_xdg_config_home(previous: Option<std::ffi::OsString>) {
        // SAFETY: callers hold `REGISTRY_ENV_LOCK` while restoring the prior value.
        unsafe {
            match previous {
                Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }

    const UNRANKED_PROFILE_TOML: &str = r#"
[custom]
binary = "custom-agent"

[custom.access.unranked]
args = ["--dangerously-write-anywhere"]
"#;

    #[test]
    fn access_profile_privilege_rejects_unranked_profile() {
        let _lock = REGISTRY_ENV_LOCK.lock().unwrap();
        let (_temp, previous) = install_temp_agent_registry(UNRANKED_PROFILE_TOML);
        let error = access_profile_privilege("custom", "unranked").unwrap_err();
        restore_xdg_config_home(previous);
        assert!(
            error
                .to_string()
                .contains("has no declared privilege rank"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn resolved_access_profile_rejects_unranked_default_fallback() {
        let _lock = REGISTRY_ENV_LOCK.lock().unwrap();
        let toml = r#"
[custom]
binary = "custom-agent"

[custom.access.default]
args = ["--dangerously-write-anywhere"]
"#;
        let (_temp, previous) = install_temp_agent_registry(toml);
        let config = AgentConfig {
            access_mode: AccessMode::Edit,
            ..default_config()
        };
        let error = resolved_access_profile("custom", &config).unwrap_err();
        restore_xdg_config_home(previous);
        assert!(
            error
                .to_string()
                .contains("has no declared privilege rank"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn registry_edit_rejects_unranked_access_profile_override() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("agents.toml");
        std::fs::write(
            &path,
            r#"
[custom]
binary = "custom-agent"

[custom.access.unranked]
args = ["--dangerously-write-anywhere"]

[custom.access.workspace-write]
args = ["--write"]
"#,
        )
        .unwrap();
        let (registry, warnings) = agents::Registry::load_with_user_path(Some(&path)).unwrap();
        assert!(warnings.is_empty());
        let config = AgentConfig {
            access_mode: AccessMode::Edit,
            access_profile_override: Some("unranked".into()),
            ..default_config()
        };

        let error = resolve_registry_access_profile(&registry, "custom", &config).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("has no declared privilege rank"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn registry_edit_accepts_narrow_ranked_default_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("agents.toml");
        std::fs::write(
            &path,
            r#"
[custom]
binary = "custom-agent"

[custom.access.default]
args = ["--read-only"]

[custom.access.read-only]
args = ["--read-only"]
"#,
        )
        .unwrap();
        let (registry, warnings) = agents::Registry::load_with_user_path(Some(&path)).unwrap();
        assert!(warnings.is_empty());
        let config = AgentConfig {
            access_mode: AccessMode::Edit,
            ..default_config()
        };

        let profile = resolve_registry_access_profile(&registry, "custom", &config).unwrap();

        assert_eq!(profile, "default");
    }

    #[test]
    fn agent_access_profile_override_rejects_unknown_profile() {
        let driver = CodexDriver;
        let config = AgentConfig {
            access_profile_override: Some("read_only".to_string()),
            ..default_config()
        };

        let err = driver.build_session_args(&config).unwrap_err();

        assert_eq!(
            err.to_string(),
            "agent codex has no access profile read_only"
        );
    }

    #[test]
    fn agent_access_profile_override_cannot_widen_access_mode() {
        let driver = CodexDriver;
        let config = AgentConfig {
            access_mode: AccessMode::ReadOnly,
            access_profile_override: Some("workspace-write".to_string()),
            ..default_config()
        };

        let err = driver.build_session_args(&config).unwrap_err();

        assert_eq!(
            err.to_string(),
            "access profile workspace-write would widen ReadOnly access mode for agent codex"
        );
    }

    #[test]
    fn registry_profile_accepts_rendered_access_profile_override() {
        let driver = RegistryProfileDriver::new("cursor");
        let config = AgentConfig {
            access_profile_override: Some("read-only".to_string()),
            ..default_config()
        };

        let cmd = driver.build_session_args(&config).unwrap();

        assert_eq!(cmd.access_profile, "read-only");
        assert_eq!(arg_after(&cmd.args, "--mode"), Some("ask"));
    }

    #[test]
    fn registry_profile_rejects_unsupported_model() {
        let driver = RegistryProfileDriver::new("agy");
        let config = AgentConfig {
            model: Some("gpt-4".into()),
            ..default_config()
        };
        let err = driver.build_session_args(&config).unwrap_err();
        assert!(err.to_string().contains("model selection"));
    }

    #[test]
    fn registry_profile_rejects_unsupported_system_prompt() {
        let driver = RegistryProfileDriver::new("cursor");
        let config = AgentConfig {
            system_prompt: Some("be helpful".into()),
            ..default_config()
        };
        let err = driver.build_session_args(&config).unwrap_err();
        assert!(err.to_string().contains("system prompt"));
    }

    #[test]
    fn registry_profile_rejects_unsupported_reasoning_config() {
        let driver = RegistryProfileDriver::new("cursor");
        let config = AgentConfig {
            reasoning_level: Some(ReasoningLevel::High),
            ..default_config()
        };
        let err = driver.build_session_args(&config).unwrap_err();
        assert_eq!(err.to_string(), "agent does not support reasoning config");
    }

    #[test]
    fn registry_profile_rejects_unsupported_budget_limit() {
        let driver = RegistryProfileDriver::new("cursor");
        let config = AgentConfig {
            max_budget_usd: Some(1.25),
            ..default_config()
        };
        let err = driver.build_session_args(&config).unwrap_err();
        assert_eq!(err.to_string(), "agent does not support budget limit");
    }

    #[test]
    fn registry_profile_rejects_unsupported_turn_limit() {
        let driver = RegistryProfileDriver::new("cursor");
        let config = AgentConfig {
            max_turns: Some(3),
            ..default_config()
        };
        let err = driver.build_session_args(&config).unwrap_err();
        assert_eq!(err.to_string(), "agent does not support turn limit");
    }

    #[test]
    fn registry_profile_rejects_unsupported_web_search_toggle() {
        let driver = RegistryProfileDriver::new("cursor");
        let config = AgentConfig {
            tool_toggles: ToolToggles {
                web_search: Some(false),
            },
            ..default_config()
        };
        let err = driver.build_session_args(&config).unwrap_err();
        assert_eq!(err.to_string(), "agent does not support web search toggle");
    }

    #[test]
    fn registry_profile_rejects_unsupported_disallowed_tools() {
        let driver = RegistryProfileDriver::new("cursor");
        let config = AgentConfig {
            disallowed_tools: Some(vec!["Bash(rm *)".into()]),
            ..default_config()
        };
        let err = driver.build_session_args(&config).unwrap_err();
        assert_eq!(err.to_string(), "agent does not support tool denylist");
    }

    #[test]
    fn registry_interaction_pattern_maps_auto_response() {
        let pattern = registry_interaction_pattern(&agents::InteractionPatternSpec::new(
            "Press Enter to continue".to_owned(),
            agents::InteractionKind::AutoRespond,
            "Continue prompt".to_owned(),
            Some("y".to_owned()),
            false,
        ))
        .unwrap();

        assert_eq!(
            pattern.kind,
            InteractionKind::AutoRespond {
                response: "y".to_owned()
            }
        );
        assert_eq!(pattern.pattern, "Press Enter to continue");
        assert_eq!(pattern.description, "Continue prompt");
        assert!(!pattern.send_enter);
    }

    #[test]
    fn builtin_regex_patterns_compile() {
        use regex::Regex;

        for pattern in shared_destructive_patterns() {
            assert!(
                Regex::new(pattern).is_ok(),
                "shared destructive pattern failed to compile: {pattern}"
            );
        }

        for (driver_name, patterns) in [
            ("claude", ClaudeDriver.interaction_patterns()),
            ("codex", CodexDriver.interaction_patterns()),
        ] {
            for pattern in patterns {
                assert!(
                    Regex::new(&pattern.pattern).is_ok(),
                    "interaction pattern for {driver_name} failed to compile ({}): {}",
                    pattern.description,
                    pattern.pattern
                );
            }
        }
    }
}
