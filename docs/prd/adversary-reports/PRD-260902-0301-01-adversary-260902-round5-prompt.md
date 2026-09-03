You are the plan adversary — read-only, sandboxed. Hunt wrongness in the planning artifacts below against the actual repo at $PROJECT_CWD. Do not modify any project file.

**Artifact paths:** docs/prd/PRD-260902-0301-01-deterministic-test-suite.md

**Referenced artifacts:** docs/adr/260902-0312-deterministic-test-tiers.md · CONTEXT.md (repo root) · docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md · docs/adr/260622-0208-tmux-bin-resolution.md · docs/testing.md · CLAUDE.md · justfile · .github/workflows/rust-tests.yml · Cargo.toml

**Scale:** epic

**Source decision:** author-supplied

**Wrongness rubric:** the six classes in PLAN-ADVERSARY.md (wrong problem · codebase-reality collision · missed simpler alternative · hidden coupling · sequencing errors · unjustified stack/dependency assumptions).

**Graceful degradation — source decision slot:** no on-disk source exists (conversation-synthesized PRD). Set the report header to `source-decision: author-supplied` and check wrong-problem against the artifact's own problem/summary statements plus this ask-summary: "I'd like to understand what are we testing and why our tests are time dependant - we should not test tmux-tools, it should test itself, most of our tests should test the logic, not external dependencies, the vast majority of tests should not have waits with real timers at all."

**This is round 5.** Round 4's five findings were accepted in full, and the maintainer's conclusion was that the artifact was not churning on design but on *implementation mechanism specified inside a PRD*. A deliberate restatement pass therefore **withdrew** the mechanism commitments and replaced them with constraints the implementation must satisfy:

- The tier **selector** is no longer prescribed. The constraints are: the default command runs the logic tier alone; the exclusion must cover in-crate integration-tier tests (FIFO, storage-permission, process-group, tmux guards), not just out-of-crate targets; one opt-in switch adds the tier back; the switch must not collide with the `regen-docs` writer. The PRD explicitly records that target-level `required-features` alone cannot satisfy this.
- **Liveness comes from a happens-before edge**, not a wall-clock guard. The previous "durations never paid on the passing path are allowed in both tiers" allowance is withdrawn; a finite timeout is now a last resort and a recorded false-red source.
- Stamping a record with the clock is allowed, but **deriving identity** from it is not — a time-ordered id entering state needs an injected generator, or its path is not logic-tier eligible.
- The injected sink is **necessary but not sufficient** to remove the database; `apply_join_result`'s registry read and the context type carrying the database are named as the slice's own finding.
- A missing external binary must **never read as success**; the encoding is the implementation's choice, since Rust's harness has no dynamic skip.

**Your task this round:** hunt the six-class rubric fresh against this narrower artifact. Do not re-report the round-4 findings as if unaddressed — instead judge whether converting them to constraints actually resolved them or merely relocated the problem, and whether any constraint as written is unsatisfiable against this repo. Also check whether the PRD now under-specifies anything an implementer genuinely cannot proceed without.

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
