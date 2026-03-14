# Getting Started

## Prerequisites

- **Rust** (stable toolchain) — for building the backend
- **Node.js** (v18+) and **npm** — for building the frontend
- **just** — command runner ([install guide](https://github.com/casey/just#installation))
- At least one supported agent CLI installed and on PATH:
  - `claude` (Claude Code CLI)
  - `codex` (OpenAI Codex CLI)
  - `gemini` (Gemini CLI)

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

| Variable | Default | Description |
|----------|---------|-------------|
| `SILVERBOND_ROOT` | Current working directory | Override the application root directory |
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

## Creating Your First Workflow

1. Start the application (dev or production mode)
2. Click **New** in the sidebar to create a workflow
3. Add nodes (task, approval, split, collector) via the graph editor
4. Connect nodes with edges to define control flow
5. Configure node properties in the inspector panel
6. Click **Run** to execute the workflow
7. Monitor execution in the run panel; approve approval nodes when prompted
