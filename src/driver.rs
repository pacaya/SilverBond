//! Agent Abstraction Layer — driver trait and implementations for CLI-based agent backends.
//!
//! Each agent backend (Claude, Codex, etc.) is represented by an [`AgentDriver`] implementation
//! that knows how to build CLI arguments for interactive PTY sessions and parse agent output.

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pty_output::{CostInfo, ContextInfo};


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
    pub tool_toggles: ToolToggles,
    pub allowed_tools: Option<Vec<String>>,
    pub disallowed_tools: Option<Vec<String>>,
    pub cwd: String,
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
            tool_toggles: ToolToggles::default(),
            allowed_tools: None,
            disallowed_tools: None,
            cwd: String::new(),
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

/// CLI arguments and extra environment variables produced by [`AgentDriver::build_session_args`].
#[derive(Debug, Clone)]
pub struct CommandArgs {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
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
}

// ===========================================================================
// Claude driver
// ===========================================================================

/// Driver for the Anthropic **Claude Code** CLI.
pub struct ClaudeDriver;

impl AgentDriver for ClaudeDriver {
    fn name(&self) -> &str {
        "claude"
    }

    fn capabilities(&self) -> AgentCapabilities {
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
        }
    }

    fn build_session_args(&self, config: &AgentConfig) -> anyhow::Result<CommandArgs> {
        let mut args = Vec::new();

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

        // Access mode / tool control ------------------------------------------------
        // Fine-grained tool control overrides the access-mode shorthand when present.
        if config.allowed_tools.is_some() || config.disallowed_tools.is_some() {
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
        } else {
            match config.access_mode {
                AccessMode::ReadOnly => {
                    args.push("--permission-mode".to_string());
                    args.push("plan".to_string());
                }
                AccessMode::Edit => {
                    for tool in &[
                        "Read",
                        "Edit",
                        "Write",
                        "Glob",
                        "Grep",
                        "Bash(git *)",
                    ] {
                        args.push("--allowedTools".to_string());
                        args.push(tool.to_string());
                    }
                }
                AccessMode::Execute | AccessMode::Unrestricted => {
                    args.push("--dangerously-skip-permissions".to_string());
                }
            }
        }

        // Web search toggle
        if let Some(false) = config.tool_toggles.web_search {
            args.push("--disallowedTools".to_string());
            args.push("WebSearch".to_string());
            args.push("--disallowedTools".to_string());
            args.push("WebFetch".to_string());
        }

        Ok(CommandArgs {
            args,
            env: Vec::new(),
            temp_dir: None,
        })
    }

    fn cost_command(&self) -> Option<&str> { Some("/cost") }
    fn context_command(&self) -> Option<&str> { Some("/context") }
    fn exit_command(&self) -> &str { "/exit" }

    fn parse_cost_response(&self, output: &str) -> Option<CostInfo> {
        crate::pty_output::parse_claude_cost(output)
    }

    fn parse_context_response(&self, output: &str) -> Option<ContextInfo> {
        crate::pty_output::parse_claude_context(output)
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
        }
    }

    fn build_session_args(&self, config: &AgentConfig) -> anyhow::Result<CommandArgs> {
        let mut args = Vec::new();

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

        // Web search
        if let Some(true) = config.tool_toggles.web_search {
            args.push("--search".to_string());
        }

        Ok(CommandArgs {
            args,
            env: Vec::new(),
            temp_dir: None,
        })
    }

    fn cost_command(&self) -> Option<&str> { None }
    fn context_command(&self) -> Option<&str> { None }
    fn exit_command(&self) -> &str { "/exit" }
    fn parse_cost_response(&self, _output: &str) -> Option<CostInfo> { None }
    fn parse_context_response(&self, _output: &str) -> Option<ContextInfo> { None }
}

// ===========================================================================
// Gemini driver
// ===========================================================================

/// Driver for the Google **Gemini CLI**.
pub struct GeminiDriver;

impl GeminiDriver {
    /// Thinking budget for each reasoning level.
    fn thinking_budget(level: &ReasoningLevel) -> u32 {
        match level {
            ReasoningLevel::Low => 2048,
            ReasoningLevel::Medium => 8192,
            ReasoningLevel::High => 32768,
        }
    }

