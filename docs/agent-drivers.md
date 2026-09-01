# Agent Drivers

SilverBond executes workflow tasks by launching local agent CLIs inside **tmux panes**. Every agent invocation — full worker tasks and lightweight classifier calls (orchestrator prompt refinement, loop verdicts) alike — runs through the same tmux-based path. The driver layer (`driver.rs`) provides a uniform abstraction over different agent implementations; the runtime (`tmux_exec.rs`) owns pane lifecycle, readiness detection, and output capture.

There are only two execution modes:

| Mode | Status | Description |
|------|--------|-------------|
| **Interactive CLI TUI** | Current | Agents run in tmux panes with full terminal interactivity. Observable via tmux attach. |
| **Direct API** | Future | Planned headless API calls without tmux (not yet implemented). |

The legacy `--print` / `call_claude_print` path and direct-PTY (`expectrl`) execution have been removed entirely.

## Architecture

```
Runtime (tmux_exec.rs)
  │
  ├── resolve_agent_config()   # Merge node → workflow → driver defaults
  │
  ├── build_tmux_invocation()  # runAs → sudo prefix + automatic run socket
  │
  ├── tmux server (per-run socket) → pane per agent invocation
  │
  └── AgentDriver trait
        ├── ClaudeDriver           # claude CLI
        ├── CodexDriver            # codex CLI
        └── RegistryProfileDriver  # registry-defined CLIs (cursor-agent, agy, …)
```

## User Switch and `runAs`

The user switch happens **once** at the tmux server boundary, not per pane:

- **Per-run sockets** — each run gets its own tmux server socket under the target user's per-UID socket directory (mode `0700`), isolating sessions between runs and users.
- **Control commands** — tmux control operations (`new-session`, `send-keys`, `capture-pane`, etc.) run as the target user via a `sudo -u <user> -H --` prefix and `-L silverbond-<run-id>`.
- **Workloads** — agent CLIs launch through the target user's login+interactive shell: `zsh -lic`.

Workflows can set a top-level `runAs` field to control this behavior:

```json
{
  "runAs": {
    "user": "agent-sandbox"
  }
}
```

| Field | Effect |
|-------|--------|
| `user` | Synthesizes a `sudo -u <user> -H --` prefix for tmux control commands |
| `command` | Verbatim argv-prefix escape hatch (overrides the synthesized `sudo` prefix) |

When both `user` and `command` are set, `command` takes precedence. SilverBond always assigns a dedicated `silverbond-<run-id>` socket.

To observe a running agent pane:

```
sudo -u <user> tmux -L silverbond-<run-id> attach -t <session>
```

The `GET /api/capabilities` endpoint exposes a `features.runAs` flag and per-run `attachCommand` hints for observability. Session history is no longer served via a dedicated API endpoint — attach to the tmux pane directly.

Beyond the two built-in drivers (Claude, Codex), additional agents are defined as
**tmux-tools registry profiles** and driven generically by `RegistryProfileDriver`. The
built-in registry ships **Cursor** (`cursor-agent`) and **Antigravity** (`agy`, Google's
successor to the now-removed Gemini CLI); users can add more via
`~/.config/tmux-tools/agents.toml`.

## AgentDriver Trait

Every driver implements:

```rust
trait AgentDriver: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> AgentCapabilities;
    fn build_args(&self, prompt: &str, config: &AgentConfig) -> Result<CommandArgs>;
    fn parse_output(&self, stdout: &str, stderr: &str, exit_code: i32) -> Result<AgentOutput>;
    fn interaction_patterns(&self) -> Vec<InteractionPattern> { vec![] }
    fn destructive_blocklist(&self) -> &[&str] { shared_destructive_patterns() }
}
```

- **`name()`** — identifier used in workflow `agent` field
- **`capabilities()`** — declares what the driver supports
- **`build_args()`** — constructs CLI command and arguments from prompt + config
- **`parse_output()`** — parses CLI stdout/stderr into structured `AgentOutput`
- **`interaction_patterns()`** — returns regex patterns for detecting interactive prompts in tmux pane output
- **`destructive_blocklist()`** — returns patterns that always require human approval

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
Built-in `claude` / `codex` capabilities come from the tmux-tools registry; the registry-driven
agents (Cursor, Antigravity) declare only `workerExecution` by default and can be enriched per
agent in `agents.toml` (`[<agent>.capabilities]`).

