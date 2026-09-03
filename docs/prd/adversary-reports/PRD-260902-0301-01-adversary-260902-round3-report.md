## Plan adversary report

- scale: epic
- source-decision: author-supplied
- artifacts: docs/prd/PRD-260902-0301-01-deterministic-test-suite.md

### Findings

#### Wrong problem

- **class:** wrong problem
- **impact:** The epic can remain blocked on a cross-repository production API migration after the deterministic-test acceptance property has already been delivered.
- **evidence:** The stated problem and ask are that SilverBond's tests wait on wall clock and exercise external-process machinery instead of decision logic (`PRD`, “Problem Statement”; ask-summary), and acceptance is explicitly the Logic Tier producing identical pass/fail results under load with the default gate invoking that tier (`PRD:288-295`). Yet the solution also commits the separately pinned `tmux-tools` repository to a Tokio process/time rewrite, sequenced only after SilverBond's seams have removed process execution from the Logic Tier (`PRD:191-220`). That rewrite changes production blocking/async architecture, while the acceptance test neither observes it nor requires it; what part of the source problem remains unsolved if the tier/seam work passes acceptance but this external migration does not land?

#### Codebase-reality collision

- **class:** codebase-reality collision
- **impact:** No single Cargo tier mapping described by the PRD can both make the gate Logic-only and keep the two deterministic committed-file invariant targets in that gate.
- **evidence:** The artifact simultaneously says “A test under `tests/` is integration tier by construction” and uses a Cargo feature only as the marker for an **in-crate** integration test (`PRD:94-111`), while assigning generated-catalog staleness and shipped-template validation to the Logic Tier and gate (`PRD:46-50,276-282`). Cargo metadata for this package exposes all three files as peer auto-discovered test targets—`http_api`, `docs_catalog`, and `bundled_templates`—with no `required_features`; `Cargo.toml` has neither `[features]` nor `[[test]]` declarations. A function-level feature can suppress marked in-crate tests when disabled, but it does not stop plain `cargo test --locked` from running `tests/http_api.rs`; target-level feature requirements or explicit target selection would be an additional mechanism. Applying the artifact's directory rule to all three targets would also classify `docs_catalog` and `bundled_templates` as Integration and remove them from the Logic-only gate. Which authoritative tier classification and Cargo command satisfy both statements?

- **class:** codebase-reality collision
- **impact:** The proposed event-sink extraction can still leave purported Logic Tier tests dependent on the real clock, violating the tier contract and producing non-reproducible checkpoint values.
- **evidence:** The Logic Tier forbids a real clock (`PRD:46-51`), but the extraction claims that an injected async event sink removes the database obstacle from six decision functions and names `select_next_decision` as the already half-cut example (`PRD:170-188`). In the actual example, successful decide and loop arms write `DecisionLog.timestamp = now_iso()` after the awaited emission (`src/runtime.rs:6387-6411,6485-6507`), and `now_iso()` is `Utc::now()` (`src/util.rs:37-39`). Closely coupled decision helpers also stamp the same clock—`record_transition_for_cursor` at `src/runtime.rs:6336-6352`, `record_batch_item_result` at `:5603-5654`, and `handle_split_node` at `:5725-5798`; split handling additionally mints time-based v7 UUIDs. Injecting only an event sink preserves the existing awaited append failure boundary, but how does it make these functions eligible for a tier whose explicit rule is “no real clock”?

- **class:** codebase-reality collision
- **impact:** The scope and failure-boundary audit for the sink extraction cannot be completed, so different implementation slices can choose different “six” and leave decision logic behind the database seam.
- **evidence:** The artifact twice says “The six are named in the issue that owns this slice” (`PRD:180-189`), but its frontmatter contains `issues: []`, and the repository contains no issue for this PRD naming those functions; the only owning issue is the closed diagnostic `ISSUE-260901-0216-03`, which does not enumerate them. The repo has many async decision/mutation functions with interleaved `emit_event(...).await?` calls and checkpoint mutations—for example `prepare_cursor_visit` (`src/runtime.rs:3255-3330`), `handle_split_node` (`:5725-5865`), `record_transition_for_cursor` (`:6327-6352`), and `select_next_decision` (`:6355-6568`). Which exact six boundaries are the production and fake sinks required to cover so that all append-before-mutation and mutation-before-append failure behavior can be checked?

#### Missed simpler alternative

none.

#### Hidden coupling

- **class:** hidden coupling
- **impact:** Replacing intermediate-state polling with the raw registry receiver can introduce a missed-event race, leaving approval tests schedule-dependent under the same load the epic is meant to tolerate.
- **evidence:** The artifact says the general seam “already exists,” is directly awaitable, and carries pending and successive queued approvals (`PRD:133-139`). The events do exist: `queue_approval` emits `approval_queued`, and `activate_next_approval` emits `approval_required` only after installing the approval sender (`src/runtime.rs:3598-3661`); the two-approval test currently waits for exactly those successive states through SQLite (`:12856-12931`). But `RunRegistry::subscribe` is a live broadcast subscription with no replay and returns `None` once the active entry is cleared (`:918-967,1118-1123`), while `start_run` generates the ID, spawns the executor, and only then returns that ID (`:1402-1440`), so the caller cannot subscribe before early approval events. Existing direct event tests avoid this by registering a known run ID and subscribing before spawning their producer (`:8364-8405`); the race-free journal-plus-live replay exists only inside the private HTTP SSE path (`src/api.rs:885-971`), which the PRD assigns to Integration. What in-process happens-before edge is reachable by `start_run`-based runtime tests if the first `approval_required` or `approval_queued` is emitted before they can obtain the receiver?

#### Sequencing errors

none.

#### Unjustified stack/dependency assumptions

none.