    /// Build a Gemini settings JSON object for overrides that require a settings file
    /// (reasoning budget and/or web search toggle). Returns `None` if no settings needed.
    fn build_settings(config: &AgentConfig) -> Option<Value> {
        let needs_reasoning = config.reasoning_level.is_some();
        let needs_web_toggle = config.tool_toggles.web_search.is_some();

        if !needs_reasoning && !needs_web_toggle {
            return None;
        }

        let mut settings = serde_json::Map::new();

        // Web search toggle
        if let Some(enabled) = config.tool_toggles.web_search {
            settings.insert(
                "web_search".to_string(),
                Value::String(if enabled { "live" } else { "disabled" }.to_string()),
            );
        }

        // Reasoning / thinking budget via modelConfigs overrides
        if let Some(level) = &config.reasoning_level {
            let budget = Self::thinking_budget(level);
            settings.insert(
                "modelConfigs".to_string(),
                serde_json::json!({
                    "overrides": [{
                        "generateContentConfig": {
                            "thinkingConfig": {
                                "thinkingBudget": budget
                            }
                        }
                    }]
                }),
            );
        }

        Some(Value::Object(settings))
    }

    /// Create a temporary directory with a `settings.json` file for Gemini CLI overrides.
    /// Returns the temp directory path to set as `GEMINI_CLI_HOME`.
    fn write_temp_settings(settings: &Value) -> anyhow::Result<PathBuf> {
        let temp_dir = tempfile::Builder::new()
            .prefix("silverbond-gemini-")
            .tempdir()
            .context("Failed to create temp directory for Gemini settings")?;
        let settings_path = temp_dir.path().join("settings.json");
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(settings)?,
        )
        .context("Failed to write Gemini temp settings")?;
        // keep() prevents automatic cleanup — caller is responsible for removal
        Ok(temp_dir.keep())
    }

}

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
            let ty = prop
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("any");
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

impl AgentDriver for GeminiDriver {
    fn name(&self) -> &str {
        "gemini"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            worker_execution: true,
            prompt_refinement: true,
            branch_choice: true,
            loop_verdict: true,
            structured_output: true,
            native_json_schema: false,
            session_reuse: true,
            model_selection: true,
            reasoning_config: true,
            system_prompt: false,
            budget_limit: false,
            turn_limit: false,
            cost_reporting: false,
            tool_allowlist: false,
            web_search: true,
        }
    }

    fn build_session_args(&self, config: &AgentConfig) -> anyhow::Result<CommandArgs> {
        let mut args = Vec::new();
        let mut env = Vec::new();
        let mut temp_dir = None;

        // Model selection
        if let Some(model) = &config.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }

        // Session resume (no ephemeral flag available for Gemini)
        if let Some(session_id) = &config.resume_session_id {
            args.push("--resume".to_string());
            args.push(session_id.clone());
        }

        // Access mode → approval mode
        match config.access_mode {
            AccessMode::ReadOnly => {
                args.push("--approval-mode".to_string());
                args.push("plan".to_string());
            }
            AccessMode::Edit => {
                args.push("--approval-mode".to_string());
                args.push("auto_edit".to_string());
            }
            AccessMode::Execute | AccessMode::Unrestricted => {
                args.push("--approval-mode".to_string());
                args.push("yolo".to_string());
            }
        }

        // Temp settings for reasoning level and/or web search
        if let Some(settings) = Self::build_settings(config) {
            let dir = Self::write_temp_settings(&settings)?;
            env.push(("GEMINI_CLI_HOME".to_string(), dir.display().to_string()));
            temp_dir = Some(dir);
        }

        Ok(CommandArgs {
            args,
            env,
            temp_dir,
        })
    }

    fn cost_command(&self) -> Option<&str> { None }
    fn context_command(&self) -> Option<&str> { None }
    fn exit_command(&self) -> &str { "/exit" }
    fn parse_cost_response(&self, _output: &str) -> Option<CostInfo> { None }
    fn parse_context_response(&self, _output: &str) -> Option<ContextInfo> { None }
}

// ===========================================================================
// Driver registry
// ===========================================================================

/// Look up a driver by CLI executable name.
pub fn get_driver(name: &str) -> Option<Box<dyn AgentDriver>> {
    match name {
        "claude" => Some(Box::new(ClaudeDriver)),
        "codex" => Some(Box::new(CodexDriver)),
        "gemini" => Some(Box::new(GeminiDriver)),
        _ => None,
    }
}

