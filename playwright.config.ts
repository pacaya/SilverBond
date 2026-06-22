import { defineConfig } from "@playwright/test";

const testUnlockPasswordHash =
  process.env.SILVERBOND_UNLOCK_PASSWORD_HASH ??
  "sha256:391e4e352311dadd725ec267e897ae5e8b5c101746c955d5bd8f9585255aa744";

const inheritedEnv = Object.fromEntries(
  Object.entries(process.env).filter((entry): entry is [string, string] =>
    typeof entry[1] === "string"
  ),
);

export default defineConfig({
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
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
