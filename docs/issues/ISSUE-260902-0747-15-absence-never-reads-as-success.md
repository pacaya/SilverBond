---
id: ISSUE-260902-0747-15
kind: issue
category: bug
status: needs-info
summary: Every external-dependency guard in the Rust suite returns early and reports a pass when its dependency is absent, so a machine missing tmux, an interactive shell, or root privilege reports the same green as one that exercised the guarded contract
prd: PRD-260902-0301-01
adrs: [ADR-260902-0312-01]
terms: [Integration Tier]
blocked_by: [ISSUE-260902-0747-01]
---

## Agent Brief

**Category:** bug
**Summary:** Choose one encoding for "this test's external dependency is absent" and apply it to every guard in the Rust suite, so absence is never indistinguishable from success.

**Baseline:** this slice is blocked by `ISSUE-260902-0747-01`, so every criterion below is read against
the tree after the tier selector exists. Read it against the tip of `feature/tmux-panes`, not `main`.

**Current behavior:**
Tests across the suite begin by consulting an availability probe and returning early when it is false.
Rust's default harness reports an early return as `ok`, so a machine without the dependency produces
the same green as one that ran the guarded contract. The probes differ from each other, and at least
one is weaker than it looks: it asks whether an invocation *ran*, not whether it *succeeded*, so only
an absent binary takes the early return.

Derive the set rather than working from a count:

```
rg -n -B4 '^\s+return;\s*$' src/ tests/ | rg 'available\(\)|geteuid|is_ok_and'
```

Read each hit. The command's `-B4` window and its `return;` shape are both approximations — widen the
window or the pattern if a guard's early return is spelled differently, and record any hit the pattern
missed so the next reader has it.

**Desired behavior:**

*One encoding, chosen once and applied to every guard.* The point of doing these together is that a
reader learns the convention once. Applying a different encoding per dependency is the outcome to
avoid.

Rust's default harness has no dynamic skipped outcome, so "skip loudly" is not directly expressible.
The encoding is the implementer's choice among a static opt-in, a hard failure where the dependency is
**expected**, or a reporter that surfaces the condition (`ADR-260902-0312-01` § Consequences, "A test
that needs a real external dependency gets an isolated one, and its absence never reads as success").
Two bounds hold whichever is chosen: a run on a machine missing the dependency must not be
indistinguishable from a run that exercised the guarded tests and passed; and if the encoding is a
failure, its message names the missing dependency specifically, so the condition is distinguishable
from a genuine regression in the contract under test. If a probe is kept in any form, it checks the
exit code and not merely that a process ran.

*What "expected" means here.* A dependency is **expected** when CI installs it — today `tmux` and
`zsh`, per `.github/workflows/rust-tests.yml`. Absence of an expected dependency is a hard failure: it
means the environment is broken.

Root plus `sudo` plus a system account is **not expected**. No GitHub-hosted runner provides it
(`runs-on: ubuntu-latest`, no `container:`, so `libc::geteuid() != 0` always holds) and few developer
machines do. A hard failure there would make the Integration Tier job permanently non-green for a
reason that is not a defect, which `ADR-260902-0312-01` § Consequences rejects on the same grounds it
rejects hiding the tier from CI. Encode that guard as the **static opt-in** arm: the test is not run
unless the environment is opted in, and not running is visible in what the command reports rather than
reported as a pass.

This is one convention with two arms selected by a stated property of the dependency, not two
conventions.

*Tier.* Every guarded test here is Integration Tier — they spawn processes and touch shared mutable OS
state. They are **kept and repaired, not deleted**: they assert SilverBond's own contracts, not the
dependency's behavior, so the "a dependency is tested by the repository that owns it" rule does not
reach them (`ADR-260902-0312-01` § "A dependency is tested by the repository that owns it", second
paragraph).

**Key interfaces:**
- The chosen encoding, as it appears at one guard — the thing a reader reads once to learn the
  convention.
- The availability probes — removed, or corrected to check the exit code and make absence observable.

**Acceptance criteria:**
- [ ] `rg -n 'if !tmux_available\(\)' src/tmux_exec.rs` returns no matches; it returns an early-return
      guard at each tmux guard test before this change.
- [ ] With the Integration Tier switch on and tmux absent from `PATH`, the tmux guard tests do not
      report as passed, and the reported outcome names the missing tmux dependency — a signal the
      passing path never emits. With tmux present they run and pass. Observable at the Integration Tier
      command introduced by `ISSUE-260902-0747-01`.
- [ ] The same holds for the interactive-shell guard. Both halves are required and neither alone
      suffices. A message is not enough on its own: the privilege-guarded FIFO test already prints a
      line naming its missing dependency on the very path where it reports a pass, so a criterion
      satisfied by a message is green before the change. A non-pass outcome is not enough on its own
      either: deleting the guards satisfies it — the unguarded body panics on the absent dependency —
      while building none of the encoding and producing a failure indistinguishable from a real
      regression. The acted-on complement: with the shell present, the same tests run and pass.
- [ ] The privilege-guarded FIFO test is not executed unless its environment is opted in, and its
      not-running is visible in what the Integration Tier command reports. It does **not** report a
      pass when root, `sudo` or the system account is unavailable, and it does **not** fail the run.
      The acted-on complement for this guard is the opt-in itself: with the opt-in off the test is
      absent from the report, with it on the test is present. Do not discharge this by asserting a
      pass on an environment the project has none of.
- [ ] All guarded dependencies use the **same** encoding, with the expected/not-expected arms selected
      by the rule in Desired behavior. A reader can state the convention after reading one guard.
- [ ] The discovery command in Current behavior returns no guard that reports a pass on absence after
      this change, and returns every guard listed above before it.
- [ ] Every hit the discovery command returns either carries the encoding or carries a one-line note
      in this record saying why it could not. A hit left alone with no note does not satisfy this.

**Out of scope:**
- Isolating the guard tests on a dedicated socket — `ISSUE-260902-0747-04`.
- Any change to production tmux binary resolution. `ADR-260622-0208-01` is respected, not revisited.
- Deleting the guard tests, or moving them upstream to the `tmux-tools` repository. They assert an
  owned contract.
- The process-group termination test, which is irreducible and stays as written in the Integration
  Tier. It is unguarded — it needs no absent-dependency encoding.
- Changing what any guarded test asserts. This issue changes how absence is reported, not the contract
  under test.
- Adding a CI runner, container, or privileged environment so the FIFO test can run. That is an ops
  decision this epic does not take.

## Triage Notes

Split from `ISSUE-260902-0747-04` on 2026-09-05 by maintainer decision, after that record's round-4
readiness gate fired class 6 on both prongs. Prong (a): the acceptance criteria required a hard
failure for a dependency the project has no environment for, which is an ops decision no artifact
authorized. Prong (b): the socket-isolation batch is four RAII guard tests in `src/tmux_exec.rs`, while
the honest-absence work reaches those four plus the interactive-shell guard and the privilege-guarded
FIFO test in `src/api.rs` — no shared test function with the socket batch, the same shape on which
`ISSUE-260902-0747-14` was split out at round 3.

`PRD-260902-0301-01` § Implementation Decisions authorizes one slice owning the encoding across all
guards; that constraint is satisfied here and was not the reason for the split.

The expected/not-expected rule in Desired behavior is the maintainer's answer to the round-4 finding
that AC4's privilege complement named an environment available nowhere in-repo or in CI.

**Readiness gate:** not yet run on this record.
