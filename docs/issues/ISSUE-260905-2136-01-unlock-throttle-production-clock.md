---
id: ISSUE-260905-2136-01
kind: issue
category: bug
status: needs-triage
summary: The unlock-throttle endpoint tests assert throttling with no sleep and no elapsed-time assertion, but depend on the second request landing inside a five-second production window read from Instant::now()
---

## Triage Notes

Filed 2026-09-05, from the adversarial review of `PRD-260902-0301-01` (Codex, `gpt-6-astra`).
Deliberately **not** folded into that PRD: the epic was narrowed to the reproduced failure plus the
forward-facing tier rule, and this is a distinct latent flake with its own seam.

**The defect.** `UnlockThrottle::verify_unlock_secret` reads `Instant::now()`, compares it against
`blocked_until`, and accepts a correct secret once the block has expired
(`src/app.rs:150`, `:157`, `:167`). `INITIAL_UNLOCK_RETRY_DELAY` is five seconds (`src/app.rs:129`).
`create_run_throttles_correct_secret_after_failed_unlock_attempt` and
`create_run_counts_failed_unlock_before_workflow_validation` (`src/api.rs:3082`, `:3100`) submit a bad
secret then a correct one and expect the second to be refused. Neither test sleeps, reads elapsed
time, or names a duration. If the process is descheduled for more than five seconds between the two
requests, the block expires and the expected error becomes a success.

Verified by direct source inspection 2026-09-05. Not reproduced — a five-second window makes this
low-frequency, and it is almost certainly **not** the failure `ISSUE-260901-0216-03` reproduced twice.

**Why it matters beyond its own frequency.** This is the clearest known case of a Logic Tier
violation that no syntactic check can see. The test body contains no timing construct at all; the
clock is read in production code two calls down. `ISSUE-260902-0747-10` enforces the tier rule by
scanning test sources, so this class passes the checker while breaking the rule. The ADR records
call-path isolation as a review obligation for exactly this reason
(`docs/adr/260902-0312-deterministic-test-tiers.md` § Enforcement is scoped, or advisory).

**Shape of the fix, for triage to confirm.** Supply the instant at the throttle boundary so these
endpoint tests can drive it: one assertion before expiry, one at or after. Marking the tests
Integration would conceal the gap rather than close it — the property under test is application
policy, not infrastructure.

**Discovery.** Other tests reaching a production clock through the same shape:

```
rg -n 'Instant::now|SystemTime::now' src/ --glob '!src/**/tests*'
```

then, for each hit, find test callers that assert on behavior gated by the comparison.

**Related.** `PRD-260902-0301-01` (the tier rule this violates), `ISSUE-260902-0747-10` (the checker
that cannot catch it), `ISSUE-260905-2136-02` (the sibling ambient-input defect).
