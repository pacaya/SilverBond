# SilverBond

Rust backend (`src/`) + Svelte 5 frontend (`ui/`). The backend embeds built frontend assets from `public/`.

## Commands

```bash
just setup          # npm install + configure repository Git hooks
just dev            # Vite dev server on :5173 (proxies /api to :3333)
just server         # Rust backend on :3333
just build          # Build frontend to public/
just test           # Run all tests (cargo test && npm test)
just test-rust      # Rust tests only
just test-ui        # Frontend vitest unit tests
just test-e2e       # Build + Playwright e2e tests
just typecheck      # svelte-check
just check          # cargo check
```

## Frontend bundle freshness

`just setup` sets this clone's `core.hooksPath` to `.githooks/`. The pre-commit
hook verifies that staged `public/` assets were built from the staged `ui/src/`
tree without modifying the working tree. Run `git config --unset core.hooksPath`
to disable the repository hooks for this clone.

## Test details

- Frontend unit tests: `npm test` (runs `vitest run --config ui/vite.config.ts`)
- E2e tests: `npm run test:e2e` (builds frontend, starts Rust backend, runs Playwright)
- Rust tests: `cargo test`

## Conventions

- Svelte 5 runes (`$state`, `$derived`, `$effect`) — no legacy reactive stores
- Workflows must be canonical `version: 4` schema
- Backend is authoritative for validation, traversal, and checkpoint semantics
