# Handoff: `/to-issues` over PRD-260902-0301-01 — breakdown written, gate failed 10/10, tier rule changed mid-repair

> Prepared 2026-09-02 14:19 from a Claude Code session in `/Users/Shared/Data/work/Programming/SilverBond`, branch `feature/tmux-panes`, HEAD `70a878b`.
> Audience: a fresh Claude instance with no memory of the prior conversation. Read top to bottom.
> First handoff for this strand. (`docs/handoff/docs-truth-to-issues-2026-08-26-0425.md` is an unrelated strand.)

## TL;DR

`/to-issues` was run over `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md`. Ten `ISSUE-260902-0747-*` records were written (step 5) and the readiness gate (step 6) was run on all ten in parallel — **all ten returned FAIL**. No gate stamps were written; no briefs were repaired. Mid-repair the maintainer challenged the tier rule the whole breakdown rests on, and the rule was changed: `docs/adr/260902-0312-deterministic-test-tiers.md` and `CONTEXT.md` **have already been amended**; the PRD **has not**. **Next action: make the four agreed PRD edits listed under "Next actions", then re-gate.**

The gate reports exist **only in the prior conversation** — they were never written to disk. The compressed findings in this document are the sole surviving record of them.

## Goal

Break the deterministic-test-suite epic into independently-grabbable, gated `ISSUE-*` records under `docs/issues/`. The epic itself fixes a real, twice-reproduced flake: the Rust suite fails intermittently under CPU load because tests assert on wall-clock time and the test seam sits below the process boundary. Diagnostic evidence and the contractual per-site timing classification live in `docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md` § *Timing-site audit* — that table is the sweep's authoritative input and does **not** decay.

Constraint the maintainer enforces: no rule, tier assignment, or scope boundary gets invented to fill a gap. Ask instead. See "User preferences".

## Current state

| Artifact | State |
|---|---|
| `docs/issues/ISSUE-260902-0747-01..10-*.md` | **Written, untracked, ungated.** All carry `status: ready-for-agent` with **no gate stamp behind it** — that is wrong and must be reconciled (either stamp or downgrade). All ten need rewriting against the new tier rule regardless. |
| `docs/adr/260902-0312-deterministic-test-tiers.md` | **Amended this session** (untracked). Tier rules rewritten to the isolation criterion; new `## Doubles are fakes, and every fake names its port` section at line ~100. |
| `CONTEXT.md` | **Amended this session** (` M`). Both tier glossary entries rewritten; `_Avoid_: unit tests` deleted, replaced by `_Avoid_: solitary test`. |
| `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md` | **Not amended.** Still carries `gate: passed 2026-09-02` and the *old* rule. Its `issues:` list was updated with all ten IDs. Four edits agreed, unmade. |
| `justfile` | ` M` — carries a `test-under-load` recipe at line 55 that is **not in HEAD** (`git show HEAD:justfile \| rg test-under-load` → exit 1). Six issue records consume it; none creates it. Unowned. |
| Step 7 (adversarial breakdown pass) | **Not started.** Blocked on step 6 stamping every brief. |

Nothing is committed. Nothing is stamped.

## Key decisions and rationale

- **Tier membership is decided by what a test *isolates*, not by what it touches.** Maintainer's criterion: test logic with dependencies isolated (ports faked and/or controlled data) → Logic Tier; test whole workflows against real infrastructure → Integration Tier.
  - *Why:* the previous ADR rule ("determinism under load, **not** the absence of I/O") conflated the property the gate needs with the design rule for where seams go, and it licensed an invented extrapolation (see Dead ends).
  - *Alternatives considered:* keeping determinism-under-load as the gate admission rule with isolation as a separate design rule — rejected by the maintainer in favour of one rule.

- **"Controlled data counts as isolation."** A dependency the test fully owns and constructs per-test — a temp-dir SQLite database, a read-only committed file — is Logic Tier even with no fake.
  - *Why:* it is the maintainer's own phrasing ("isolating … all dependencies (ports) with doubles **and/or controlled data**") and it keeps the first-party invariant checks in the gate. **Consequence: the practical tier assignments barely moved** — the ~85 runtime tests using temp-dir `Database` stay in the gate once their clock waits go.

- **Doubles means *fakes*** — alternative implementations behind the same interface (dictionary/list-backed), not mocks. Verify state by reading back through the fake; never assert call mechanics. Every double names the port it is the second adapter for.
  - *Why:* matches `~/.claude/skills/tdd/mocking.md` verbatim. `NodeRunner` already has 8 implementations in `src/runtime.rs:7560-8012` — the pattern is in-tree.

