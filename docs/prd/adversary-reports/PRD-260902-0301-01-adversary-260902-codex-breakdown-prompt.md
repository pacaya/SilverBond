You are running in read-only-against-project mode. Sandbox details:
- Project under review: /Users/Shared/Data/work/Programming/SilverBond — read via absolute paths only; the sandbox will reject any write here with "Operation not permitted" and you should NOT retry.
- Scratch dir for any output files: /tmp/codex-research — this is your only writable location.
- You may use `git -C /Users/Shared/Data/work/Programming/SilverBond status|diff|log|show|ls-files` for context; mutating git ops will fail and that's expected.

Task:

# Adversarial second opinion on an epic breakdown

A Rust + Svelte repo has a flaky test suite: the Rust suite fails intermittently under CPU load because tests assert on wall-clock time and the test seam sits below the process boundary. A PRD and twelve issue briefs have been written to fix it. Five rounds of a "cold reader" gate have already run over the PRD; each round found real defects. I want your independent, adversarial read — specifically on two open decisions, and on anything those five rounds missed.

**Nothing here is committed.** All the documents below are working-tree state on branch `feature/tmux-panes`.

## Read these (absolute paths)

Planning artifacts:
- /Users/Shared/Data/work/Programming/SilverBond/docs/prd/PRD-260902-0301-01-deterministic-test-suite.md
- /Users/Shared/Data/work/Programming/SilverBond/docs/adr/260902-0312-deterministic-test-tiers.md
- /Users/Shared/Data/work/Programming/SilverBond/CONTEXT.md (§ Testing only)
- /Users/Shared/Data/work/Programming/SilverBond/docs/issues/ISSUE-260902-0747-01-draw-tier-boundary.md through -12-scripted-delays-encode-orderings.md (twelve files, glob `docs/issues/ISSUE-260902-0747-*.md`)
- /Users/Shared/Data/work/Programming/SilverBond/docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md — the diagnostic record. Its `### Timing-site audit` § "Classification" table is treated as CONTRACTUAL and outranks prose elsewhere.

Source (verify claims against it — do not trust the documents):
- /Users/Shared/Data/work/Programming/SilverBond/src/runtime.rs (large; the runtime + its test module)
- /Users/Shared/Data/work/Programming/SilverBond/src/api.rs
- /Users/Shared/Data/work/Programming/SilverBond/src/storage.rs
- /Users/Shared/Data/work/Programming/SilverBond/tests/http_api.rs

## Background you need

The tier rule (from the ADR and CONTEXT.md): a **Logic Tier** test isolates its unit — every port replaced by a fake or supplied with controlled data — and touches no clock. "No clock" was recently restated as three forbidden forms: (a) sleeping to sequence work, (b) asserting on elapsed time, (c) any wall-clock bound that is the **termination condition of a polling or convergence wait**. A bound that wraps an await which a happens-before edge ends on the passing path, and so fires only on a hang, is permitted as a last resort. The **Integration Tier** is the small tier that runs against real infrastructure and is excluded from the default `cargo test` command, which is the gate.

Relevant slices:
- `-01` draws the tier boundary and marks every Integration Tier test.
- `-05` deletes the polling helpers `wait_for_run` / `wait_for_terminal_run` / `wait_for_event` / `wait_for_registry_empty` (`src/runtime.rs`) and `wait_for_pending_approval` (`src/api.rs`), replacing them with an in-process run-lifecycle event handle.
- `-06` owns the abort path: four elapsed-time promptness assertions and a supervisor select-arm.
- `-11` converts the out-of-crate HTTP target (`tests/http_api.rs`) to await the run stream; it carries the epic's acceptance.
- `-12` (newest) converts `ScriptedStep::with_delay` — 32 call sites across 14 tests in `src/runtime.rs` — from real sleeps to a test-controlled happens-before edge.

## Decision 1 — does `-05`'s AC6 install the defect?

`-05` acceptance criterion 6 reads: "The runtime and API tests that lose their clock wait and spawn no process are executed by the default command after this change and were not before it."

All 14 scripted-delay tests use `wait_for_terminal_run` (which ends at a 5s deadline over a poll of persisted state, `src/runtime.rs:9858-9876`) and spawn no process. So if `-05` lands before `-12`, AC6 appears to *require* promoting into the gate 14 tests that still sleep a scripted total of ~3.7 seconds and still settle their orderings by race. `-06` has a guard against exactly this (its AC5: a test that loses its elapsed assertion but still sleeps through a scripted delay is not promoted); `-05` does not.

Proposal on the table: give `-05` the same guard, and make `-12` `blocked_by` `-05` as well as `-01`, so `-05` removes the deadline waits without promoting anything that still sleeps, and `-12` removes the sleeps and performs the promotion.

Questions for you:
1. Is the reading of AC6 correct, or is it being read uncharitably?
2. Is the proposed fix right, or is there a better cut? Consider the alternative ordering (`-05 blocked_by -12`).
3. Are there tests *other than* those 14 that AC6 would wrongly promote? Check `src/api.rs` too — AC6 says "runtime and API tests".

## Decision 2 — the calibrated pair split across two slices

`parallel_batch_abort_cancels_pending_items_after_next_completion` (`src/runtime.rs:11423`) scripts `ScriptedStep::success("done a").with_delay(2000)` at `:11432`, and asserts `abort_started.elapsed() < Duration::from_millis(500)` at `:11472`. The diagnostic record states these are calibrated against each other. `-12` deletes the delay; `-06` owns the assertion; neither is blocked by the other.

Proposal on the table: give the whole test to `-06` (it owns the harder half and must build the ordering mechanism anyway), have `-12` exclude it wholesale and gain a `blocked_by` on `-06` so `-12`'s AC1 (`rg -n 'delay_ms' src/runtime.rs` returns nothing) can stay absolute.

Questions for you:
1. Verify the calibration claim in the source. What actually breaks if the delay is removed first?
2. Is "give it to `-06`" right, or should `-12` take both halves, or is there a third cut?
3. Does `-06`'s existing AC3 (an ordering assertion for the supervisor's abort arm) interact with this test?

## Part 3 — what did five rounds miss?

Independently of the two decisions, hunt for defects in the breakdown. I am most interested in:
- **Acceptance criteria that are already green at their own declared baseline** (vacuous), or that can be satisfied without doing the work.
- **Ordering hazards** between slices: a slice whose acceptance depends on state another slice creates or destroys, without a `blocked_by` edge.
- **Claims about the source that do not reproduce.** Verify aggressively; several such claims have already been found and fixed, and I expect more.
- **Work in scope for the epic that no slice owns**, or owned twice.
- Anywhere the tier rule as written gives the wrong answer for a real test in this repo.

## Output

Write your full reply as markdown to /tmp/codex-research/codex-response-B5BC7D14.md and reply in the pane only with `DONE: /tmp/codex-research/codex-response-B5BC7D14.md`.

Structure it as: Decision 1, Decision 2, then Part 3 findings ranked by severity. For every finding give the evidence — file:line, or a command and its actual output. Say explicitly when you could not verify something rather than asserting it. Where you disagree with the proposals on the table, say so directly and give your alternative; I want your judgment, not validation.
