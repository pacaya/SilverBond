## Plan adversary report

- scale: epic
- source-decision: author-supplied
- artifacts: docs/prd/PRD-260902-0301-01-deterministic-test-suite.md

### Findings

#### Wrong problem

- **class:** wrong problem
- **impact:** The Logic Tier can still false-red under CPU contention at an allowed wall-clock guard, so the epic can meet its tier rules without making the default gate deterministic under load.
- **evidence:** The artifact allows “a duration that is never paid on the passing path” in both tiers while promising the Logic Tier gives the same result “on any machine under any load” and under the reproduced heavy-contention acceptance run (PRD, `## Testing Decisions`, lines 270-278 and 308-315). A timeout is still a race against wall time when the awaited producer is starved: the repo’s `abort_and_wait` races `active.drained_token.cancelled()` against a 250ms test-profile `tokio::time::timeout` (`src/runtime.rs:43-50,975-990`). The diagnostic demonstrates the same failure mode at a larger guard: `wait_for_run` normally returns before its five-second deadline, yet under load the deadline won and produced the reproduced false reds (`tests/http_api.rs:143-159`; `docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md:50-67`). What finite wall-clock guard is compatible with the unconditional determinism claim when correct work can be scheduler-starved past it?

#### Codebase-reality collision

- **class:** codebase-reality collision
- **impact:** Following the stated target-level selector leaves the in-crate resource tests in bare `cargo test`, so the supposed Logic-only gate still executes subprocess, FIFO, database-permission, and real-tmux coverage.
- **evidence:** The artifact orders explicit `[[test]]` targets with `required-features = ["integration-tests"]`, then says a function-level gate is insufficient “alone” and concludes bare `cargo test` runs only “in-crate logic tests” (PRD:115-123). Cargo metadata for this package has one library target plus only three `test` targets (`http_api`, `docs_catalog`, `bundled_templates`); `required-features` on `[[test]]` entries can skip those external targets but cannot select individual tests inside the single `src/lib.rs` harness. The artifact itself assigns existing in-crate tests to Integration—FIFO tests in `src/api.rs`, database migration/permission tests in `src/storage.rs`, process-group tests in `src/proc.rs`, and the guard tests at `src/tmux_exec.rs:3941-4044` (PRD:296-306)—and rejects moving them outward (PRD:100-107). No function-level marker is actually specified as part of the authoritative selector. Which mechanism removes those tests from the library harness when `integration-tests` is absent?

- **class:** codebase-reality collision
- **impact:** At least one of the six proposed Logic-tier decision seams still creates control-state identity from the wall clock, so the timestamp exemption does not establish the claimed deterministic boundary.
- **evidence:** The artifact justifies clock eligibility because `now_iso()` values are inert record stamps that “gate no control flow,” then names `release_collectors_if_ready` among the six sink-extracted functions (PRD:192-209). In the all-dead collector path, that function instead calls `new_cursor_id` (`src/runtime.rs:6039-6049`); `new_cursor_id` is `Uuid::now_v7()` (`src/runtime.rs:1913-1915`). The generated value is used to find/insert the representative cursor, written into checkpoint state and execution logs, and emitted as `cursorId` (`src/runtime.rs:6058-6066,6098-6126`). `handle_terminal_cursor_status` reaches this path transitively through `release_collectors_if_ready` (`src/runtime.rs:6322`). Does the clock exception extend to a time-derived identifier that participates in state and event identity, or are these paths not actually Logic-tier eligible?

#### Missed simpler alternative

No findings.

#### Hidden coupling

- **class:** hidden coupling
- **impact:** Injecting only the event sink cannot make all six functions database-free, so the extraction is under-scoped and at least `apply_join_result` still requires either a real `Database`-bearing context or another unplanned port.
- **evidence:** The artifact says “An injected sink removes the database” from all six named functions (PRD:192-209). `apply_join_result` also reads abort state through `ctx.registry.is_aborted(...)` and passes the same full context into downstream decision functions (`src/runtime.rs:4542-4591`), independently of event emission. The only current context constructors require a `Database`, and `RuntimeContext` stores it directly (`src/runtime.rs:1369-1399`); `emit_event` is only one consumer of that context (`src/runtime.rs:6985-6989`). What database-free dependency contract supplies abort state while preserving the named function seam?

#### Sequencing errors

No findings.

#### Unjustified stack/dependency assumptions

- **class:** unjustified stack/dependency assumptions
- **impact:** The four real-tmux tests can remain silent passes on machines without tmux, so the planned repair may not deliver the visible skip state it relies on.
- **evidence:** The artifact requires the tmux-absent path to “skip loudly” rather than return vacuous success (PRD:218-227). The current tests dynamically check `tmux_available()` and return (`src/tmux_exec.rs:3941-4044`), but Rust’s standard test harness has no dynamic skipped outcome: `#[ignore]` is a static test attribute, while output from a passing dynamically-returning test is captured by normal `cargo test`. The repo’s default commands are plain `cargo test --locked` (`justfile`, `test-rust`; `.github/workflows/rust-tests.yml:50-51`), with neither `--show-output` nor another reporter. What supported harness behavior makes a runtime dependency absence visibly “skipped” in those commands without turning it into an ordinary pass or failure?
