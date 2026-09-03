> **External adversarial review — OpenAI Codex (`gpt-5.6-sol`, reasoning effort xhigh), 2026-09-02.**
> Read-only against the repo, driven through `/codex-researcher`; session `01a06443-71fe-7df2-b820-7d4cba25b8f6`.
> Prompt: `PRD-260902-0301-01-adversary-260902-codex-breakdown-prompt.md` in this directory.
> Commissioned as a second perspective after five `cold-reader` rounds on `PRD-260902-0301-01`, over
> two open sequencing decisions and anything those rounds missed.
>
> **Disposition:** every finding was independently verified against the source before being acted on,
> and all were applied — see the session's handoff for the per-finding record. Two corrections to the
> report itself were found during that verification: it identified three API tests as definite
> non-promotions where **all five** it named write an executable `#!/bin/sh` fixture and chmod it; and
> its proposed A/C-classification split for the guard rule does not work, because `wait_for_run` is an
> A-site and is the reproduced failure. Its Critical finding — that the HTTP target spawns real tmux
> through the production runner, invalidating the "HTTP endpoint tests are logic tier" premise five
> gate rounds had accepted — reproduced exactly and was the most consequential result of the review.
> One finding was **not** acted on by decision: the tier of the pure performance assertion, which is
> recorded as `[OPEN: perf-test-tier]` in the PRD's `## Open Questions` rather than resolved.

# Decision 1

## 1. AC6 really does require the bad promotion

This is not an uncharitable reading. It is the literal result of combining the issue's tier outcome and AC6:

- `ISSUE-260902-0747-05-run-lifecycle-event-handle.md:91-92` says every runtime/API test that loses its clock wait and spawns no process moves to Logic Tier.
- Its AC6 at `:124-127` requires those tests to be newly executed by the default command in this slice.
- `ScriptedRunner` actually sleeps when `delay_ms > 0` (`src/runtime.rs:7587-7588`), so removing `wait_for_terminal_run` does not make such a test clock-free.
- The ADR forbids sleeping to sequence work in **both** tiers (`docs/adr/260902-0312-deterministic-test-tiers.md:69-71`), and the Logic Tier definition independently excludes it (`:25-32`).

I reproduced the population and aggregate delay from the source:

```text
$ rg -o '\.with_delay\([0-9]+\)' src/runtime.rs | wc -l
32
$ rg -o '\.with_delay\([0-9]+\)' src/runtime.rs \
    | sed -E 's/.*\(([0-9]+)\)/\1/' \
    | awk '{s+=$1} END {print s}'
3685
```

Walking those hits to their enclosing test functions gives 14 tests. Each of those functions also calls `wait_for_terminal_run`; one, `parallel_batch_abort_cancels_pending_items_after_next_completion`, additionally asserts elapsed time. Thus AC6 would require thirteen still-sleeping tests, plus one still-sleeping and elapsed-asserting test, to become unmarked/default immediately after `-05`. That installs exactly the defect the epic is meant to remove.

## 2. Recommended cut

The proposed direction is right, with one strengthening: make `-05` apply the **entire tier predicate**, not merely copy a scripted-sleep exception.

1. Replace `-05` AC6 with: a test is promoted only if, after the slice, it satisfies the ADR's complete Logic Tier rule. Explicitly say that removing a polling deadline is insufficient while the test still has a scripted sleep, an elapsed assertion, a process, network access, or shared mutable OS state.
2. Add `ISSUE-260902-0747-05` to `-12`'s `blocked_by` list.
3. Let `-12` remove the scripted sleeps and promote its now-eligible tests. Continue excluding the calibrated parallel-batch abort test from `-12`'s promotion criterion; Decision 2 assigns that whole test to `-06`.

This ordering is better than `-05 blocked_by -12`. In the alternative order, `-12` AC4 (`ISSUE-260902-0747-12-scripted-delays-encode-orderings.md:106-109`) would itself be wrong: it requires the converted tests to enter the default command while they still terminate progress waits through `wait_for_terminal_run`. The alternative can be made correct only by weakening `-12`'s promotion AC and deferring promotion to `-05`. That is possible, but it makes the delay-conversion slice work against the old polling seam and gives no compensating benefit.

The clean join is therefore:

```text
-01 -> -05 --+
              +-> -12
-01 -> -06 --+
```

