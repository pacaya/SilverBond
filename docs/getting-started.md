# Getting Started

## Prerequisites

- **Rust** (stable toolchain) — for building the backend
- **Node.js** (v18+) and **npm** — for building the frontend
- **just** — command runner ([install guide](https://github.com/casey/just#installation))
- **tmux** — all agent execution (worker tasks and lightweight classifier calls) runs in tmux panes
- At least one supported agent CLI installed and on PATH:
  - `claude` (Claude Code CLI)
  - `codex` (OpenAI Codex CLI)
  - `cursor-agent` (Cursor CLI)
  - `agy` (Antigravity CLI)

## Installation

```bash
# Clone the repository
git clone <repo-url>
cd SilverBond

# Install frontend dependencies
just setup
```

## Running the Application

### Development Mode (recommended)

Run two terminals:

```bash
# Terminal 1: Start the Rust backend on :3333
just server

# Terminal 2: Start the Vite dev server on :5173 (proxies /api to :3333)
just dev
```

Open `http://127.0.0.1:5173` in your browser.

### Production Mode

Build the frontend and run the standalone server:

```bash
just build          # Build frontend assets to public/
just server         # Serves embedded frontend + API on :3333
```

Open `http://127.0.0.1:3333`.

### Desktop Mode (Tauri)

```bash
just tauri-dev      # Dev mode with hot reload
just tauri-build    # Package as native desktop app
```

## Environment Variables

Process-launching workflows must be configured through one of two paths before they can start:

- Consent path: set `SILVERBOND_UNLOCK_PASSWORD_HASH` to a `sha256:<hex>` password hash. The UI asks for that password once at run start before allowing operator-privilege execution.
- Containment path: set `SILVERBOND_AGENT_USER` to a real low-privilege OS user. Workflows without an explicit `runAs` run agents as that user without an unlock prompt.

These variables apply to `just server`, production builds, and the packaged desktop app. With neither variable set, SilverBond rejects process-launching runs with guidance instead of asking for an unlock password that cannot verify.

| Variable | Default | Description |
|----------|---------|-------------|
| `SILVERBOND_ROOT` | Current working directory | Override the application root directory |
| `SILVERBOND_UNLOCK_PASSWORD_HASH` | Unset | `sha256:<hex>` hash for the operator unlock password used to authorize privileged process-launching runs |
| `SILVERBOND_AGENT_USER` | Unset | Low-privilege OS user that receives default process-launching agent runs without an unlock prompt |
| `RUST_LOG` | `silverbond=info,tower_http=info` | Tracing log filter |

## Directory Structure

When running, SilverBond uses the following directory layout under the app root:

```
<app-root>/
├── workflows/           # Saved workflow JSON files
├── templates/           # Template workflow files
└── .silverbond/
    └── silverbond.sqlite  # SQLite database for runs, events, and logs
```

In Tauri mode, the app root is the platform's app-data directory. In standalone mode, it defaults to the current working directory (overridable with `SILVERBOND_ROOT`).

## Available Commands

All commands are defined in the `justfile`:

| Command | Description |
|---------|-------------|
| `just setup` | Install npm dependencies |
| `just dev` | Start Vite dev server on :5173 (proxies /api to :3333) |
| `just server` | Start Rust backend on :3333 |
| `just build` | Build frontend to `public/` |
| `just build-release` | Build frontend + cargo release build |
| `just tauri-dev` | Start Tauri in dev mode |
| `just tauri-build` | Package Tauri desktop app |
| `just test` | Run all tests (Rust + frontend) |
| `just test-rust` | Run Rust tests only |
| `just test-ui` | Run frontend Vitest unit tests |
| `just test-e2e` | Build frontend + run Playwright e2e tests |
| `just typecheck` | Run svelte-check for type errors |
| `just check` | Run cargo check |
| `just clean` | Remove built frontend assets |

## Agent Execution and Observability

SilverBond launches every agent — full worker tasks and lightweight classifier calls alike — in **tmux panes**. There is no direct subprocess or `--print` path; observability is via tmux attach.

### Agent-User Sandbox

Workflows can set a top-level `runAs` field to run agents under a dedicated Unix user:

```json
{
  "runAs": {
    "user": "agent-sandbox"
  }
}
```

This isolates agent sessions on a per-user tmux socket (mode `0700`). Control commands run as the target user via `sudo -u <user>`, and agent workloads launch through `zsh -lic`. See [Workflow Schema](workflow-schema.md#run-as-runas) for the full `runAs` shape.

### Watching Running Agents

To attach to a running agent pane:

```
sudo -u <user> tmux -L <socket> attach -t <session>
```

The run panel and capabilities API (`GET /api/capabilities`) expose `attachCommand` hints for the current run. Use tmux attach to observe live agent output — there is no separate session-history endpoint.

## Creating Your First Workflow

1. Start the application (dev or production mode)
2. Click **New** in the sidebar to create a workflow
3. Add nodes (task, approval, split, collector) via the graph editor
4. Connect nodes with edges to define control flow
5. Configure node properties in the inspector panel
6. Optionally set `runAs` in the workflow inspector to run under a dedicated agent user
7. Click **Run** to execute the workflow
8. Monitor execution in the run panel; attach to tmux panes for live agent output; approve approval nodes when prompted
