//! Agent Abstraction Layer — driver trait and implementations for CLI-based agent backends.
//!
//! Each agent backend (Claude, Codex, etc.) is represented by an [`AgentDriver`] implementation
//! that knows how to build CLI arguments and parse the agent's stdout into a normalized
//! [`AgentOutput`].

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Convert a `u64` counter to `Option<u64>`, mapping zero to `None`.
fn nonzero_u64(v: u64) -> Option<u64> {
    (v > 0).then_some(v)
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

/// Fully-resolved configuration passed to a driver's `build_args` method.
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
// Command args returned by build_args
// ---------------------------------------------------------------------------

/// CLI arguments and extra environment variables produced by [`AgentDriver::build_args`].
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

/// A backend-specific driver that knows how to invoke an agent CLI and parse its output.
pub trait AgentDriver: Send + Sync {
    /// CLI executable name (e.g. `"claude"`, `"codex"`).
    fn name(&self) -> &str;

    /// Capability flags for this backend.
    fn capabilities(&self) -> AgentCapabilities;

    /// Build the CLI argument list for a given prompt and configuration.
    fn build_args(&self, prompt: &str, config: &AgentConfig) -> anyhow::Result<CommandArgs>;

    /// Parse raw CLI output into a normalised [`AgentOutput`].
    fn parse_output(
        &self,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
    ) -> anyhow::Result<AgentOutput>;
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

    fn build_args(&self, prompt: &str, config: &AgentConfig) -> anyhow::Result<CommandArgs> {
        let mut args = vec![
            "--print".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
        ];

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

        // JSON Schema for structured output
        if let Some(schema) = &config.json_schema {
            args.push("--json-schema".to_string());
            args.push(serde_json::to_string(schema)?);
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

        // Prompt is always the final positional argument.
        args.push(prompt.to_string());

        Ok(CommandArgs {
            args,
            env: Vec::new(),
            temp_dir: None,
        })
    }

    fn parse_output(
        &self,
        stdout: &str,
        _stderr: &str,
        exit_code: i32,
    ) -> anyhow::Result<AgentOutput> {
        let parsed: Value = serde_json::from_str(stdout.trim())
            .context("Failed to parse Claude JSON output")?;

        let subtype = parsed
            .get("subtype")
            .and_then(|v| v.as_str())
            .unwrap_or("success");

        let outcome = match subtype {
            "success" => NodeOutcome::Success,
            "error_max_turns" => NodeOutcome::ErrorMaxTurns,
            "error_max_budget_usd" => NodeOutcome::ErrorMaxBudget,
            "error_during_execution" => NodeOutcome::ErrorExecution,
            _ => {
                if exit_code != 0 {
                    NodeOutcome::ErrorExecution
                } else {
                    NodeOutcome::Success
                }
            }
        };

        let response_text = parsed
            .get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let session_id = parsed
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let cost_usd = parsed.get("total_cost_usd").and_then(|v| v.as_f64());

        let usage = parsed.get("usage");
        let input_tokens = usage
            .and_then(|u| u.get("input_tokens"))
            .and_then(|v| v.as_u64());
        let output_tokens = usage
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_u64());
        let cache_read_tokens = usage
            .and_then(|u| u.get("cache_read_input_tokens"))
            .and_then(|v| v.as_u64());
        let cache_write_tokens = usage
            .and_then(|u| u.get("cache_creation_input_tokens"))
            .and_then(|v| v.as_u64());

        let num_turns = parsed
            .get("num_turns")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let duration_api_ms = parsed.get("duration_ms").and_then(|v| v.as_u64());

        let model_used = parsed
            .get("modelUsage")
            .and_then(|v| v.as_object())
            .and_then(|m| m.keys().next())
            .map(|s| s.to_string());

        let structured_output = parsed.get("structured_output").cloned();

        let is_error = parsed
            .get("is_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let error_message = if is_error || !outcome.is_success() {
            Some(response_text.clone())
        } else {
            None
        };

        Ok(AgentOutput {
            response_text,
            session_id,
            cost_usd,
            input_tokens,
            output_tokens,
            thinking_tokens: None,
            cache_read_tokens,
            cache_write_tokens,
            model_used,
            num_turns,
            duration_api_ms,
            structured_output,
            outcome,
            error_message,
        })
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

    fn build_args(&self, prompt: &str, config: &AgentConfig) -> anyhow::Result<CommandArgs> {
        let mut args = Vec::new();

        // Sub-command: `exec resume <id>` or plain `exec`
        if let Some(session_id) = &config.resume_session_id {
            args.push("exec".to_string());
            args.push("resume".to_string());
            args.push(session_id.clone());
        } else {
            args.push("exec".to_string());
        }

        // JSON output
        args.push("--json".to_string());

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

        // JSON Schema for structured output — write to temp file
        let mut temp_dir = None;
        if let Some(schema) = &config.json_schema {
            let dir = tempfile::Builder::new()
                .prefix("silverbond-codex-schema-")
                .tempdir()
                .context("Failed to create temp directory for Codex output schema")?;
            let schema_path = dir.path().join("output_schema.json");
            std::fs::write(&schema_path, serde_json::to_string(schema)?)?;
            args.push("--output-schema".to_string());
            args.push(schema_path.to_string_lossy().to_string());
            temp_dir = Some(dir.keep());
        }

        // Prompt is the final positional argument.
        args.push(prompt.to_string());

        Ok(CommandArgs {
            args,
            env: Vec::new(),
            temp_dir,
        })
    }

    fn parse_output(
        &self,
        stdout: &str,
        _stderr: &str,
        _exit_code: i32,
    ) -> anyhow::Result<AgentOutput> {
        let mut session_id = None;
        let mut input_tokens: u64 = 0;
        let mut output_tokens: u64 = 0;
        let mut reasoning_tokens: u64 = 0;
        let mut response_text = String::new();
        let mut outcome = NodeOutcome::Success;
        let mut error_message = None;

        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(event) = serde_json::from_str::<Value>(line) else {
                continue;
            };

            let event_type = event
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match event_type {
                "thread.started" => {
                    session_id = event
                        .get("thread_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
                "turn.completed" => {
                    if let Some(tokens) = event.get("input_tokens").and_then(|v| v.as_u64()) {
                        input_tokens += tokens;
                    }
                    if let Some(tokens) = event.get("output_tokens").and_then(|v| v.as_u64()) {
                        output_tokens += tokens;
                    }
                    if let Some(tokens) =
                        event.get("reasoning_output_tokens").and_then(|v| v.as_u64())
                    {
                        reasoning_tokens += tokens;
                    }
                }
                "turn.failed" => {
                    outcome = NodeOutcome::ErrorExecution;
                    error_message = event
                        .get("error")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| Some("Turn failed".to_string()));
                }
                "item.completed" => {
                    // Extract text from the last text content block.
                    if let Some(text) = event
                        .get("content")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| {
                            arr.iter()
                                .rev()
                                .find(|item| {
                                    item.get("type").and_then(|v| v.as_str()) == Some("text")
                                })
                        })
                        .and_then(|item| item.get("text"))
                        .and_then(|v| v.as_str())
                    {
                        response_text = text.to_string();
                    }
                    // Fallback: direct `text` field.
                    if response_text.is_empty() {
                        if let Some(text) = event.get("text").and_then(|v| v.as_str()) {
                            response_text = text.to_string();
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(AgentOutput {
            response_text,
            session_id,
            cost_usd: None,
            input_tokens: nonzero_u64(input_tokens),
            output_tokens: nonzero_u64(output_tokens),
            thinking_tokens: nonzero_u64(reasoning_tokens),
            cache_read_tokens: None,
            cache_write_tokens: None,
            model_used: None,
            num_turns: None,
            duration_api_ms: None,
            structured_output: None,
            outcome,
            error_message,
        })
    }
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

    fn build_args(&self, prompt: &str, config: &AgentConfig) -> anyhow::Result<CommandArgs> {
        let mut args = vec![
            "--prompt".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
        ];

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

        // Structured output fallback: prompt injection for JSON schema
        // (Gemini doesn't support native --json-schema)
        let final_prompt = if let Some(schema) = &config.json_schema {
            let hint = schema_to_prompt_hint(schema);
            format!(
                "{prompt}\n\nIMPORTANT: You MUST respond with valid JSON only. \
                 No markdown, no explanation — just a single JSON object.{hint}"
            )
        } else {
            prompt.to_string()
        };

        args.push(final_prompt);

        Ok(CommandArgs {
            args,
            env,
            temp_dir,
        })
    }

    fn parse_output(
        &self,
        stdout: &str,
        _stderr: &str,
        exit_code: i32,
    ) -> anyhow::Result<AgentOutput> {
        let parsed: Value = serde_json::from_str(stdout.trim())
            .context("Failed to parse Gemini JSON output")?;

        // Check for error object
        let error = parsed.get("error");
        let error_code = error.and_then(|e| e.get("code")).and_then(|v| v.as_i64());
        let error_message = error
            .and_then(|e| e.get("message"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let outcome = match (exit_code, error_code) {
            (0, None) => NodeOutcome::Success,
            (_, Some(53)) => NodeOutcome::ErrorMaxTurns,
            (_, Some(42)) => NodeOutcome::ErrorExecution, // input error
            _ => {
                if error.is_some() || exit_code != 0 {
                    NodeOutcome::ErrorExecution
                } else {
                    NodeOutcome::Success
                }
            }
        };

        let response_text = parsed
            .get("response")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let session_id = parsed
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Extract token stats from stats.models.*
        let mut input_tokens: u64 = 0;
        let mut output_tokens: u64 = 0;
        let mut thinking_tokens: u64 = 0;
        let mut cached_tokens: u64 = 0;
        let mut model_used = None;

        if let Some(models) = parsed
            .get("stats")
            .and_then(|s| s.get("models"))
            .and_then(|m| m.as_object())
        {
            for (model_name, stats) in models {
                if model_used.is_none() {
                    model_used = Some(model_name.clone());
                }
                if let Some(v) = stats.get("prompt").and_then(|v| v.as_u64()) {
                    input_tokens += v;
                }
                if let Some(v) = stats.get("candidates").and_then(|v| v.as_u64()) {
                    output_tokens += v;
                }
                if let Some(v) = stats.get("thoughts").and_then(|v| v.as_u64()) {
                    thinking_tokens += v;
                }
                if let Some(v) = stats.get("cached").and_then(|v| v.as_u64()) {
                    cached_tokens += v;
                }
            }
        }

        let final_error = if !outcome.is_success() {
            error_message.or_else(|| Some(response_text.clone()))
        } else {
            None
        };

        Ok(AgentOutput {
            response_text,
            session_id,
            cost_usd: None, // Gemini doesn't report cost
            input_tokens: nonzero_u64(input_tokens),
            output_tokens: nonzero_u64(output_tokens),
            thinking_tokens: nonzero_u64(thinking_tokens),
            cache_read_tokens: nonzero_u64(cached_tokens),
            cache_write_tokens: None,
            model_used,
            num_turns: None,
            duration_api_ms: None,
            structured_output: None,
            outcome,
            error_message: final_error,
        })
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
    // ClaudeDriver::build_args
    // -----------------------------------------------------------------------

    #[test]
    fn claude_default_args() {
        let driver = ClaudeDriver;
        let cmd = driver.build_args("hello", &default_config()).unwrap();
        assert!(args_contain(&cmd.args, "--print"));
        assert_eq!(arg_after(&cmd.args, "--output-format"), Some("json"));
        assert!(args_contain(&cmd.args, "--no-session-persistence"));
        assert!(args_contain(&cmd.args, "--dangerously-skip-permissions"));
        assert_eq!(cmd.args.last().unwrap(), "hello");
    }

    #[test]
    fn claude_model_selection() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            model: Some("claude-sonnet-4-20250514".into()),
            ..default_config()
        };
        let cmd = driver.build_args("hi", &config).unwrap();
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
        let cmd = driver.build_args("go", &config).unwrap();
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
        let cmd = driver.build_args("do it", &config).unwrap();
        assert_eq!(
            arg_after(&cmd.args, "--append-system-prompt"),
            Some("Be concise.")
        );
    }

    #[test]
    fn claude_json_schema() {
        let driver = ClaudeDriver;
        let schema = json!({"type": "object", "properties": {"name": {"type": "string"}}});
        let config = AgentConfig {
            json_schema: Some(schema.clone()),
            ..default_config()
        };
        let cmd = driver.build_args("extract", &config).unwrap();
        let schema_str = arg_after(&cmd.args, "--json-schema").unwrap();
        let parsed: Value = serde_json::from_str(schema_str).unwrap();
        assert_eq!(parsed, schema);
    }

    #[test]
    fn claude_session_resume() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            resume_session_id: Some("sess-42".into()),
            ephemeral_session: false,
            ..default_config()
        };
        let cmd = driver.build_args("continue", &config).unwrap();
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
        let cmd = driver.build_args("look", &config).unwrap();
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
        let cmd = driver.build_args("fix", &config).unwrap();
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
        let cmd = driver.build_args("search", &config).unwrap();
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
        let cmd = driver.build_args("peek", &config).unwrap();
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
    // CodexDriver::build_args
    // -----------------------------------------------------------------------

    #[test]
    fn codex_default_args() {
        let driver = CodexDriver;
        let cmd = driver.build_args("hello", &default_config()).unwrap();
        assert_eq!(cmd.args[0], "exec");
        assert!(args_contain(&cmd.args, "--json"));
        assert!(args_contain(&cmd.args, "--ephemeral"));
        assert!(args_contain(&cmd.args, "--full-auto"));
        assert_eq!(cmd.args.last().unwrap(), "hello");
    }

    #[test]
    fn codex_model_selection() {
        let driver = CodexDriver;
        let config = AgentConfig {
            model: Some("o3-mini".into()),
            ..default_config()
        };
        let cmd = driver.build_args("hi", &config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--model"), Some("o3-mini"));
    }

    #[test]
    fn codex_reasoning_level() {
        let driver = CodexDriver;
        let config = AgentConfig {
            reasoning_level: Some(ReasoningLevel::High),
            ..default_config()
        };
        let cmd = driver.build_args("think", &config).unwrap();
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
        let cmd = driver.build_args("go on", &config).unwrap();
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
        let cmd = driver.build_args("look", &config).unwrap();
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
        let cmd = driver.build_args("fix", &config).unwrap();
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
        let cmd = driver.build_args("yolo", &config).unwrap();
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
        let cmd = driver.build_args("find", &config).unwrap();
        assert!(args_contain(&cmd.args, "--search"));
    }

    #[test]
    fn codex_json_schema_creates_temp_file() {
        let driver = CodexDriver;
        let schema = json!({"type": "object", "properties": {"name": {"type": "string"}}});
        let config = AgentConfig {
            json_schema: Some(schema.clone()),
            ..default_config()
        };
        let cmd = driver.build_args("extract", &config).unwrap();
        assert!(args_contain(&cmd.args, "--output-schema"));
        let schema_path = arg_after(&cmd.args, "--output-schema").unwrap();
        // Verify temp file was created and contains the schema
        let contents = std::fs::read_to_string(schema_path).unwrap();
        let parsed: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed, schema);
        // Clean up temp dir
        if let Some(dir) = cmd.temp_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    // -----------------------------------------------------------------------
    // ClaudeDriver::parse_output
    // -----------------------------------------------------------------------

    #[test]
    fn claude_parse_success() {
        let driver = ClaudeDriver;
        let stdout = r#"{
            "type": "result",
            "subtype": "success",
            "session_id": "abc-123",
            "result": "Hello world",
            "total_cost_usd": 0.054,
            "usage": {
                "input_tokens": 1245,
                "output_tokens": 856,
                "cache_creation_input_tokens": 512,
                "cache_read_input_tokens": 256
            },
            "modelUsage": {
                "claude-sonnet-4-20250514": {
                    "inputTokens": 1245,
                    "outputTokens": 856,
                    "costUSD": 0.054
                }
            },
            "num_turns": 3,
            "duration_ms": 12345,
            "is_error": false
        }"#;

        let out = driver.parse_output(stdout, "", 0).unwrap();
        assert_eq!(out.outcome, NodeOutcome::Success);
        assert_eq!(out.response_text, "Hello world");
        assert_eq!(out.session_id.as_deref(), Some("abc-123"));
        assert_eq!(out.cost_usd, Some(0.054));
        assert_eq!(out.input_tokens, Some(1245));
        assert_eq!(out.output_tokens, Some(856));
        assert_eq!(out.cache_write_tokens, Some(512));
        assert_eq!(out.cache_read_tokens, Some(256));
        assert_eq!(out.num_turns, Some(3));
        assert_eq!(out.duration_api_ms, Some(12345));
        assert_eq!(out.model_used.as_deref(), Some("claude-sonnet-4-20250514"));
        assert!(out.error_message.is_none());
    }

    #[test]
    fn claude_parse_error_max_turns() {
        let driver = ClaudeDriver;
        let stdout = r#"{
            "type": "result",
            "subtype": "error_max_turns",
            "session_id": "abc-456",
            "result": "Max turns reached",
            "total_cost_usd": 0.1,
            "usage": { "input_tokens": 5000, "output_tokens": 3000 },
            "num_turns": 10,
            "is_error": true
        }"#;

        let out = driver.parse_output(stdout, "", 1).unwrap();
        assert_eq!(out.outcome, NodeOutcome::ErrorMaxTurns);
        assert!(!out.outcome.is_success());
        assert_eq!(out.response_text, "Max turns reached");
        assert_eq!(out.error_message.as_deref(), Some("Max turns reached"));
        assert_eq!(out.num_turns, Some(10));
        assert_eq!(out.cost_usd, Some(0.1));
    }

    #[test]
    fn claude_parse_error_max_budget() {
        let driver = ClaudeDriver;
        let stdout = r#"{
            "type": "result",
            "subtype": "error_max_budget_usd",
            "session_id": "abc-789",
            "result": "Budget exceeded",
            "total_cost_usd": 2.0,
            "usage": { "input_tokens": 10000, "output_tokens": 8000 },
            "num_turns": 20,
            "is_error": true
        }"#;

        let out = driver.parse_output(stdout, "", 1).unwrap();
        assert_eq!(out.outcome, NodeOutcome::ErrorMaxBudget);
        assert_eq!(out.error_message.as_deref(), Some("Budget exceeded"));
    }

    // -----------------------------------------------------------------------
    // CodexDriver::parse_output
    // -----------------------------------------------------------------------

    #[test]
    fn codex_parse_success() {
        let driver = CodexDriver;
        let stdout = r#"{"type":"thread.started","thread_id":"thread-789"}
{"type":"turn.completed","input_tokens":500,"output_tokens":200,"reasoning_output_tokens":50,"total_tokens":750}
{"type":"item.completed","content":[{"type":"text","text":"Result from codex"}]}"#;

        let out = driver.parse_output(stdout, "", 0).unwrap();
        assert_eq!(out.outcome, NodeOutcome::Success);
        assert_eq!(out.response_text, "Result from codex");
        assert_eq!(out.session_id.as_deref(), Some("thread-789"));
        assert_eq!(out.input_tokens, Some(500));
        assert_eq!(out.output_tokens, Some(200));
        assert_eq!(out.thinking_tokens, Some(50));
        assert!(out.error_message.is_none());
    }

    #[test]
    fn codex_parse_error() {
        let driver = CodexDriver;
        let stdout = r#"{"type":"thread.started","thread_id":"thread-err"}
{"type":"turn.failed","error":"Something went wrong"}"#;

        let out = driver.parse_output(stdout, "", 1).unwrap();
        assert_eq!(out.outcome, NodeOutcome::ErrorExecution);
        assert_eq!(
            out.error_message.as_deref(),
            Some("Something went wrong")
        );
        assert_eq!(out.session_id.as_deref(), Some("thread-err"));
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
    // GeminiDriver::build_args
    // -----------------------------------------------------------------------

    #[test]
    fn gemini_default_args() {
        let driver = GeminiDriver;
        let cmd = driver.build_args("hello", &default_config()).unwrap();
        assert!(args_contain(&cmd.args, "--prompt"));
        assert_eq!(arg_after(&cmd.args, "--output-format"), Some("json"));
        assert_eq!(arg_after(&cmd.args, "--approval-mode"), Some("yolo"));
        assert_eq!(cmd.args.last().unwrap(), "hello");
        assert!(cmd.temp_dir.is_none());
    }

    #[test]
    fn gemini_model_selection() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            model: Some("gemini-2.5-pro".into()),
            ..default_config()
        };
        let cmd = driver.build_args("hi", &config).unwrap();
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
        let cmd = driver.build_args("continue", &config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--resume"), Some("gem-sess-1"));
    }

    #[test]
    fn gemini_access_mode_read_only() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            access_mode: AccessMode::ReadOnly,
            ..default_config()
        };
        let cmd = driver.build_args("look", &config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--approval-mode"), Some("plan"));
    }

    #[test]
    fn gemini_access_mode_edit() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            access_mode: AccessMode::Edit,
            ..default_config()
        };
        let cmd = driver.build_args("fix", &config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--approval-mode"), Some("auto_edit"));
    }

    #[test]
    fn gemini_access_mode_execute() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            access_mode: AccessMode::Execute,
            ..default_config()
        };
        let cmd = driver.build_args("run", &config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--approval-mode"), Some("yolo"));
    }

    #[test]
    fn gemini_access_mode_unrestricted() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            access_mode: AccessMode::Unrestricted,
            ..default_config()
        };
        let cmd = driver.build_args("go", &config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--approval-mode"), Some("yolo"));
    }

    #[test]
    fn gemini_reasoning_level_creates_temp_settings() {
        let driver = GeminiDriver;
        let config = AgentConfig {
            reasoning_level: Some(ReasoningLevel::High),
            ..default_config()
        };
        let cmd = driver.build_args("think", &config).unwrap();

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
        let cmd = driver.build_args("search", &config).unwrap();

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
        let cmd = driver.build_args("go", &config).unwrap();

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
        let cmd = driver.build_args("hello", &default_config()).unwrap();
        assert!(cmd.temp_dir.is_none());
        assert!(cmd.env.is_empty());
    }

    #[test]
    fn gemini_json_schema_prompt_injection() {
        let driver = GeminiDriver;
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "User name" },
                "age": { "type": "integer" }
            },
            "required": ["name"]
        });
        let config = AgentConfig {
            json_schema: Some(schema),
            ..default_config()
        };
        let cmd = driver.build_args("extract data", &config).unwrap();
        let prompt = cmd.args.last().unwrap();
        assert!(prompt.contains("extract data"));
        assert!(prompt.contains("valid JSON only"));
        assert!(prompt.contains("name (string, required)"));
        assert!(prompt.contains("age (integer)"));
        assert!(prompt.contains("User name"));
    }

    #[test]
    fn gemini_schema_to_prompt_hint_empty() {
        let schema = json!({"type": "object"});
        let hint = schema_to_prompt_hint(&schema);
        assert!(hint.is_empty());
    }

    // -----------------------------------------------------------------------
    // GeminiDriver::parse_output
    // -----------------------------------------------------------------------

    #[test]
    fn gemini_parse_success() {
        let driver = GeminiDriver;
        let stdout = r#"{
            "session_id": "gem-123",
            "response": "Hello from Gemini",
            "stats": {
                "models": {
                    "gemini-2.5-pro": {
                        "prompt": 800,
                        "candidates": 400,
                        "total": 1200,
                        "cached": 100,
                        "thoughts": 50
                    }
                },
                "tools": { "totalCalls": 2, "totalSuccess": 2 },
                "files": { "totalLinesAdded": 10, "totalLinesRemoved": 3 }
            }
        }"#;

        let out = driver.parse_output(stdout, "", 0).unwrap();
        assert_eq!(out.outcome, NodeOutcome::Success);
        assert_eq!(out.response_text, "Hello from Gemini");
        assert_eq!(out.session_id.as_deref(), Some("gem-123"));
        assert_eq!(out.input_tokens, Some(800));
        assert_eq!(out.output_tokens, Some(400));
        assert_eq!(out.thinking_tokens, Some(50));
        assert_eq!(out.cache_read_tokens, Some(100));
        assert_eq!(out.model_used.as_deref(), Some("gemini-2.5-pro"));
        assert!(out.cost_usd.is_none()); // Gemini doesn't report cost
        assert!(out.error_message.is_none());
    }

    #[test]
    fn gemini_parse_error_turn_limit() {
        let driver = GeminiDriver;
        let stdout = r#"{
            "session_id": "gem-456",
            "response": "",
            "error": {
                "type": "turn_limit",
                "message": "Session turn limit exceeded",
                "code": 53
            }
        }"#;

        let out = driver.parse_output(stdout, "", 53).unwrap();
        assert_eq!(out.outcome, NodeOutcome::ErrorMaxTurns);
        assert_eq!(
            out.error_message.as_deref(),
            Some("Session turn limit exceeded")
        );
        assert_eq!(out.session_id.as_deref(), Some("gem-456"));
    }

    #[test]
    fn gemini_parse_error_input() {
        let driver = GeminiDriver;
        let stdout = r#"{
            "error": {
                "type": "input_error",
                "message": "Invalid prompt",
                "code": 42
            }
        }"#;

        let out = driver.parse_output(stdout, "", 42).unwrap();
        assert_eq!(out.outcome, NodeOutcome::ErrorExecution);
        assert_eq!(out.error_message.as_deref(), Some("Invalid prompt"));
    }

    #[test]
    fn gemini_parse_general_error() {
        let driver = GeminiDriver;
        let stdout = r#"{
            "response": "partial output",
            "error": {
                "type": "general",
                "message": "Something broke"
            }
        }"#;

        let out = driver.parse_output(stdout, "", 1).unwrap();
        assert_eq!(out.outcome, NodeOutcome::ErrorExecution);
        assert_eq!(out.error_message.as_deref(), Some("Something broke"));
        assert_eq!(out.response_text, "partial output");
    }

    #[test]
    fn gemini_parse_no_stats() {
        let driver = GeminiDriver;
        let stdout = r#"{
            "session_id": "gem-789",
            "response": "Simple response"
        }"#;

        let out = driver.parse_output(stdout, "", 0).unwrap();
        assert_eq!(out.outcome, NodeOutcome::Success);
        assert_eq!(out.response_text, "Simple response");
        assert!(out.input_tokens.is_none());
        assert!(out.output_tokens.is_none());
        assert!(out.thinking_tokens.is_none());
        assert!(out.model_used.is_none());
    }

    // -----------------------------------------------------------------------
    // GeminiDriver capabilities
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Stage 9: Capability gating — unsupported options silently ignored
    // -----------------------------------------------------------------------

    #[test]
    fn claude_ignores_reasoning_level() {
        let driver = ClaudeDriver;
        let config = AgentConfig {
            reasoning_level: Some(ReasoningLevel::High),
            ..default_config()
        };
        let cmd = driver.build_args("think", &config).unwrap();
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
        let cmd = driver.build_args("go", &config).unwrap();
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
        let cmd = driver.build_args("go", &config).unwrap();
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
        let cmd = driver.build_args("go", &config).unwrap();
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
        let cmd = driver.build_args("go", &config).unwrap();
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
        let cmd = driver.build_args("go", &config).unwrap();
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
        let cmd = driver.build_args("go", &config).unwrap();
        assert!(!args_contain(&cmd.args, "--allowedTools"));
        assert!(!args_contain(&cmd.args, "--disallowedTools"));
    }

    // -----------------------------------------------------------------------
    // Stage 9: Full config combination tests (mixed agent scenarios)
    // -----------------------------------------------------------------------

    #[test]
    fn claude_full_config_combination() {
        let driver = ClaudeDriver;
        let schema = json!({"type": "object", "properties": {"x": {"type": "string"}}});
        let config = AgentConfig {
            model: Some("claude-opus-4-20250514".into()),
            system_prompt: Some("You are helpful.".into()),
            max_turns: Some(10),
            max_budget_usd: Some(2.0),
            json_schema: Some(schema),
            access_mode: AccessMode::Edit,
            tool_toggles: ToolToggles { web_search: Some(false) },
            ephemeral_session: true,
            ..default_config()
        };
        let cmd = driver.build_args("do everything", &config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--model"), Some("claude-opus-4-20250514"));
        assert_eq!(arg_after(&cmd.args, "--append-system-prompt"), Some("You are helpful."));
        assert_eq!(arg_after(&cmd.args, "--max-turns"), Some("10"));
        assert_eq!(arg_after(&cmd.args, "--max-budget-usd"), Some("2"));
        assert!(args_contain(&cmd.args, "--json-schema"));
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
        let cmd = driver.build_args("do it", &config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--model"), Some("o3-mini"));
        assert_eq!(arg_after(&cmd.args, "-c"), Some("model_reasoning_effort=medium"));
        assert_eq!(arg_after(&cmd.args, "--sandbox"), Some("workspace-write"));
        assert!(args_contain(&cmd.args, "--search"));
        assert!(args_contain(&cmd.args, "--ephemeral"));
    }

    #[test]
    fn gemini_full_config_combination() {
        let driver = GeminiDriver;
        let schema = json!({"type": "object", "properties": {"y": {"type": "integer"}}});
        let config = AgentConfig {
            model: Some("gemini-2.5-flash".into()),
            reasoning_level: Some(ReasoningLevel::Low),
            access_mode: AccessMode::ReadOnly,
            tool_toggles: ToolToggles { web_search: Some(true) },
            json_schema: Some(schema),
            ..default_config()
        };
        let cmd = driver.build_args("analyze", &config).unwrap();
        assert_eq!(arg_after(&cmd.args, "--model"), Some("gemini-2.5-flash"));
        assert_eq!(arg_after(&cmd.args, "--approval-mode"), Some("plan"));
        // Reasoning + web search → temp settings with GEMINI_CLI_HOME
        assert!(cmd.env.iter().any(|(k, _)| k == "GEMINI_CLI_HOME"));
        // Schema injected into prompt (no native support)
        let prompt = cmd.args.last().unwrap();
        assert!(prompt.contains("valid JSON only"));
        assert!(prompt.contains("y (integer)"));
        // Cleanup
        if let Some(dir) = cmd.temp_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    // -----------------------------------------------------------------------
    // Stage 9: Claude capabilities
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
