---
id: ISSUE-260902-0747-06
kind: issue
category: enhancement
status: needs-triage
summary: Abort is only checked at the top of the supervisor's loop so worst-case latency is a full poll tick, and the abort path carries elapsed-time promptness assertions against fixed wall-clock bounds that a loaded machine closes
adrs: [ADR-260902-0312-01]
terms: [Run, Logic Tier, Integration Tier]
blocked_by: [PRD-260902-0301-01]
---

## Deferral note

Deferred 2026-09-05, out of `PRD-260902-0301-01`'s delivery scope and into the backlog, following the
maintainer's decision to narrow that epic to the reproduced failure plus the forward-facing tier rule.
The narrowed epic delivers `-01`, `-11`, `-04`, `-03`, `-14` and a scoped `-10`; this record is good
work that is not that task.

Deferring it does not retire the problem it describes. It is retained debt under
`docs/adr/260902-0312-deterministic-test-tiers.md` § Retained debt: the tests it would have repaired
stay in the Integration Tier, stay in the non-gating job, and must not be described as fixed.

**Do not implement this brief as written without re-triage.** It was authored against the pre-narrowing
ADR and PRD, and its `blocked_by` chain assumes slices that are no longer sequenced.

**Known defect, from the 2026-09-05 adversarial review.** The brief requires the reconciled audit
population to be carried in production source comments alongside assertions and constants. That
recreates a decaying inventory in `src/` after the PRD correctly rejected the same pattern in planning
prose. If revived, keep comments to invariants, units and ownership; evidence belongs in this record.

## Agent Brief

**Category:** enhancement
**Summary:** Give the supervisor's select loops an abort arm, and replace each wall-clock promptness assertion with the ordering it was standing in for.

**Baseline:** this slice is blocked by `ISSUE-260902-0747-01`, so every criterion below is read
against the tree **after** the tier selector exists.

**Current behavior:**
*Abort waits for a tick.* The run supervisor consults the registry's abort state at the top of its
loop and nowhere else, so an abort delivered just after that check is not acted on until the loop
comes round again. Worst-case abort latency is therefore one full poll tick, even though the abort
signal is a cancellation token that could be selected on directly. Other paths already do exactly
that — the node-execution path and the parallel-batch join both carry a cancellation arm — so the
supervisor is the outlier.

*Derive the elapsed-time assertions on the abort path; do not take a count from anywhere.* Successive
drafts of this record and of the PRD have each named a set that was too small. Build the set from two
independent passes and reconcile them: the table keyed by owning function under
`### Timing-site audit` in `ISSUE-260901-0216-03`, which is contractual for classification; **and** an
independent sweep for elapsed-time reads on the abort path
(`rg -n '\.elapsed\(\)' src/runtime.rs`, then attribute each hit to its owning test). Expect the two
to disagree, and treat the disagreement as the finding.

Two members are named here because a sweep has missed each of them before, not as the roster. One
guards the force-clear path when a drain never arrives and is bounded at two seconds rather than a
half-second. The other is
`abort_kills_active_panes_before_draining_running_tasks` (`src/runtime.rs:11070`), which asserts
`started.elapsed() < TMUX_CLEANUP_TEST_TIMEOUT`, and whose sibling bound at `:11062` wraps a terminal
wait in a timeout reported as "abort did not reach a terminal state promptly" — the forbidden
convergence-wait shape. **It is in scope.** An earlier draft excluded it on the ground that the audit
classifies it `A`, which `ADR-260902-0312-01` § Liveness forbids as an instrument for drawing this
line: classifying a bound as `A` does not make it permissible. `ISSUE-260901-0216-03` names that same
test as the epic's **second independent reproduction**, so excluding it would drop a reproduced
failure out of the record that exists to fix it.

Each of these asserts that elapsed time since abort was delivered is under a fixed bound, and the
bounds are not one size: they run from a half-second through a two-second force-clear bound to the
fifteen-second tmux-cleanup ceiling. Do not assume a shared magnitude or a shared classification.
Raising a bound deletes whatever property it was carrying, and leaving it deletes nothing but
reliability — but *which* of those two costs applies is per site, and the audit's A/B/C column is
evidence for that judgement rather than the judgement itself. At least one member of the set the
audit scores `A` is nonetheless load-bearing here, which is why the derivation reconciles two passes
instead of reading one column. One member sits inside the abort-promptness test originally reported
as flaky, so a bound intended to catch a regression is itself a false-red source.

*One constant carries two opposite polarities.* The tmux-cleanup test timeout is used both as a
liveness bound — wait at most this long — and, in the same test, as the ceiling of a promptness
assertion — this must have completed within this long. Raising it to buy headroom therefore silently
weakens an assertion. The two uses must be decoupled before either can be touched.

