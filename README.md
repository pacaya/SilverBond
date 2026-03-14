# SilverBond

SilverBond is a local-first graph workflow runner.

Today it is built as:

- a Rust runtime on `tokio` + `axum`
- a Svelte 5 + TypeScript graph editor authored in `ui/`
- generated `public/` assets embedded into the Rust binary
- an optional Tauri shell in `src-tauri/` that wraps the same localhost app
- a v3-only workflow schema with `nodes[]`, `edges[]`, `entryNodeId`, per-agent `agentDefaults`, and optional `ui.canvas`
- executable `task`, `approval`, `split`, and `collector` nodes with per-node `agentConfig` overrides
- multi-cursor checkpoints with execution epochs, split families, and collector barriers
- SQLite-backed checkpoints, event history, interrupted runs, and execution logs
- per-node agent configuration: model selection, reasoning level, system prompt, access mode, tool control, budget/turn limits, session continuity

The runtime shells out to locally installed agent CLIs (`claude`, `codex`, and `gemini`).
Tauri is packaging and windowing, not a second runtime model.

## Key Documents

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [docs/](docs/README.md) — full project documentation index
- [docs/svelte-specifics.md](docs/svelte-specifics.md)

## Development Commands

Prerequisites: Rust toolchain, Node.js/npm, and optionally [`just`](https://github.com/casey/just).
For real agent execution you also need at least one local agent CLI: `claude`, `codex`, or `gemini`.

Run `just` (or `just --list`) to see all available recipes. The raw `npm`/`cargo` commands still
work; `just` is a convenience layer.

```bash
just setup          # npm install
just dev            # Vite dev server on :5173 (proxies /api to :3333)
just server         # Rust backend on :3333 — open http://127.0.0.1:3333
just build          # Build frontend assets to public/
just build-release  # Frontend build + cargo build --release
just tauri-dev      # Tauri desktop shell in dev mode
just tauri-build    # Package desktop app
just test           # Run all tests (Rust + frontend unit)
just test-rust      # Rust tests only
just test-ui        # Frontend vitest unit tests
just test-e2e       # Build + Playwright e2e tests
just typecheck      # Svelte/TS type checking
just check          # Rust type checking (fast)
just clean          # Remove frontend build artifacts
```

For frontend development, run `just dev` and `just server` side by side. The Vite dev server on
[http://127.0.0.1:5173](http://127.0.0.1:5173) proxies `/api` to the Rust backend on port `3333`.
Rebuild with `just build` before shipping or packaging the backend.

By default the standalone app stores workflows, templates, and SQLite state under the current
working directory. Set `SILVERBOND_ROOT=/path/to/root` before `just server` to use a different
local app root.

The Tauri shell (`just tauri-dev`) starts the same Rust HTTP/SSE runtime on an ephemeral localhost
port and opens the web UI against that URL. In Tauri mode, app data lives under the platform
app-data directory and bundled templates are seeded on first launch.

`just test-e2e` builds the frontend, starts the real Rust backend, and exercises the browser UI
against the backend contract instead of a Vite-only shell.

## Repository Layout

- `src/` Rust runtime, API, storage, host lifecycle, and frontend embedding
- `src-tauri/` optional Tauri shell that launches the same runtime in a desktop window
- `ui/` Svelte 5 frontend source
- `public/` generated frontend build output embedded by Rust
- `workflows/` saved workflow JSON files
- `templates/` workflow template JSON files
- `.silverbond/` SQLite runtime state
- `tests/` Rust HTTP and integration tests
- `ui/e2e/` Playwright browser tests

## Current Frontend Shape

The frontend is a graph-native Svelte app, not a translated legacy step editor.

Current frontend stack:

- Svelte 5 runes
- Vite
- TanStack Svelte Query
- `@xyflow/svelte` for the canvas
- a class-based editor store in `ui/src/lib/stores/workflowStore.svelte.ts`

Key frontend modules:

- `ui/src/app/AppShell.svelte` composes data loading, run control, and layout
- `ui/src/features/editor/GraphEditor.svelte` owns SvelteFlow integration
- `ui/src/features/editor/InspectorPanel.svelte` edits workflow, node, and edge fields
- `ui/src/features/history/HistoryPanel.svelte` renders interrupted runs and persisted logs
- `ui/src/features/runtime/RunPanel.svelte` renders live run output and approvals
- `ui/src/lib/api/client.ts` is the browser contract to the Rust backend

## Current Constraints

- workflows must be canonical `version: 3`
- the backend owns multi-cursor checkpoints, execution epochs, validation, and traversal semantics
- `split` and `collector` are executable today; explicit `join` nodes are still planned
- workflow validation and traversal semantics are backend-authoritative
- the Tauri shell preserves the existing HTTP/SSE browser contract; it is packaging, not a rewrite
