# Handoff: deterministic-test-suite — round-8 findings and three maintainer decisions applied; thirteen briefs repaired; re-gate wave is the open work

> Prepared 2026-09-02 22:41 from a Claude Code session in `/Users/Shared/Data/work/Programming/SilverBond`,
> branch `feature/tmux-panes`, HEAD `70a878b` (unchanged — **nothing committed**).
> Audience: a fresh Claude instance with no memory of the prior conversation. Read top to bottom.
> **Supersedes `docs/handoff/deterministic-test-tiers-2026-09-02-2142.md`.** From that document,
> what is now stale: its § "Current state" table (every row), its § "Open questions" (both closed),
> its § "Next actions" items 1–4 (done). What still stands: its § "Conventions and gotchas observed"
> in full, its § "User preferences and working style", and its § Reference fact list — every entry
> there was re-verified or used this session and none was falsified.
> The `-1419` handoff's § "User preferences" remains the fullest statement.

## TL;DR

The open question from the previous handoff is answered and applied. Round 8's six findings, three
maintainer decisions (vacuous-skip scope, clock-identity quantifier, `perf-test-tier`), and a scoped
population rule are all in the PRD/ADR/CONTEXT.md; all twelve briefs are repaired and a thirteenth was
created by splitting `-05`. **Nothing is committed and nothing is re-gated.** The single next action is
the re-gate wave: PRD round 9, `-01`…`-10` round 3, `-11`/`-12` round 2, `-13` round 1.

## Goal

Break `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md` into independently-grabbable, gated
`ISSUE-*` records. The epic fixes a twice-reproduced flake: the Rust suite fails intermittently under
CPU load because tests assert on wall-clock time and the test seam sits below the process boundary.

Authoritative non-decaying input: the `### Timing-site audit` **classification** table in
`docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md`. Contractual; outranks any
prose count. The rest of that record is evidence, not contract.

**Maintainer's hard constraints, both still in force:**
1. Never invent a rule, tier assignment, or scope boundary to fill a gap — ask.
2. A question put to them with no recommendation attached is malformed.

**New constraint stated this session, and it governs all further writing:**
> "we write instructions to an agent what to do, not a log of our findings or list of arguments to win"

Briefs and PRD prose are instructions to the implementing agent. Not evidence trails, not round
histories, not counts quoted to justify a claim. Where a set matters, tell the agent to derive it.
Several passages were rewritten this session for exactly this — if you find prose arguing with an
earlier draft of itself, that is the defect.

## Current state

| Artifact | State |
|---|---|
| `PRD-260902-0301-01` | **Repaired, ungated.** Round 8's F1–F6 applied; F7 disposed of (scan row now `decided`); F8 gone with the blockquote rewrite. Three decisions applied. § Open Questions is `None.` No `gate:` line. **Awaiting round 9.** |
| `ADR-260902-0312-01` | Retitled `Test tiers: …` (was `Two test tiers: …`); third-category section added; dependency-absence consequence widened to the genus; `terms:` gained `Performance Check`. `docs/adr/INDEX.md` line updated to match. Not separately gated. |
| `CONTEXT.md` | Clock-identity clause now "state or event identity". **`Performance Check` minted** at line 73, `_(planned — ADR-260902-0312-01)_`. |
| `ISSUE-260902-0747-01` … `-12` | **All twelve repaired.** Each carries a `### Round 2 repairs applied 2026-09-02` (or `### Post-PASS edit`) note saying what changed. All `status: needs-info`. None `ready-for-agent`. |
| `ISSUE-260902-0747-13` | **New.** `run-completion-signal`. Created by splitting `-05`. Never gated — `**Readiness gate:** not yet run`. |
| `-03`, `-08` | Carry `**Readiness gate: REOPENED**` after their round-2 `PASS`, per P3 Atomicity. Both were edited only to add the baseline sentence. |
| Dependency graph | Verified programmatically: 13 records, **no dangling blockers, no cycles**. `-10` is the join node behind all ten Rust slices plus `-13`. |
| Commit | Not done, and blocked: `~/.claude/skills/to-issues/SKILL.md` § "Commits follow gates". |

### Reports on disk — read these rather than reconstructing

