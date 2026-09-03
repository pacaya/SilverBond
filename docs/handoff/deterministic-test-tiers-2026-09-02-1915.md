# Handoff: deterministic-test-suite breakdown — PRD amended through five gate rounds plus a Codex review, twelve briefs ungated

> Prepared 2026-09-02 19:15 from a Claude Code session in `/Users/Shared/Data/work/Programming/SilverBond`,
> branch `feature/tmux-panes`, HEAD `70a878b` (unchanged — **nothing committed**).
> Audience: a fresh Claude instance with no memory of the prior conversation. Read top to bottom.
> **Supersedes `docs/handoff/deterministic-test-tiers-2026-09-02-1537.md`.** From that document, what
> still stands: its § "Conventions and gotchas observed", its § "User preferences and working style",
> and the `cold-reader` spawn shape in its § Reference snippets. Everything else in it is either done
> or overtaken — in particular its three "decisions made under a bare 'agreed'" were all confirmed by
> the maintainer at the start of this session, and its claim that eleven briefs exist is now twelve.

## TL;DR

The PRD went through **five** `cold-reader` gate rounds this session (all FAIL, each finding real
defects) plus one external adversarial review by **Codex**, which found a defect all five rounds had
missed. A twelfth brief was minted. Every finding from all six passes has been applied. **Nothing is
gated in its current form and nothing is committed.** The next action is to re-gate the PRD, then gate
all twelve briefs.

## Goal

Break `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md` into independently-grabbable, gated
`ISSUE-*` records. The epic fixes a twice-reproduced flake: the Rust suite fails intermittently under
CPU load because tests assert on wall-clock time and the test seam sits below the process boundary.

The authoritative, non-decaying input is the timing-site audit table under `### Timing-site audit` in
`docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md`. It is **contractual** and
outranks any count in PRD prose. Note its classification covers elapsed-time constructs; it does
**not** enumerate the scripted-delay fixtures (see `-12`).

**The maintainer's hard constraint: never invent a rule, tier assignment, or scope boundary to fill a
gap — ask.** Second constraint, stated this session: *a question put to them with no recommendation
attached is malformed* — supply a recommendation with its alternatives rather than asking it bare.

## Current state

Everything is untracked or modified, none of it committed. `git status --short` is the inventory.

| Artifact | State |
|---|---|
| `PRD-260902-0301-01` | Heavily amended. No `gate:` line by design; void blockquote above `## Dimension Scan`. Now carries a `## Open Questions` ledger with one entry, and `testing/seams` is `deferred`. **Last gated at round 5 (FAIL); substantially edited since.** |
| `ADR-260902-0312-01` | Amended this session — clock rule restated, selector clause, method count, `terms:` gained `Port`/`Double`. |
| `CONTEXT.md` | `Port` and `Double` minted in § Testing; Logic Tier entry's clock rule restated. |
| `ISSUE-260902-0747-01` … `-12` | **Twelve** briefs, all `status: needs-info`, all carrying a round-1 `FAIL` stamp. `-12` is new this session and has never been gated. All twelve edited since any gate ran. |
| `justfile:55` `test-under-load` | Still untracked in HEAD. **Recipe was fixed this session** — it now verifies its load workers with `kill -0` and fails rather than reporting a load it did not establish. `-01` commits it and has an AC on that property. |
| Readiness gate round 2 on the briefs | **Never started.** This is the main outstanding spend. |
| Step 7 (adversarial breakdown pass) | Not started. Blocked on the briefs being stamped. |

## What happened this session, in order

1. **Confirmed the three provisional decisions** the previous session had made under a bare "agreed".
   All three stand, with one substantive change: `-06`'s AC3 was **retracted and replaced**. The
   previous session's ordering assertion was vacuous — the supervisor's select carries an independent
   250ms sleep arm (`src/runtime.rs:3007`, `:3022`, `:3772`), so a blocked runner does not hold the
   tick open, and `abort_kills_active_panes_before_draining_running_tasks` already exercises that
   ordering at baseline. AC3 is now **structural**: the supervisor obtains the abort token and selects
   on it (`abort_signal` at `src/runtime.rs:1002` has no supervisor caller today). The retraction and
   its reasoning are recorded in `-06`'s `## Triage Notes` so it is not re-attempted.
