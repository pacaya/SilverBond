# Adversarial second opinion

## 1. Verdict on the diagnosis

**The pattern is real, but the proposed genus is too broad.** The precise recurring defect is:

> A current-tree membership or reachability population that affects implementation is written as an exhaustive prose set without an owned derivation or freshness boundary.

That diagnosis fits four of the five instances cleanly. The first instance — checking the clock half of a two-conjunct tier rule while missing isolation — is a closely related cognitive error, but it is an **incomplete predicate**, not literally a closed population. Calling all five the same genus is useful process shorthand; calling it a proven single cause is post-hoc compression.

| Instance | Independent read | Verdict |
|---|---|---|
| Clock half taken for the whole Logic Tier rule | The omission is real. `test_router_with_security` installs `RuntimeContext::new` (`tests/http_api.rs:103-121`), that constructor installs the production `TmuxNodeRunner` (`src/runtime.rs:1379-1388`), and a task is a runner node kind (`src/runtime.rs:2086-2098`). Removing a polling deadline alone therefore does not provide process isolation. The current PRD now says this explicitly (`docs/prd/PRD-260902-0301-01-deterministic-test-suite.md:264-282`). | Real defect, but not strictly a population-derivation defect. It is failure to apply the whole predicate. |
| One shared fixture taken for the HTTP isolation population | `create_terminal_echo_run` is called by only the tests at `tests/http_api.rs:272` and `:361` (`:185-216`, `:274`, `:363`). `run_control_routes_return_typed_client_errors` constructs its own task workflow and waits directly (`:563-608`). The preview endpoint is a separate unported process path (`tests/http_api.rs:889-920`; `src/api.rs:659-694`). The old brief claim that the shared fixture's callers were exactly the reproduction was false (`docs/issues/ISSUE-260902-0747-11-http-target-awaits-the-run-stream.md:42-52`). | Exact genus: an example was mistaken for an exhaustive population. |
| Two decision functions taken for the clock-identity reachability closure | `release_collectors_if_ready` reaches `new_cursor_id` through a function reference (`src/runtime.rs:6039-6044`). `handle_terminal_cursor_status` reaches that function (`:6226-6323`); `complete_subflow_if_at_exit` reaches the terminal handler (`:4819-4841`); `handle_approval_resolution` reaches both completion and terminal handling (`:5014-5015`, `:5064-5074`); and `apply_join_result` reaches completion and terminal handling (`:4542-4591`). Of the six named sink candidates, only `select_next_decision` has no such path. The brief really did attach a tier consequence to a two-function statement (`docs/issues/ISSUE-260902-0747-07-inject-event-sink-into-decision-functions.md:63-75`). | Exact genus: a non-local closure was asserted without deriving it. |
| Round 7's mint, abort, and tier-roster sets | The source supports the reported failures: runtime identity has run, cursor, frame, and split-family kinds (`src/runtime.rs:1408`, `:1564`, `:1913-1922`); the missed abort test makes an elapsed assertion at `src/runtime.rs:11059-11073`; and the PRD now correctly demotes its tier inventory to illustration (`docs/prd/PRD-260902-0301-01-deterministic-test-suite.md:605-613`). | Exact genus for the mint and abort populations and the roster. |
| Round 8's “run-stream tests” | The two tests at `tests/http_api.rs:272` and `:361` are stream tests. The third reproduced failure is named and written as a run-control test at `:434-640`; it exercises resume, restart, interaction, and abort routes and never opens the run stream. | Exact genus: a category label was used as though it denoted the full reproduced set. |

I therefore disagree with the strongest version of the diagnosis. This is not a homogeneous 5-for-5 experiment. The rounds are not independent, round 7 groups several populations, and most local repairs remained repaired while another site was later found. “5-for-5 failure rate” is rhetoric, not a meaningful rate. The evidence does establish a recurring authoring habit serious enough to change how populations are represented.

Round 8 itself also contains defects outside this genus:

