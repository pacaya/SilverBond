## Plan adversary report

- scale: epic
- source-decision: author-supplied
- artifacts: docs/prd/PRD-260902-0301-01-deterministic-test-suite.md

### Findings

#### Wrong problem

- **class:** wrong problem
- **impact:** The epic can satisfy its stated acceptance criterion while the headline `cargo test --locked` gate remains load-dependent, leaving the maintainer problem it opens with unresolved.
- **evidence:** The problem and first user story require the whole `cargo test --locked` result to be trustworthy under load (`PRD:20-25,64`), but the Solution and acceptance narrow success to “the logic tier” (`PRD:59-60,202-204`) while expressly permitting the integration tier to retain real SQLite, subprocesses, FIFOs, clocks, and generous timing budgets (`PRD:50-53,173-175,192-200`). The repository’s local and CI gates both still invoke unfiltered `cargo test --locked` (`justfile:47-48`; `.github/workflows/rust-tests.yml:50-51`), which runs both unit and integration targets. If a retained integration deadline flakes under contention, the epic can pass its logic-tier acceptance while the named gate is still a false red; which of those two outcomes is the actual success contract?

#### Codebase-reality collision

- **class:** codebase-reality collision
- **impact:** Tests classified as integration cannot be relocated using the visibility surface the PRD budgets, so the location-based tier split does not compile as described.
- **evidence:** The PRD accepts that tests moved to `tests/` “may need a `pub(crate)` test-support surface” (`PRD:91-95`). Rust integration tests are separate crates, so they cannot access `pub(crate)` items in the library, and the library’s `#[cfg(test)]` items are not compiled for them; `src/lib.rs:1-15` demonstrates the distinction by exporting normal modules but compiling `test_support` only under `cfg(test)`, while `tests/http_api.rs:1-18` imports the library as the external crate `silverbond`. The understated cost is live in the heavy suites: `RuntimeContext::with_runner` is private and `#[cfg(test)]` (`src/runtime.rs:1391-1400`), and database migration/permission tests—explicitly assigned to the integration tier at `PRD:194-196`—reach through private `Database.connection` and `with_connection` (`src/storage.rs:30-39,1397-1405,1810-1840,2053-2103`). What callable surface are those relocated tests expected to compile against if `pub(crate)` is the accepted escape hatch?

- **class:** codebase-reality collision
- **impact:** Relocating resource tests can silently exchange short test bounds for ten-minute production bounds, making the proposed integration tier hang rather than merely run with a generous budget.
- **evidence:** The PRD cites “a timeout shimmed by `cfg!(test)`” as correct prior art (`PRD:160-164`) and assigns pane-stream setup/teardown over real FIFOs to `tests/` (`PRD:192-196`). An integration target compiles the library without `cfg(test)`: `pane_stream_owner_wait_timeout` therefore returns 600 seconds rather than 250ms (`src/api.rs:69-76`), even though the current inline regression `pane_stream_subscribe_force_clears_when_owner_never_completes` relies on the short branch (`src/api.rs:5406-5449`). The same collision exists for `abort_and_wait_drain_timeout`, 250ms under unit-test compilation and 600 seconds otherwise (`src/runtime.rs:44-50`), with its force-clear regression currently inline at `src/runtime.rs:8108-8128`. Which tier owns these liveness behaviors under the location rule, and which compiled timeout is their intended observable?

- **class:** codebase-reality collision
- **impact:** The production select-arm change does not establish the structural property claimed for the three named promptness tests, so the plan’s asserted removal of all three elapsed-time checks is unsupported.
- **evidence:** The PRD says adding the abort token to the supervisor selects at `src/runtime.rs:3007,3022,3772` “dissolves three wall-clock promptness assertions” (`PRD:115-119`), and the diagnostic names `src/runtime.rs:8528,11360,11472`. Those assertions are on three different paths that already have their own token arms: the `:8528` test calls `escalate_agent_interaction` directly, outside the supervisor, and that function selects on `abort_token.cancelled()` at `:7278-7287`; the decide path selected by the `:11360` test already selects the token against its blocking join at `:4042-4080`; and the parallel-batch path selected by the `:11472` test already selects the token against `running.join_next()` at `:5165-5200`. How can a new arm in the outer supervisor be the structural replacement for assertions governed by these existing inner arms—especially the direct interaction test that never runs the supervisor?