- `docs/prd/adversary-reports/PRD-260902-0301-01-spec-gate-260902-rounds7-8-report.md` — round 8's F1–F8, **now all applied or disposed**
- `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md` — the twelve-brief round, all findings now applied
- `docs/prd/adversary-reports/PRD-260902-0301-01-spec-gate-260902-round6-report.md`
- `docs/prd/adversary-reports/PRD-260902-0301-01-adversary-260902-codex-remedy-report.md` — the Codex second opinion that reshaped decisions 1 and 6, with its prompt beside it as `…-codex-remedy-prompt.md`. Moved out of `/tmp` into the repo 2026-09-02 22:47.

## Key decisions and rationale

All four below were put to the maintainer in prose with a recommendation and alternatives, and
approved (`agreed on all`, then `yes, thank you. Let's do it!`).

1. **Scoped population rule, not a blanket one.** § Implementation Decisions now opens with "How to
   read a set named in this document": four kinds — decision, observed snapshot, current-tree
   population, illustration — of which only the first two bind.
   *Why:* the recurring defect was closed sets asserted where derivations were owed, five instances
   across five rounds, the fifth inside the fix for the fourth. A per-instance remedy has a
   demonstrated failure rate.
   *Alternative rejected:* the blanket "this document asserts no populations" sentence originally
   recommended. Codex showed it fatally self-undermining — the reproduction's three tests are an
   immutable fact about a recorded event, and "one opt-in switch" is a normative decision; a blanket
   denial makes both less trustworthy. It also licenses wrong prose: calling the fixture paragraph
   "illustrative" would not have stopped an implementer editing `test_router_with_security` when the
   owner is `create_terminal_echo_run`.
2. **Vacuous-skip scope authorized upstream, brief not narrowed.** The PRD constraint now reads "a
   missing external dependency must not read as success" over the genus.
   *Why:* the encoding is one decision (Rust has no dynamic skip), and `-04` had already generalised.
   *Alternative rejected:* cut `-04` back to tmux and file a follow-up — cleaner process, but ships
   the defect twice and makes the follow-up re-derive the same decision.
3. **Clock-identity quantifier = the ADR's "state or event identity"**, propagated to CONTEXT.md and
   the PRD.
   *Why:* CONTEXT.md's unqualified form forbids benign transient mints with no defect behind them;
   the PRD's "checkpoint state" was narrower than both siblings and excluded ids entering run/event
   state. The ADR owns the decision and sat in the middle.
4. **`perf-test-tier` closed: a third non-gating category, and the test splits.**
   `validate_workflow_many_calls_against_large_subflow_within_budget` (`src/model.rs:6631`) asserts
   *two* properties — a 15s budget and "a large valid workflow yields no error issues". Only the
   timing half moves; the correctness half stays in the gate.
   *Why:* broadening the Integration Tier to mean "non-gating" would spend the isolation rule the
   whole epic rests on; moving to a benchmark converts a failing test into a number someone reads,
   deleting the guard the `C` classification says is load-bearing.
   *Cost accepted:* `-01`'s selector and `-10`'s check must express three categories, not two.
   *The split was not in any gate finding* — it came from reading the test source this session.
5. **Term minted: `Performance Check`,** not "Performance Tier".
   *Why:* tier membership is decided by what a test *isolates*; this category is decided by what a
   test *asserts*. Calling it a tier re-imports the confusion the decision exists to avoid. Its
   `_Avoid_` list carries both "benchmark" and "performance tier".
6. **Acceptance split at two points.** `-11` carries **regression** acceptance for the reproduced
   failure; `-10` carries **closing** acceptance over the final gate population.
   *Why:* `-11` is `blocked_by: [ISSUE-260902-0747-01]` only, while `-10` is blocked by all ten Rust
   slices — so a load run at `-11` passes before seven later slices promote tests into the gate. The
   PRD had called the recipe "the closing acceptance criterion of `-11`". `-10:107-115` already had
   this right; the PRD contradicted its own child. **Found by Codex, verified here.**