- F3 is a **scope-authorization mismatch**. The PRD decides how missing tmux is represented (`docs/prd/PRD-260902-0301-01-deterministic-test-suite.md:451-459`), while `-04` expands that to every external-dependency guard (`docs/issues/ISSUE-260902-0747-04-repair-tmux-boundary-tests.md:18-19`, `:34-47`). A slice cannot acquire that scope merely because it derived more members.
- F5 is a **normative contract conflict**, not a stale population. CONTEXT forbids clock-derived identity without qualification (`CONTEXT.md:69`); the ADR limits the rule to state or event identity (`docs/adr/260902-0312-deterministic-test-tiers.md:34-38`); the PRD narrows state further to checkpoint state (`docs/prd/PRD-260902-0301-01-deterministic-test-suite.md:68-81`). The extra mints at `src/api.rs:681` and `src/tmux_exec.rs:1000,2597` make that difference concrete.
- F6 is a **wrong antecedent/citation**. `test_router_with_security` constructs state; it does not start the task workflow (`tests/http_api.rs:103-121`). `create_terminal_echo_run` does that (`:185-216`). A population disclaimer cannot tell an implementer which function to edit.

Those distinctions matter because a remedy aimed only at populations cannot repair scope, contract, or referent defects.

## 2. Verdict on (a), (b), and (c)

### (a) Keep patching instances — reject as the governing strategy

Individual corrections are still mandatory: F2, F5, and F6 cannot be waved away. But continuing an unbounded find-one/fix-one loop preserves the representation that keeps generating the errors: volatile source inventories embedded throughout a 779-line PRD. It also gives every new edit another chance to introduce a neighboring claim, as the round-7 rephrase did at PRD line 177.

Use instance patches to close known defects, not as the process for establishing completeness.

### (b) One normative sentence plus one sweep — reject as written

The anticipated objections are not merely cosmetic.

1. **“This document asserts no populations” is fatally self-undermining.** The document legitimately contains at least three different kinds of closed set:

   - A historical observation: run 1's three failing tests are an immutable fact about one recorded execution, not a live tree inventory (`docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md:24-38`).
   - A normative decision: exactly one opt-in switch is a chosen interface constraint (`docs/prd/PRD-260902-0301-01-deterministic-test-suite.md:157-167`). It should not “lose to” a later slice's derivation.
   - A curated contractual classification: the A/B/C table is explicitly judgment that a mechanical duration sweep cannot reconstruct (`docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md:86-107`).

   A document-wide denial of populations makes all three less trustworthy. This objection is fatal to (b)'s exact sentence, but survivable if the rule is scoped to **current-tree derived populations**.

2. **A blanket disclaimer does license wrong prose.** F6 is the clearest counterexample: calling the fixture description illustrative would not stop an implementer from converting `test_router_with_security` when the actual owner is `create_terminal_echo_run`. A false set should be deleted, generated, or explicitly labeled a dated observation; it should not remain wrong under a legal fiction. This objection is fatal to blanket subordination.

3. **It relocates the defect into the briefs unless the derivation has a shared source.** The readiness gate found unpinned counts or inventories in 8 of 12 records (`docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md:69-73`), and `-01` already carries the stale whole-target exception (`docs/issues/ISSUE-260902-0747-01-draw-tier-boundary.md:179-188`). Requiring each brief to reinvent a derivation is better than copying a count, but twelve independently written derivations can disagree just as twelve copied counts can. This objection is correct and fatal unless the briefs consume named, shared derivation artifacts.

The limited round-7 disclaimer at PRD lines 605-613 succeeded because it did something narrower and stronger than (b): it explicitly classified one paragraph as an illustration and named the authoritative operation that produces the roster. That supports typed/scoped population semantics, not a document-wide “nothing here is a population” rule.

### (c) Stop gating the PRD and spend the effort on briefs — reject now, adopt shortly

Stopping immediately is unsafe. F2 can move a reproduced failure out of the gate; F5 changes which paths need injected identity; F3 changes issue scope; and F6 can direct work at the wrong function. A readiness gate cannot reliably repair an upstream normative conflict: it can only expose that every child chose a different reading. The 8-of-12 class-7 result is evidence that the current briefs are not yet a firewall.

After one bounded PRD repair, however, most population verification should move to the owning briefs. The PRD should cease being re-gated for every stale count once its rules, ownership, and cross-slice constraints are coherent.

### My pick

**Pick a fourth option: a scoped version of (b), backed by shared derivation artifacts, followed by (c).** Do not adopt (b)'s blanket sentence.

## 3. Fourth option: typed populations plus executable query packs

Give every implementation-relevant set exactly one of four statuses:

1. **Observed snapshot** — closed at an event and cited with date/run/source. The three-test reproduction belongs here and remains exact.
2. **Normative set** — deliberately decided by the PRD or ADR. It remains authoritative until that decision is amended. “One opt-in switch” belongs here.
3. **Derived current-tree population** — owned by a named issue and produced from a named query or audit procedure at that issue's baseline. PRD prose may describe examples but must not state its membership or count.
4. **Illustration** — explicitly non-exhaustive and incapable of assigning work or acceptance.

