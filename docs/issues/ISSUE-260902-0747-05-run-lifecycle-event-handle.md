---
id: ISSUE-260902-0747-05
kind: issue
category: enhancement
status: needs-triage
summary: Every run-lifecycle entry point registers, spawns and only then returns, so a caller cannot subscribe before the first event is emitted and tests can only observe a run's progress by polling persisted state against a wall-clock deadline
adrs: [ADR-260902-0312-01]
terms: [Run, Runtime Event, Logic Tier, Integration Tier]
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

**Known blocking defect, from the 2026-09-05 adversarial review.** The brief's core instruction — await
the corresponding event, then read persisted state once — is unsound for approvals as written.
`queue_approval` mutates the in-memory queue and emits `approval_queued`; `activate_next_approval` emits
`approval_required` after installing the response sender; the executor persists the checkpoint later,
and `emit_event` commits the event journal, not the checkpoint (`src/runtime.rs:3608`, `:3618`, `:3640`,
`:3645`, `:2946`, `:6985`). A consumer can therefore receive both events, read the database once, and
still observe the earlier checkpoint. This is a different race from the subscribe-after-spawn race the
brief already addresses; subscribing earlier fixes acquisition, not the commit ordering.

Before revival this record must name, per removed state predicate, the exact event or snapshot that
establishes it — or introduce a checkpoint-committed notification with a defined order. Note that `done`
and `log_saved` *do* follow their persistence (`src/runtime.rs:6606`, `:6622`, `:6632`); the contract
must not be generalised from them to every event.

Secondary: the brief permits a raw live receiver while promising no missed events. Registration uses a
bounded broadcast channel of 512 (`src/runtime.rs:924`), which can lag if a consumer is descheduled.
Pin lag and closure behavior if revived.

## Agent Brief

**Category:** enhancement
**Summary:** Make a run's events observable through a handle obtained before the run starts, and delete the clock-bound polling helpers that exist because they were not.

**Baseline:** this slice is blocked by `ISSUE-260902-0747-01` and `ISSUE-260902-0747-13`, so every
criterion below is read against the tree **after both have landed** — the tier selector exists and the
completion signal has replaced the terminal-wait helpers. Read against today's tree the pre-change
halves look already-satisfied, because today the default command executes everything.

**Current behavior:**
The runtime already broadcasts the events tests want, and already lets a test await them directly —
but only if the test registered the run id itself and subscribed before spawning its own producer.
No caller of the real entry points can do that.

*The entry points spawn before they return.* Starting a run, resuming a run and restarting a run from
a checkpoint all register the run, spawn the supervised executor, and only then hand the run id back.
A caller therefore cannot subscribe before the first event is emitted, so it cannot observe an
approval being queued or required without racing the executor.

*The live subscription has no replay and no afterlife.* The registry's subscribe path returns a live
broadcast receiver with no replay, and returns nothing at all once the run is cleared.

*So the tests poll.* The runtime's test module carries helpers that poll persisted state on a fixed
wall-clock deadline — for a run reaching a predicate, and for an event of some kind to appear — and
the API test module carries another that polls for a pending approval. This is the mechanism of the
reproduced failure: a deadline sized for an idle machine, closed by contention, with the producer
merely descheduled rather than broken.

Discovery command for the helper definitions:

```
rg -n 'async fn wait_for_(run|event)\b' src/runtime.rs
rg -n 'async fn wait_for_pending_approval' src/api.rs
```

Derive the call-site population from those commands at your baseline rather than from any count
recorded here or upstream.

**Desired behavior:**
A caller can obtain a handle on a run's event stream **before any of that run's events are emitted**,
without polling and without a wall-clock deadline. Every runtime and API test that waited on a clock
to observe a run's progress awaits an event instead.

*Race-free acquisition, at every entry point that spawns.* The constraint binds to starting a run,
resuming a run, and restarting from a checkpoint alike — all three have the same
register-spawn-then-return ordering, and existing tests of all three follow them immediately with the
polling helpers this issue deletes. Covering only the start path would leave the resume and restart
tests polling a fixed deadline, which is the defect verbatim.

The **shape** is delegated to the implementer among the three the PRD sanctions
(`PRD-260902-0301-01` § Implementation Decisions, "The database stays real; the polling goes"): the
entry point hands back the receiver alongside the run id; or it accepts a pre-registered run id, so
the caller subscribes first; or, if a live subscription proves insufficient, the race-free
journal-plus-replay shape that already exists inside the HTTP streaming path. The bound on all three:
after the call returns, a caller holding the handle has missed no event of that run, without having
had to win a race.

