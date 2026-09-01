---
id: ISSUE-260901-0216-03
kind: issue
category: bug
status: needs-triage
summary: Three closed records independently sighted intermittent test failures and each declined to file; the observation is preserved here, but no mechanism has been reproduced and the first proposed diagnosis was falsified
---

## Triage Notes

Filed 2026-09-01 during a completeness audit of the `docs-truth` epic, to keep an observation from
being lost when that epic closed. **This record deliberately carries no `## Agent Brief`.** It is
not ready for an agent: the symptom is attested but no mechanism has been reproduced, and the first
attempt at a diagnosis was wrong.

**The observation.** Three separate closed records sighted intermittent test failures and each
declined to file a record for them:

- `ISSUE-260826-0637-04` — named two Rust tests and predicted they "will make the CI job from
  ISSUE-260826-0637-01 flaky".
- `ISSUE-260830-1925-01` — said the behavior "makes `cargo test --locked` an unreliable acceptance
  gate… worth its own record".
- `ISSUE-260826-0240-01` — recorded a frontend inspector-panel test timing out, "a candidate for
  its own record".

Three independent sightings is why this is worth keeping. That is the whole of the evidence.

**Retracted diagnosis (round 1).** This record was first written asserting that the named tests
contend on a shared real-tmux server namespace and that isolation was the fix. The readiness gate
falsified that, and the falsification was independently reproduced before accepting it:

- Both named Rust tests are hermetic. `completed_run_cleans_non_persistent_active_panes` and
  `decide_abort_returns_promptly_and_kills_pane` both live in the runtime module's inline test
  module and build a `TempDir`, a per-test `Database`, and a **fake** tmux invocation. Pane names
  are per-test literals. Neither reaches a real tmux server, so there is no shared server namespace
  between them to partition.
- The tests that do drive a real tmux binary are guarded on tmux availability and **already**
  derive their session names from a per-test UUID.
- `npm test` was executed ten consecutive times during gating: ten green, with the
  inspector-panel test completing well inside its timeout each run.

The proposed mechanism was inferred from three records *mentioning* flakes, not observed. Do not
resurrect it without evidence.

**What triage needs before this can carry a brief.** A reproduction, or a decision to close.
Suggested order:

1. Establish whether the flakes still occur at all. The three sightings span 2026-08-09 to
   2026-08-30 and predate several changes to the tests involved; the symptom may already be gone.
   Repeated full-suite runs under the same conditions CI uses is the cheapest probe.
2. If reproduced, capture the actual failure output rather than the fact of failure. Which
   assertion, which value, which run. The two Rust tests being hermetic means any nondeterminism in
   them is internal — ordering, timing against a fake invocation, or shared process-level state —
   not cross-test tmux contention.
3. If not reproduced after a fair attempt, close as `wontfix` with the probe recorded, so the next
   sighting starts from evidence rather than from these three second-hand mentions.

**Relationship to other records.** `ISSUE-260901-0216-05` asks a human to observe the CI workflow's
first run. That run is a free probe for step 1: if it fails on one of these tests, this record gets
its reproduction; if it passes, that is one data point against the symptom still being live. Neither
record blocks the other.
