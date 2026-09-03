You are running in read-only-against-project mode. Sandbox details:
- Project under review: /Users/Shared/Data/work/Programming/SilverBond — read via absolute paths only; the sandbox will reject any write here with "Operation not permitted" and you should NOT retry.
- Scratch dir for any output files: /tmp/codex-research — this is your only writable location.
- You may use `git -C /Users/Shared/Data/work/Programming/SilverBond status|diff|log|show|ls-files` for context; mutating git ops will fail and that's expected.

Task:

# Adversarial second opinion: a PRD that keeps failing its spec gate on one recurring defect genus

## Situation

A Rust + Svelte project (SilverBond) has an in-repo "harness" of documentation records: PRDs, ADRs,
ISSUE briefs, and adversarial gate reports. A PRD is being broken down into independently-grabbable
ISSUE records. Before that can happen, the PRD must pass a "spec gate" (an adversarial cold-reader
review) and each brief must pass a "readiness gate".

The PRD is `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md`. It has now failed **eight**
spec-gate rounds. Rounds 6 and 7 were applied in full. Round 8's findings are unapplied — that is the
open work.

The epic behind the PRD is real and narrow: the Rust test suite flakes intermittently under CPU load
because tests assert on wall-clock time and the test seam sits below the process boundary. The fix is
to draw a tier boundary (Logic Tier = clock-free and process-free; Integration Tier = explicitly
marked, allowed to do both) and to move or repair the offending tests.

## The recurring defect genus

Across five of the eight rounds, the gate has found the **same class** of defect:

> **The PRD asserts a closed set where a derivation is owed.**

Instances, in order:
1. Rounds 1–5 — the "clock half" of the tier rule was taken for the whole rule.
2. Briefs round — one fixture was taken for the only isolation violation (brief `-11`).
3. Briefs round — two functions named where the reachability closure is actually five (brief `-07`).
4. Round 7 — one clock-derived identity mint named where the runtime has four; four abort-path
   elapsed-assertions named where a sweep finds five; a tier inventory read as a complete roster.
5. Round 8 — **inside the fix written for instance 4.** Round 7 rephrased "the target stays unmarked"
   to "the **run-stream tests** stay unmarked". That replaced a too-large set with a too-small one:
   the epic's reproduction has three failing tests, and one of them
   (`run_control_routes_return_typed_client_errors`, `tests/http_api.rs:434`) is a run-**control**
   test, not a run-stream test. Under the PRD's own marking rule it would leave the gate — which is
   exactly what the exception clause exists to prevent.

So: a per-instance remedy has a 5-for-5 failure rate, including the time it was applied deliberately
by an author who knew about the pattern.

## The three candidate remedies (decide among these, or propose a better one)

**(a) Keep patching instances.** Fix each closed set as the gate finds it. Cheapest per round.

**(b) One normative sentence + one sweep.** Add a rule near the top of the PRD saying: the document
asserts no populations; every set is derived by the slice that acts on it; any population named in
prose is illustrative and loses to the rule. Then sweep round 8's own 100-sentence ledger once and
subordinate the six defective entries to that rule. Evidence offered for this working: round 7 added
exactly this disclaimer to *one* section (the tier inventory), and round 8 deliberately tried to
falsify it and could not. Cost: downstream ISSUE briefs can no longer lift counts out of the PRD
prose — each must carry its own derivation pin. (The readiness gate's class 7 already demands such a
pin, so this may be work the briefs owe regardless.)

**(c) Stop gating the PRD's prose; spend the effort on the briefs instead**, where the readiness gate
forces derivation per record. Fastest route to `ready-for-agent`. Risk: wrong sets propagate — brief
`-01` has *already* inherited a false "two unmarked tests" count from the PRD.

## What I want from you

Be adversarial. I am not looking for validation of option (b), which is the current recommendation.

1. **Attack the diagnosis before the remedy.** Is "a closed set asserted where a derivation is owed"
   actually one genus, or is it a post-hoc label stitched over several unrelated defects? Read the
   five instances against the artifacts and say whether the pattern is real. If it is not, say so
   plainly — that changes everything downstream.

2. **Attack option (b) specifically.** The strongest objections I can anticipate, which you should
   test rather than repeat: (i) a document-wide "this document asserts no populations" rule may be
   self-undermining, because the PRD *does* legitimately decide some populations (the reproduction's
   three failing tests, for instance, is a fact about a specific observed failure, not an open
   derivation); (ii) a blanket disclaimer may be a licence to keep writing wrong sets, since the rule
   absorbs the error instead of the author fixing it; (iii) it may simply relocate the defect into the
   briefs rather than eliminating it. Are these fatal, survivable, or wrong?

