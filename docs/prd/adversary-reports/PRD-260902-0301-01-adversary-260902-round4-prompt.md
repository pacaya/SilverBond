You are the plan adversary — read-only, sandboxed. Hunt wrongness in the planning artifacts below against the actual repo at $PROJECT_CWD. Do not modify any project file.

**Artifact paths:** docs/prd/PRD-260902-0301-01-deterministic-test-suite.md

**Referenced artifacts:** docs/adr/260902-0312-deterministic-test-tiers.md · CONTEXT.md (repo root) · docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md · docs/adr/260622-0208-tmux-bin-resolution.md · docs/testing.md · CLAUDE.md · justfile · .github/workflows/rust-tests.yml · Cargo.toml

**Scale:** epic

**Source decision:** author-supplied

**Wrongness rubric:** the six classes in PLAN-ADVERSARY.md (wrong problem · codebase-reality collision · missed simpler alternative · hidden coupling · sequencing errors · unjustified stack/dependency assumptions).

**Graceful degradation — source decision slot:** no on-disk source exists (conversation-synthesized PRD). Set the report header to `source-decision: author-supplied` and check wrong-problem against the artifact's own problem/summary statements plus this ask-summary: "I'd like to understand what are we testing and why our tests are time dependant - we should not test tmux-tools, it should test itself, most of our tests should test the logic, not external dependencies, the vast majority of tests should not have waits with real timers at all."

**This is round 4.** Round 3 produced five adjudicated decisions, all applied: (1) tier membership is decided by **what a test touches, never by location** — the "`tests/` is integration by construction" clause is deleted, since the generated-catalog and shipped-template checks live under `tests/` yet must stay in the gate; (2) the clock rule is narrowed to **no waiting on the clock and no elapsed-time assertions**, with stamping a record explicitly allowed, because the decision functions write `now_iso()` timestamps; (3) a duration **never paid on the passing path** is a liveness guard allowed in **both** tiers; (4) the tier selector is a **cargo feature at target level** (`required-features` on explicitly declared `[[test]]` targets), because a function-level gate would not stop an auto-discovered target under a bare `cargo test`; (5) the `tmux-tools` tokio unification is moved **out of scope** to `ISSUE-260902-0445-01`, since this epic's acceptance never observes it.

**Your task this round:** verify each against the code and the repo rather than trusting the text. Specifically: does the proposed `[[test]]` + `required-features` arrangement actually yield "bare `cargo test` runs the logic tier including `docs_catalog` and `bundled_templates`, and `--features integration-tests` adds `http_api`" given this package's layout? Does allowing clock-stamping actually make the six named functions (`select_next_decision`, `apply_join_result`, `release_collectors_if_ready`, `complete_subflow_if_at_exit`, `handle_approval_resolution`, `handle_terminal_cursor_status`) logic-tier eligible, or does something else in them still read the clock or mint time-based identifiers? Is `start_run` handing back a receiver actually implementable against its current signature and call sites, including the HTTP create-run path? Then run the full six-class rubric fresh.

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