Discovery commands:

```
rg -n 'Duration::from_millis\(500\)' src/runtime.rs
rg -n 'TMUX_CLEANUP' src/runtime.rs
```

**Desired behavior:**

*The abort token is an arm of the supervisor's select loops.* An abort delivered while the supervisor
is between poll ticks takes effect without waiting for the next tick. This change stands on its own
merits — it reduces real abort latency for a user who presses stop — and is not a testability
concession.

*The promptness assertions get structural replacements, decided per path.* Adding the select arm
does **not** dissolve them, and a brief that assumed it would would be wrong. State no count of the
set: derive it by the two-pass reconciliation above and then read each member's path. What you will
find is that they do not share one story — some sit on paths that already carry their own
cancellation arms, at least one never runs the supervisor at all, the interaction-escalation path
already selects on the abort token, and at least one **does** sit on a supervisor path this change
alters. A blanket claim in either direction is the error this instruction exists to prevent. Each assertion therefore needs its
own replacement, chosen after reading the path it guards:

- Replace the elapsed-time assertion with the **ordering** that promptness was standing in for — that
  the abort-triggered effect is observed before some other event that would only follow a tick, or
  that a signal handed out before the work started resolves. This is the preferred outcome and is what
  `ADR-260902-0312-01` § Consequences means by "where a test asserted that an operation was prompt, it
  asserts the ordering that promptness was standing in for".
- Or, where the property genuinely *is* temporal — a promptness guarantee the system owes a user, not
  a convenience bound — keep it as a timing assertion in the **Integration Tier**, with a generous
  bound and a comment saying why it is irreducible. A half-second is not a generous bound, so a
  retained assertion is re-sized as part of retaining it.

Which of the two applies to each assertion is the implementer's finding, made against the path in
hand. Record the reasoning for each at its site.

*The cleanup-timeout polarities are decoupled.* The liveness bound and the promptness assertion
reference two distinctly named constants, so that changing the headroom of one cannot silently move
the threshold of the other. Do this first: it is a precondition for touching either.

**Key interfaces:**
- The run supervisor's select loops — an additional arm on the run's abort cancellation token,
  alongside the existing top-of-loop abort check, which stays as the state check it is.
- The tmux-cleanup timeout constant — split into named constants by role, one per role it actually
  serves. Two roles are known (the liveness bound and the promptness ceiling) and a third exists
  inside `AbortBlockingRunner`; whether the third takes a name of its own or is folded into one of the
  other two is the implementer's call, provided the choice is explicit and the polarity hazard cannot
  recur.
- The elapsed-time assertions on the abort path and their surrounding tests, each of which may change
  tier. Promotion is decided by the tier rule in `ADR-260902-0312-01` — what the test isolates, and
  whether it asserts elapsed time — not by whether it spawns a process. One of these tests drives a
  runner double that sleeps to simulate work; losing its elapsed-time assertion does not by itself
  make it Logic Tier while that sleep remains.

**Acceptance criteria:**
- [ ] No Logic Tier test asserts on elapsed time anywhere on the abort path, and **every** former
      promptness assertion in the set you derived is accounted for by name — replaced by an ordering,
      or retained as an Integration Tier temporal assertion with a generous bound. The record states
      the set it derived and the two passes it reconciled to get there;
      `abort_kills_active_panes_before_draining_running_tasks` is in it. "The record states" means at
      the site the work lands — beside the replaced assertions, or beside the split constants — not
      under this record's `## Triage Notes`, which no later reader of the code opens. Anchoring this to the literal
      `Duration::from_millis(500)` is **not** sufficient and must not be the whole criterion:
      re-sizing that literal to another value turns such a check green with none of the work done.
- [ ] `rg -n 'TMUX_CLEANUP' src/runtime.rs` returns distinct constant names for distinct roles, and
      no single name serves both the liveness bound and the promptness ceiling. Note there is a
      **third** use inside `AbortBlockingRunner` (`src/runtime.rs:7950`) which is neither of those two
      roles; assign it explicitly rather than letting a two-way split silently absorb it. A single
      name serves every role before this change.