`-05` and `-06` do not need an edge between each other if both use the full-rule promotion guard. Whichever lands first leaves the calibrated test marked; whichever removes its final forbidden construct may promote it. `-12` should wait for both so its absolute `delay_ms` criterion and exclusions describe a settled tree.

## 3. API callers

There are 14 calls to `wait_for_pending_approval` outside its definition, not an additional hidden scripted-delay family:

```text
$ rg -n 'wait_for_pending_approval\(' src/api.rs
3243, 3283, 3307, 3340, 3373, 3406, 3438,
3625, 3696, 3725, 3980, 4032, 4952, 5039
```

Nine callers are unambiguously in-process approval/API cases after the wait is replaced:

- `resume_run_rejects_an_already_active_run_with_a_distinct_conflict`
- the six event/stream token and origin tests at `src/api.rs:3275-3450`
- `interrupted_runs_excludes_registered_executors`
- `pane_context_errors_use_sanitized_client_messages`

I found no residual process, socket, sequencing sleep, or elapsed assertion in those nine paths. They are legitimate `-05` promotions.

Three are unambiguously **not** Logic Tier merely because the approval wait disappears:

- `run_observability_bounds_tmux_session_lookup_concurrency` creates and invokes an executable fake tmux whose script itself sleeps (`src/api.rs:3654-3714`).
- `pane_death_before_live_bytes_sends_pane_stream_unavailable_frame` creates an executable fake tmux, binds a real `TcpListener`, and connects a real WebSocket (`src/api.rs:4928-5007`).
- `pane_writer_that_never_starts_sends_pane_stream_unavailable_frame` has the same process/network shape (`src/api.rs:5011-5059` and following).

Two more require explicit treatment rather than promotion by slogan:

- `run_observability_prefers_single_registered_session_over_tmux_lookup` deliberately creates a poison fake and proves it was not used for the lookup, then registers a session and aborts (`src/api.rs:3601-3649`).
- `pane_context_uses_persisted_invocation_without_running_resolver` likewise proves its resolver was not run, registers an active pane, and aborts (`src/api.rs:4002-4055`).

The assertions in those two are negative process controls, but abort cleanup is performed asynchronously by the spawned run supervisor and can reach pane/session cleanup. I could not prove from static source that the test runtime always tears the task down before that cleanup attempts a process, nor that it always lets the cleanup run. I therefore would not classify either through AC6's phrase alone. Give them a named review in `-05` (or remove the process-shaped teardown) and apply the full tier rule.

So my answer is: I found no additional **verified** wrong promotion beyond the 14 runtime tests, provided “spawn no process” is evaluated correctly. I did find three definite API non-promotions and two ambiguous ones that AC6 does not specify robustly. That is another reason to replace AC6's two-condition shorthand with the full ADR predicate and an explicit caller accounting table.

# Decision 2

## 1. The calibration is real, and removing the delay first destroys the witness

The source does calibrate the two values against each other:

- Item `a` is delayed 2,000 ms; `b`, `c`, and `d` are delayed 300 ms (`src/runtime.rs:11429-11445`).
- Batch concurrency is one (`:11448-11453`).
- The test waits for `cursor_spawned`, starts the abort timer, aborts, and waits for terminal state (`:11463-11468`).
- It requires abort in under 500 ms and all four items cancelled with none succeeded (`:11470-11489`).

The ordering detail is more damaging than the issue prose makes explicit. `spawn_batch_item_task` emits `cursor_spawned` at `src/runtime.rs:5465-5477`, but does not put the item future into the `JoinSet` until `:5526`. The test can therefore wake on `cursor_spawned` while the first item is not yet spawned, or after it has begun. The 2,000 ms delay makes both schedules safe: item `a` cannot complete before the abort.

If `-12` removes the delay first:

- Item `a` can complete before the test task gets scheduled to call `abort_run`. The batch loop can then consume the completion and start later items. The fixed expectations `succeeded == 0` and `cancelled == 4` become nondeterministic.
- More subtly, the `< 500 ms` assertion stops testing prompt cancellation. A broken implementation that waits for the active item to complete can now finish quickly because the item is instantaneous. The assertion remains green while its regression-detection power is gone.

The existing production path confirms what the test is meant to witness: the parallel batch obtains the abort token at `src/runtime.rs:5163-5166` and selects it against `running.join_next()` at `:5192-5200`. The delay is currently the only guarantee that cancellation wins before a successful join.

## 2. Give the whole test to `-06`

