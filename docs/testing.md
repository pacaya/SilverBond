# Testing

SilverBond has three test layers: Rust integration tests, frontend unit tests, and end-to-end tests.

## Running Tests

```bash
just test           # Run all tests (Rust + frontend unit)
just test-rust      # Rust integration tests only
just test-ui        # Frontend Vitest unit tests only
just test-e2e       # Build frontend + Playwright e2e tests
just typecheck      # svelte-check type validation
just check          # cargo check (compilation only)
```

## Rust Integration Tests

**Location:** `tests/http_api.rs`

These tests start a real `ApplicationHost` on an ephemeral port, make HTTP requests, and verify responses.

### Test Cases

| Test | Description |
|------|-------------|
| `exposes_health` | Verifies `GET /api/health` returns `{ "ok": true }` |
| `validates_and_saves_workflows` | Full workflow roundtrip: validate → save → list → retrieve |
| `rejects_legacy_workflow_payloads` | Ensures v2 workflows are rejected |
| `validates_workflow_with_agent_config` | Tests `agentDefaults`, `agentConfig`, `cwd`, and `continueSessionFrom` roundtrip |

### Running

```bash
cargo test
# or
just test-rust
```

Each test creates a temporary directory for the app root, starts a host, runs assertions, and shuts down cleanly.

## Frontend Unit Tests

**Location:** `ui/src/**/*.test.ts`

Unit tests use Vitest with jsdom environment.

### Test Files

| File | Coverage |
|------|----------|
| `ui/src/lib/stores/workflowStore.test.ts` | Store mutations, undo/redo, selection, dirty tracking |
| `ui/src/features/editor/flowNodes.test.ts` | Workflow → SvelteFlow node conversion, validation indexing |

### Running

```bash
npm test
# or
just test-ui
```

### Configuration

Tests are configured in `ui/vite.config.ts`:

```typescript
test: {
  environment: "jsdom",
  setupFiles: "./src/test/setup.ts",
  css: true,
  exclude: ["e2e/**"]
}
```

The setup file (`ui/src/test/setup.ts`) initializes the jsdom environment for Svelte component testing.

## End-to-End Tests

**Location:** `ui/e2e/app.spec.ts`

E2E tests use Playwright to test the full stack (Rust backend + built frontend).

### Test Cases

| Test | Description |
|------|-------------|
| `shows_completed_history_for_a_real_approval_run` | Creates a workflow, starts a run, approves an approval node, and verifies the history view |

### Running

```bash
npm run test:e2e
# or
just test-e2e
```

### Configuration

Playwright is configured in `playwright.config.ts`:

```typescript
export default defineConfig({
  testDir: "./ui/e2e",
  use: {
    baseURL: "http://127.0.0.1:3333",
    trace: "on-first-retry"
  },
  webServer: {
    command: "export SILVERBOND_ROOT=$(mktemp -d); cargo run",
    url: "http://127.0.0.1:3333/api/health",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000
  }
});
```

The test infrastructure:
1. Builds the frontend (`npm run build`)
2. Starts the Rust backend with a temporary app root
3. Waits for the health endpoint
4. Runs Playwright tests against the live server
5. Cleans up on completion

### CI Considerations

- `reuseExistingServer` is disabled in CI (`process.env.CI`) to ensure a fresh server
- Build timeout is 120 seconds to accommodate Rust compilation
- Traces are captured on first retry for debugging failures

## Type Checking

```bash
just typecheck
```

Runs `svelte-check` with the project's `tsconfig.json` to validate TypeScript types across all Svelte and TypeScript files.

## Adding Tests

### Rust Tests

Add integration tests to `tests/http_api.rs` or create new test files in `tests/`. Use the existing test helper pattern:

1. Create a temporary directory
2. Build `ApplicationConfig` with the temp path
3. Start `ApplicationHost`
4. Make HTTP requests and assert responses
5. Host cleans up on drop

### Frontend Unit Tests

Create `.test.ts` files adjacent to the code being tested. Import from `vitest` and follow existing patterns in the store and flowNodes tests.

### E2E Tests

Add Playwright test files to `ui/e2e/`. Tests have access to the full running application and can interact with the UI through Playwright's page API.