The contractual A/B/C table is a special form of category 2: manually curated judgment. It should remain the authority because its own record explains why syntax cannot recover the classification (`docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md:92-105`).

For mechanically recoverable populations, check in a small static query pack and a generated manifest outside the PRD. Each manifest entry should record the query version, source revision, result, and owning issue. A freshness check should fail when the query output and manifest diverge. Examples include test attributes, `wait_for_run` callers, elapsed assertions, `Uuid::now_v7` sites, fixture writers, and marker membership. A single hand-maintained appendix with a freshness date is weaker: it centralizes the stale data but does not make staleness observable. A generated appendix is acceptable if the query is the source and freshness is checked.

Not every derivation is executable. Tier membership asks what the test's subject isolates; reachability can hide behind function references; and A/B/C is semantic judgment. Scripts should produce candidates and witnesses, not pretend to decide those questions. The owning brief should record the manual classification over the candidate set and the readiness gate should verify its required pin.

For the current work, I would do exactly this:

- Repair F1/F2/F3/F5/F6 as real semantic defects; delete or qualify F4's false supporting sentence rather than sheltering it under a disclaimer.
- Add a scoped rule saying only that **current-tree membership and reachability populations are never authoritative in PRD prose**; they are owned by a named derivation. Preserve observed and normative sets.
- Make the HTTP exception derive “tests in `tests/http_api.rs` whose current Logic Tier violations are owned by `-11`,” rather than naming “run-stream tests” or the whole target.
- Give each issue a required derivation pin, but have related issues consume the same query-pack entry rather than copying counts.
- Run one final, focused PRD gate over normative consistency, ownership, and the population-status rule. Then freeze the PRD and repair/readiness-gate the briefs.

## 4. Whether to keep gating

**Continuing the same full-prose spec-gate loop is no longer rational.** One bounded closeout round is rational; an open-ended ninth round is diminishing-returns perfectionism.

The gate was valuable: F2, F3, F5, and F6 can change the build or authorized scope. But the binary FAIL also gives F7's scan-row interpretation and F8's transient round-count prose the same procedural force as moving a reproduced failure out of the gate (`docs/prd/adversary-reports/PRD-260902-0301-01-spec-gate-260902-rounds7-8-report.md:25-34`). That is miscalibrated. “Does this change an implementer's allowed behavior, ownership, or acceptance?” should be the blocking line now.

The deeper problem is artifact shape. This PRD is carrying a PRD, a source audit, a tier roster, a call-graph analysis, and much of twelve implementation briefs. Its own inventory disclaimer admits that source rosters do not belong there (`docs/prd/PRD-260902-0301-01-deterministic-test-suite.md:605-613`). The earlier gate-basis records an additional ten-round completeness campaign and says gating was stopped because it was oscillating rather than converging (`docs/prd/adversary-reports/PRD-260902-0301-01-gate-basis-260902.md:22-37`). Eight more failures after reopening are strong evidence that the process has repeated the same convergence failure.

Do not break down immediately with F2/F5 unresolved. Do this closeout instead:

- Resolve every normative mismatch and build-changing finding, including `[OPEN: perf-test-tier]`, which presently leaves a test with no legal tier (`docs/prd/PRD-260902-0301-01-deterministic-test-suite.md:634-642`, `:708-721`).
- Remove or reclassify volatile PRD populations.
- Gate once against those explicit exit conditions.
- Proceed to brief repair even if a later reader can find another harmless stale illustrative example. Such an example should be corrected as documentation, not restart the epic's spec gate.

That is a real stop rule. “Continue until no quantifier can be challenged” is not.

## 5. F1 and F2 verification

### F2 — verified

The recorded reproduction is exactly the three functions listed at `docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md:29-33`.

- `streaming_routes_allow_same_origin_sse_and_require_ws_origin` is at `tests/http_api.rs:272` and calls the stream endpoint repeatedly (`:276-357`).
- `run_stream_requires_matching_stream_token` is at `tests/http_api.rs:361` and calls the stream endpoint (`:365-401`).
- `run_control_routes_return_typed_client_errors` is at `tests/http_api.rs:434`. It exercises `/resume`, `/restart-from`, `/respond-interaction`, and `/abort` (`:438-639`); its inline task run and terminal wait are at `:563-608`. It does not open the run stream.