*Completion is a separate slice.* The drained-run signal, the clear-before-cancel ordering defect and
the two terminal-wait helpers are `ISSUE-260902-0747-13`, which is sequenced **before** this record
because `wait_for_terminal_run` is implemented on the `wait_for_run` that this record deletes. Do not
touch the completion token here, and do not delete `wait_for_run` until `-13` has landed.

*The seam here is the intermediate states.* The runtime event stream already carries what these
helpers waited on — a queued approval, a required approval, one queued approval then the next — and
that is what this record exposes race-free. Terminal completion is `-13`'s, and it is the larger share
of the polling call sites, not the smaller; an earlier draft claimed the opposite and used that claim
to argue the two halves belonged in one record.

*The database stays real.* `Database` is an owned, embedded implementation detail, not an external
dependency to be faked. What makes these tests fragile is that they poll persisted state rather than
being told it changed. In-memory SQLite is rejected outright, and a dictionary-backed repository fake
is separately considered and deferred (`ADR-260902-0312-01` § "A repository fake is considered and
deferred"). A per-test temporary-directory store is **controlled data** and therefore Logic Tier under
the isolation rule, so removing the polling is what promotes these tests — not removing the
database.

*Deletion, not softening.* The three polling helpers this record owns are removed rather than given
longer deadlines. Raising a deadline is rejected on evidence and no stopgap is taken.

*Tier outcome — the whole rule, not a two-condition shorthand.* A test this record touches moves to
the Logic Tier only if, after this change, it satisfies `ADR-260902-0312-01`'s Logic Tier rule
**entirely**: every port faked or supplied with controlled data, and none of the forbidden forms —
no spawned process, no network, no shared mutable OS state, no sleep to sequence work, no assertion
on elapsed time, and no wall-clock bound ending a polling or convergence wait. Losing a polling
deadline is necessary and **not** sufficient, and an earlier draft of this record said otherwise.

Two families make that concrete:

- *Runtime tests carrying scripted delays* — **not promotable here.** These take their ordering from
  `ScriptedStep::with_delay`, which sleeps for real, and they still sleep to sequence work after this
  record lands. Most of them do **not** lose a wait to this record at all — their terminal wait is
  `ISSUE-260902-0747-13`'s — so do not assume this record touches them; derive which ones actually
  hold one of the three helpers this record deletes, and expect that to be a small minority of the
  family. `ISSUE-260902-0747-12` removes those sleeps and owns
  their promotion; this record leaves them marked. Derive the family with
  `rg -F -n '.with_delay(' src/runtime.rs` and attribute each hit to its owning test rather than
  carrying a count.
- *API tests that write executable fixtures.* **Some** callers of the pending-approval helper this
  record deletes also write an executable `#!/bin/sh` fake and set an executable mode, and some of
  those additionally bind a real `TcpListener` and open a real WebSocket. Those are process, network
  and shared-mutable-OS-state violations that this record does not touch, so those callers stay
  marked. Among them are the ones that write a *poison* fake to prove it is never invoked: writing and
  chmod-ing the script is itself the violation, and whether the run's asynchronous abort teardown
  reaches it cannot be settled by reading the test.

  **One further caller looks clean by every check named here and is not.** Its workflow sets `run_as`
  with no run invocation, so `start_run` resolves the invocation through production code and reaches
  process execution — no fixture write, no executable mode, no socket bind, nothing a read of the test
  body would show. It survives today only because the path it would exec does not exist on the
  machine, which is an accident of the filesystem rather than a property of the test. **Leave it
  marked**, and say in the record that it is marked for indirect process reach rather than for any of
  the three mechanisms below. Derive it by reading each caller's workflow for a `run_as` whose
  invocation is unset; the PRD names this residue as one no grep over a test body finds, so the
  reading is the check.

  **The remaining callers are Logic Tier clean once their polling wait is gone**, and this record
  promotes them. Do not treat "calls the pending-approval helper" as implying "writes a fixture" — an
  earlier draft did, and would have left rule-satisfying tests marked, which `ISSUE-260902-0747-01`'s
  marking criterion forbids. Derive the split per caller:

  ```
  rg -F -n 'wait_for_pending_approval(' src/api.rs
  ```

  then read each owning test function and check it against the full Logic Tier rule — fixture write,
  executable mode, socket bind — rather than against the helper call.

Promotion is therefore decided per test against the full rule, and the record states which tests it
promoted and which it left marked, by name.

*The HTTP target is a separate slice.* Tests that reach the runtime only over HTTP receive a
serialized run id and no in-process handle, so the handle this record hands out does not reach them.
They are Logic Tier all the same — an endpoint test with controlled data and no clock satisfies the
tier's rule — and their clock waits are removed by awaiting the journal-backed run stream instead. That is
`ISSUE-260902-0747-11`, which carries the epic's *regression* acceptance for the reproduced failure;
the closing acceptance over the final gate population is `ISSUE-260902-0747-10`'s. This record must not convert them,
and must not leave its own helpers alive on their behalf.

**Key interfaces:**
- The three run-lifecycle entry points — starting a run, resuming a run, restarting from a checkpoint.
  Their signatures change to satisfy the acquisition bound; the shape is the implementer's choice
  within it.
- The run registry's subscribe path — the live receiver with no replay, which is what makes
  subscribe-before-spawn impossible today.
- The three polling helpers this record deletes, along with their call sites' reliance on a deadline.

**Acceptance criteria:**
- [ ] `rg -n 'async fn wait_for_(run|event)\b' src/runtime.rs` returns no matches; both helper
      definitions are present before this change. The terminal-run and registry-empty helpers are
      `ISSUE-260902-0747-13`'s and are already gone at this record's baseline.
- [ ] `rg -n 'async fn wait_for_pending_approval' src/api.rs` returns no matches; the helper
      definition is present before this change.
- [ ] A test that starts a run, and a test that resumes one, and a test that restarts one from a
      checkpoint, each observe that run's first emitted event without registering the run id
      themselves and without racing the executor. Observable at the run-lifecycle entry points.
- [ ] No runtime or API test awaits a fixed wall-clock deadline to observe a run's **progress** —
      an event emitted, an approval queued or required. Every such wait is an awaited event.
      The terminal-state *helpers* are `ISSUE-260902-0747-13`'s and are already gone at this
      baseline, but two call sites observe a run's terminal `done` event through **this record's**
      `wait_for_event` rather than through those helpers, so they are this record's to convert.
      Derive them rather than taking them on trust: `rg -F -n 'wait_for_event(' src/runtime.rs`,
      attributed to owning function, and read what each call waits for.
- [ ] The callers of the deleted pending-approval helper that satisfy the Logic Tier rule in full
      once their wait is gone are promoted, and the rest are left marked with the reason stated per
      caller — writing an executable fixture, setting an executable mode, binding a socket, or
      reaching process execution indirectly through production code. The last is not visible in the
      test body and is not optional to check. The record names both sets. A record that
      leaves every caller marked has not satisfied this criterion.
- [ ] Every test this record promotes satisfies `ADR-260902-0312-01`'s Logic Tier rule in full at this
      record's completion — not merely "lost its clock wait and spawns no process". A test that still
      sleeps to sequence work, asserts elapsed time, writes or executes a process fixture, binds a
      socket, or ends a wait at a wall-clock deadline is **not** promoted, whatever else changed about
      it. At least one test is promoted and at least one is deliberately left marked, and the record
      names both sets, so the criterion cannot be satisfied by promoting nothing.
- [ ] The promoted tests are executed by the default command after this change and were not before
      it. Read this from what the command reports it executed, not from `cargo test -- --list`, which
      enumerates `#[ignore]`d tests and so cannot witness an exclusion.
- [ ] Existing behavior of the entry points is otherwise unchanged: a run still registers before it
      spawns, and a second concurrent registration of the same run id is still refused.

**Out of scope:**
- The abort path's poll tick and its promptness assertions — `ISSUE-260902-0747-06`.
- The database dependency of the six large decision functions — `ISSUE-260902-0747-07`. This issue
  removes the *polling*, not the database.
- The filesystem-sentinel wait used by tests that spawn fake scripts. That is a convergence wait on a
  real process, not on persisted run state; it stays as an Integration Tier bounded wait unless a
  seam slice removes its test's dependency on the process.
- Converting the HTTP-transport tests, or removing the polling helper that lives in the out-of-crate
  HTTP target. That target holds the epic's reproduced failure and is `ISSUE-260902-0747-11`.
- Faking SQLite or moving to in-memory storage. Reconsidered only after the sleeps are gone.

## Triage Notes

Minted 2026-09-02 from `PRD-260902-0301-01`; breakdown approved by the maintainer the same day.
Blocked by `ISSUE-260902-0747-01` for the tier assignments in its acceptance criteria.

`PRD-260902-0301-01` frames the two production changes here as design gaps the tests merely exposed:
a fire-and-forget spawn with no completion signal is under-specified regardless of how it is tested.
They are not testability hacks and should not be reviewed as such.

**Scale.** Derive it with the brief's discovery commands, attributing each call site to its owning
test; do not carry a number here. Note `rg -c` prints a count per file rather than a total, which is
how an earlier snapshot overstated the figure. The three helpers this record owns are
`wait_for_run`, `wait_for_event` and `wait_for_pending_approval`; the terminal-run and
registry-empty helpers belong to `ISSUE-260902-0747-13`.

The per-site A/B/C classification is in `ISSUE-260901-0216-03` § "Timing-site audit": each of these
helpers is an A-site with a B-site poll tick — arbitrary headroom better replaced by a happens-before
edge, not load-bearing for an assertion.

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

- Failure class 6 prong (b): AC4 is satisfiable by level-triggering alone with **zero** entry-point signature changes, and AC3 needs no completion signal at all.
- The helper set splits cleanly: a completion signal serves `wait_for_terminal_run` (~42 of ~79 measured call sites); an event handle serves the other four helpers. **Split recommended.**
- *"terminal waits … the minority of the polling sites"* is false at this record's scope — 42/79 is a majority.
- Templated `just test-under-load` closing AC is already green at the declared baseline (systematic defect 4).

### Readiness gate round 2 — findings

**Readiness gate (cold-reader): FAIL** (round 2)

- **Class 7 kind (a), build-changing — the governing premise is false.** "Every API test that calls the pending-approval helper also writes an executable `#!/bin/sh` fake and sets an executable mode" holds for 5 of 14 callers. Nine write no fixture, set no mode, bind no socket: their fixture is `create_test_state` plus `approval_only_workflow`. The brief instructs leaving nine Logic-Tier-clean tests marked, which `-01`'s AC1 forbids.
- **Class 7 kind (a) — "the minority of the polling sites" is false.** Terminal call sites are the larger share. Corrected upstream in the PRD, which carried the same error.
- **Class 6 prong (b) fires. Maintainer decision 2026-09-02: split**, at the terminal/intermediate line the helper set already follows. Two production surfaces, two seams, two independently demoable halves; the brief's only cohesion argument was the false minority claim.
- **Class 4 — retention for the level-triggered signal is unbounded.** `RunRegistry::clear` removes the entry outright; a tombstone's lifetime is unstated in brief, PRD and ADR alike.
- **Class 4 — the brief never states its baseline.** Settled upstream: a slice's baseline is the tree after its blockers land, stated per brief.
- Seven further class-7 kind (a) rows (entry-point counts, the fourteen scripted-delay tests, the fixture sub-counts).

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`

### Round 2 repairs applied 2026-09-02

**This record split.** The completion half — the drained-run token, the clear-before-cancel ordering
defect, and the `wait_for_terminal_run` / `wait_for_registry_empty` helpers — is now
`ISSUE-260902-0747-13`, which is sequenced before this record because `wait_for_terminal_run` is
implemented on the `wait_for_run` this record deletes. The maintainer decided the split 2026-09-02 on
the gate's class 6 prong (b): two production surfaces, two seams, two independently demoable halves.

The cohesion argument that had kept them together — that terminal waits are "the minority of the
polling sites" — is false and is removed from this record and from the PRD.

The premise that every pending-approval caller writes an executable fixture is also false, and was
re-derived here before being acted on: of the fourteen callers, five write a fixture, set an
executable mode or bind a socket, and nine do none of those. This record now promotes the clean ones
and instructs the implementer to derive the split per caller rather than inferring it from the helper
call. Leaving all fourteen marked would have violated `-01`'s marking criterion.

Baseline stated. Awaiting round 3.

### Round 3 findings — 2026-09-02

**Readiness gate (cold-reader): FAIL** (round 3)

AC4's completion carve-out was false: two call sites observe a run's terminal `done` event through
*this* record's `wait_for_event`, which `-13` does not touch and hands back here. They are this
record's to convert, by derivation rather than on trust.

A caller that passes every promotion check and still reaches process execution **through production
code** is now named and left marked — its workflow sets `run_as` with no invocation, so `start_run`
resolves through production and execs; it survives only because that path does not exist on the
machine. AC5's marked-side partition admits that fourth reason. The scripted-delay family paragraph no
longer claims those tests lose a wait here; most lose theirs to `-13`.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round3.md`.
Awaiting re-gate.