3. **Is there a fourth option I have not considered?** Specifically consider: making the derivations
   *executable* (a checked-in script or test that derives the populations from the tree, so prose
   counts are generated rather than asserted); or restructuring the PRD so that populations live in
   exactly one appendix with a freshness stamp; or something else.

4. **Does the count of eight failing rounds itself indicate a different problem** — that the gate is
   miscalibrated, or the PRD is trying to carry more than a PRD should, or the breakdown should have
   proceeded already? Give an honest read on whether continuing to gate is rational or is
   diminishing-returns perfectionism. I want a real opinion here, not both-sides framing.

5. **Verify or falsify these two specific round-8 findings**, which I intend to fix under any option:
   - **F2**: PRD line 177 says "The **run-stream tests** therefore stay unmarked and in the gate while
     they violate…". The reproduction's three tests are at `tests/http_api.rs:272`
     (`streaming_routes_allow_same_origin_sse_and_require_ws_origin`), `:361`
     (`run_stream_requires_matching_stream_token`), and `:434`
     (`run_control_routes_return_typed_client_errors`). Does the phrase exclude the third?
   - **F1**: PRD line ~194 says "Two tests are unmarked while failing the Logic Tier rule". The gate
     claims the true population is five, the fifth being `creates_and_approves_runs`
     (`tests/http_api.rs:1184`) with unconditional 50ms sleeps at `:1222` and `:1258`. Is the gate's
     count right? Derive it yourself; do not take my word or the gate's.

## Files to read (absolute paths)

- `/Users/Shared/Data/work/Programming/SilverBond/docs/prd/PRD-260902-0301-01-deterministic-test-suite.md`
  — the PRD under gate (779 lines). Read all of it.
- `/Users/Shared/Data/work/Programming/SilverBond/docs/prd/adversary-reports/PRD-260902-0301-01-spec-gate-260902-rounds7-8-report.md`
  — rounds 7 and 8. Round 8 is the open work.
- `/Users/Shared/Data/work/Programming/SilverBond/docs/prd/adversary-reports/PRD-260902-0301-01-spec-gate-260902-round6-report.md`
- `/Users/Shared/Data/work/Programming/SilverBond/docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`
  — the twelve-brief gate (2 PASS, 10 FAIL).
- `/Users/Shared/Data/work/Programming/SilverBond/docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md`
  — the diagnostic record. Its `### Timing-site audit` **classification table** is contractual and
  outranks any prose count anywhere, including in the PRD. The rest of that record is evidence, not
  contract.
- `/Users/Shared/Data/work/Programming/SilverBond/docs/adr/260902-0312-deterministic-test-tiers.md`
- `/Users/Shared/Data/work/Programming/SilverBond/docs/issues/ISSUE-260902-0747-01-draw-tier-boundary.md`
  … through `-12-*.md` in the same directory — the twelve briefs. Skim; read `-01`, `-07`, `-11` closely.
- `/Users/Shared/Data/work/Programming/SilverBond/docs/handoff/deterministic-test-tiers-2026-09-02-2142.md`
  — session handoff with the decision history. Treat its claims as claims, not facts.
- Source, for verification: `/Users/Shared/Data/work/Programming/SilverBond/tests/http_api.rs`,
  `src/runtime.rs`, `src/api.rs`, `src/host.rs`, `src/tmux_exec.rs`, `src/proc.rs`.

**Do not run the test suite, cargo, or npm.** Several tests spawn real tmux on the user's default
socket. Static reading and `rg`/`grep` only.

## Output

Write your full reply as markdown to `/tmp/codex-research/codex-response-EDA3F9CF-1788400164.md`.
Reply in the pane only with `DONE: /tmp/codex-research/codex-response-EDA3F9CF-1788400164.md`.

Structure it as: (1) verdict on the diagnosis; (2) verdict on each of (a)/(b)/(c) with your pick and
why; (3) any fourth option; (4) your read on whether to keep gating; (5) F1/F2 verification with the
evidence you derived yourself; (6) anything the gate rounds have all missed — the most valuable thing
you can give me is a defect nobody has named yet. Cite file:line throughout. Where you disagree with
me, say so directly.