Therefore “the **run-stream tests** stay unmarked” at `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md:169-179` excludes the third test in ordinary repository vocabulary and under the test's own name and behavior. The same error recurs in the claim that the reproduction “lives wholly in the run-stream tests” at PRD lines 303-307. F2 is not pedantry: because the marker is per test and unmarked means gated (`:204-208`), the wording permits the run-control reproduction to be marked out.

### F1 — substantially verified, but the gate overstates the number's universality

The source derivation is:

1. `wait_for_run` is the forbidden deadline-terminated convergence helper (`tests/http_api.rs:143-160`).
2. `create_terminal_echo_run` calls it (`:185-216`) and has exactly two test callers, at `:274` and `:363`. That yields two clock-violating tests.
3. `run_control_routes_return_typed_client_errors` calls `wait_for_run` directly twice (`:537-540`, `:602-608`). That yields a third.
4. `creates_and_approves_runs` sleeps unconditionally twice to sequence work (`:1184-1222`, `:1258`). The contractual table independently classifies those two constructs under that owner (`docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md:168-169`), and the Logic Tier rule forbids sleeping to sequence work regardless of the table's B classification (`docs/prd/PRD-260902-0301-01-deterministic-test-suite.md:52-61`). That yields a fourth HTTP test.
5. `validate_workflow_many_calls_against_large_subflow_within_budget` reads elapsed time and asserts it is below 15 seconds (`src/model.rs:6631-6652`). The PRD deliberately leaves it unmarked pending the open decision (`docs/prd/PRD-260902-0301-01-deterministic-test-suite.md:634-642`). That yields five.

So **five is correct for the PRD's intended interim state** if the preview task is marked Integration Tier and the four HTTP violations assigned to `-11` remain unmarked until `-11` repairs them. In that state, “Two tests are unmarked” at PRD lines 194-202 is false: it counts a multi-test HTTP set as one while insisting elsewhere that membership is per test.

But five is **not** currently an unconditional cross-artifact truth. The unrepaired `-01` AC says the entire HTTP target carries no Integration Tier marker (`docs/issues/ISSUE-260902-0747-01-draw-tier-boundary.md:179-188`). Read literally, that also leaves `test_node_accepts_v3_task_node` unmarked, even though it executes the preview task (`tests/http_api.rs:889-920`) and the PRD has decided it is Integration Tier (`docs/prd/PRD-260902-0301-01-deterministic-test-suite.md:290-309`). Under the brief as written, the count is therefore at least six. Round 7 already identified that stale whole-target criterion, but it remains unapplied in the brief.

My verdict on F1 is thus: **the “two” is definitely wrong; the source-derived intended population is five; the artifact set as presently written has no single coherent count.** Repair the semantics and derivation, not merely `Two` → `Five`.

## 6. A valuable defect the gate rounds missed

**Epic acceptance is bound to the wrong point in the dependency graph.**

The PRD calls `just test-under-load` the epic acceptance and makes it the closing criterion of `-11` (`docs/prd/PRD-260902-0301-01-deterministic-test-suite.md:648-663`); it repeats that acceptance lands at the HTTP repair (`:675-679`). But `-11` is blocked only by `-01` (`docs/issues/ISSUE-260902-0747-11-http-target-awaits-the-run-stream.md:10`). It can therefore pass before `-05`, `-06`, `-07`, `-08`, `-09`, and `-12` promote additional tests into the Logic Tier.

The `-10` brief has already noticed the consequence, although none of the consolidated gate findings names the upstream contradiction: `-10` is the join node blocked by every Rust slice and says it is the first point where the final default-command population is settled (`docs/issues/ISSUE-260902-0747-10-enforce-tier-rule.md:107-115`; dependency list at `:10`). A load run at `-11` can prove the original HTTP reproduction was repaired and still say nothing about clock-sensitive tests promoted later.

The PRD should distinguish two acceptances:

- `-11`: regression acceptance for the original reproduced HTTP failure, with its deterministic structural companions.
- `-10`: **epic closing acceptance** over the final Logic Tier population, plus one Integration Tier run.

Without that change, the implementation can satisfy the PRD's named epic acceptance before the epic has assembled the gate it actually ships. That is more consequential than another stale prose count.

A smaller residue reinforces the same need for a final semantic sweep: `-11` still says the failed run was “merely descheduled” (`docs/issues/ISSUE-260902-0747-11-http-target-awaits-the-run-stream.md:18-32`) and later correctly says the producer was a real process (`:42-52`). Round 6 repaired this in the PRD, but the brief retained both mechanisms.

---

This review was static only. I did not run Cargo, npm, or any test command.