- [ ] The supervisor obtains the run's abort cancellation token and **every** select loop it owns
      carries an arm awaiting that token's cancellation — the running-task select in both its
      approval-pending and plain forms, and the approval wait it delegates to — so an abort delivered
      between poll ticks is acted on without waiting for the next tick. The top-of-loop registry check
      stays as the state check it is. Witness by **owner**, not by hit count: attribute each
      `rg -n 'abort_signal' src/runtime.rs` hit to its enclosing function and check that the
      supervisor's own function appears among them after this change and does not before it. The bare
      command is green at baseline — it returns hits either way — so the command alone cannot witness
      this and must not be cited as if it could. This criterion is structural
      because the change is a **delivery path**, not a latency: see § "AC3 is structural, and why an
      ordering assertion cannot stand in for it" in the triage notes before proposing a behavioral
      substitute.
- [ ] `parallel_batch_abort_cancels_pending_items_after_next_completion` asserts abort-beats-completion
      **structurally**, and its scripted delay is gone rather than merely unasserted. The delay is
      what currently guarantees the property: batch concurrency is one, the test wakes on
      `cursor_spawned` — which the runtime emits *before* the item future enters the join set, so the
      first item may or may not have started — and only the two-second delay makes both schedules
      safe. Replace it with a runner double that signals when the item has entered the port and then
      blocks on a release the passing abort path never needs, deliver the abort after that signal, and
      assert the same batch summary: no item succeeded and all four were cancelled. Deleting the delay
      while keeping the elapsed bound is a defect — the bound goes green on an instantaneous item with
      none of the property preserved.
- [ ] Each former promptness assertion either asserts an ordering, or carries an Integration Tier
      marker together with a comment naming the temporal property it asserts and why that property is
      irreducible. No elapsed-time assertion remains in a Logic Tier test.
- [ ] A test that no longer asserts elapsed time but still sleeps to sequence work — through a runner
      double with a scripted delay, for instance — is **not** promoted. Promotion requires the tier
      rule to be satisfied, not merely the assertion to be gone.
- [ ] Any member of the derived set that ends up asserting an ordering, spawning no process, and
      satisfying `ADR-260902-0312-01`'s Logic Tier rule in full — which permits a bound that fires
      only on a hang, so "waits on no clock" is not the test — is executed by the default command
      after this change and was not before it. Where no member qualifies at this record's baseline,
      say so and name why per member: several hold waits that `ISSUE-260902-0747-05` and
      `ISSUE-260902-0747-13` remove later, and this record does not promote on their behalf. Read this from
      what the command reports it executed, not from `cargo test -- --list`.
- [ ] Abort semantics are otherwise unchanged: an aborted run still drains, still clears its registry
      entry, and still kills its active panes before draining running tasks.

**Out of scope:**
- The run-lifecycle event handle and the deletion of the polling helpers —
  `ISSUE-260902-0747-05`. This issue touches the abort path only.
- The scripted-delay fixtures whose bare integers encode concurrency orderings, **with one exception
  wholly owned here**: `parallel_batch_abort_cancels_pending_items_after_next_completion`. Its
  `with_delay(2000)` and its half-second bound are two halves of one calibration — the audit records
  them as calibrated against each other — so this record converts **both**. It also owns **every
  other `with_delay` call in that same test**, not only the two-second one: `ISSUE-260902-0747-12`
  excludes the test entirely and its AC1 is absolute over the file, so any scripted delay left
  standing in it blocks `-12` from deleting the field and `-12` is forbidden from reaching in. Derive
  them with `rg -F -n '.with_delay(' src/runtime.rs` attributed to that function. Every scripted-delay
  fixture in any *other* test belongs to `-12` and is not touched here.
- Any change to what abort does. This issue changes when the supervisor notices it, not the effect.
- Adopting a virtual clock. `tokio`'s test-util feature stays disabled and time pausing stays unused;
  the select arm is what removes the poll tick on this path.

## Triage Notes

Minted 2026-09-02 from `PRD-260902-0301-01`; breakdown approved by the maintainer the same day.
Blocked by `ISSUE-260902-0747-01` for the tier assignments in its acceptance criteria.

Kept separate from `ISSUE-260902-0747-05` at the maintainer's confirmation, on the ground that the
select arm and the run handle solve different problems on different paths — the PRD is explicit that
the former does not dissolve the latter's assertions, and an earlier PRD draft that claimed it would
was corrected.

**Scale snapshot (non-contractual):** `rg -c 'select!' src/runtime.rs` → a handful of select sites,
of which the supervisor's are the subject here (2026-09-02). The A/B/C classification for the abort
tests is in `ISSUE-260901-0216-03` § "Timing-site audit"; the three half-second bounds and the
cleanup-timeout polarity hazard are both called out there under the paragraph beginning "Two hazards
would corrupt any mechanical sweep".

### Readiness gate round 1 — findings

**Readiness gate (cold-reader): FAIL** (round 1)

