# SilverBond development commands
# Run `just` or `just --list` to see available recipes

default:
    @just --list

# Install frontend dependencies
setup:
    npm install
    just install-hooks

# Start Vite frontend dev server (port 5173)
dev:
    npm run dev

# Start Rust backend (port 3333)
server:
    cargo run

# Build frontend assets to public/
build:
    npm run build

# Install repository-managed Git hooks
install-hooks:
    git config core.hooksPath .githooks
    @echo "Installed Git hooks from .githooks/"

# Full release build (frontend + Rust)
build-release:
    npm run build
    cargo build --release

# Tauri desktop shell in dev mode
tauri-dev:
    npm run tauri:dev

# Package desktop app
tauri-build:
    npm run tauri:build

# Run all tests (Rust + frontend unit)
test: test-rust
    npm test

# Rust tests only
test-rust:
    cargo test --locked

# Frontend vitest unit tests
test-ui:
    npm test

# Frontend freshness pre-commit hook regression
test-pre-commit:
    bash scripts/test-pre-commit-frontend-freshness.sh

# Build + Playwright e2e tests
test-e2e:
    npm run test:e2e

# Svelte/TS type checking
typecheck:
    npm run typecheck

# Rust type checking (fast)
check:
    cargo check

# Guard against v3 canonical-format drift in user-facing docs/UI
check-v4-docs:
    bash scripts/check-canonical-v4-docs.sh

# Regenerate node-catalog blocks in docs/workflow-schema.md
regen-docs:
    SB_REGEN_DOCS=1 cargo test --locked --test docs_catalog regeneration_writes_to_disk -- --ignored --exact

# Clean frontend build output
clean:
    rm -rf public/assets