7. **`-05` splits; `-13` is sequenced *before* it.**
   *Why:* two production surfaces (drained-token lifetime; race-free event-stream acquisition), two
   seams, two demoable halves. `-05`'s only cohesion argument was the false "terminal waits are the
   minority" claim.
   *Ordering, and this is the non-obvious part:* `wait_for_terminal_run` (`src/runtime.rs:9878`) is
   implemented **on top of** `wait_for_run`, which `-05` deletes. Doing `-05` first would leave the
   terminal wait without an implementation. So `-13` (completion) precedes `-05` (event stream), and
   `-05` gained `blocked_by: [-01, -13]`.
8. **Stop rule adopted for the PRD gate.** A finding blocks only if it changes an implementer's
   allowed behavior, the ownership of a slice, or an acceptance criterion; stale-but-harmless prose
   is corrected as documentation and does not fail the round. Written into the void blockquote above
   § Dimension Scan.
   *Why:* eight rounds, and `…-gate-basis-260902.md:22-37` records a prior ten-round campaign stopped
   for oscillating rather than converging. The old binary FAIL gave F8's round-count typo the same
   procedural force as F2 relocating a reproduced failure out of the gate.

### What was NOT adopted from Codex

- **Executable "query packs"** — a checked-in script + generated manifest with a freshness check.
  *Why not:* new tooling on the critical path of a flake fix, and Codex itself concedes tier
  membership, reachability-through-references, and the A/B/C classification are not mechanically
  decidable — which is most of what is contested. Recorded here in case it is wanted later.
- **Codex's "5-for-5 is rhetoric, not a rate"** — fair as statistics, irrelevant as decision input.
  Independence was never claimed; the operative fact, which Codex does not dispute, is that instance 5
  landed inside the fix for instance 4, written by an author who knew the pattern.

## Files touched this session

