# Agent Drivers

SilverBond executes workflow tasks by shelling out to local agent CLIs. The driver layer (`driver.rs`) provides a uniform abstraction over different agent implementations.

## Architecture

```
Runtime
  │
  ├── resolve_agent_config()   # Merge node → workflow → driver defaults
  │
  └── AgentDriver trait
        ├── ClaudeDriver       # claude CLI
        ├── CodexDriver        # codex CLI
        └── GeminiDriver       # gemini CLI
```

## AgentDriver Trait

Every driver implements:

```rust
trait AgentDriver: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> AgentCapabilities;
    fn build_args(&self, prompt: &str, config: &AgentConfig) -> Result<CommandArgs>;
    fn parse_output(&self, stdout: &str, stderr: &str, exit_code: i32) -> Result<AgentOutput>;
}
```

- **`name()`** — identifier used in workflow `agent` field
- **`capabilities()`** — declares what the driver supports
- **`build_args()`** — constructs CLI command and arguments from prompt + config
- **`parse_output()`** — parses CLI stdout/stderr into structured `AgentOutput`

## Agent Config

Configuration resolved per node execution:

| Field | Type | Description |
|-------|------|-------------|
| `model` | `String` | Model identifier |
| `reasoning_level` | `Low \| Medium \| High` | Reasoning effort |
| `system_prompt` | `String` | System prompt |
| `max_turns` | `u32` | Turn limit |
| `max_budget_usd` | `f64` | Cost limit |
| `resume_session_id` | `String` | Session to continue |
| `ephemeral_session` | `bool` | Don't persist session |
| `json_schema` | `Value` | Output schema |
| `access_mode` | `ReadOnly \| Edit \| Execute \| Unrestricted` | Permission level |
| `tool_toggles` | `{ web_search }` | Tool on/off |
| `allowed_tools` | `Vec<String>` | Tool whitelist |
| `disallowed_tools` | `Vec<String>` | Tool blacklist |
| `cwd` | `String` | Working directory |

Resolution order: node `agentConfig` → workflow `agentDefaults[agent]` → driver defaults.

## Agent Output

Structured result from every agent execution:

| Field | Description |
|-------|-------------|
| `response_text` | Agent's text response |
| `session_id` | Session ID for reuse |
| `cost_usd` | Execution cost |
| `input_tokens` | Input token count |
| `output_tokens` | Output token count |
| `thinking_tokens` | Reasoning token count |
| `cache_read_tokens` | Cache read token count |
| `cache_write_tokens` | Cache write token count |
| `model_used` | Actual model that was used |
| `num_turns` | Number of conversation turns |
| `duration_api_ms` | API-reported duration |
| `structured_output` | Parsed JSON if schema was provided |
| `outcome` | `NodeOutcome` enum value |
| `error_message` | Error details on failure |

### Node Outcomes

| Outcome | Description |
|---------|-------------|
| `Success` | Completed successfully |
| `ErrorExecution` | General execution error |
| `ErrorMaxTurns` | Exceeded turn limit |
| `ErrorMaxBudget` | Exceeded cost limit |
| `ErrorSchemaValidation` | Output didn't match schema |
| `ErrorTimeout` | Execution timed out |
| `ErrorNotFound` | Agent CLI not found |

## Capability Flags

Each driver declares 15 capability flags. The frontend uses these to show/hide config options.

| Capability | Claude | Codex | Gemini | Description |
|-----------|--------|-------|--------|-------------|
| `workerExecution` | Yes | Yes | Yes | Can execute task prompts |
| `promptRefinement` | Yes | No | No | Can refine prompts (orchestrator) |
| `branchChoice` | Yes | No | No | Can choose branches (orchestrator) |
| `loopVerdict` | Yes | No | No | Can decide loop exit (orchestrator) |
| `structuredOutput` | Yes | Yes | Yes | Supports JSON output |
| `sessionReuse` | Yes | Yes | Yes | Can resume sessions |
| `nativeJsonSchema` | Yes | Yes | No | Accepts JSON Schema natively |
| `modelSelection` | Yes | Yes | Yes | Supports model override |
| `reasoningConfig` | Yes | Yes | No | Supports reasoning level |
| `systemPrompt` | Yes | No | No | Supports system prompt |
| `budgetLimit` | Yes | No | No | Supports cost limit |
| `turnLimit` | Yes | No | No | Supports turn limit |
| `costReporting` | Yes | No | No | Reports execution cost |
| `toolAllowlist` | Yes | No | No | Supports tool allow/deny lists |
| `webSearch` | Yes | Yes | Yes | Supports web search toggle |

## Claude Driver

The most capable driver. Uses `claude` CLI.

**CLI construction:**
```
claude -p "<prompt>" --output-format json \
  [--model <model>] \
  [--system-prompt "<prompt>"] \
  [--max-turns <n>] \
  [--max-budget-usd <n>] \
  [--output-schema '<json>'] \
  [--resume <session-id>] \
  [--permission-mode <mode>] \
  [--allowedTools '<tools>'] \
  [--disallowedTools '<tools>'] \
  [--web-search]
```

**Access mode mapping:**
- `read_only` → `--permission-mode bypassPermissions --allowedTools 'Read,Glob,Grep'`
- `edit` → `--permission-mode bypassPermissions --allowedTools 'Read,Glob,Grep,Edit,Write'`
- `execute` → `--permission-mode bypassPermissions`
- `unrestricted` → `--permission-mode bypassPermissions`

**Output parsing:** Single JSON blob with `result`, `session_id`, `usage`, `total_cost_usd`, `modelUsage`, `structured_output`. Maps `subtype` field to `NodeOutcome`.

## Codex Driver

Uses `codex` CLI. More limited capabilities than Claude.

**CLI construction:**
```
codex exec [resume <session-id>] "<prompt>" --json \
  [--model <model>] \
  [-c model_reasoning_effort=<level>] \
  [--ephemeral] \
  [--output-schema <tempfile>] \
  [--approval-mode <mode>]
```

**Access mode mapping:**
- `read_only` → `--approval-mode full` + `--sandbox networking`
- `edit` → `--approval-mode suggest`
- `execute` → `--approval-mode auto-edit`
- `unrestricted` → `--approval-mode full-auto`

**Output parsing:** JSONL stream — accumulates events: `thread.started` (session_id), `turn.completed` (tokens), last text `item.completed` (response), `turn.failed` (error).

## Gemini Driver

Uses `gemini` CLI. Limited capability set.

**CLI construction:**
```
gemini --prompt "<prompt>" --output-format json \
  [--model <model>] \
  [--resume <session-id>] \
  [--approval-mode <mode>]
```

**Access mode mapping:**
- `read_only` → `--approval-mode full`
- `edit` → `--approval-mode patch`
- `execute` → `--approval-mode auto-patch`
- `unrestricted` → `--approval-mode full-auto`

**Special handling:** Reasoning and web search are configured via temporary settings file using `GEMINI_CLI_HOME` environment variable.

**Output parsing:** Single JSON blob with `response`, `session_id`, `stats.models.*` (including `thoughts` token count). Error codes: 42 → input error, 53 → turn limit.

## Agent Discovery

The runtime resolves agent executables from PATH, with macOS-specific fallbacks to common GUI-install locations:
- `/opt/homebrew/bin`
- `/usr/local/bin`

The `GET /api/capabilities` endpoint reports which agents are available and their binary paths.