I agree with the proposal. `-06` owns the semantic property—abort must win while an item is definitely in flight—and already owns the elapsed assertion that inadequately represents it. Splitting the fixture ordering from its assertion makes either half temporarily meaningless.

The implementation should use a dedicated blocking runner/barrier for this test, in the style of `AbortBlockingRunner`, rather than merely teaching every scripted step another special case:

1. The runner signals that item `a` has entered the port and then waits on a test-controlled release that is never needed on the passing abort path.
2. The test waits for that entered signal, delivers abort, and observes terminal cancellation through a happens-before signal/lifecycle handle.
3. It asserts the same batch summary: zero successes and four cancellations.
4. A generous hang-only wrapper is acceptable only if needed to keep a broken implementation from hanging the suite; it is not the passing-path termination condition.

This directly proves “abort cancels an active item without waiting for that item to complete,” without an elapsed threshold. It also keeps `-06`'s accounting of all four contractual C-sites truthful.

Revise the records as follows:

- In `-06`, remove the current out-of-scope statement that scripted-delay fixtures are untouched (`ISSUE-260902-0747-06-abort-observable-structurally.md:124-131`) and name this one calibrated test as the exception wholly owned by `-06`.
- In `-12`, exclude this test **wholesale**, rather than removing only its delay (`ISSUE-260902-0747-12-scripted-delays-encode-orderings.md:75-82,106-109,116-118`).
- Add `ISSUE-260902-0747-06` to `-12`'s blockers. Together with Decision 1, `-12` should be blocked by `-01`, `-05`, and `-06`.

Giving both halves to `-12` is worse: it makes a scripted-runner cleanup slice own one of the four abort-path C-sites that `-06` promises to account for by name. A third dedicated issue would be coherent, but this one test is too small to justify another coordination boundary; it would still have to block both `-06`'s complete C-site accounting and `-12`'s absolute `delay_ms` deletion.

## 3. `-06` AC3 is separate

AC3 does not subsume this test and this test cannot satisfy AC3.

- AC3 requires the **run supervisor's** own approval-pending, running-task, and delegated approval waits to select on the abort token (`ISSUE-260902-0747-06-abort-observable-structurally.md:100-110`). It is deliberately structural because it adds a delivery path.
- The parallel-batch test exercises the already-existing select in the batch join (`src/runtime.rs:5166,5192-5200`), below a different control-flow boundary.

They are related only in subject matter. Keep AC3 as its own production-structure criterion, and count the converted parallel-batch test as one of `-06`'s four per-path structural replacements under AC1/AC4—not as the behavioral witness for AC3.

# Part 3 — findings ranked by severity

## Critical — `-11` does not make the whole HTTP target Logic Tier; three tests still spawn real tmux

The central source claim in the PRD and `-11` is false as written. The PRD says HTTP endpoint tests are Logic Tier once their clock waits are removed (`PRD-260902-0301-01-deterministic-test-suite.md:224-234`), and `-11` says the entire target then satisfies the rule outright (`ISSUE-260902-0747-11-http-target-awaits-the-run-stream.md:75-82,105-113`). At least three tests still cross the process/tmux boundary:

1. `test_router_with_db` installs `RuntimeContext::new` (`tests/http_api.rs:95-121`).
2. `RuntimeContext::new` installs the production `TmuxNodeRunner` (`src/runtime.rs:1380-1388`); the out-of-crate target cannot use the crate's `#[cfg(test)]` `with_runner` constructor at `:1391-1395`.
3. `create_terminal_echo_run` starts a task workflow whose agent is `echo` (`tests/http_api.rs:162-216`). It is called by `streaming_routes_allow_same_origin_sse_and_require_ws_origin` (`:271-274`) and `run_stream_requires_matching_stream_token` (`:360-363`). `run_control_routes_return_typed_client_errors` builds and starts the same task itself (`:563-608`).
4. The `echo` path is not in-process. `TmuxNodeRunner::run` calls `run_agent_interactive` (`src/tmux_exec.rs:162-187`), which dispatches `echo` to `run_agent_sequence` (`:1021-1045`). That constructs an echo command and acquires a `PaneGuard` (`:1346-1386`); the guard calls `spawn_pane` (`:416-451`), which executes `tmux new-session` through `tmux::run_checked_owned` (`:2177-2229`).