2. **Five PRD gate rounds**, 10 → 4 → 3 → 3 → 4 findings. Every finding verified independently before
   being acted on; several agent claims were wrong and were corrected rather than applied.
3. **One Codex adversarial review** (`/codex-researcher`, `gpt-5.6-sol`, xhigh), filed at
   `docs/prd/adversary-reports/PRD-260902-0301-01-adversary-260902-codex-breakdown-report.md` with its
   prompt beside it. It found the Critical defect below.
4. **All findings applied.** One question was deliberately left open rather than answered.

## Decisions the maintainer made this session

Each was put to them in prose with a recommendation and alternatives, and each was approved.

1. **`-06`'s AC3 is structural, not behavioral** (option D of four). Poll and push differ only in
   latency, and latency is a clock property; no behavioral criterion can separate them without
   parameterizing the poll interval or pausing the clock, neither of which is authorized.
2. **The selector constraint is per-test, not per-target.** The old phrasing excluded out-of-crate
   targets wholesale, which would have excluded the gate's own contents. Fixed in the PRD *and* the
   ADR.
3. **The storage exemplar is the named integration subset**, not "the storage module", and
   `writer_progresses_while_a_read_connection_is_checked_out` is named as the excluded exception.
4. **"Component test" is not minted** — the phrase is gone; the substance is carried by "logic-tier".
5. **One named, temporary tier-marking exception** for the out-of-crate HTTP target: it stays unmarked
   and in the gate while it still violates the rule, because marking it would let the epic satisfy its
   acceptance by relocating the reproduced failure. `-01` records it, `-11` closes it, `-10` checks
   none survives. The maintainer's words: *"we need to hold on the spirit of the law, not the letter."*
6. **The logic tier's clock rule admits a narrow guard**, with the line drawn at **what ends the wait
   on the passing path**. Forbidden: sleeping to sequence work, asserting on elapsed time, and any
   wall-clock bound that is the termination condition of a polling or convergence wait. Permitted as a
   last resort: a bound wrapping an await a happens-before edge ends anyway, firing only on a hang.
   **Do not use the audit's A/B/C classification to draw this line** — `wait_for_run` is an A-site and
   *is* the reproduced failure. Stated in six places; sizing and the site comment are review-enforced,
   only the structural pair is machine-checkable.
7. **A twelfth slice, `-12`**, owns converting `ScriptedStep::with_delay` (32 sites, 14 tests) to a
   happens-before edge.
8. **`-05` promotes by the full tier predicate**, not "lost its clock wait and spawns no process".
9. **The calibrated abort test goes wholly to `-06`** — delay and assertion are one calibration.
10. **The HTTP fixture becomes approval-only** (see below).

## The Codex Critical finding — read this before touching `-11` or `-01`

**Five cold-reader rounds accepted a premise that was false.** The PRD's central amendment said HTTP
endpoint tests are logic tier and reaching that needs "no new production surface". The clock half was
checked repeatedly. The **isolation half was never checked**, and it fails:

```
tests/http_api.rs:117   test_router_with_security → RuntimeContext::new(db)
src/runtime.rs:1380     RuntimeContext::new installs the production TmuxNodeRunner
src/runtime.rs:1391     with_runner (takes a double) is #[cfg(test)] — unreachable out-of-crate
src/tmux_exec.rs:1035   an `echo` task node dispatches to run_agent_sequence
src/tmux_exec.rs:1378   → PaneGuard::acquire
src/tmux_exec.rs:439    → spawn_pane
src/tmux_exec.rs:2215   → tmux::run_checked_owned("new-session")
```

