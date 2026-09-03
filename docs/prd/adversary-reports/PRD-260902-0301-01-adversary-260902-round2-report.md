## Plan adversary report

- scale: epic
- source-decision: author-supplied
- artifacts: docs/prd/PRD-260902-0301-01-deterministic-test-suite.md

### Findings

#### Wrong problem

- **class:** wrong problem
- **impact:** The epic's own tier-discoverability story cannot be satisfied by the accepted tier design, so completion can still leave contributors unable to infer a test's tier from its location.
- **evidence:** User story 6 requires a developer to know the tier “from where it lives” (`PRD:69`), but the revised decision rejects location as the marker, says the marker will be explicit in-crate, and accepts that it “must be remembered rather than being structural” (`PRD:91-100`); the accepted ADR repeats that a test's path is not the marker (`docs/adr/260902-0312-deterministic-test-tiers.md`, “The tier is a contract ... not about where its file sits”). Which of these incompatible outcomes is the epic actually required to deliver?

#### Codebase-reality collision

- **class:** codebase-reality collision
- **impact:** The default gate can continue to execute the flaky out-of-crate HTTP target, or the epic inherits an unstated reverse migration of all existing Cargo integration targets.
- **evidence:** The PRD says “Two tiers, both inside the crate” and that the integration tier includes “end-to-end HTTP round-trips” (`PRD:91-100,244-248`), but the reproduced failures live in `tests/http_api.rs`, which imports `silverbond` as an external crate (`tests/http_api.rs:1-18`) and contains the real-clock `wait_for_run` (`:143-159`). Two other test targets also remain under `tests/`, while `src/lib.rs:15-16` confirms that `test_support` exists only in the library's unit-test compilation. `cargo test --locked` currently runs all of these targets (`justfile:47-48`; `.github/workflows/rust-tests.yml:50-51`). Does “both inside the crate” require moving the existing targets inward, or are they exceptions that continue to compile the library without `cfg(test)` and need a different tier mechanism?

- **class:** codebase-reality collision
- **impact:** Implementing the production change as written recreates the exact missed-completion race the preceding paragraph says must be eliminated.
- **evidence:** The PRD correctly states that an accessor shaped like `abort_signal` is insufficient because a fast run can disappear before lookup, and requires a registration-time or level-triggered seam (`PRD:115-121`), but its subsequently enumerated decided change is “The drained-run signal gets an accessor mirroring the existing abort-signal accessor” (`PRD:129-132`). In the repo, `abort_signal` performs an `Option` lookup in the active map (`src/runtime.rs:1002-1008`), `clear` removes the entry and only then cancels its token (`:1118-1122`), and `start_run` spawns before returning the ID (`:1402-1440`). Which non-racy contract supersedes the explicitly decided mirrored accessor?

- **class:** codebase-reality collision
- **impact:** The proposed extraction cannot make the six functions synchronous while also preserving the durable event failure and ordering contract, so the named seam is not implementable as described.
- **evidence:** The PRD says “Parameterizing the event sink makes them synchronous functions” while also requiring the sink to await durable append and preserve the existing failure boundary (`PRD:152-162`). The real sink necessarily awaits `Database::append_event`, obtains the durable sequence, and then awaits registry broadcast (`src/runtime.rs:6985-6989`). The ordering is observable inside the cited half-cut decision seam: `select_next_decision` awaits `emit_event` and propagates failure before it mutates `checkpoint.execution_log.decisions` (`src/runtime.rs:6387-6411`). How can a synchronous parameterized sink complete an asynchronous append before allowing the following mutation, without deferring/buffering or changing the failure boundary the PRD forbids changing?

#### Missed simpler alternative

none.

#### Hidden coupling

- **class:** hidden coupling
- **impact:** A run-completion token can remove terminal polling but leaves clock-bound polling for intermediate states and for router-level callers, so “the polling goes” is not achieved by the decided seam.
- **evidence:** The PRD attributes runtime polling to the missing drained-run signal and says the completion seam must replace “the polling helpers” (`PRD:109-121`). The token is cancelled only by `RunRegistry::clear` (`src/runtime.rs:1118-1122`), which represents drain/completion, but existing uses of `wait_for_run` must observe states strictly before completion: for example, the queued-approval test waits first for `approve_a` and then `approve_b` before it can supply either response (`src/runtime.rs:12886-12916`), and the external HTTP test waits for `pending_approval` before issuing its next router request (`tests/http_api.rs:520-540`). The registration-time alternative also returns from `RuntimeContext::start_run`, whereas router tests receive only the serialized `runId` after `api::create_run` consumes that return. What happens-before signal covers these pre-terminal predicates and the HTTP seam?

- **class:** hidden coupling
- **impact:** The logic-only gate can demote deterministic, repository-owned contract checks to a non-gating job merely because they read files, reducing existing acceptance coverage while solving timer and external-process flakes.
- **evidence:** The tier contract says Logic Tier uses “no OS resources” and only Logic Tier gates (`PRD:46-53,102-107`). Yet the current gate includes deterministic first-party invariants outside the PRD's module assignment: `tests/docs_catalog.rs` reads the committed schema and fails when generated catalog content is stale (`:745-756`) and verifies the `justfile` regeneration contract (`:776-785`); `tests/bundled_templates.rs` reads and validates the repository's shipped template JSON (`:17-42`). These touch the filesystem, so the stated contract excludes them from Logic Tier even though they use no external dependency or clock. Which tier owns these checks, and are their failures still supposed to block the gate?

#### Sequencing errors

- **class:** sequencing errors
- **impact:** CI splitting, the default gate, and the mechanical rule all depend on a tier selector that the plan postpones, so downstream slices can choose incompatible commands and never establish the promised gate.
- **evidence:** The PRD declares two named, enforced tiers and a logic-only gate, but leaves the selector—Cargo feature, `#[ignore]`, or equivalent—as implementation work (`PRD:44,91-106`) and conditions its only mechanical check on whatever boundary later emerges (`:218-221`). The repo currently has no Cargo `[features]` section (`Cargo.toml:1-39`), `just test-rust` and the sole CI job both invoke unfiltered `cargo test --locked` (`justfile:47-48`; `.github/workflows/rust-tests.yml:14-17,50-51`), and `#[ignore]` is already used for a non-tier maintenance writer (`tests/docs_catalog.rs:2704-2711`) invoked separately by `just regen-docs` (`justfile:74-76`). What must land first, and what exact selector contract can the CI and enforcement slices rely on before either is built?

#### Unjustified stack/dependency assumptions

none.