This violates the Logic Tier's “every port faked/controlled; no spawned process or shared OS state” rule (`docs/adr/260902-0312-deterministic-test-tiers.md:25-32`). Removing a database poll changes none of that.

The fixture also masks the problem: `create_terminal_echo_run` accepts `Completed`, `Failed`, **or** `Aborted` as terminal and never asserts successful echo execution (`tests/http_api.rs:208-215`). A missing/broken tmux can satisfy the helper by failing the run, so these API authorization tests pay for real tmux without validating its success.

**Fix:** replace these fixtures with an approval-only or otherwise in-process run that can be terminalized through HTTP, if the tests merely need a terminal run and token. If a task execution is essential, expose an injectable runner port through a public/test-support application-construction seam usable by out-of-crate tests. Until then, mark the three tests Integration and change `-11` AC5/AC6 from “the HTTP target” to a per-test accounting. The single target-wide exception in `-01` currently hides a process-boundary violation as well as the intended clock violation.

## Critical — `-01` AC1 and AC4 cannot both pass for the mandated storage tests

`-01` AC1 defines marker membership as an exact closed set: process, FIFO, tmux, process group, sequencing sleep, elapsed assertion, or polling/convergence deadline—and says a test doing none of those must **not** carry the marker (`ISSUE-260902-0747-01-draw-tier-boundary.md:142-153`). AC4 simultaneously requires storage permission and migration tests to be excluded as Integration (`:170-175`).

Real examples do not match any AC1 mechanism:

- `database_and_wal_sidecars_are_not_group_or_world_accessible` uses a temporary real SQLite database and reads actual filesystem modes (`src/storage.rs:1810-1841`). It has no process, FIFO, tmux, process group, sleep, elapsed assertion, or polling deadline.
- `init_backfills_and_retires_legacy_checkpoint_tmux_sessions` exercises migration-on-`init` against a real SQLite file (`src/storage.rs:2053-2104`) with none of those constructs.
- `init_persists_structurally_migrated_v3_workflow_as_v4` has the same shape (`src/storage.rs:2144` and following).

The issue requires each to be marked by subject, then prohibits marking it by its mechanical equivalence criterion. This is an unsatisfiable acceptance set.

**Fix:** AC1 must test the ADR's semantic tier rule, not claim its timing/process grep is extensionally complete. Enumerate the real-infrastructure behavior families required by AC4 (filesystem permission semantics, migration-on-init, real pool concurrency, etc.) as marker-positive cases, and phrase “no false-positive marker” against the full rule. Keep the grep only as one audit input.

## High — the interim boundary uses Integration as a quarantine for tests that belong to neither defined tier

The ADR says sequencing sleeps are forbidden in **both** tiers (`docs/adr/260902-0312-deterministic-test-tiers.md:69-71`). Integration is defined by a genuine real-infrastructure subject, not by “currently nondeterministic” (`:46-53`). Yet `-01` explicitly marks the tree as currently written (`ISSUE-260902-0747-01-draw-tier-boundary.md:68-73`), and `-10` confirms that the scripted runner tests are temporarily marked Integration because they sleep and poll (`ISSUE-260902-0747-10-enforce-tier-rule.md:27-40`). Those tests use an in-process node-runner fake and a per-test controlled database; their subject is workflow logic, not real infrastructure.

Under the actual ADR they are not Integration, but they are ineligible for Logic. The two-tier model has no legal state for them. Calling the Integration marker a temporary quarantine also misrepresents the non-gating job's meaning.

**Fix:** choose one of these coherent models:

- sequence `-05`/`-12` before enforcing the boundary for this family, so no illegal interim classification is committed;
- introduce an explicitly temporary quarantine marker distinct from Integration; or
- amend the ADR to define Integration as all non-gating temporal tests as well as real-infrastructure tests.

The first is preferable. Broadening Integration weakens the useful claim that tier membership describes what a test isolates.

## High — epic acceptance is attached before the final gate membership exists

`-11` has only `-01` as a blocker (`ISSUE-260902-0747-11-http-target-awaits-the-run-stream.md:10`) and carries `just test-under-load` as the epic acceptance (`:114-117`). It can therefore pass before `-04` through `-09` and `-12` modify and promote many tests. Later slices can put newly converted tests into the default gate without the epic acceptance ever running against that final population.

