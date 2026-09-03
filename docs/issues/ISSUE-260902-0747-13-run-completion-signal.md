---
id: ISSUE-260902-0747-13
kind: issue
category: enhancement
status: needs-info
summary: The run registry drops a run's entry before cancelling its completion token, so a run that finishes quickly leaves no edge to await and every test waiting for a run to reach a terminal state polls persisted state against a wall-clock deadline
prd: PRD-260902-0301-01
adrs: [ADR-260902-0312-01]
terms: [Run, Logic Tier, Integration Tier]
blocked_by: [ISSUE-260902-0747-01, ISSUE-260902-0747-06]
---

## Agent Brief

**Category:** enhancement
**Summary:** Make a run's completion observable after the fact, and delete the two clock-bound helpers that poll for a terminal run and for a drained registry.

**Baseline:** this slice is blocked by `ISSUE-260902-0747-01`, so every criterion below is read
against the tree **after** the tier selector exists. Read against today's tree the tier halves look
already-satisfied, because today the default command executes everything.

**Current behavior:**
Each active run holds a drained token that is cancelled when the registry clears the run — but clear
removes the registry entry **before** cancelling it. An accessor shaped like the existing abort-signal
accessor resolves through the entry that clear has already removed, so it returns nothing for a run
that finished quickly, with no edge left to await. Exposing the token symmetrically is not sufficient,
and the naive version is racy.

Because completion is unreachable by construction, the runtime's test module carries two helpers that
poll instead: one that polls persisted state until the run reaches a terminal status
(`wait_for_terminal_run`, `src/runtime.rs:9878`) and one that polls the registry until a run's active
pane targets clear (`wait_for_registry_empty`, `:8994`). Both fail at a fixed wall-clock deadline
sized for an idle machine. This is the mechanism of the reproduced failure: a deadline closed by
contention, with the producer merely descheduled rather than broken.

The terminal wait is the **heaviest** of the polling helpers in this suite by call-site count — it
carries more call sites than the other four combined. An earlier draft of the sibling record called
terminal waits "the minority of the polling sites", which is false and was the only argument for
keeping the two halves in one record.

`wait_for_terminal_run` is implemented on top of `wait_for_run`, the predicate poller that
`ISSUE-260902-0747-05` deletes. That is why this record is sequenced **before** it: removing the
predicate poller first would leave the terminal wait without an implementation.

Discovery command for the helper definitions and their call sites:

```
rg -n 'async fn wait_for_(terminal_run|registry_empty)\b' src/runtime.rs
rg -F -n 'wait_for_terminal_run(' src/runtime.rs
rg -F -n 'wait_for_registry_empty(' src/runtime.rs
```

Derive the call-site population from those commands at your baseline rather than from any count
recorded here or upstream.

**Desired behavior:**
A caller can observe a run's completion **after the fact**, without polling and without a wall-clock
deadline, and the same holds for the registry having drained a run's pane targets.

*Completion observable after the fact.* The drained-run signal is either handed out at registration —
so a caller holding it from before the run started still gets a resolved answer once the run has
completed and been cleared — or it is level-triggered, so a caller asking the registry for the first
time **after** completion and clear still receives a resolved answer rather than nothing. Which shape
is the implementer's choice; a registry lookup that resolves through an already-removed entry is the
failure mode to design out either way.

*State the retention bound.* A level-triggered answer needs something to survive `RunRegistry::clear`,
and nothing in this epic's records currently says for how long. Decide it in this record and state it:
how long a completed run's signal remains answerable, what bounds the set of retained entries, and
what a caller gets once that bound is passed. An unbounded tombstone map is a leak; an unstated
retention is the same defect one layer down. If the chosen shape is handout-at-registration and no
retention is required, say so explicitly rather than leaving the question unanswered.

*Deletion, not softening.* Both helpers are removed rather than given longer deadlines. Raising a
deadline is rejected on evidence (`PRD-260902-0301-01` § Problem Statement) and no stopgap is taken.

