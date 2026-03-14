# SilverBond development commands
# Run `just` or `just --list` to see available recipes

default:
    @just --list

# Install frontend dependencies
setup:
    npm install

# Start Vite frontend dev server (port 5173)
dev:
    npm run dev

# Start Rust backend (port 3333)
server:
    cargo run

# Build frontend assets to public/
build:
    npm run build

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
test:
    cargo test
    npm test

# Rust tests only
test-rust:
    cargo test

# Frontend vitest unit tests
test-ui:
    npm test

# Build + Playwright e2e tests
test-e2e:
    npm run test:e2e

# Svelte/TS type checking
typecheck:
    npm run typecheck

# Rust type checking (fast)
check:
    cargo check

# Clean frontend build output
clean:
    rm -rf public/assets
