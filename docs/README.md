# SilverBond Documentation

SilverBond is a local-first graph workflow runner that executes multi-step workflows using local agent CLIs (Claude, Codex, Gemini). It consists of a Rust backend, a Svelte 5 frontend, SQLite persistence, and an optional Tauri desktop shell.

## Documentation Index

| Document | Description |
|----------|-------------|
| [Getting Started](getting-started.md) | Setup, installation, and running the application |
| [Architecture Overview](architecture-overview.md) | High-level system design and core principles |
| [Workflow Schema](workflow-schema.md) | Complete v3 workflow format reference |
| [Backend](backend.md) | Rust backend modules and their responsibilities |
| [Frontend](frontend.md) | Svelte 5 UI architecture, components, and patterns |
| [API Reference](api-reference.md) | HTTP and SSE endpoint documentation |
| [Execution Model](execution-model.md) | Runtime engine, checkpoints, cursors, and events |
| [Agent Drivers](agent-drivers.md) | Agent CLI integration layer (Claude, Codex, Gemini) |
| [Tauri Desktop](tauri-desktop.md) | Optional desktop packaging with Tauri |
| [Testing](testing.md) | Test infrastructure and how to run tests |
| [Svelte Specifics](svelte-specifics.md) | Critical Svelte 5 / SvelteFlow patterns and pitfalls |
| [Agent Improvements Plan](agent-improvements-plan.md) | Stage 1-9 agent abstraction implementation plan |

## Quick Reference

```bash
just setup          # Install dependencies
just dev            # Vite dev server on :5173
just server         # Rust backend on :3333
just build          # Build frontend to public/
just test           # Run all tests
```
