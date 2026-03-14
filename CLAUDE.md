# SilverBond

Rust backend (`src/`) + Svelte 5 frontend (`ui/`). The backend embeds built frontend assets from `public/`.

## Commands

```bash
just setup          # npm install
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

## Test details

- Frontend unit tests: `npm test` (runs `vitest run --config ui/vite.config.ts`)
- E2e tests: `npm run test:e2e` (builds frontend, starts Rust backend, runs Playwright)
- Rust tests: `cargo test`

## Conventions

- Svelte 5 runes (`$state`, `$derived`, `$effect`) — no legacy reactive stores
- Workflows must be canonical `version: 3` schema
- Backend is authoritative for validation, traversal, and checkpoint semantics
