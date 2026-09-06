---
id: ISSUE-260902-0747-11
kind: issue
category: bug
status: needs-info
summary: The out-of-crate HTTP target observes runs by polling persisted state against a fixed five-second deadline and by sleeping to sequence work, which is where the epic's reproduced failure lives; replace both with awaits on the journal-backed run stream
prd: PRD-260902-0301-01
adrs: [ADR-260902-0312-01]
terms: [Run, Runtime Event, Logic Tier]
blocked_by: [ISSUE-260902-0747-01]
---

## Agent Brief

**Category:** bug
**Summary:** Make the HTTP target observe run progress by awaiting the run's event stream instead of polling a wall-clock deadline or sleeping, so the reproduced failure leaves the gate for good.

**Baseline:** this slice is blocked by `ISSUE-260902-0747-01`, so every criterion below is read
against the tree **after** the tier selector exists and the named exception holds this target's
rule-failing tests in the gate.

**Current behavior:**
This is where the epic's reproduced failure actually lives. Under CPU contention, tests in the
out-of-crate HTTP target fail at a fixed deadline while the run they are waiting on has not finished,
and the failing set changes between runs with no code change
(`ISSUE-260901-0216-03` § "The symptom is real and still live", and § Mechanism for the panic site).
The producer those tests wait on is a **real process** — a `tmux new-session` plus an echo plus a
teardown — not a descheduled async task, which is why a five-second bound closes so readily under
load. That correction matters for the fix: it is the reason removing the clock wait alone is not
enough.

Three mechanisms produce it. Two are clock waits; the third is that real process:

*A polling helper on a fixed deadline.* The target carries a helper that observes a run by reading
persisted state in a loop, sleeping a short interval between reads, and asserting that a fixed
wall-clock deadline has not passed. Several tests use it. The tests that fail consume a large
fraction of that deadline even on a quiet machine, so the margin is small and any contention closes
it. This is the defect verbatim: a deadline sized for an idle machine, against a producer that is slow
rather than broken.

*Unconditional sleeps used to sequence work.* The run creation-and-approval test sleeps a fixed
interval twice to let the run advance, because nothing in its current shape gives it an edge to wait
on. What it should await is the **run stream** — it already holds the run id and the stream token the
creation response returns, and it currently discards the token. A database handle is *not* the answer
here: polling one would recreate the defect in a new place. The sibling router helper that returns the
handle is useful only for a one-shot state assertion made *after* the corresponding stream event has
arrived, never as the thing awaited. Sleeping to sequence work is forbidden in both tiers.

*The shared fixture spawns a real tmux session per run.* The target builds its application state with
the runtime's production constructor, which installs the production tmux node runner; the constructor
that accepts a double is `#[cfg(test)]` and so is structurally unreachable from an out-of-crate
target. The shared run-creating fixture then starts a workflow whose only node is an `echo` **task**,
and a task node is a runner node kind, so the run spawns a pane through a real `tmux new-session`.
Every test using that fixture therefore fails the Logic Tier's isolation half regardless of what
happens to its clock waits.

**The shared fixture is not the whole isolation problem, and converting it is not the whole job.**
Derive the target's spawn sites rather than assuming the fixture covers them:

- `run_control_routes_return_typed_client_errors` (`tests/http_api.rs:434`) builds its **own inline**
  `echo`-task workflow instead of calling the shared fixture, so a fixture-only conversion leaves it
  spawning tmux. It is one of the three tests the reproduction names, and it is a run-*control* test,
  not a run-stream one — a conversion scoped to "the run-stream tests" misses it.
- `test_node_accepts_v3_task_node` (`tests/http_api.rs:889`) is a **different genus**: `POST
  /api/test-node` spawns unconditionally and no runner port stands in front of it. It builds its
  invocation through `build_tmux_invocation`, which resolves the tmux binary through an interactive
  login shell, and `run_node_preview` reaches `run_tmux_oneshot` directly. No fixture change reaches
  that path, and the test asserts the preview **succeeded**, so execution is its subject. It is
  **Integration Tier** by the rule the PRD already states for endpoint tests that genuinely require
  task execution, it is marked as such by `ISSUE-260902-0747-01`, and it is **not** converted here. The fixture also accepts `Completed`, `Failed` **or** `Aborted` as terminal and never