All ten `ISSUE-260902-0747-*` briefs were gated in parallel on 2026-09-02 (`cold-reader`, `model: opus`,
one per record, rubric `~/.claude/skills/triage/READINESS-GATE.md`). All ten returned FAIL. The reports
were not written to disk; the compressed findings below and in
`docs/handoff/deterministic-test-tiers-2026-09-02-1419.md` are the sole surviving record of them.

`status:` is `needs-info` because two upstream things are owed. `PRD-260902-0301-01` was amended after
this round (HTTP endpoint tests moved to the Logic Tier, storage split per-test, the Logic Tier rule
restated as isolation with controlled data) and is itself awaiting a re-gate; and
`ADR-260902-0312-01` now decides tier membership by **what a test isolates**, not by what it touches,
which every brief in this batch predates.

**Systematic defects across the batch** (fix in one sweep, not per record):

1. **Count-as-polarity** — pre-change polarity written as a count beside a discovery command
   ("It returns four test call sites as well before this change"). `AGENT-BRIEF.md`
   § *Qualitative polarity vs decaying state* requires qualitative phrasing, never a count.
   Present in `-02`, `-04`, `-06`, `-07`, `-08`, `-09`; the correct form is already used in `-01`,
   `-03`, `-05`, `-10`.
2. **Broken listing template** — "appears in the Logic Tier listing (`cargo test --locked -- --list`)
   after this change and did not before it" is false wherever a test is already deterministic.
   Confirmed false in `-07` and `-08`. Present in `-05` through `-09`.
3. **`cargo test -- --list` includes `#[ignore]`d tests** — proven by `regeneration_writes_to_disk`
   appearing in the listing. Any `#[ignore]`-shaped selector makes every listing-based criterion in
   the batch unfalsifiable. This constrains `-01`'s selector choice.
4. **Templated `just test-under-load` closing AC** in `-05`, `-06`, `-07`, `-09`, `-10` is already
   green at each record's declared baseline (post-`-01`); only the unexecutable qualifier carries
   content. `just test-under-load` is also not in version control and has no owning slice.

**This record:**

**AC3 is structural, and why an ordering assertion cannot stand in for it (2026-09-02, amended).**
The round-1 gate found AC3 unsatisfiable: the supervisor's poll ticks are inline duration literals, a
virtual clock is out of scope, so a behavioral assertion appeared to need a poll-interval parameter
this record is not authorized to add. Two branches were offered — parameterize the interval, or drop
AC3 — and a third was tried and **retracted**. This note records the retraction so it is not
re-attempted.

*The retracted third option.* AC3 was briefly restated as an ordering: a node-runner double blocks at
a barrier, the test issues the abort while the runner is blocked, and asserts the abort effect is
observed before the runner is released — on the premise that a blocked runner "holds the tick open",
making the ordering unreachable without the select arm. **That premise is false.** The supervisor's
selects each carry an independent `tokio::time::sleep(Duration::from_millis(250))` arm
(`src/runtime.rs:3007`, `:3022`, `:3772`) that fires whether or not a runner is blocked; the loop then
re-enters and the top-of-loop `ctx.registry.is_aborted` check at `src/runtime.rs:2824` runs
`kill_active_run_panes`. The tree already demonstrates it: `AbortBlockingRunner`
(`src/runtime.rs:7897`) blocks until the kill marker appears — its release *is* the abort effect — and
`abort_kills_active_panes_before_draining_running_tasks` (`src/runtime.rs:11039`) exercises exactly
that ordering today, with no select arm anywhere. The ordering is therefore green at baseline. It is a
vacuous criterion of the same class the round-1 gate rejected in `-04`'s AC6.

*Why no behavioral criterion can separate the two.* Poll and push differ only in latency — worst case
one tick versus none — and latency is a clock property. Distinguishing them behaviorally requires
either controlling the tick interval (the rejected production surface) or stopping the clock
(`tokio::time::pause()`, which reopens the virtual-clock decision the ADR closed and on which `-07`'s
tier outcome already depends). Neither is authorized here.

*What AC3 asserts instead.* The delivery path itself, witnessed structurally — the supervisor obtains
the abort token and selects on it. This is the same idiom as this record's `TMUX_CLEANUP` criterion,
it is red at baseline (`abort_signal` at `src/runtime.rs:1002` is called by `run_decide_node` `:4042`,
`handle_parallel_batch_node` `:5166` and `escalate_agent_interaction` `:7278`, and by no supervisor
code), and it cannot be turned green by re-sizing a literal or by deleting an assertion. The existing
barrier test stays as a regression guard on abort semantics — it is not the discriminator, and under
the timing audit its two timing sites are A-classified, so it is not one of the four promptness
assertions AC1 enumerates.