| File | Change |
|---|---|
| `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md` | Population rule added at head of § Implementation Decisions; F1 (count→kinds+derivation), F2 (both sites: exception + "lives wholly in"), F4 (open set), F6 (fixture referent split); vacuous-skip widened to genus; clock quantifier; perf decision + split; § Open Questions → `None.`; scan row `testing/seams` → `decided`; acceptance split + reconciled with "Acceptance does not land at the tier split"; void blockquote → gating instruction; two round-history passages de-narrated; `issues:` gained `-13`; `terms:` gained `Performance Check` |
| `docs/adr/260902-0312-deterministic-test-tiers.md` | H1 retitled; § "A third, non-gating category holds performance assertions" added after Integration Tier; dependency-absence consequence widened; `terms:` updated |
| `docs/adr/INDEX.md:10` | Title line matched to the ADR |
| `CONTEXT.md:69` | Clock-identity clause → "state or event identity" |
| `CONTEXT.md:73` | **New term** `Performance Check` |
| `docs/issues/ISSUE-260902-0747-01-*.md` | Perf paragraph → Performance Check + split instruction; exception restated per-test with derivation; AC1 exception clause; AC2 rewritten + **exception-entry form pinned** (for `-10`'s control); three unassigned test sets named in body (host-startup, self-exec pair with `--exact` hazard, `writer_progresses_…`); selector gains third-category constraint; two new ACs; baseline; out-of-scope acceptance binding |
| `-02` | Third fixture form (module-local); discovery command widened to `from_mode`; AC1 restated structurally (fixes AC1/AC2 contradiction); directory-permission tests excluded; out-of-scope opened; AC4 stated as coverage; scale count removed; baseline |
| `-03` | Baseline; `REOPENED` stamp |
| `-04` | AC6 → non-pass outcome + acted-on complement; scope note citing upstream authorization; baseline; split-decision recorded |
| `-05` | **Split.** Completion half removed; `blocked_by` gains `-13`; summary/AC narrowed to three helpers; false "minority" claim removed; false "every pending-approval caller writes a fixture" premise replaced with per-caller derivation + promotion AC; baseline |
| `-06` | Elapsed-set → two-pass derivation; `abort_kills_active_panes_…` (`src/runtime.rs:11070`) explicitly **in scope**; AC2 accommodates the third `TMUX_CLEANUP` role (`:7950`); AC3 witness by owning function; baseline |
| `-07` | Clock-identity closure → derivation with both hazards named; new AC requiring the derivation be recorded; quantifier aligned; baseline |
| `-09` | Discovery switched to `retry_count`; **citation-guard hazard costed** with both remedies authorized + new AC; AC2 no longer contradicts Desired behavior; baseline |
| `-10` | Clean-run ACs paired with violating controls in the same invocation (kills the report-nothing hole); closing-acceptance AC gains the intermittency limit; exception-control depends on `-01`'s pinned form; check scope covers three categories; `blocked_by` gains `-13`; baseline |
| `-11` | Fourth spawn site (`test_node`) named in body as Integration Tier, not converted; inline duplicate at `tests/http_api.rs:434` named; AC5 control → committed mechanical variant scoped to Logic Tier members; four-term pairing no longer claims uniform red baseline; `test-under-load` restated as regression acceptance; "merely descheduled" corrected; baseline |
| `-12` | "sleep gone ⇒ Logic Tier" shorthand removed; `blocked_by` gains `-13`; excluded test's four `with_delay` calls named; counts → attribution to owning tests; baseline |
| `docs/issues/ISSUE-260902-0747-13-run-completion-signal.md` | **New record** |

## Dead ends and traps hit this session

- **`Edit` failed on a hard-wrapped `old_string`.** Matching `…quantifies over **every clock-derived identifier that\nenters checkpoint state…` returned not-found; matching the single line `enters checkpoint state or event identity**, …` worked. This is the ~100-column wrap trap **both** predecessor handoffs warn about and it still bit. Prefer single-line anchors, or `python3` with an exact triple-quoted block.
- **`tmux-tools kill` refused the pane it had just spawned:** `Error: refusing to kill pane %1151 owned by another cwd (/private/tmp/codex-research); pass --any to override`. Because `codex-researcher` spawns with `--cwd /tmp/codex-research`, teardown of a research pane **always** needs `--any`. The skill's own § 8 does not mention this.
- **`tmux list-panes -a | grep -i codex` reported "no codex panes" while `%1151` was still alive** — it greps the command name, and the pane had already dropped back to a shell. Use `-F '#{pane_id} #{pane_current_command} #{pane_title}'`.
- **A foreign pane `%1138` (command `codex`, title `claude`) exists and is not ours.** Left alone. Do not reap it without asking.
- **Round-numbering slip:** wrote "Awaiting round 3" into `-12`, corrected to round 2 — `-11` and `-12` were gated for the *first* time in the last batch. Check `docs/prd/adversary-reports/…-briefs-round2.md` § header before stamping any round number.

## Conventions and gotchas observed

Everything in `-2142` § "Conventions and gotchas observed" still holds — in particular that
**`rg -c` prints per-file counts, not a total**, which is the origin of at least two wrong figures now
removed from `-02` and `-12`. New this session:

- **The citation guard covers exactly one file.** `tests/docs_catalog.rs` `assert_workflow_schema_citations_fresh` reads only `WORKFLOW_SCHEMA_DOC`. PRD/ADR/brief `file:line` citations are **not** guarded and can be added freely. `-09` is the one record whose *code* change can trip the guard.
- **Verify agent claims before acting, still.** Codex's headline finding (acceptance binding) was verified against `-11`'s frontmatter and `-10:107-115` before anything was edited. The pending-approval premise in `-05` was re-derived from source rather than taken from the gate — it came back exactly as the gate stated (14 callers, 5 with fixtures, 9 clean).
- **Do not run `cargo test`, `just test-under-load`, or anything touching `tests/http_api.rs`.** They spawn real tmux on the user's default socket. Static reading and `rg` only; Codex was instructed the same way and complied.

## User preferences and working style

`-1419`'s section remains the fullest statement. Confirmed and extended this session:

- **Artifacts are instructions, not arguments.** Stated explicitly (quoted in § Goal). They pushed back on prose that recites findings or numbers counter-arguments. This applies to briefs and PRD prose; conversational replies to *them* still want rationale and honest alternatives.
- They answer with a bare **`agreed`** / **`agreed on all`**, which attaches to the stated recommendations and authorizes the plan.
- They authorize spend in units and want a report at the boundary before the next tranche. They asked for the tranche to be scoped before it started.
- They asked for the Codex second opinion **unprompted**, and it changed the outcome — the blanket population rule became a scoped one, and the acceptance-binding defect surfaced. Treat "get a second opinion" as a live and productive move here, not ceremony.
- They want honest attribution of mistakes, including the assistant's own.

## Open questions

None blocking. One judgement call the next session will meet:

1. How should the re-gate wave be scoped — one batch of fifteen spawns, or PRD round 9 first and the briefs only after it passes? The maintainer asked to scope tranches before starting; put this to them with a recommendation. (Recommendation: PRD round 9 alone first — every brief cites it, and a round-9 finding would invalidate brief work done in parallel.)

## Next actions

1. **Ask the maintainer how to scope the re-gate wave** (open question 1), with the recommendation attached.
2. **PRD round 9** — one `cold-reader`, `model: opus`, effort xhigh. Gate it against the **new exit conditions** in the void blockquote, not against "no quantifier can be challenged". Keep round 8's spawn-prompt instruction to sweep set-asserting sentences mechanically; that is what found F2.
3. **Brief re-gates:** `-01`…`-10` → round 3; `-11`, `-12` → round 2; `-13` → round 1. Note READINESS-GATE § "Round escalation (D4)" makes the round *after* an authoritative round 3 a full-enumeration round, so `-01`…`-10` hit that at round 4 — round 3 is the last cheap one.
4. **Step 7** — one `/codex-researcher` adversarial pass at `scale: breakdown` over the approved set, framed as a follow-up (one already ran over the unapproved set; its report is on disk).
5. **Commit**, per `~/.claude/skills/to-issues/SKILL.md` § "Commits follow gates" — not before.

## Reference — verified this session

```
tests/http_api.rs:103   test_router_with_security      — builds state; does NOT start the run
tests/http_api.rs:143   wait_for_run                   — the forbidden convergence helper
tests/http_api.rs:185   create_terminal_echo_run       — the fixture that starts the echo task run
tests/http_api.rs:272   streaming_routes_allow_same_origin_sse_and_require_ws_origin   } the
tests/http_api.rs:361   run_stream_requires_matching_stream_token                      } reproduction's
tests/http_api.rs:434   run_control_routes_return_typed_client_errors  ← run-CONTROL   } three tests
tests/http_api.rs:889   test_node_accepts_v3_task_node — Integration Tier; 4th spawn site, no port
tests/http_api.rs:1184  creates_and_approves_runs      — unconditional 50ms sleeps at :1222, :1258
src/runtime.rs:8994     wait_for_registry_empty        — 2s deadline / 20ms poll        → -13
src/runtime.rs:9878     wait_for_terminal_run          — implemented ON wait_for_run    → -13
src/runtime.rs:11070    started.elapsed() < TMUX_CLEANUP_TEST_TIMEOUT — 2nd reproduction → -06
src/runtime.rs:7950     third TMUX_CLEANUP_TEST_TIMEOUT role, inside AbortBlockingRunner
src/model.rs:6631       validate_workflow_many_calls_… — asserts BOTH budget and no-error-issues
tests/docs_catalog.rs:1820-1831  check_citation → DiscriminantOutsideSymbol  (-09's hazard)
docs/workflow-schema.md:1073     the citation -09 can break
```

Re-derived from source this session (the one figure worth trusting; still re-derive rather than cite):

```
wait_for_pending_approval callers in src/api.rs: 14 total
  5 write a fixture / set a mode / bind a socket:
      run_observability_prefers_single_registered_session_over_tmux_lookup
      run_observability_bounds_tmux_session_lookup_concurrency
      pane_context_uses_persisted_invocation_without_running_resolver
      pane_death_before_live_bytes_sends_pane_stream_unavailable_frame
      pane_writer_that_never_starts_sends_pane_stream_unavailable_frame
  9 clean → promotable by -05 once their wait is gone
```

Graph, verified programmatically (13 records, no dangling edges, no cycles):

```
01 <- (root)          06 <- 01              11 <- 01
02 <- (root)          07 <- 01              12 <- 01,05,06,13
03 <- (root)          08 <- 01,02           13 <- 01
04 <- 01              09 <- 01
05 <- 01,13           10 <- 01,02,04,05,06,07,08,09,11,12,13
```

Spend this session: **1 `codex-researcher` pane** (`gpt-5.6-sol`, effort xhigh, 10m23s, ~221k tokens,
`SESSION_ID: 01a064f5-b0fb-74d3-a980-d835b91a7f65` — resumable). No `cold-reader` spawns; all editing
done in the main session.