`-10` is the actual join node: it is blocked by every tier-moving slice (`ISSUE-260902-0747-10-enforce-tier-rule.md:10`). But none of its ACs reruns `just test-under-load`; they only validate the enforcement/advisory mechanism (`:72-114`). Consequently no issue owns “the final default test population passes under load.”

`-11` AC5 is also already green at its declared post-`-01` baseline: `-01` explicitly requires the HTTP target to remain unmarked and executed by default (`ISSUE-260902-0747-01-draw-tier-boundary.md:154-161`). AC5 at `-11:105-106` witnesses no `-11` work. AC6 (exception removal) is the meaningful tier criterion.

**Fix:** keep `-11`'s focused HTTP regression checks, but move the epic-level load acceptance to a final join—either `-10` with blockers covering every Rust-changing slice, including `-02`, or a small final acceptance issue blocked by all relevant slices. That final issue should run the default tier under load and the Integration Tier once, so a promotion cannot silently escape the acceptance run.

## High — the promised protection against newly unmarked process/network tests is unowned

The PRD explicitly accepts an unmarked-default hazard and names mechanical enforcement as its mitigation: a new process-spawning test otherwise lands silently in the gate (`PRD-260902-0301-01-deterministic-test-suite.md:172-181`). `-01` repeats that `-10` is what keeps the default honest (`ISSUE-260902-0747-01-draw-tier-boundary.md:61-66`).

But `-10` checks only the three clock forms (`ISSUE-260902-0747-10-enforce-tier-rule.md:43-51`) and expressly excludes process spawning, network, and shared OS state (`:116-122`). It may also decline all gating enforcement and ship advice (`:53-59`). No other slice supplies the mitigation the PRD claims for a future unmarked process/network test.

**Fix:** either give `-10` cheap structural positive controls for process/network/shared-state violations where recognizable (with advisory review for shapes that are not), or stop claiming that the enforcement check mitigates that hazard and choose a marker scheme that fails closed. Consolidating fake-process fixtures in `-02` improves readability but does not prevent a new unmarked test.

## High — `-07` AC3 specifies an impossible event ordering

The production contract is:

```rust
let seq = ctx.db.append_event(run_id, &event).await?;
event.seq = Some(seq);
ctx.registry.send_event(run_id, event).await;
```

That is `src/runtime.rs:6985-6989`: successful durable append, sequence assignment, broadcast. On append failure, `?` propagates immediately; sequence assignment and broadcast do not occur.

`-07` instead requires “durable append, then failure propagation, then sequence assignment, then broadcast” for all six functions (`ISSUE-260902-0747-07-inject-event-sink-into-decision-functions.md:82-87,103-104`). No implementation can preserve that four-step order: failure propagation and the success-only steps are mutually exclusive branches.

**Fix:** state two contracts:

- success: durable append -> assign returned durable sequence -> broadcast;
- failure: propagate the distinguished append error before decision-log mutation, with no broadcast and no later state mutation.

The issue's AC2 already has most of the correct failure-branch shape (`:96-102`); AC3 should not contradict it.

## Medium — `-02` has an undeclared dependency on `-01`

`-02` has no `blocked_by` field, but its AC4 requires observing the same test set through both the default command and “the Integration Tier switch introduced by `-01`” (`ISSUE-260902-0747-02-consolidate-fake-process-fixtures.md:66-78`). An agent can legally pick `-02` first and be unable to satisfy its acceptance criterion.

**Fix:** either add `blocked_by: [ISSUE-260902-0747-01]`, or make the prefactor acceptance independent of tier machinery by recording the current full-suite command/result before and after. I prefer the latter because the refactor is mechanically independent and need not sit behind the boundary work.

## Medium — a pure performance test has no valid tier and no slice owns it

`validate_workflow_many_calls_against_large_subflow_within_budget` is a pure validation test but reads `Instant` and asserts completion under 15 seconds (`src/model.rs:6631-6652`). It is not Logic under the clock rule, but it is also not Integration under the ADR's real-infrastructure definition (`docs/adr/260902-0312-deterministic-test-tiers.md:46-53`). Marking it Integration solely because it asserts elapsed time contradicts “membership is decided by what a test isolates.” Leaving it default contradicts the Logic rule.

I found no reference to this named test in the PRD or the twelve issue files:

```text
$ rg -n 'validate_workflow_many_calls_against_large_subflow_within_budget' \
    docs/prd/PRD-260902-0301-01-deterministic-test-suite.md \
    docs/issues/ISSUE-260902-0747-*.md
# no output
```