/// Return every registered driver.
pub fn all_drivers() -> Vec<Box<dyn AgentDriver>> {
    vec![
        Box::new(ClaudeDriver),
        Box::new(CodexDriver),
        Box::new(GeminiDriver),
    ]
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn default_config() -> AgentConfig {
        AgentConfig::default()
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
        assert_eq!(arg_after(&cmd.args, "--model"), Some("claude-sonnet-4-20250514"));
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
    fn claude_fine_grained_tools_override_access_mode() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            access_mode: AccessMode::ReadOnly,
            allowed_tools: Some(vec!["Read".into(), "Grep".into()]),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        // Should NOT have --permission-mode because fine-grained overrides
        assert!(!args_contain(&cmd.args, "--permission-mode"));
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
        assert_eq!(arg_after(&cmd.args, "-c"), Some("model_reasoning_effort=high"));
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
    fn registry_get_gemini() {
        let driver = get_driver("gemini").expect("gemini driver should exist");
        assert_eq!(driver.name(), "gemini");
    }

    #[test]
    fn registry_all_drivers() {
        let drivers = all_drivers();
        assert_eq!(drivers.len(), 3);
        let names: Vec<&str> = drivers.iter().map(|d| d.name()).collect();
        assert!(names.contains(&"claude"));
        assert!(names.contains(&"codex"));
        assert!(names.contains(&"gemini"));
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

    // -----------------------------------------------------------------------
    // GeminiDriver::build_session_args
    // -----------------------------------------------------------------------

    #[test]
    fn gemini_default_args() {
        let driver = GeminiDriver;
        let cmd = driver.build_session_args(&default_config()).unwrap();
        // Interactive mode: no --prompt, no --output-format json
        assert!(!args_contain(&cmd.args, "--prompt"));
        assert!(!args_contain(&cmd.args, "--output-format"));
        assert_eq!(arg_after(&cmd.args, "--approval-mode"), Some("yolo"));
        assert!(cmd.temp_dir.is_none());
    }

    #[test]
    fn gemini_model_selection() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            model: Some("gemini-2.5-pro".into()),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--model"), Some("gemini-2.5-pro"));
    }

    #[test]
    fn gemini_session_resume() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            resume_session_id: Some("gem-sess-1".into()),
            ephemeral_session: false,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--resume"), Some("gem-sess-1"));
    }

    #[test]
    fn gemini_access_mode_read_only() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            access_mode: AccessMode::ReadOnly,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--approval-mode"), Some("plan"));
    }

    #[test]
    fn gemini_access_mode_edit() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            access_mode: AccessMode::Edit,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--approval-mode"), Some("auto_edit"));
    }

    #[test]
    fn gemini_access_mode_execute() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            access_mode: AccessMode::Execute,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--approval-mode"), Some("yolo"));
    }

    #[test]
    fn gemini_access_mode_unrestricted() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            access_mode: AccessMode::Unrestricted,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--approval-mode"), Some("yolo"));
    }

    #[test]
    fn gemini_reasoning_level_creates_temp_settings() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            reasoning_level: Some(ReasoningLevel::High),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();

        // Should have GEMINI_CLI_HOME env var pointing to a temp dir
        let home = cmd
            .env
            .iter()
            .find(|(k, _)| k == "GEMINI_CLI_HOME")
            .map(|(_, v)| v.clone());
        assert!(home.is_some(), "GEMINI_CLI_HOME should be set");

        // Verify settings file exists with thinking budget
        let settings_path = PathBuf::from(home.unwrap()).join("settings.json");
        assert!(settings_path.exists());
        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let budget = content
            .get("modelConfigs")
            .and_then(|mc| mc.get("overrides"))
            .and_then(|o| o.as_array())
            .and_then(|arr| arr.first())
            .and_then(|o| o.get("generateContentConfig"))
            .and_then(|gc| gc.get("thinkingConfig"))
            .and_then(|tc| tc.get("thinkingBudget"))
            .and_then(|v| v.as_u64());
        assert_eq!(budget, Some(32768));

        // Cleanup
        if let Some(dir) = cmd.temp_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn gemini_reasoning_level_budgets() {
        assert_eq!(GeminiDriver::thinking_budget(&ReasoningLevel::Low), 2048);
        assert_eq!(GeminiDriver::thinking_budget(&ReasoningLevel::Medium), 8192);
        assert_eq!(GeminiDriver::thinking_budget(&ReasoningLevel::High), 32768);
    }

    #[test]
    fn gemini_web_search_creates_temp_settings() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            tool_toggles: ToolToggles {
                web_search: Some(false),
            },
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();

        let home = cmd
            .env
            .iter()
            .find(|(k, _)| k == "GEMINI_CLI_HOME")
            .map(|(_, v)| v.clone());
        assert!(home.is_some());

        let settings_path = PathBuf::from(home.unwrap()).join("settings.json");
        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(
            content.get("web_search").and_then(|v| v.as_str()),
            Some("disabled")
        );

        if let Some(dir) = cmd.temp_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn gemini_web_search_enabled_settings() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            tool_toggles: ToolToggles {
                web_search: Some(true),
            },
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();

        let home = cmd
            .env
            .iter()
            .find(|(k, _)| k == "GEMINI_CLI_HOME")
            .map(|(_, v)| v.clone());
        assert!(home.is_some());

        let settings_path = PathBuf::from(home.unwrap()).join("settings.json");
        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(
            content.get("web_search").and_then(|v| v.as_str()),
            Some("live")
        );

        if let Some(dir) = cmd.temp_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn gemini_no_temp_settings_when_not_needed() {
        let driver = GeminiDriver;
        let cmd = driver.build_session_args(&default_config()).unwrap();
        assert!(cmd.temp_dir.is_none());
        assert!(cmd.env.is_empty());
    }

    #[test]
    fn gemini_schema_to_prompt_hint_empty() {
        let schema = json!({"type": "object"});
        let hint = schema_to_prompt_hint(&schema);
        assert!(hint.is_empty());
    }

    // -----------------------------------------------------------------------
    // GeminiDriver PTY methods
    // -----------------------------------------------------------------------

    #[test]
    fn gemini_cost_command() {
        let driver = GeminiDriver;
        assert_eq!(driver.cost_command(), None);
    }

    #[test]
    fn gemini_exit_command() {
        let driver = GeminiDriver;
        assert_eq!(driver.exit_command(), "/exit");
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

    #[test]
    fn gemini_ignores_system_prompt() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            system_prompt: Some("Be concise.".into()),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert!(!args_contain(&cmd.args, "--append-system-prompt"));
        assert!(!cmd.args.iter().any(|a| a.contains("Be concise")));
    }

    #[test]
    fn gemini_ignores_budget_and_turn_limits() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            max_budget_usd: Some(5.0),
            max_turns: Some(10),
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert!(!args_contain(&cmd.args, "--max-budget-usd"));
        assert!(!args_contain(&cmd.args, "--max-turns"));
    }

    #[test]
    fn gemini_ignores_tool_allowlist() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            allowed_tools: Some(vec!["Read".into()]),
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
            tool_toggles: ToolToggles { web_search: Some(false) },
            ephemeral_session: true,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--model"), Some("claude-opus-4-20250514"));
        assert_eq!(arg_after(&cmd.args, "--append-system-prompt"), Some("You are helpful."));
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
            tool_toggles: ToolToggles { web_search: Some(true) },
            ephemeral_session: true,
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--model"), Some("o3-mini"));
        assert_eq!(arg_after(&cmd.args, "-c"), Some("model_reasoning_effort=medium"));
        assert_eq!(arg_after(&cmd.args, "--sandbox"), Some("workspace-write"));
        assert!(args_contain(&cmd.args, "--search"));
        assert!(args_contain(&cmd.args, "--ephemeral"));
    }

    #[test]
    fn gemini_full_config_combination() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            model: Some("gemini-2.5-flash".into()),
            reasoning_level: Some(ReasoningLevel::Low),
            access_mode: AccessMode::ReadOnly,
            tool_toggles: ToolToggles { web_search: Some(true) },
            ..default_config()
        };
        let cmd = driver.build_session_args(&config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--model"), Some("gemini-2.5-flash"));
        assert_eq!(arg_after(&cmd.args, "--approval-mode"), Some("plan"));
        // Reasoning + web search → temp settings with GEMINI_CLI_HOME
        assert!(cmd.env.iter().any(|(k, _)| k == "GEMINI_CLI_HOME"));
        // Cleanup
        if let Some(dir) = cmd.temp_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
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
    fn gemini_capabilities() {
        let driver = GeminiDriver;
        let caps = driver.capabilities();
        assert!(caps.worker_execution);
        assert!(caps.prompt_refinement);
        assert!(caps.branch_choice);
        assert!(caps.loop_verdict);
        assert!(caps.structured_output);
        assert!(!caps.native_json_schema);
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
}