*The database stays real.* `Database` is an owned, embedded implementation detail, not an external
dependency to be faked. What makes these tests fragile is that they poll persisted state rather than
being told it changed. A per-test temporary-directory store is **controlled data** and therefore Logic
Tier under the isolation rule, so removing the polling is what promotes these tests — not removing the
database. In-memory SQLite is rejected outright and a dictionary-backed repository fake is deferred
(`ADR-260902-0312-01` § "A repository fake is considered and deferred").

*Tier outcome — the whole rule, not a two-condition shorthand.* A test this record touches moves to
the Logic Tier only if, after this change, it satisfies `ADR-260902-0312-01`'s Logic Tier rule
**entirely**: every port faked or supplied with controlled data, and none of the forbidden forms — no
spawned process, no network, no shared mutable OS state, no sleep to sequence work, no assertion on
elapsed time, and no wall-clock bound ending a polling or convergence wait. Losing a polling deadline
is necessary and **not** sufficient. Derive per test which condition each call site is left in, and
record which tests you promoted and which you left marked, by name.

Do not expect a single family. Derive the left-marked set rather than assuming it, because tests stay
marked here for **three** different reasons and only the first is obvious:

- They still sleep to sequence work. `rg -F -n '.with_delay(' src/runtime.rs`, attributed to owning
  function, derives these; `ISSUE-260902-0747-12` removes those sleeps and owns their promotion.
- They still hold a clock-bound wait this record does not delete — `wait_for_run`, `wait_for_event`
  or `wait_for_pending_approval`. Those are `ISSUE-260902-0747-05`'s, and it is sequenced *after* this
  record, so a test holding one stays marked here for a reason that has nothing to do with sleeping.
  This set overlaps the first and is not contained in it.
- They wait on a filesystem sentinel through `wait_for_path`, which **no slice of this epic converts**
  (see Out of scope). Those tests stay marked at the end of the epic, not just at the end of this
  record, and the left-marked set says so rather than implying a later slice picks them up.

The second set is shared with `ISSUE-260902-0747-05`, so derive it **once**, here, and record it by
name under this record's completion note. `-05` consumes that named derivation rather than writing its
own — a disagreement between the two must be visible, not silent.

**Key interfaces:**
- The run registry's drained-run token and its accessor — the accessor must not be shaped like the
  existing abort-signal accessor, which resolves through the entry the clear path has already removed.
- `RunRegistry::clear` — the ordering defect lives here; whatever survives clear is what makes a late
  answer possible.
- The two polling helpers, deleted along with their call sites' reliance on a deadline.

**Acceptance criteria:**
- [ ] `rg -n 'async fn wait_for_(terminal_run|registry_empty)\b' src/runtime.rs` returns no matches;
      both helper definitions are present before this change.
- [ ] The completion signal answers after the fact, in the **level-triggered** form: a caller that
      asks the registry's completion accessor for the first time **after** the run has completed and
      been cleared receives a resolved answer rather than nothing. The PRD sanctions two forms and
      this record can only take one of them: handing a signal out at lifecycle start requires a
      run-lifecycle entry point to return it, which is `ISSUE-260902-0747-05`'s declared interface
      change, and `-05` is sequenced after this record. Taking the handout form here would mean
      changing those signatures twice. This is impossible before the change: the clear path removes
      the entry before cancelling the token, so a late lookup finds nothing.
- [ ] The retention bound is stated **at the accessor's own definition in the code** — not under this
      record's notes, which reach no later reader — and enforced there: a test drives the bound past
      its limit and observes the documented answer rather than an unbounded set or a panic. The
      level-triggered form retains something by construction, so this criterion is not dischargeable
      by declaring that no retention is needed. What the bound is — a count, a duration, a
      generation — is the implementer's, within one constraint: it is bounded, and the answer past
      the bound is documented and asserted.
- [ ] No runtime test awaits a fixed wall-clock deadline to observe that a run reached a terminal
      state or that the registry drained. Every such wait is an awaited signal.
- [ ] Every test this record promotes satisfies `ADR-260902-0312-01`'s Logic Tier rule in full at this
      record's completion — not merely "lost its clock wait". At least one test is promoted and at
      least one is deliberately left marked, and the record names both sets, so the criterion cannot be
      satisfied by promoting nothing.