**Fix:** define a third benchmark/performance category outside the correctness gate, or explicitly broaden/rename Integration to include irreducible non-gating performance tests. A better long-term home is a benchmark; either way, one slice must own the move and its execution in CI.

## Medium — `test-under-load` has no positive control for the load and its red baseline is stochastic

The recipe starts `yes` processes and prints the number of PIDs it appended, but never verifies that any worker is still alive (`justfile:54-71`). A failed background exec can leave a dead PID in the array while line 64 still reports “busy processes.” The recipe therefore can claim loaded acceptance while providing no load.

Also, `RUNS="3"` is a sampling choice, not proof that an intermittent failure reproduces. The PRD and `-11` assert that the recipe is red before the fix (`PRD:492-501`; `-11:114-117`), but a flaky suite can pass all three attempts without contradiction. That AC is not reliably red at baseline.

I did **not** run this recipe in the review environment: it intentionally saturates CPUs and, on the current target, can invoke real tmux on the default/shared process boundary. I therefore cannot verify the empirical claim that this particular three-run invocation currently reproduces the failure.

**Fix:** after starting the workers, verify each with `kill -0` (and fail if the requested load was not established); record the load parameters in output. Treat the focused deterministic regression as the red/green acceptance and the under-load repetition as a final resilience check, not as a guaranteed red baseline.

## Medium — `-03` permits a nominal one-millisecond “generous” margin

`-03` says the configured timeout must be greater than 10,790 ms “by a margin the implementer chooses” (`ISSUE-260902-0747-03-frontend-generous-test-timeout.md:39-66`), but AC2 checks only strict greater-than and inequality with Vitest's default (`:76-82`). A value of 10,791 ms plus a comment saying “1 ms margin” satisfies the acceptance criteria while providing essentially no headroom over the observed 10.79 s run.

**Fix:** make generosity falsifiable: specify a minimum absolute/multiplicative margin (for example, at least 2x the observed maximum or a settled 30/60-second value), then test that bound. The provenance comment should explain the selected value, not substitute for a minimum.

## Medium — `-11` names the wrong thing as the awaitable seam

`-11` says `creates_and_approves_runs` moves to a router helper that returns the database “so it has something to await” (`ISSUE-260902-0747-11-http-target-awaits-the-run-stream.md:60-64,87-92`). A `Database` is not the promised happens-before signal; polling it would recreate the defect. The source test already receives the serialized `runId` and can retain the returned `streamToken`, but currently ignores the token and sleeps twice (`tests/http_api.rs:1184-1271`, sleeps at `:1222` and `:1258`).

**Fix:** say that the test awaits the HTTP run stream via `router`/`runId`/`streamToken`. Return the database only if a one-shot persisted-state assertion remains after the corresponding stream event; do not describe the database handle itself as something to await.

## Low — `-05`'s allowed designs conflict with its post-clear AC

The desired behavior permits either handing a completion receiver out at registration or providing a level-triggered after-the-fact mechanism (`ISSUE-260902-0747-05-run-lifecycle-event-handle.md:62-73`). AC4, however, specifically requires asking the registry's completion accessor **after** the run has completed and been removed (`:118-121`). A handout-only design can correctly let a previously obtained handle resolve after completion but cannot satisfy a new post-clear lookup without adding a tombstone/journal.

**Fix:** make AC4 conditional on the selected sanctioned shape: either a handle acquired at lifecycle start still resolves when awaited after clear, or a level-triggered accessor acquired after clear resolves. If new callers genuinely must first acquire after completion, remove the handout-only alternative from the desired design.

## Low — `-08` has an unmeasurable acceptance phrase

`-08` AC2 requires a test that uses supplied instants while the test itself consumes “no measurable wall-clock time” (`ISSUE-260902-0747-08-extract-interactive-poll-step.md:76-80`). Every executed test consumes measurable time, and adding an elapsed assertion to prove otherwise would violate the rule being established.

**Fix:** use the structural property: the test performs no deliberate wait and reads no clock; all temporal inputs are supplied instants. That is reviewable and deterministic.

## Verification scope

All source and planning evidence above was inspected from the uncommitted `feature/tmux-panes` working tree. I made no writes to the SilverBond repository. I did not execute the Rust suites or the CPU-load recipe, so I make no empirical claim about their current pass rate; the findings are source/contract and dependency-graph findings, except where I explicitly report command output from static searches.
