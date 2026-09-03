You are the plan adversary — read-only, sandboxed. Hunt wrongness in the planning artifacts below against the actual repo at $PROJECT_CWD. Do not modify any project file.

**Artifact paths:** docs/prd/PRD-260902-0301-01-deterministic-test-suite.md

**Referenced artifacts:** docs/adr/260902-0312-deterministic-test-tiers.md · CONTEXT.md (repo root) · docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md · docs/adr/260622-0208-tmux-bin-resolution.md · docs/testing.md · CLAUDE.md · justfile · .github/workflows/rust-tests.yml · Cargo.toml

**Scale:** epic

**Source decision:** author-supplied

**Wrongness rubric:** the six classes in PLAN-ADVERSARY.md (wrong problem · codebase-reality collision · missed simpler alternative · hidden coupling · sequencing errors · unjustified stack/dependency assumptions).

**Graceful degradation — source decision slot:** no on-disk source exists (conversation-synthesized PRD). Set the report header to `source-decision: author-supplied` and check wrong-problem against the artifact's own problem/summary statements plus this ask-summary: "I'd like to understand what are we testing and why our tests are time dependant - we should not test tmux-tools, it should test itself, most of our tests should test the logic, not external dependencies, the vast majority of tests should not have waits with real timers at all."

**This is round 2.** Round 1 found eight findings and all were adjudicated by the maintainer and applied. The five that changed the plan's shape were: (1) directory location cannot be the tier marker — `tests/` is a separate crate, `pub(crate)`/`#[cfg(test)]` items are unreachable, and `cfg!(test)` timeout shims flip to production values — so both tiers are now marked in-crate; (2) the abort select-arm does NOT dissolve the three `elapsed()` promptness assertions, since those paths carry their own cancellation arms — the claim was withdrawn; (3) the drained-token accessor was racy because `RunRegistry::clear` removes the entry before cancelling, so the seam is now handed out at registration or level-triggered; (4) acceptance now binds to the gate, which runs the logic tier only; (5) the four real-tmux tests assert SilverBond's own `SessionGuard`/`PaneGuard` RAII policy, so they are kept and repaired (dedicated socket, loud skip) rather than deleted. Three further corrections narrowed claims about `emit_event`'s durable-append semantics, the `with_invocation` thread-local scope across `await`, and an over-promising user story.

**Your task this round:** verify those revisions are actually correct against the code — do not assume the fix landed just because the text changed — and hunt for wrongness the revisions may have introduced. In particular: does marking tiers in-crate actually avoid the `cfg!(test)` problem, or does it merely relocate it? Does "the gating job runs the logic tier only" have a workable mechanism given `justfile` and `.github/workflows/rust-tests.yml` as they stand? Is a registration-time or level-triggered completion seam actually implementable against `RunRegistry` and `start_run` as written? Then run the full six-class rubric fresh.

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