- **A dictionary-backed `Database` fake is considered and deferred**, recorded in the ADR as a live option.
  - *Why:* `Database` exposes ~28 public methods, so the fake is a maintenance surface whose drift from real SQLite semantics — notably the durable sequence `emit_event` assigns, whose ordering is load-bearing for `-07` — fails green not red; and the measured cost of the real store is *sleeping*, not I/O.
  - *Correction recorded in the ADR:* the PRD rejected **in-memory SQLite** (`:memory:`), a *different* proposal. Its objections (pool gives each checkout a private DB; connection manager not file-agnostic) do **not** transfer to a fake.

- **`tests/http_api.rs` becomes Logic Tier**, not Integration.
  - *Why:* an endpoint test with controlled data and no clock is exactly a component test. And it is mechanically possible: `resync_run_stream_from_journal` (`src/api.rs:1060-1073`) replays from `db.list_events(run_id)` through a `SeqFilter`, so the run stream is **journal-backed, not live-only** — a test can start a run, open the stream afterwards, and still receive every event without racing. No new production surface needed.
  - *Consequence:* **the epic's story inverts.** The reproduced failure gets fixed in place rather than relocated out of the gate. This retires the concern that slice `-01` satisfies the epic's acceptance by emptying the gate.

- **Tier names `Logic Tier` / `Integration Tier` are kept.** *Why:* renaming is a sweep across ten briefs + PRD + ADR + CONTEXT.md; maintainer judged the names "make sense, also maybe not the most descriptive".

- **Ten slices, two tiers, two CI jobs, two-state selector.** Maintainer explicitly declined a third tier ("I don't need them to be three - we can stay at two").

## Files touched

| File | Change |
|---|---|
| `docs/issues/ISSUE-260902-0747-01-draw-tier-boundary.md` | New. Selector + tier marking + CI split + tier docs. Largest slice. |
| `docs/issues/ISSUE-260902-0747-02-consolidate-fake-process-fixtures.md` | New. Fake-process fixtures behind named helpers. |
| `docs/issues/ISSUE-260902-0747-03-frontend-generous-test-timeout.md` | New. vitest `testTimeout`. |
| `docs/issues/ISSUE-260902-0747-04-repair-tmux-boundary-tests.md` | New. Dedicated socket + honest absence encoding + non-resolving constructor. |
| `docs/issues/ISSUE-260902-0747-05-run-lifecycle-event-handle.md` | New. Pre-spawn run handle + level-triggered completion signal. |
| `docs/issues/ISSUE-260902-0747-06-abort-observable-structurally.md` | New. Abort select arm + 3 promptness assertions + constant polarity split. |
| `docs/issues/ISSUE-260902-0747-07-inject-event-sink-into-decision-functions.md` | New. Injected async event sink for six runtime functions. |
| `docs/issues/ISSUE-260902-0747-08-extract-interactive-poll-step.md` | New. `poll_step` extraction in `src/tmux_exec.rs`. |
| `docs/issues/ISSUE-260902-0747-09-residual-sleeps-and-latent-retry.md` | New. HTTP sleeps + latent 2s retry shim. |
| `docs/issues/ISSUE-260902-0747-10-enforce-tier-rule.md` | New. Mechanical check or advisory. |
| `docs/adr/260902-0312-deterministic-test-tiers.md` | Amended: Logic/Integration rule paragraphs; membership heading → "what a test isolates"; new fakes/ports section; Consequences updated. |
| `CONTEXT.md:69-70` | Amended: both tier entries rewritten to the isolation rule; anti-unit-test clause removed. |
| `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md` | Only `issues: []` → the ten IDs. **Body still carries the old rule.** |

## Gate findings — all ten FAIL, round 1 (residue: exists nowhere else)

Spawned as `cold-reader`, `model: "opus"`, one per record, prompt carrying record path + referenced artifacts + `~/.claude/skills/triage/READINESS-GATE.md`.

**Systematic defects spanning the batch** (fix in one sweep, not per record):

1. **Count-as-polarity.** Pre-change polarity written as a count beside a discovery command — `"It returns four test call sites as well before this change"`. `AGENT-BRIEF.md` § *Qualitative polarity vs decaying state* requires qualitative phrasing, never a count. Present in `-02`, `-04`, `-06`, `-07`, `-08`, `-09`. Correct form already used in `-01`, `-03`, `-05`, `-10`.
2. **Broken listing template.** `"appears in the Logic Tier listing (cargo test --locked -- --list) after this change and did not before it"` is false wherever a test is already deterministic. Confirmed false in `-07` (two tests already call `select_next_decision` directly with `TempDir` + `EchoRunner`) and `-08` (`poll_loop_pattern_scan_uses_capture_coordinates_on_redraw` is already pure). Present in `-05`–`-09`.
3. **`cargo test -- --list` includes `#[ignore]`d tests** — proven by `regeneration_writes_to_disk` appearing. Any `#[ignore]`-shaped selector makes every listing-based criterion in the batch unfalsifiable. Constrains `-01`'s selector choice.
4. **Templated `just test-under-load` closing AC** in `-05`, `-06`, `-07`, `-09`, `-10` is already green at each record's declared baseline (post-`-01`); only the unexecutable qualifier carries content.