Those tests **spawn real tmux sessions**, and the three tests in the reproduction are exactly the ones
that do. Two consequences worth carrying forward: the producer their five-second deadline races is a
real process, not a descheduled task (so the PRD's original mechanism was understated); and the shared
fixture accepts `Completed | Failed | Aborted` and never asserts the echo ran, so a broken tmux
satisfies it.

**The fix, approved and applied:** an approval-only workflow. `NodeKind::Approval` is not a runner node
kind (`src/runtime.rs:2086-2098`), so such a run terminalizes on `POST /api/runs/{id}/approve` without
touching the runner — and `creates_and_approves_runs` (`tests/http_api.rs:1184`) already drives exactly
that shape over HTTP. No new production surface. `-11` owns the swap and has a positive control for it:
**with `tmux` absent from `PATH`, the whole target passes** — red today, and explicitly not satisfiable
by a fixture that merely tolerates a failed run.

If a future endpoint test genuinely needs task execution, it is Integration Tier, or it needs an
injectable runner port across the crate boundary — **new production surface, a maintainer decision, not
taken.**

## The one open question — do not answer it unilaterally

`validate_workflow_many_calls_against_large_subflow_within_budget` (`src/model.rs:6631`) asserts a
fifteen-second budget. The audit classifies it C (load-bearing). It isolates nothing external. So it
fails the Logic Tier rule *and* the Integration Tier's definition, and the tier model has no legal
state for it. Recorded as `[OPEN: perf-test-tier]` with a `## Open Questions` ledger entry listing
three candidate answers, `resolve-by: during-triage`; `-01` is where it comes due and its brief says to
bring it to the maintainer. **This is the artifact's first `deferred` scan row** — no previous gate
round has judged a table containing one.

## Dependency graph as it now stands

```
-01 ─┬─→ -02 (no edge; deliberately independent)
     ├─→ -04, -07, -08, -09
     ├─→ -05 ─┐
     ├─→ -06 ─┼─→ -12 ─┐
     └─→ -11 ─┴────────┴─→ -10   (-10 also blocked by -02, -04, -07, -08, -09)
```

`-10` is the join node and now owns the epic acceptance **over the final gate population**; `-11` keeps
its own run of the recipe, because it is the slice that removes the reproduced failure.

## Traps a fresh instance will otherwise fall into

- **The under-load recipe is a resilience check, not a red/green witness.** The failure is
  intermittent: three passing runs are not proof of a fix and three failing ones are not proof of a
  regression. The deterministic half is each slice's structural observables. Both the PRD and `-11`
  now say this; do not "helpfully" restore a stronger claim.
- **`cargo test -- --list` enumerates `#[ignore]`d tests** — never evidence that a selector excluded
  something. Proven: `tests/docs_catalog.rs:2705`.
- **Never grep a phrase that might wrap a line.** Three "component test" sites survived a sweep this
  session because the phrase spanned a newline. Use `rg -U` or normalize whitespace.
- **Cold readers correctly refuse to run `just test-under-load`** (it spawns `2 × nproc` `yes`
  processes and creates real tmux sessions on the user's default socket). Do not ask them to.
- **Verify agent claims before acting.** Across six review passes, wrong or misleading claims included:
  an A/C-classification split that would have blessed the reproduced failure; "three" API tests where
  all five write executable fixtures; a `Database` method count wrong twice (it is 21 — 18 `pub`, 2
  `pub(crate)`, 1 private, `src/storage.rs:154-888`); and a claim that `-10`'s premise was "wrong in
  both halves" when it was a true statement of an interim state.
- `## Triage Notes` is **context, not a delivery channel** — no consumer downstream of the pass stamp
  reads it. Binding constraints go in `## Agent Brief`.
- Gate stamp grammar and round numbering: `~/.claude/skills/triage/READINESS-GATE.md` § Gate stamp.
  `~/.claude/skills/to-spec/SPEC-GATE.md` owns the PRD-scale gate; `to-spec` is on disk but **not** in
  the session's invocable skill list, so follow its SKILL.md directly.

## Next actions

1. **Re-gate the PRD** (`SPEC-GATE.md` § Cold-reader procedure — one `cold-reader`, its definition
   already pins `model: opus` / `effort: xhigh`). Round 6. Give it: the rewritten HTTP paragraph
   (dimension 4/9), the rewritten accepted-cost paragraph (dimension 4), the new `## Open Questions`
   ledger and the first `deferred` scan row (dimension 9, marker law, reciprocity), and the round-5
   repairs. On pass, write `gate: passed <date>` into the frontmatter and delete the void blockquote.
2. **Readiness-gate all twelve briefs** (round 2 for `-01`…`-11`, round 1 for `-12`). Spawn shape is in
   the `-1537` handoff's § Reference snippets. This is the main unspent cost.
3. **Route findings** per `~/.claude/skills/to-issues/SKILL.md` step 6; stamp, then promote
   `needs-info` → `ready-for-agent`. **No record may carry `ready-for-agent` without a pass stamp.**
4. **Step 7** — one `/codex-researcher` adversarial pass at `scale: breakdown` over the approved set.
   Note one has already run over the *unapproved* set; its report is on disk and step 7 should be
   framed as a follow-up rather than a first look.
5. **Commit.** Per `~/.claude/skills/to-issues/SKILL.md` § "Commits follow gates" — not before.

## Reference — baselines re-verified 2026-09-02 19:15

Change-introducing observables, all red:

```
rg -n 'Logic Tier' docs/testing.md                        → 0
rg -n 'ADR-260902-0312-01' CLAUDE.md                      → 0
rg -n 'integration' .github/workflows/rust-tests.yml      → 0
rg -n '^test-integration' justfile                        → 0
rg -n 'testTimeout' ui/vite.config.ts                     → 0
rg -n 'guard_test_socket' src/tmux_exec.rs                → 0
rg -n 'fn poll_step' src/tmux_exec.rs                     → 0
rg -n 'Logic Tier' justfile                               → 0
git show HEAD:justfile | rg -n '^test-under-load'         → exit 1
rg -n 'abort_signal' src/runtime.rs                       → no supervisor caller (-06 AC3)
```

Present-at-baseline observables, all non-zero:

```
rg -c 'delay_ms' src/runtime.rs                                       → 8   (-12)
rg -c '\.with_delay\(' src/runtime.rs                                 → 32 across 14 tests (-12)
rg -c 'unwrap_or\(2\)' src/runtime.rs                                 → 2   (-09)
rg -c 'Duration::from_secs\(5\)' tests/http_api.rs                    → 1   (-11)
rg -c 'tokio::time::sleep' tests/http_api.rs                          → 3   (-11)
rg -c 'async fn wait_for_pending_approval' src/api.rs                 → 1   (-05)
rg -n --glob '*.test.ts' '\}, [0-9][0-9_]*\);?$' ui/src               → 1   (GraphEditor.test.ts:60)
rg -n -B4 '^\s+return;\s*$' src/ | rg 'available\(\)|geteuid|is_ok_and' → 6  (-04)
```

Load-bearing anchors (verify line numbers before relying on them):

```
src/api.rs:893        stream_run's unconditional list_events replay — what makes -11 race-free
src/api.rs:1060       resync_run_stream_from_journal — the RECOVERY path, NOT the replay
src/api.rs:5220       pane_stream_pump_honors_explicit_drain… — the permitted hang-only guard exemplar
src/runtime.rs:1002   abort_signal — callers are the decide/batch/escalation paths, no supervisor
src/runtime.rs:2086   is_runner_node_kind — Approval is absent, which is what -11's fix rests on
src/runtime.rs:2824   the supervisor's only abort check (top of loop)
src/runtime.rs:7587   ScriptedStep's real sleep (-12)
src/runtime.rs:7897   AbortBlockingRunner — the barrier exemplar
src/runtime.rs:9858   wait_for_run — the runtime-side 5s poll deadline (-05)
src/runtime.rs:11432  with_delay(2000), calibrated against the 500ms bound at :11472 (-06)
src/model.rs:6631     the performance test with no legal tier (OPEN: perf-test-tier)
tests/http_api.rs:1184 creates_and_approves_runs — the approval-only shape -11 adopts
```
