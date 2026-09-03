## Plan adversary report

- scale: epic
- source-decision: author-supplied
- artifacts: docs/prd/PRD-260902-0301-01-deterministic-test-suite.md

### Findings

#### Wrong problem

none.

#### Codebase-reality collision

- **class:** codebase-reality collision
- **impact:** The implementation sweep has no authoritative site-by-site input, so it can weaken a load-bearing assertion or leave a load-fragile wait behind while still claiming to follow the preserved audit.
- **evidence:** The PRD says `ISSUE-260901-0216-03` “holds the reproduction, the per-site classification, and the commands that re-derive each figure,” repeats that “the full per-site classification” lives there, and makes preserving that command-backed audit a user story. The referenced issue's `### Timing-site audit` contains only aggregate A/B/C totals and two discovery-command snippets; it contains no mapping from each timing site to A, B, or C and no command that reproduces the attribution/classification. This missing mapping is load-bearing against the actual repo: `TMUX_CLEANUP_TEST_TIMEOUT` is used both as a liveness bound and as an elapsed-time assertion in `src/runtime.rs`, while `ScriptedStep::with_delay(...)` values encode ordering, so a mechanical duration sweep cannot reconstruct the categories. Which on-disk artifact is the promised per-site authority an implementation slice is meant to consume?

- **class:** codebase-reality collision
- **impact:** Epic completion has no reproducible pass/fail procedure, so different implementers can reach opposite acceptance verdicts by choosing different meanings of “heavy” and “repeated.”
- **evidence:** Acceptance is “measured the way the defect was reproduced” and requires identical results “under heavy CPU contention and on an idle machine, across repeated runs.” The referenced issue records that one observation used 16 CPU-bound processes on an 8-core machine and three suite runs, but it does not record the load-generator command, its lifecycle, or an acceptance repetition rule. No stress/load recipe or workflow exists in `justfile`, `scripts/`, or `.github/workflows/`; the only checked-in Rust commands remain bare `cargo test --locked`. What exact observable run constitutes acceptance after the rewrite removed the executable protocol from the PRD?

- **class:** codebase-reality collision
- **impact:** The frontend slice can satisfy the written decision with a no-op and leave the same timeout failure mode intact.
- **evidence:** The PRD says the frontend “gets an explicit test timeout,” identifies Vitest's current implicit value as 5000 ms, but specifies neither a different value nor a behavioral criterion for choosing one. `ui/vite.config.ts` currently has no `testTimeout`, and the locked Vitest 4.0.18 implementation resolves an unset non-browser timeout to 5000 ms. Adding `testTimeout: 5000` would therefore satisfy “explicit” byte-for-byte while changing no behavior. What behavior-changing timeout contract is the frontend implementation actually required to meet?

#### Missed simpler alternative

none.

#### Hidden coupling

none.

#### Sequencing errors

none.

#### Unjustified stack/dependency assumptions

none.