- [ ] The promoted tests are executed by the default command after this change and were not before it.
      Read this from what the command reports it executed, not from `cargo test -- --list`, which
      enumerates `#[ignore]`d tests and so cannot witness an exclusion.
- [ ] `wait_for_run` still exists and still compiles after this change. It is
      `ISSUE-260902-0747-05`'s to delete, and this record must not leave it without callers in a way
      that pre-empts that record's observables.
- [ ] Existing behavior is otherwise unchanged: a run still registers before it spawns, and a second
      concurrent registration of the same run id is still refused.

**Out of scope:**
- Race-free acquisition of a run's **event stream** at the three lifecycle entry points, and the
  deletion of `wait_for_run`, `wait_for_event` and `wait_for_pending_approval` — that is
  `ISSUE-260902-0747-05`, which is sequenced after this record.
- The abort path's poll tick and its promptness assertions — `ISSUE-260902-0747-06`.
- The scripted-delay sleeps that keep the `with_delay` family marked — `ISSUE-260902-0747-12`.
- Converting the out-of-crate HTTP target, which holds the epic's reproduced failure and is
  `ISSUE-260902-0747-11`.
- `wait_for_path`, the filesystem-sentinel wait. **No slice of this epic converts it**, and that is a
  deliberate gap rather than an omission: the tests that hold one stay Integration Tier past the end
  of the epic. Name them in the left-marked set with that reason, so a later reader does not go
  looking for the slice that picks them up.
- Faking SQLite or moving to in-memory storage.

## Triage Notes

Minted 2026-09-02 by splitting `ISSUE-260902-0747-05` at the terminal/intermediate line, on the
maintainer's decision the same day. The readiness gate fired class 6 prong (b) on `-05`: two
production surfaces (the drained token's lifetime, and race-free event-stream acquisition at the entry
points), two seams, two independently demoable halves. `-05`'s only cohesion argument was the claim
that terminal waits are "the minority of the polling sites", which the gate falsified — they are the
larger share.

This record takes the completion half and is sequenced first, because `wait_for_terminal_run` is
implemented on `wait_for_run` and `-05` deletes the latter.

`PRD-260902-0301-01` frames the production change here as a design gap the tests merely exposed: a
fire-and-forget spawn with no completion signal is under-specified regardless of how it is tested. It
is not a testability hack and should not be reviewed as one.

The contractual `### Timing-site audit` table in `ISSUE-260901-0216-03` carries a row for
`wait_for_registry_empty` — an A-site with a B-site poll tick — and **no row for**
`wait_for_terminal_run`, which appears only in that record's non-contractual scale snapshot. Neither
helper is load-bearing for an assertion — arbitrary headroom better replaced by a happens-before
edge, not load-bearing for an assertion.

**Readiness gate:** not yet run. This record has never been gated; it inherits `-05`'s round-2
findings only where they bear on the completion half, and its first gate is round 1.

### Round 1 findings — 2026-09-02

**Readiness gate (cold-reader): FAIL** (round 1)

`blocked_by` gains `ISSUE-260902-0747-06`: two sites wrap a terminal wait in a fixed timeout inside
abort-path tests, `-06` claims them, and nothing ordered the two records. AC2 now takes the
**level-triggered** form rather than offering both — handing a signal out at lifecycle start requires
an entry-point signature change that `-05` owns and `-05` is sequenced after this record — which makes
the retention bound mandatory, so AC3 is pinned to the accessor's own definition and can no longer be
discharged by declaring no retention is needed.

The left-marked set is derived rather than assumed: tests stay marked here for three different
reasons, and the scripted-delay family is only the most obvious. The set shared with `-05` is derived
once here and consumed by name there. `wait_for_path` is named as converted by **no** slice of this
epic, so the tests holding one stay marked past its end. The claim that the audit table classifies
`wait_for_terminal_run` is corrected — it has no row there.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round3.md`.
Awaiting re-gate.