*Provenance.* Chosen by the maintainer on 2026-09-02 from four options put to them (parameterize the
interval / drop AC3 / authorize a virtual clock / assert the delivery path structurally), after the
false premise above was found by reading the supervisor loop.

- AC3 has **no reachable test seam**: supervisor poll ticks are inline `Duration::from_millis(250)` literals (`src/runtime.rs:3007`, `:3022`, `:3772`), a virtual clock is out of scope, so a structural assertion needs a poll-interval parameter the brief never authorizes.
- AC5's promotion condition ("spawning no process") is not the tier rule — `parallel_batch_abort_cancels_pending_items_after_next_completion` sleeps via `with_delay(2000)` (`src/runtime.rs:7587-7588`).
- A **fourth** C-classified elapsed assertion is unclaimed: `abort_and_wait_force_clears_when_drain_never_arrives` (`src/runtime.rs:8108-8122`), 2s bound.
- AC1 anchors to the literal `from_millis(500)`, so re-sizing it to 750 turns the criterion green with none of the work done.
- Count-as-polarity, broken listing template, templated closing AC (systematic defects 1, 2, 4).

### Readiness gate round 2 — findings

**Readiness gate (cold-reader): FAIL** (round 2)

- **Class 3/1, build-changing — a fifth elapsed-time assertion, excluded on the forbidden ground.** `abort_kills_active_panes_before_draining_running_tasks` asserts `started.elapsed() < TMUX_CLEANUP_TEST_TIMEOUT` at `src/runtime.rs:11070`, on the abort path. This brief excludes it because the audit scores it **A** — the exact instrument `ADR-260902-0312-01` § Liveness forbids for drawing this line ("classifying a bound as A does not make it permissible") — and the exclusion lives in `## Triage Notes`, which no implementer reads. `ISSUE-260901-0216-03` § "The symptom is real and still live" names that same test as the epic's **second independent reproduction**. Its sibling bound at `:11062` wraps `wait_for_terminal_run` in a timeout whose message is "abort did not reach a terminal state promptly" — the forbidden convergence-wait shape. The PRD's four-item list was the source and is corrected upstream: it now states no count and requires deriving from the audit table *and* an independent sweep.
- **Class 9 arm A on AC3.** `rg -n 'abort_signal' src/runtime.rs` returns 11 hits at baseline; the discriminating content ("reports the supervisor's own function among the callers") is prose the command does not compute.
- **Class 7 kind (a)** — the caller inventory and "two tests of it" are decaying figures beside a discovery command.
- **Class 3 — `TMUX_CLEANUP_TEST_TIMEOUT` has a third role** at `:7950` inside `AbortBlockingRunner`, which AC2's "used only as" cannot assign.
- **Class 4 — AC7's "waiting on no clock"** is unsatisfiable for three of the four tests while their polling-helper waits survive; `-06` is not sequenced after `-05`, and `-12` delegates a placement this brief never makes.
- Class 6 does not fire.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`

### Round 2 repairs applied 2026-09-02

The elapsed-time set is now derived from two reconciled passes rather than stated as a count, and
`abort_kills_active_panes_before_draining_running_tasks` (`src/runtime.rs:11070`) is explicitly in
scope in the brief body. Its earlier exclusion rested on the audit's `A` classification, which
`ADR-260902-0312-01` § Liveness forbids for this purpose, and lived in `## Triage Notes` where no
implementer reads it. That test is the epic's second reproduction.

AC2 no longer assumes exactly two roles for `TMUX_CLEANUP_TEST_TIMEOUT`; the third use inside
`AbortBlockingRunner` (`:7950`) must be assigned explicitly. AC3's witness is stated by owning
function rather than as a bare `rg` that is green at baseline. Baseline stated. Awaiting round 3.

### Round 3 findings — 2026-09-02

**Readiness gate (cold-reader): FAIL** (round 3)

The derivation instruction held and its prose did not. "All four sit on paths that already carry their
own cancellation arms" and AC7's "the four tests" were the table-only answer — the error the two-pass
reconciliation exists to prevent — and both are replaced by the derivation. The paragraph calling the
set uniform load-bearing C-sites is gone: the bounds run from a half-second to fifteen seconds and at
least one member the audit scores `A` is load-bearing here.

This record now owns **every** `with_delay` call in the test it excludes from `-12`, not only the
calibrated two-second one — `-12`'s AC1 is absolute over the file and it is forbidden from reaching
in, so a delay left standing here would block it. The constant split is stated per role it serves
rather than at two. AC1's "the record states" is pinned to the code.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round3.md`.
Awaiting re-gate.
