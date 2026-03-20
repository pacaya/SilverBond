# Backend Architecture

The Rust backend is the system of record for SilverBond. It owns workflow validation, graph traversal, agent execution, checkpoint persistence, and event streaming.

## Dependencies

| Category | Crate | Version | Purpose |
|----------|-------|---------|---------|
| HTTP | `axum` | 0.8 | Web framework |
| HTTP | `tower` | — | Middleware |
| Async | `tokio` | 1.47 | Async runtime (fs, process, signal, sync, rt-multi-thread) |
| Data | `serde` / `serde_json` | 1.0 | Serialization |
| Data | `rusqlite` | 0.34 | SQLite (bundled) |
| Data | `uuid` | 1.18 | Run and cursor IDs |
| Utils | `regex` | 1.12 | Prompt template resolution |
| Utils | `chrono` | 0.4 | Timestamps |
| Utils | `anyhow` / `thiserror` | — / 2.0 | Error handling |
| Frontend | `include_dir` | 0.7 | Embed built assets |
| Frontend | `mime_guess` | 2.0 | Content-type detection |
| PTY | `expectrl` | 0.7 | PTY session management and prompt detection |
| Utils | `tempfile` | 3.20 | Temporary files for agent schemas |

## Module Overview

### `main.rs` — CLI Entrypoint

- Initializes tracing from `RUST_LOG` or default filter
- Resolves app root from `SILVERBOND_ROOT` env var or current directory
- Starts `ApplicationHost` on `127.0.0.1:3333`
- Handles graceful shutdown on Ctrl+C

### `lib.rs` — Library Root

Re-exports all modules so the crate can be used as a library (by Tauri and integration tests).

### `app.rs` — Application Composition

The composition root that wires everything together:

- **`AppPaths`**: Resolved filesystem paths (root, workflows dir, templates dir, database path)
- **`ApplicationConfig`**: Paths + whether to seed bundled templates
- **`AppState`**: Shared state holding stores, database, and runtime context
- Constructs the Axum router by mounting API routes and frontend asset handlers
- Optionally seeds bundled templates from `templates/` to the app root on first launch

### `host.rs` — Reusable Host Layer

Provides a host lifecycle abstraction used by both standalone CLI and Tauri:

- `ApplicationHost::start(config)` — binds to a socket, starts the Axum server
- `wait_for_health(timeout)` — polls `/api/health` until the server is ready
- Exposes the resolved local URL (important when binding to port `:0`)
- Graceful shutdown via a `oneshot` channel

### `model.rs` — Workflow Schema + Validation

Defines the canonical workflow data model and validation logic:

**Core enums:**
- `WorkflowNodeType` — `Task`, `Approval`, `Split`, `Collector`
- `ResponseFormat` — `Text`, `Json`
- `WorkflowEdgeOutcome` — `Success`, `Reject`, `Branch`, `LoopContinue`, `LoopExit`
- `SplitFailurePolicy` — `BestEffortContinue`, `FailFastCancel`, `DrainThenFail`

**Agent configuration types:**
- `AgentDefaults` — model, reasoning level, system prompt, access mode, tool toggles, max turns, max budget, `auto_approve`, `orchestrator` (OrchestratorConfig)
- `AgentNodeConfig` — extends `AgentDefaults` with allowed/disallowed tool lists
- `resolve_agent_config()` — merges node config → workflow defaults → driver defaults

**Validation:**
- Graph reachability analysis from entry node
- Dead-end detection (nodes with no outgoing edges that aren't terminal)
- Duplicate node ID detection
- Edge target existence verification
- Task node prompt requirement
- Output schema auto-migration (legacy `{"field": "type"}` → JSON Schema)

### `driver.rs` — Agent Abstraction Layer

Defines the `AgentDriver` trait and concrete implementations. The trait includes `interaction_patterns()` for PTY prompt detection and `destructive_blocklist()` for safety-critical pattern matching. See [Agent Drivers](agent-drivers.md) for full details.

### `runtime.rs` — Execution Engine

The core execution engine. Includes interaction management methods (`respond_interaction()`) for handling PTY prompt escalation. See [Execution Model](execution-model.md) for full details.

### `storage.rs` — Persistence Layer

Manages all persistence:

**SQLite tables:**
- `runs` — checkpoint state (status, current node, serialized cursor/barrier state, workflow snapshot)
- `run_events` — ordered event journal for replay and SSE catch-up
- `logs` — durable execution summaries for history views

**File-backed stores:**
- `WorkflowStore` — reads/writes workflow JSON files from `workflows/` directory
- `TemplateStore` — reads template JSON files from `templates/` directory
- `seed_bundled_templates()` — copies bundled templates to the app root

### `session.rs` — PTY Session Management

Manages interactive PTY sessions for agent CLIs using `expectrl`. Responsibilities:

- **Session lifecycle**: `create_session()` spawns PTY-backed agent processes, `close_session()` / `close_all()` clean up
- **Interactive prompt handling**: `send_prompt_interactive()` watches for PTY prompts during execution using sentinel markers
- **4-tier escalation**: Auto-responds to warmup patterns (Tier 1), auto-approves with destructive blocklist (Tier 2), scaffolds orchestrator classification (Tier 3), escalates to UI (Tier 4)
- **Interaction resolution**: `respond_to_interaction()` sends human responses back to the PTY session
- **Session state tracking**: `SessionState` enum tracks `Idle`, `Processing`, `WaitingInteraction`, `Completed`, `Error`
- **Warmup**: `warmup_session()` runs initial auto-response phase for trust prompts
- **History and inspection**: `get_history()` returns conversation log, `list_sessions()` provides session summaries

### `pty_output.rs` — PTY Output Parsing

Parses agent CLI output from PTY sessions. Uses `LazyLock<Regex>` for zero-allocation regex compilation (compiled once on first use). Patterns include cost parsing (`COST_RE`), token count extraction (`INPUT_RE`, `OUTPUT_RE`, `THINKING_RE`, `CACHE_READ_RE`, `CACHE_WRITE_RE`), and context window tracking (`CONTEXT_RE`).

### `frontend.rs` — Embedded Asset Serving

- Embeds the built `public/` directory using `include_dir!` at compile time
- Serves `index.html` for the root path and SPA fallback routes
- Serves hashed assets with correct MIME types via `mime_guess`

### `api.rs` — HTTP/SSE Routes

Defines all HTTP endpoints. See [API Reference](api-reference.md) for full details.

Error handling uses an `ApiError` struct that maps to appropriate HTTP status codes and JSON error bodies.

### `util.rs` — Helpers

- `safe_name()` — validates workflow names (alphanumeric + `-_. `, rejects `..`, `/`, `\`, `%`, null bytes)
- `now_iso()` — ISO 8601 timestamp generation
- `slugify_filename()` — converts display names to safe filenames
- `djb2()` — DJB2 hash function used for deterministic split cursor IDs
- `ensure_dir()` — creates directories recursively
