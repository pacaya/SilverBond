import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./ui/e2e",
  use: {
    baseURL: "http://127.0.0.1:3333",
    trace: "on-first-retry",
  },
  webServer: {
    command: "/bin/zsh -lc 'export SILVERBOND_ROOT=$(mktemp -d /tmp/silverbond-e2e.XXXXXX); cargo run'",
    url: "http://127.0.0.1:3333/api/health",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
