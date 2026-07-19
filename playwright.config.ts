import { defineConfig } from "@playwright/test";
import { ensureE2EUnlockCredentials } from "./ui/e2e/global-setup";

// Playwright 1.58 starts webServer plugins before invoking globalSetup. Run the
// same idempotent setup while loading the config so webServer.env gets the pair.
ensureE2EUnlockCredentials();

const testUnlockPasswordHash =
  process.env.SILVERBOND_UNLOCK_PASSWORD_HASH ??
  "sha256:391e4e352311dadd725ec267e897ae5e8b5c101746c955d5bd8f9585255aa744";

const inheritedEnv = Object.fromEntries(
  Object.entries(process.env).filter((entry): entry is [string, string] =>
    typeof entry[1] === "string"
  ),
);

export default defineConfig({
  globalSetup: "./ui/e2e/global-setup.ts",
  testDir: "./ui/e2e",
  use: {
    baseURL: "http://127.0.0.1:3333",
    trace: "on-first-retry",
  },
  webServer: {
    command: "/bin/zsh -lc 'export SILVERBOND_ROOT=$(mktemp -d /tmp/silverbond-e2e.XXXXXX); cargo run'",
    env: {
      ...inheritedEnv,
      SILVERBOND_RUNNER: process.env.SILVERBOND_RUNNER ?? "tmux",
      SILVERBOND_UNLOCK_PASSWORD_HASH: testUnlockPasswordHash,
    },
    url: "http://127.0.0.1:3333/api/health",
    // Stop `just server` before `just test-e2e`; a process on :3333 is a hard failure.
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