- **class:** codebase-reality collision
- **impact:** Deleting the four real-tmux tests removes coverage of SilverBond-owned cleanup policy, not duplicate coverage of tmux behavior as the PRD claims.
- **evidence:** The PRD says the tests are deleted because “What they cover is tmux’s own behavior” (`PRD:143-146`). In the repo, the first two construct SilverBond’s private `SessionGuard` and verify its Drop/disarm policy (`src/tmux_exec.rs:3940-3977`), while the next two construct SilverBond’s `PaneGuard` and verify its `kill_after` Drop/disarm policy (`:3979-4044`). Those policies are implemented in SilverBond: `SessionGuard::drop` chooses `kill-session` (`:383-404`), and `PaneGuard::drop` chooses whether to kill a SilverBond-owned session/pane and clear its active registration (`:407-489`). The shared socket, vacuous return, and real `sleep 600` hazards are real, but why does that make the SilverBond RAII contract the dependency repository’s responsibility?

#### Missed simpler alternative

No findings.

#### Hidden coupling

- **class:** hidden coupling
- **impact:** A drained-token getter mirroring the abort getter is not itself a reliable happens-before seam, so replacing persistent-state polling with it can introduce a new schedule-dependent miss.
- **evidence:** The PRD says a symmetric drained-run accessor “replaces the polling helpers with a real happens-before edge” (`PRD:100-105,113-122`). The existing mirrored shape looks up an active entry and returns `Option<CancellationToken>` (`RunRegistry::abort_signal`, `src/runtime.rs:1002-1008`), while `clear` removes the registry entry before cancelling its drained token (`:1118-1122`). `RuntimeContext::start_run` spawns the supervised task before returning the generated run ID (`:1402-1440`), so a fast run can execute `finalize_run` and remove the entry before its caller can request the drained token. What edge is observable in that already-drained interleaving when the accessor can only return `None`?

- **class:** hidden coupling
- **impact:** Making the six runtime functions synchronous can alter durable event ordering and error semantics, while the PRD treats event emission as a replaceable notification side effect.
- **evidence:** The PRD says “Six large runtime functions are `async` only because they emit runtime events” and that parameterizing the sink makes them synchronous, but it neither names the six nor specifies the sink contract (`PRD:131-134`). In this repo `emit_event` is not merely an in-memory notification: it awaits `Database::append_event`, propagates that failure, assigns the durable sequence, and only then broadcasts (`src/runtime.rs:6985-6989`). Callers mutate checkpoint state on both sides of those awaited emissions—for example `select_next_decision` emits before appending decision-log entries (`:6355-6568`)—so deferring or buffering the sink changes observable failure and ordering behavior. The cited “called directly by four tests” evidence is also four call sites in only two tests: three calls inside `unmatched_branch_conditions_route_to_first_branch_edge` (`:14440-14568`) and one inside `decide_missing_branch_edge_degrades_instead_of_bailing` (`:14570-14630`). Which exact six functions and what event-sink semantics preserve the current append-before-broadcast and failure boundaries?

#### Sequencing errors

- **class:** sequencing errors
- **impact:** The first CI execution can occur before the work intended to make that signal trustworthy, making the “interpretable first run” user story impossible for this epic to guarantee.
- **evidence:** User story 2 asks for the CI job’s first execution to be interpretable (`PRD:65`), but the PRD explicitly says the issue that pushes the branch and observes that first run “is not blocked by this epic,” concedes that the run has an elevated false-red probability, and accepts merely reading it in light of this PRD (`PRD:271-274`). The workflow triggers on every push and immediately runs the full flaky suite (`.github/workflows/rust-tests.yml:3-5,50-51`). If `ISSUE-260901-0216-05` proceeds first as allowed, how can later completion of this epic retroactively satisfy its own first-run story?

#### Unjustified stack/dependency assumptions

- **class:** unjustified stack/dependency assumptions
- **impact:** Treating the tmux-tools Tokio conversion as a process-API substitution can break per-run invocation isolation or expand into an unplanned cross-repository API redesign.
- **evidence:** The narrow factual premise is correct: SilverBond enables Tokio’s `process` feature and has no `tokio::process` use (`Cargo.toml:28-30`), and the pinned tmux-tools core already enables Tokio `full` while busy-polling `std::process::Child::try_wait` (`tmux-tools core/Cargo.toml:6-16`; pinned `core/src/tmux.rs:205-239`). The PRD nevertheless concludes that switching process/time APIs “deletes the blocking bridges and the hand-rolled thread” (`PRD:148-153`). At the pinned revision, tmux-tools resolves invocation through a thread-local override whose RAII scope is a synchronous closure (`core/src/tmux.rs:66-114`); an async future may be polled after that closure restores the override and may migrate threads. SilverBond depends on that scope broadly (`with_invocation` has 34 call sites under `src/`) and its own active-pane and interaction adapters explicitly require a `spawn_blocking` thread so synchronous code can call `Handle::block_on` (`src/tmux_exec.rs:546-677,679-745`). What cross-`await` invocation and adapter contract justifies the claim that the existing bridges simply disappear?