asserts the echo ran, so a missing or broken tmux satisfies it: these tests pay for real tmux and
validate none of it. That also means the producer their deadline races is a process, not a
descheduled task, which is why a five-second bound closes so readily under load.

Discovery commands:

```
rg -n 'Duration::from_secs\(5\)' tests/http_api.rs
rg -n 'tokio::time::sleep' tests/http_api.rs
rg -n 'RuntimeContext::new|"agent": "echo"|kind.*task' tests/http_api.rs
```

**Desired behavior:**
No **Logic Tier** test in the HTTP target waits on the clock to observe a run, **and none of them
spawns a process.** The qualifier is load-bearing and is not a hedge: membership is per test, and
`test_node_accepts_v3_task_node` stays in this file as an Integration Tier test that spawns by design.
A criterion or a claim phrased over "the target" rather than over its Logic Tier members is wrong;
`PRD-260902-0301-01` § Implementation Decisions says so in terms.

Each Logic Tier member awaits the run's event stream — a happens-before edge that cannot be starved
into a false failure. The polling helper and the unconditional sleeps are both gone, and no run
started by this target reaches the node runner at all.

*The fixture stops executing tasks.* These tests need a run in a terminal state and its stream token;
they do not need a task to execute. An approval-only workflow — a single `approval` node with no
edges — is not a runner node kind, so it reaches a pending approval, terminalizes on the approve
endpoint, and never touches the runner. The target already drives exactly that workflow over HTTP in
`creates_and_approves_runs` (`tests/http_api.rs:1184`), so this is adopting a shape the file already
carries, not inventing one. **No new production surface is required, and none is authorized here:**
if a test is found that genuinely needs task execution over HTTP, it is Integration Tier, and an
injectable runner port reachable across the crate boundary is a separate decision for the maintainer,
not a judgment call for this issue.

*The stream is reachable after the fact, so there is no race to lose.* The run stream endpoint
replays from the event journal rather than serving live events only: it reads the run's persisted
events through a sequence filter before switching to live delivery. A test may therefore create a
run over HTTP, open the stream **afterwards**, and still receive every event of that run including
the ones already emitted. This is what makes the conversion possible without any new production
surface, and it is why an out-of-crate caller holding only a serialized run id is not disadvantaged
here (`PRD-260902-0301-01` § Implementation Decisions, "HTTP endpoint tests are logic-tier tests").

*Awaiting the stream, not the store.* Prefer awaiting the event that marks the state the test cares
about over reading persisted state at all. Where a test genuinely asserts on persisted state, it
awaits the corresponding event first and then reads once, rather than polling until the read
succeeds.

**This is safe only where the event actually follows the persistence it stands for, and that is
per-event, not general.** `done` and `log_saved` are emitted after their terminal checkpoint and log
writes respectively (`src/runtime.rs:6606`, `:6622`, `:6632`), so awaiting them establishes what a
test then reads. Approval events do not carry that guarantee: `queue_approval` emits
`approval_queued` from the in-memory queue and `activate_next_approval` emits `approval_required`
after installing the response sender, while the executor persists the checkpoint later, and
`emit_event` commits the event journal rather than the checkpoint (`src/runtime.rs:3608`, `:3618`,
`:3640`, `:3645`, `:6985`). A test that awaits an approval event and then reads once can still see
the earlier checkpoint.

So: for each converted assertion, name the event you await and confirm it is emitted after the state
you read. Where no such event exists, keep the persisted-state assertion and await a terminal event
that does carry the ordering, rather than weakening the assertion to "two notifications arrived".
The general form of this problem belongs to `ISSUE-260902-0747-05`, which is deferred; handle it
per-assertion here.

`creates_and_approves_runs` replaces both of its sleeps with awaits on the run stream. It does **not**
move to the router helper that returns the database handle: it asserts on endpoint responses
(`/api/interrupted-runs`, `/api/logs`), not on persisted state, so it needs no handle, and taking one
would reintroduce the polling shape this record deletes.

