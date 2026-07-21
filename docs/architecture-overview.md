# Architecture Overview

## What SilverBond Is

SilverBond is a local-first graph workflow runner. It lets users visually author directed graphs of tasks, then executes them by launching local agent CLIs inside **tmux panes** (Claude, Codex, Cursor, Antigravity). All agent calls — worker tasks and lightweight classifier invocations — use the same tmux-based execution path. The system is designed around a few core ideas:

- **Local-first**: No cloud coordinator, remote queue, or managed database required
- **Runtime-authoritative**: The Rust backend owns all business logic — validation, traversal, execution, checkpoints
- **Graph-native**: Control flow is explicit in the workflow document (`entryNodeId`, `nodes[]`, `edges[]`)
- **Observable**: Every runtime decision is inspectable via SSE events, persisted checkpoints, event journal replay, execution logs, and live tmux pane attach

## System Topology

```
┌─────────────────────────────────────────────────┐
│                  Browser / Tauri                 │
│  ┌─────────────────────────────────────────────┐ │
│  │           Svelte 5 Frontend (ui/)           │ │
│  │  GraphEditor · Inspector · RunPanel · History│ │
│  └──────────────────┬──────────────────────────┘ │
└─────────────────────┼───────────────────────────┘
                      │ HTTP + SSE
┌─────────────────────┼───────────────────────────┐
│              Rust Backend (src/)                 │
│  ┌──────────────────┴──────────────────────────┐ │
│  │         Axum HTTP + SSE API (api.rs)        │ │
│  └───────┬──────────────┬──────────────────────┘ │
│  ┌───────┴───────┐ ┌────┴─────────────────────┐ │
│  │ Workflow Model│ │   Execution Runtime       │ │
│  │ + Validation  │ │   (runtime.rs)            │ │
│  │ (model.rs)    │ │                           │ │
│  └───────────────┘ └────┬───────────┬─────────┘ │
│                    ┌────┴────┐ ┌────┴─────────┐ │
│                    │ Agent   │ │   SQLite      │ │
│                    │ Drivers │ │   Persistence │ │
│                    │(driver) │ │  (storage.rs) │ │
│                    └────┬────┘ └──────────────┘ │
└─────────────────────────┼───────────────────────┘
                          │ tmux control (sudo -u <user> -L <socket>)
┌─────────────────────────┼───────────────────────┐
│              tmux server (per-UID 0700 socket)     │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌─────┐ │
│  │ pane     │ │ pane     │ │ pane     │ │ ... │ │
│  │ claude   │ │ codex    │ │ cursor   │ │     │ │
│  └──────────┘ └──────────┘ └──────────┘ └─────┘ │
└───────────────────────────────────────────────────┘
         workloads via zsh -lic in each pane
```

Each run has its own tmux server on a `silverbond-<run-id>` socket in the target user's per-UID socket directory (mode `0700`). The backend issues control commands through `sudo -u <user>` (or a workflow `runAs.command` override) and launches agent workloads through `zsh -lic`. Workflows can set `runAs: { user?, command? }` to run under a dedicated agent-user sandbox.

## Deployment Modes

### Standalone (CLI)

Launched with `cargo run`. Binds to `127.0.0.1:3333`. Serves the embedded frontend and API from the same origin. State is stored under the current working directory (or `SILVERBOND_ROOT`).

### Tauri Desktop

The optional Tauri shell (`src-tauri/`) starts the same Rust host in-process on an ephemeral port (`:0`). The desktop window points at the resolved localhost URL. State goes under the platform app-data directory. Tauri is pure packaging — it does not fork runtime semantics.

## Source Layout

```
SilverBond/
├── src/                    # Rust backend
│   ├── main.rs             # CLI entrypoint
│   ├── lib.rs              # Library root (re-exports)
│   ├── app.rs              # Application composition
│   ├── host.rs             # Reusable host lifecycle
│   ├── api.rs              # HTTP/SSE routes
│   ├── model.rs            # Workflow schema + validation
│   ├── runtime.rs          # Execution engine
│   ├── driver.rs           # Agent CLI abstraction
│   ├── tmux_exec.rs        # tmux pane lifecycle, agent execution, interaction escalation
│   ├── pty_output.rs       # Pane output parsing (LazyLock regexes)
│   ├── storage.rs          # SQLite + file persistence
│   ├── frontend.rs         # Embedded asset serving
│   └── util.rs             # Helpers
├── src-tauri/              # Optional Tauri shell
│   └── src/main.rs         # Tauri entrypoint
├── ui/                     # Svelte 5 frontend
│   └── src/
│       ├── app/            # App shell components
│       ├── features/       # Feature modules (editor, runtime, history, reference)
│       └── lib/            # Shared code (API client, stores, types, utils)
├── templates/              # Bundled example workflows
├── tests/                  # Integration tests
├── public/                 # Built frontend assets (generated)
└── docs/                   # This documentation
```

## Data Flow

1. **Authoring**: User edits workflows in the Svelte graph editor. Changes are local state until saved.
2. **Validation**: The frontend sends the workflow to `POST /api/validate-workflow`. The backend runs graph analysis (reachability, dead-ends, duplicate IDs, missing prompts) and returns issues.
3. **Persistence**: `POST /api/workflows` saves the workflow as a JSON file in the `workflows/` directory.
4. **Execution**: `POST /api/runs` creates a run. The runtime validates the workflow, creates an initial checkpoint, and begins graph traversal.
5. **Agent Calls**: For each task node, the runtime resolves the prompt (variable substitution, context sources), selects the appropriate agent driver, builds CLI arguments, and launches the agent in a tmux pane (respecting workflow `runAs` for user selection and assigning the run-scoped socket automatically). Lightweight classifier calls (orchestrator) use the same tmux path.
6. **Events**: Runtime decisions are emitted as events, persisted to SQLite, and streamed to the frontend via SSE.
7. **Checkpoints**: After each node execution, the runtime persists a checkpoint to SQLite. This enables resume after process restart.
8. **History**: When a run completes, a durable execution log is persisted for later review.

## Key Design Rules

1. **Is this runtime truth or just presentation?** — Runtime logic belongs in Rust, not the frontend.
2. **If it changes traversal semantics, is it represented in the schema?** — The workflow document is the source of truth for control flow.
3. **Can it survive process restart?** — All run state must be recoverable from SQLite checkpoints.
4. **Can the user inspect what happened?** — Every runtime decision should be visible in the event journal.
5. **Does it move toward a graph-native runtime?** — Avoid implicit traversal rules or frontend-owned behavior.
