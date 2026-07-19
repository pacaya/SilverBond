import { createHash, randomBytes } from "node:crypto";

export function ensureE2EUnlockCredentials(): void {
  if (
    process.env.SILVERBOND_E2E_UNLOCK_SECRET &&
    process.env.SILVERBOND_UNLOCK_PASSWORD_HASH
  ) {
    return;
  }

  const secret = randomBytes(32).toString("hex");
  const digest = createHash("sha256").update(secret).digest("hex");

  process.env.SILVERBOND_E2E_UNLOCK_SECRET = secret;
  process.env.SILVERBOND_UNLOCK_PASSWORD_HASH = `sha256:${digest}`;
}

export default function globalSetup(): void {
  ensureE2EUnlockCredentials();
}