*Deletion, not softening.* The polling helper is removed rather than given a longer deadline.
Raising a deadline is rejected on evidence and no stopgap is taken. A test that still needs a failure
bound after conversion may keep one only in the permitted shape: it wraps an await that a
happens-before edge ends on the passing path, it fires only on a hang, and it carries a comment naming
the hang it guards (`PRD-260902-0301-01` § Testing Decisions, the paragraph beginning "The line
between a forbidden bound and a permitted one"). What it must **not** be is another convergence wait —
a read repeated until it succeeds, ended by a deadline. That is the shape being deleted here, and
re-introducing it under a longer bound does not satisfy this record.

*Tier.* The HTTP endpoint tests are **Logic Tier** tests: the unit is a use case reached
at its API boundary, the per-test temporary-directory store is controlled data, and after this change
no clock **ends a wait** — which is the tier's criterion. Clocks are still read on this path:
`start_run` mints a time-ordered run id (`src/runtime.rs:1408`) and checkpoint persistence stamps
`now_iso()` (`src/runtime.rs:7014`). Both are permitted — a stamp written as data gates no control
flow, and the run id is not load-bearing identity under `ADR-260902-0312-01`. The claim this record
makes is about waits, not about clock reads. They are in the gate, and they were never taken out of it: `ISSUE-260902-0747-01`
leaves **the reproduction's tests** unmarked as its one named exception, because marking them would
have carried the reproduced failure out of the gate. The exception is over tests, not over the target
— `-01` is explicit about that, and it marks `test_node_accepts_v3_task_node` in this same file by the
ordinary rule. **This record closes that exception.** Until it lands the target
satisfies the tier rule in spirit but not on the letter, and the exception is what holds it in the
gate; once the clock waits are gone the target satisfies the letter too and the exception has nothing
left to cover. Removing the exception entry is part of this change, not a follow-up.

*The out-of-crate constraint holds.* This target compiles the library without `cfg(test)`, so it must
not depend on the build-profile timeout shims; their production budgets would apply.

**Key interfaces:**
- The run stream endpoint, as the seam the tests await. No production change: the journal replay it
  already performs is what makes post-hoc subscription safe.
- The run stream as the awaited edge for the creation-and-approval test, reached with the run id and
  the stream token its creation response already returns. The sibling router helper that yields the
  database handle is available for a one-shot assertion after the awaited event, not as the wait
  itself. No helper signature changes.
- The polling helper and its call sites, which are removed.

**Acceptance criteria:**
- [ ] `rg -n 'Duration::from_secs\(5\)' tests/http_api.rs` returns no matches; the polling helper's
      deadline is present before this change.
- [ ] `rg -n 'tokio::time::sleep' tests/http_api.rs` returns no matches; unconditional sleeps and the
      helper's poll interval are present before this change.
- [ ] No test in the HTTP target observes a run's progress by reading persisted state in a loop.
      Every such observation is an awaited event.
- [ ] A test creates a run over HTTP, opens the run stream **after** the run has already reached a
      terminal state, and still receives the run's earlier events. This is the property that makes
      the conversion race-free, and it is asserted rather than assumed. Observable at the run stream
      endpoint.
- [ ] **No run started by this target reaches the node runner.** Every test that starts a run drives it
      to a terminal state through a node kind the runtime does not dispatch to a runner, so no pane is
      created.

      Witness this **structurally, from the fixture**, not by breaking tmux resolution. Assert that
      every workflow the target builds contains no runner-kind node — `is_runner_node_kind`
      (`src/runtime.rs`) is the predicate, and the assertion runs over the fixtures the target
      actually constructs. A fixture that grows a runner-kind node fails the assertion, which is the
      red this criterion needs.

      **Do not witness it by pointing tmux resolution at a bad path.** The two paths in this target
      do not share a resolution seam: a run built by `RuntimeContext::new` carries
      `run_invocation: None`, no workflow here sets `runAs`, so the invocation is the hardcoded
      literal in `run_scoped_tmux_invocation` and `resolve_tmux_bin` is never reached; the preview
      endpoint calls `build_tmux_invocation` directly. One control therefore cannot redden both, and
      `resolve_tmux_bin` swallows a resolution failure back to the literal `"tmux"` anyway, so the
      signal would not name the configured path. Adding a knob that joins the two paths is **not
      authorized here** — this record adds no new production surface.

      **Order the two halves.** Replacing the tolerant terminal helper comes *first*: the shared
      fixture today accepts `Completed`, `Failed` **or** `Aborted`, so a run that dies for any reason
      reads as terminal and the structural assertion is the only thing that can distinguish a
      converted fixture from a broken one.
- [ ] The HTTP endpoint tests **that this record converts** are executed by the default command and
      carry no Integration Tier marker, **because they satisfy the tier rule on both halves** — no
      clock wait and no spawned process. `test_node_accepts_v3_task_node` is excluded by name: it
      keeps its marker and is not executed by the default command, and a criterion phrased over the
      whole target could only go green by unmarking it, which both this record and
      `ISSUE-260902-0747-01` forbid. Read execution from what the command reports it executed, not from
      `cargo test -- --list`. Note this criterion is not witnessed by the target being unmarked and
      default-executed on its own: `ISSUE-260902-0747-01` already holds it that way under a named
      exception, so the observable is the exception's removal in the criterion below, plus the
      tmux-absent control above.
- [ ] The named exception `ISSUE-260902-0747-01` recorded for this target is **gone** — the target is
      unmarked because it satisfies the tier rule outright, not because an exception holds it in the
      gate. The entry is present before this change and absent after it, and the suite carries no
      exception entry of any kind afterwards. "Outright" means the **whole** tier rule, both halves:
      isolation — no spawned process, which is what the fixture change buys — and the clock rule as
      § Testing Decisions states it, no sleep to sequence work, no assertion on elapsed time, and no
      wall-clock bound ending a polling or convergence wait. A residual hang-only guard of the
      permitted shape does not block this criterion; a surviving convergence wait does, whatever its
      bound, and so does a single surviving run that reaches the node runner.
- [ ] **`just test-under-load` passes, unchanged, at the repetition count the recipe already
      carries.** This is the **regression** acceptance for the reproduced failure
      (`PRD-260902-0301-01` § Testing Decisions), and it lands here because this record is what removes
      that failure. It is **not** the epic's closing acceptance: this record is blocked only by
      `ISSUE-260902-0747-01`, so later slices go on promoting tests into the gate after it passes.
      `ISSUE-260902-0747-10` is the join node and carries the closing run. Read its baseline polarity
      honestly: the failure is intermittent, so three runs failing is evidence and three runs passing
      is not proof of a fix — the recipe is a **resilience check**, not a deterministic red/green
      witness, and an implementer who sees it pass at baseline has learned nothing and should not
      conclude the work is unnecessary.
- [ ] The deterministic half of the acceptance: the target's structural observables above — no
      five-second deadline, no `tokio::time::sleep`, no Logic Tier run reaching the node runner, and
      the post-hoc stream subscription asserted. These hold or fail identically on a quiet machine and
      a loaded one, and they are what makes the under-load run meaningful rather than lucky. Not all
      four are red at baseline in the same way — check each against the tree you start from rather
      than assuming a uniform starting polarity, and state which were already true.
- [ ] Every endpoint assertion the target makes today is still made: this issue changes how the
      tests wait, never what they assert.

**Out of scope:**
- Any new production surface to inject a runner across the crate boundary. If a test is found that
  genuinely needs task execution over HTTP, it stays Integration Tier and the seam is referred to the
  maintainer as a separate decision.
- The in-crate run-lifecycle handle and the runtime's own polling helpers —
  `ISSUE-260902-0747-05`. That record serves in-crate callers, which can hold an in-process handle;
  this one serves a caller that has only a serialized run id and an HTTP surface.
- The latent retry-delay default — `ISSUE-260902-0747-09`.
- Any change to the run stream endpoint's production behavior. The journal replay is used as it
  stands; if it turns out to be insufficient, that is a finding to report, not a licence to change
  the endpoint here.
- The testing document's stale claims about this target's transport and its test-case inventory.
  That is documentation drift owned by the docs-truth family.
- Adding new endpoint tests. The set converts; it does not grow.

## Triage Notes

Minted 2026-09-02 from `PRD-260902-0301-01`, splitting `ISSUE-260902-0747-05` and absorbing the HTTP
half of `ISSUE-260902-0747-09`. Both splits were put to the maintainer and approved the same day.

**Why this record exists at all.** Until 2026-09-02 the PRD placed HTTP-only tests in the integration
tier permanently, so no slice owned the reproduced failure — drawing the tier boundary would simply
have moved it out of the gate. Amending the PRD to make HTTP endpoint tests logic-tier tests put the
failure back inside the gate and left it unowned. This record owns it, and with it the
epic's acceptance.

**Why the two halves are one slice.** The polling helper and the unconditional sleeps live in the
same target, share the same seam (the run stream), and are the same defect — the target waits on a clock because it was given nothing to await.
Splitting them would put two records on the same file with the same fix.

**Scale snapshot (non-contractual):** `rg -c 'wait_for_run|tokio::time::sleep' tests/http_api.rs` →
~7 call sites (2026-09-02).

**Readiness gate:** not yet run on this record. It was minted after the round-1 batch.

### Readiness gate round 1 — findings

**Readiness gate (cold-reader): FAIL** (round 1)

Round 1 — this record was minted after the round-1 batch and had never been gated.

- **Class 3/4, build-changing — a fourth process-spawn site the brief does not name.** `POST /api/test-node` spawns unconditionally and no port stands in front of it: `test_node` (`src/api.rs:659`) calls `build_tmux_invocation` at `:680`, which resolves the tmux binary through a real interactive login shell (`Command::new("zsh").args(["-lic", …])`, `src/tmux_exec.rs:121`), and `run_node_preview` (`src/runtime.rs:1749`) reaches `run_tmux_oneshot` directly. `test_node_accepts_v3_task_node` asserts the preview **succeeded**, so execution is the subject. The approval-only fixture conversion cannot reach it, and the run path is shell-free by contrast, so this is a separate genus. **Maintainer decision 2026-09-02: that test is Integration Tier**, by the rule the PRD already stated for endpoint tests requiring task execution.
- **Class 3 — the shared fixture is not the whole population.** `create_terminal_echo_run` is called by two tests while the reproduction names three; `run_control_routes_return_typed_client_errors` carries its own inline echo workflow.
- **Class 9 arm B req. 1 on AC5 — not waivable.** The positive control is an ambient `PATH` manipulation, not a control produced mechanically from the tree differing in exactly one dimension, and not an assertion the fixture executes at run time. It is also blind in the way the requirement exists to catch: green against the fixture path, red only because of the unnamed site above — and it stays red after the work as scoped.
- **Class 7 kind (a) — AC9's deterministic pairing is one-quarter false.** Three of its four terms are red at baseline; "no run reaching the node runner" is not, since its only stated observable is AC5's control.
- **Class 5 — two mutually exclusive seams for `creates_and_approves_runs`**: § Current behavior forbids awaiting the database handle, § Desired behavior moves the test onto the router helper that returns it.
- **Class 3 — AC7's zero-hit vocabulary is unpinned**, and the terminal state AC5's replacement helper must assert is never named.
- Race-freeness is stated correctly and cites the journal replay, not the recovery path. Class 6 does not fire.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`

### Round 2 repairs applied 2026-09-02

The fourth spawn site is named in the brief body: `POST /api/test-node` spawns unconditionally with no
runner port on the path, so no fixture change reaches it. It is Integration Tier, marked by `-01`, and
not converted here — which also means AC5's control is scoped to this target's Logic Tier members
rather than to the whole target, where it could never go green. The inline task workflow in
`run_control_routes_return_typed_client_errors` is named as a second conversion site the shared
fixture does not cover.

AC5's positive control is now a committed, mechanically-produced variant rather than an ambient `PATH`
manipulation. The four-term deterministic pairing no longer claims a uniform red baseline. The
`test-under-load` criterion is stated as regression acceptance, with closing acceptance at
`ISSUE-260902-0747-10`. The "merely descheduled" mechanism claim is corrected to the real process it
is, matching the record's own later paragraph. Baseline stated. Awaiting round 2.

### Round 2 findings — 2026-09-02

**Readiness gate (cold-reader): FAIL** (round 2)

The whole-target phrasing is gone from all three places it survived — Desired behavior, the Tier
paragraph, and AC6 — and each is restated over the target's Logic Tier members. `-01` marks
`test_node_accepts_v3_task_node` in this same file by the ordinary rule; its exception is over the
reproduction's tests, not over the target, and AC6 as previously phrased could only go green by
unmarking a test both records forbid unmarking.

AC5's control is rebuilt. Its claim that the Logic Tier members fail before the change was false —
the shared fixture accepts `Failed` and `Aborted` as terminal, so a broken tmux path already satisfies
it, a fact this brief states two paragraphs earlier. The tolerant helper's replacement is now ordered
*first*, and the control carries its own acted-on witness in the same run.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round3.md`.
Awaiting re-gate.