| Capability | Claude | Codex | Cursor | Antigravity | Description |
|-----------|--------|-------|--------|-------------|-------------|
| `workerExecution` | Yes | Yes | Yes | Yes | Can execute task prompts |
| `promptRefinement` | Yes | No | No | No | Can refine prompts (orchestrator) |
| `branchChoice` | Yes | No | No | No | Can choose branches (orchestrator) |
| `loopVerdict` | Yes | No | No | No | Can decide loop exit (orchestrator) |
| `structuredOutput` | Yes | Yes | No | No | Supports JSON output |
| `sessionReuse` | Yes | Yes | No | No | Can resume sessions |
| `nativeJsonSchema` | Yes | Yes | No | No | Accepts JSON Schema natively |
| `modelSelection` | Yes | Yes | No | No | Supports model override |
| `reasoningConfig` | Yes | Yes | No | No | Supports reasoning level |
| `systemPrompt` | Yes | No | No | No | Supports system prompt |
| `budgetLimit` | Yes | No | No | No | Supports cost limit |
| `turnLimit` | Yes | No | No | No | Supports turn limit |
| `costReporting` | Yes | No | No | No | Reports execution cost |
| `toolAllowlist` | Yes | No | No | No | Supports tool allow/deny lists |
| `webSearch` | Yes | Yes | No | No | Supports web search toggle |

## Interaction Patterns

Each driver declares regex patterns for detecting interactive prompts in tmux pane output via `interaction_patterns()`. Patterns are classified by `InteractionKind`:

| Kind | Behavior |
|------|----------|
| `AutoRespond { response }` | Automatically sends the configured response (e.g., "y" for trust prompts) |
| `PermissionRequest` | Escalates to UI for user approval (or auto-approved if `autoApprove` is enabled) |
| `SubagentActive` | Marks that a subagent is running — output silence is expected, not a stall |
| `DestructiveWarning` | Always escalates to UI, even with `autoApprove` enabled |

```rust
pub struct InteractionPattern {
    pub kind: InteractionKind,
    pub pattern: String,        // Regex pattern
    pub description: String,    // Human-readable description
}
```

## Destructive Blocklist

The shared destructive blocklist catches dangerous operations that should always require human confirmation:

- `rm -rf`
- `DROP TABLE` / `DROP DATABASE`
- `force push` / `git push --force`
- `delete N files`
- `chmod 777`
- `truncate`

All drivers inherit this shared blocklist by default via `shared_destructive_patterns()`. Individual drivers can override `destructive_blocklist()` to customize.

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

**Interaction patterns:** Trust folder prompts (auto-respond "y"), tool permission prompts (escalate to UI), subagent launch detection (2 patterns).

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

**Interaction patterns:** Action approval prompts (escalate to UI).

## Registry Profile Driver (Cursor, Antigravity, …)

Agents that aren't one of the two built-in drivers are served by `RegistryProfileDriver`, a
generic driver that resolves the executable and access-profile arguments from the tmux-tools
registry (built-in profiles plus `~/.config/tmux-tools/agents.toml`). This is how **Cursor**
(`cursor-agent`) and **Antigravity** (`agy`) are driven.

> Google replaced the standalone Gemini CLI with the Antigravity CLI (`agy`); the dedicated
> `GeminiDriver` was removed and `agy` is now a built-in registry profile.

**CLI construction:** the binary and args come straight from the matched registry profile —
`build_session_args` looks up the access profile (mapped from `access_mode`) via
`Registry::launch_argv` and passes its args through verbatim. No model / session / schema flags
are injected (those capabilities are off by default for registry profiles).

**Access mode mapping** (registry access-profile names; the shared `read-only` /
`workspace-write` / `full-access` vocabulary):
- `read_only` → `read-only` profile
- `edit` / `execute` → `workspace-write` profile
- `unrestricted` → `full-access` profile

**Profile names are declarations.** The registry profile name is how an operator declares a
profile's privilege rank — SilverBond ranks by name and does not inspect argv to second-guess it.
An operator who rebinds `[codex.access.read-only]` to broader arguments in `agents.toml` has
declared those arguments to be read-only for their setup, and runs requesting `read_only` will
launch them. This is deliberate: `agents.toml` is operator-owned, and an operator can already
launch any agent with any flags directly.

Two rules constrain it. A profile whose name is outside the `read-only` / `workspace-write` /
`full-access` vocabulary carries no rank and is refused for `edit`/`execute` runs rather than
guessed at. And a profile's rank can never exceed the run's requested `access_mode` — redefining a
rank's meaning does not let a profile escape the rank it declared.

### Cursor (`cursor-agent`)

- `read-only` → `--mode ask` (also the `default`; `plan` tier → `--mode plan`)
- `workspace-write` → `--sandbox enabled`
- `full-access` → `--force --sandbox disabled` (dangerous; requires explicit permission)

### Antigravity (`agy`)

- `workspace-write` → no extra args (the `default`; approval-gated)
- `full-access` → `--dangerously-skip-permissions` (dangerous; requires explicit permission)
- No interactive read-only mode — for a guaranteed no-write run use `agy -p "<prompt>"` headless.

**Output parsing:** registry-profile drivers report no cost/context (tmux-driven interactive
sessions). Readiness and interaction detection come from the registry profile's `ready_regex`
and the shared destructive blocklist.

## Agent Discovery

The runtime resolves agent executables from PATH, with macOS-specific fallbacks to common GUI-install locations:
- `/opt/homebrew/bin`
- `/usr/local/bin`

The `GET /api/capabilities` endpoint reports which agents are available and their binary paths.
