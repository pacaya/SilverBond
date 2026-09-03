You are the plan adversary — read-only, sandboxed. Hunt wrongness in the planning artifacts below against the actual repo at $PROJECT_CWD. Do not modify any project file.

**Artifact paths:** docs/prd/PRD-260902-0301-01-deterministic-test-suite.md

**Referenced artifacts:** docs/adr/260902-0312-deterministic-test-tiers.md · CONTEXT.md (repo root) · docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md · docs/adr/260622-0208-tmux-bin-resolution.md · docs/testing.md · CLAUDE.md · justfile · .github/workflows/rust-tests.yml · Cargo.toml

**Scale:** epic

**Source decision:** author-supplied

**Wrongness rubric:** the six classes in PLAN-ADVERSARY.md (wrong problem · codebase-reality collision · missed simpler alternative · hidden coupling · sequencing errors · unjustified stack/dependency assumptions).

**Graceful degradation — source decision slot:** no on-disk source exists (conversation-synthesized PRD). Set the report header to `source-decision: author-supplied` and check wrong-problem against the artifact's own problem/summary statements plus this ask-summary: "I'd like to understand what are we testing and why our tests are time dependant - we should not test tmux-tools, it should test itself, most of our tests should test the logic, not external dependencies, the vast majority of tests should not have waits with real timers at all."

**This is round 3.** Round 2 found six findings; two were already-fixed staleness and four were adjudicated and applied. The applied decisions were: (1) `tests/` is **sufficient but not necessary** for the integration tier — the three existing out-of-crate targets (`tests/http_api.rs`, `tests/docs_catalog.rs`, `tests/bundled_templates.rs`) stay put and must not rely on `cfg(test)` shims, while in-crate members carry an explicit **cargo feature** marker (`#[ignore]` is unavailable, already used by the docs-regeneration writer); (2) the logic tier's rule is **determinism under load**, not absence of I/O, so deterministic committed-file readers stay in it and keep gating; (3) the six runtime functions take an **injected async sink** and do NOT become synchronous, because `emit_event`'s durable-append-then-broadcast ordering and failure propagation are load-bearing; (4) the general completion seam is the **runtime event stream**, since the drained-run token only fires at `clear` and the polling helpers also await intermediate states — HTTP-level tests stay in the integration tier with bounded waits; (5) Rust does **not** adopt `tokio` `test-util` / virtual time.

**Your task this round:** verify each of those five against the code rather than assuming the text is now correct. In particular: is a cargo feature actually a workable in-crate tier selector for `cargo test` given this repo's layout, and can the gating command express "logic tier only" with it? Does the runtime event stream in fact carry the intermediate states the polling helpers wait on (pending approval, successive queued approvals), and is it reachable by the tests that need it? Does an injected async sink actually preserve `emit_event`'s ordering and failure boundary at the six call sites? Then run the full six-class rubric fresh.

**Task:** Read the artifacts and referenced files. Explore the repo — grep, read modules, check ADRs and existing seams. For each rubric class, report findings with evidence (quote the artifact; cite repo paths/facts). Classes with no findings: state "none." Do not propose fixes — findings are questions for a human.

**Report contract:**

## Plan adversary report

- scale: epic
- source-decision: author-supplied
- artifacts: docs/prd/PRD-260902-0301-01-deterministic-test-suite.md

### Findings

For each finding:
- **class:** <rubric class name>
- **impact:** <one line — what breaks if ignored>
- **evidence:** <artifact excerpt + repo fact>

(omit section when zero findings — write "No findings.")

**Pass** = zero findings across all hunted classes.