**Per record:**

- **`-01`** — worst. I wrote the membership rule as an **if-and-only-if** the ADR never states, and asserted *"real per-test SQLite round-trips … stays in the Logic Tier"* which appeared in **no** artifact. Also: the brief never cites the `ISSUE-260901-0216-03` timing-audit table (it sat only in `## Triage Notes`, which is not delivered); **no AC checks the marking at all**; Summary ("a tier decision recorded at every Rust test") contradicts Key interfaces ("readable at the test") — a ~518-annotation difference; `just test-under-load` not in VCS; AC3 cites the PRD as *naming tests* when it names module behaviors. Class 6 did **not** fire.
- **`-02`** — `rg -n '0o755' src/` is not authoritative for "every inline fake-process fixture": over-includes 6 helper-body lines, misses 3 shapes — (B) inline script delegating chmod to `make_executable` (`src/runtime.rs:10080`, `:10093`), (C) `sh -c` inline strings (`src/proc.rs:179`, `:212`), (D) self-exec child (`src/model.rs:5242`, `:5319`). Moving `make_executable` turns both ACs green while shape-B fixtures survive. Hole is **also open upstream** — the PRD says only "*Most* fake-process script fixtures are written inline".
- **`-03`** — `"the one recorded frontend flake"` is an uncommanded occurrence count, and **falsified**: `docs/issues/code-reviews/issue-260826-0520-01-code-review-20260831-021621.md:87-104` records a second frontend timeout at **~8.2–8.5s** under load, plus a second load failure of the same `InspectorPanel.test.ts` unlock test. An existing per-test `}, 20_000)` override sits at `ui/src/features/editor/GraphEditor.test.ts:60`, unmentioned. Sizing at >6160ms could land under an already-recorded duration.
- **`-04`** — AC6 (socket residue) names no observation seam and is vacuously green at baseline (no dedicated socket exists), so it cannot distinguish "cleaned up" from "never ran". Orphaned siblings with the identical vacuous-skip defect and no owner: `zsh_available()` at `src/tmux_exec.rs:3924` / early return `:3875`, and a euid/sudo self-skip at `src/api.rs:4826-4835`. **Empirically confirmed**: all four guard tests report `ok` in <10ms with tmux off `PATH`. AC5's failure-signal requirement **passed**.
- **`-05`** — class 6 prong (b). AC4 is satisfiable by level-triggering alone with **zero** entry-point signature changes; AC3 needs no completion signal. Helper set splits: completion signal serves `wait_for_terminal_run` (~42 of ~79 measured call sites), event handle serves the other four. Also: "terminal waits … the minority of the polling sites" is false at this record's scope (42/79 is a majority). Recommended split.
- **`-06`** — AC3 has **no reachable test seam**: supervisor poll ticks are inline `Duration::from_millis(250)` literals (`src/runtime.rs:3007`, `:3022`, `:3772`), virtual clock is out of scope, so a structural assertion needs a poll-interval parameter the brief never authorizes. AC5's promotion condition ("spawning no process") is not the tier rule — `parallel_batch_abort_cancels_pending_items_after_next_completion` sleeps via `with_delay(2000)` (`src/runtime.rs:7587-7588`). A **fourth** C-classified elapsed assertion is unclaimed: `abort_and_wait_force_clears_when_drain_never_arrives`, `src/runtime.rs:8108-8122`, 2s bound. AC1 anchors to the literal `from_millis(500)` — re-sizing to 750 turns it green with none of the work done.
- **`-07`** — AC6 unfalsifiable (two tests already listed; `--list` prints names, so nothing identifies which "call these decisions directly"). AC2's failure signal is generic — `assert!(result.is_err())` satisfies it, and *any* earlier error also leaves the log unmutated. AC4/AC5 end in "recorded" with no destination. My unconditional *Tier outcome* sentence contradicts branch 2 of the clock-identity fork in the same brief. **Useful positive finding: the six decision functions never touch `ctx.db` directly** — only `emit_event` plus one `ctx.registry.is_aborted` in `apply_join_result`. Injecting the sink is sufficient; no storage fake needed.
- **`-08`** — `poll_loop_pattern_scan_uses_capture_coordinates_on_redraw` (`src/tmux_exec.rs:4803-4838`) is **already pure** — calls only `next_unhandled_pattern_match`, `extract_after_prompt`, `common_prefix_len`; no fixture, no clock, no audit-table row. The set is four, not five. AC3's "Every interactive poll loop" is unpinned: three loops exist (`:1551`, `:1651`, `:1792`), and `query_agent_command` has different semantics.
- **`-09`** — class 6 prong (b). Two crates, two tiers, two independently-green AC subsets `{AC1,AC2}` / `{AC3–AC6}`, bound only by "no other home". Recommended split. (Note: `-09`'s premise that the HTTP test "stays in the Integration Tier" is now **stale** given the HTTP decision.)
- **`-10`** — class 9 arm B fires on AC2 (no positive control; "reports a violation" names no unique signal). The scripted-delay clause is vacuous: those fixtures sleep, so they are Integration Tier and a tier-scoped check never sees them. Advisory branch is self-certifying. No seam echo on any criterion. **`blocked_by` is wrong** — `-04` and `-09` also promote tests and are not listed.

## Dead ends — things tried that did not work

- **Inventing the tier rule.** I wrote a biconditional membership rule and a "real SQLite stays in the Logic Tier" clause, attached ADR citations to both, and neither claim was in the ADR. The `-01` cold-reader caught both by opening the target. *Insight:* the ADR's "determinism under load, **not** the absence of I/O" phrasing is what made the extrapolation feel licensed — it invites the reader to extend the list. The maintainer's response was direct: *"why do you invent rules instead of asking me?"*
- **Framing the epic as "acceptance is met by slice 1".** I opened the breakdown arguing that drawing the boundary honestly makes `just test-under-load` go green, because the reproduced failure was in the HTTP target which the PRD excluded from the gate. That framing is **now retired** by the HTTP-target decision — the failure stays in the gate and must actually be fixed.
- **Telling the user "all ten gate rounds are in" when nine were.** Miscounted; `-04` was still running. The decision batch presented at that point was incomplete.
- **Reading `-02`'s scope from a single grep.** `rg '0o755'` felt like an obvious authoritative command for "fake-process fixtures" and is authoritative for neither end of that set.

## Conventions and gotchas observed

- **The display filter mangles the literal phrase "unit test"** in tool output — it renders as `n` / `ns`. Don't be confused by `"Frontend vitest ns"`. Search with alternative patterns if the exact phrase matters.
- `docs/issues/` is **flat** — no `context:` subdirectories, and existing records carry no `context:` frontmatter field. The ten new records follow that convention.
- `docs/testing.md` is **stale and known-wrong**: claims the HTTP suite "start[s] a real `ApplicationHost` on an ephemeral port"; it uses `tower::ServiceExt::oneshot` in-process. Owned by the docs-truth family, explicitly out of scope for this epic.
- `test_router_with_db()` already exists at `tests/http_api.rs:95` and returns the `Database` handle — `-09`'s fix is a helper swap, not new plumbing.
- The `cfg!(test)` timeout shims are exactly two: `src/api.rs:70` (`pane_stream_owner_wait_timeout`) and `src/runtime.rs:44` (`abort_and_wait_drain_timeout`). Both are module-private `fn`, so out-of-crate targets structurally cannot reach them.
- Cold-readers correctly refuse to run `just test-under-load` (spawns `2 × nproc` `yes` processes and creates real tmux sessions on the user's default socket). Don't ask them to.

## User preferences and working style

- **Never invent a rule to fill a gap — ask.** This is the strongest signal in the session and the one thing they pushed back on explicitly.
- Wants decisions surfaced conversationally in prose with facts, alternatives and honest pros/cons **before** committing — not funnelled into `AskUserQuestion`. Reasons iteratively: proposes an option, asks for objective critique, refines.
- Wants genuine critical pushback, including on their own proposals. When I pointed out their criterion contradicted their own accepted ADR, that was the right move and they engaged with it.
- Replies are short and decisive once the option space is clear ("agreed", "We can keep, they make sense").
- Grounds everything in evidence — cite `file:line`, run the command, don't recall.

## Open questions

Seven decisions are outstanding. All were put to the maintainer; none answered.

1. **`-09` split** into two records (HTTP sleeps / retry shim)? *Recommended: yes.*
2. **`-05` split** into two records? *Recommended: yes — cut line is now in-process handle for in-crate tests, stream-based awaits for the HTTP target.*
3. **`-01` marking extent** — does every test carry an explicit tier marker (~518), or only Integration-tier members with Logic as the unmarked default? *Recommended: Integration-only.* If "every test", splitting `-01` becomes defensible.
4. **`-03` sizing basis** — size the vitest timeout against the affected test's 6160ms, or the slowest frontend duration recorded anywhere (~8.2–8.5s)? *Recommended: the latter.*
5. **`-02` fixture shapes** — shape B in scope, shapes C and D explicitly out? *Recommended: yes.*
6. **`-06` abort seam** — authorize parameterizing the supervisor poll interval so AC3 is assertable, or drop AC3 and let the select arm stand on latency merits alone?
7. **`-04`** — is AC6 an automated fixture assertion or a manual reviewer check (*recommended: automated, dedicated socket only, drop the default-socket half*)? And do the orphaned `zsh`/sudo vacuous skips get a follow-up record (an 11th) or stay unowned?

## Next actions

1. Make the four agreed PRD edits to `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md`:
   a. § Implementation Decisions — delete *"Tests that reach the runtime only over HTTP … remain in the integration tier with bounded waits"*; replace with HTTP endpoint tests as Logic Tier component tests awaiting the run stream.
   b. § Testing Decisions, "Modules tested, by tier" — drop the out-of-crate HTTP target from the Integration list; make the storage entry per-test (migration + file-permission assertions stay Integration; ~24 temp-dir tests do not).
   c. § Solution — restate the Logic Tier rule as isolation with controlled data, matching the amended ADR.
   d. § Implementation Decisions, "The database stays real" — add the rider that the rejection was of `:memory:` specifically, and that a repository fake is separately deferred per the ADR.
2. Downgrade `gate:` in the PRD frontmatter and re-gate it (`/to-spec`), since (a)–(d) change Implementation Decisions.
3. Get answers to the seven open questions above.
4. Rewrite all ten briefs: apply the four systematic fixes, the per-record findings, and the new tier rule. Reconcile `status: ready-for-agent` (no record should carry it without a stamp).
5. Give `just test-under-load` an owner — add it to `-01`'s scope and commit it, or it stays unowned while six records depend on it.
6. Re-run the readiness gate (round 2) on every brief; stamp per `~/.claude/skills/triage/READINESS-GATE.md`.
7. Only then run step 7 — one `/codex-researcher` adversarial pass at `scale: breakdown` over the whole approved set.

## Reference snippets

Baseline polarity of every acceptance command, verified red before the gate ran (re-verify after any rewrite):

```
rg -n 'Logic Tier' docs/testing.md                      → 0
rg -n 'ADR-260902-0312-01' CLAUDE.md                    → 0
rg -n 'integration' .github/workflows/rust-tests.yml    → 0
rg -n '^test-integration' justfile                      → 0
rg -n 'testTimeout' ui/vite.config.ts                   → 0
rg -n 'guard_test_socket' src/tmux_exec.rs              → 0
rg -n 'fn poll_step' src/tmux_exec.rs                   → 0
rg -c '0o755' src/api.rs src/runtime.rs src/tmux_exec.rs      → 26 / 15 / 21
rg -c 'if !tmux_available\(\)' src/tmux_exec.rs               → 4
rg -c 'build_tmux_invocation\(' src/tmux_exec.rs src/api.rs   → 5 / 2
rg -c 'async fn wait_for_(run|terminal_run|event|registry_empty)\b' src/runtime.rs → 4
rg -c 'async fn wait_for_pending_approval' src/api.rs         → 1
rg -c 'Duration::from_millis\(500\)' src/runtime.rs           → 3
rg -o 'TMUX_CLEANUP[A-Z_]*' src/runtime.rs | sort -u          → TMUX_CLEANUP_TEST_TIMEOUT (one name, two polarities)
rg -c 'from_millis\(50\)' tests/http_api.rs                   → 2
rg -c 'unwrap_or\(2\)' src/runtime.rs                         → 2
```

Cold-reader spawn shape that produced the ten reports:

```
Agent(subagent_type: "cold-reader", model: "opus")
  prompt: record path + referenced artifacts (PRD, ADR(s), ISSUE-260901-0216-03, CONTEXT.md,
          sibling records for cross-reference only)
        + "Rubric (authoritative): ~/.claude/skills/triage/READINESS-GATE.md"
        + "Authoring rules: ~/.claude/skills/triage/AGENT-BRIEF.md"
        + per-record steer at the classes most at risk
        + "Return per-class verdicts, commands executed with output, one overall PASS/FAIL.
           Do not propose edits; report only."
```
