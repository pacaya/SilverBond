# Handoff: deterministic-test-suite breakdown — PRD amended, all 11 briefs rewritten, re-gate not yet run

> Prepared 2026-09-02 15:37 from a Claude Code session in `/Users/Shared/Data/work/Programming/SilverBond`, branch `feature/tmux-panes`, HEAD `70a878b` (unchanged — nothing committed).
> Audience: a fresh Claude instance with no memory of the prior conversation. Read top to bottom.
> **Supersedes `docs/handoff/deterministic-test-tiers-2026-09-02-1419.md`.** Every "Next action" in that document is now done except its items 6–7 (re-gate, then the adversarial pass). Its most important obsolete claim: it said the round-1 gate reports "exist **only** in the prior conversation." They are now on disk, in each record's `## Triage Notes` — do **not** treat that predecessor as the surviving record of them. Its seven open questions are all answered; three more were raised and answered in this session. What still stands from it: the § "User preferences" section, the § "Conventions and gotchas" section, and the `cold-reader` spawn shape in its § Reference snippets.

## TL;DR

The PRD, the ADR and all eleven `ISSUE-260902-0747-*` briefs are now mutually consistent under the
amended (isolation-based) tier rule, and every acceptance command in every brief was re-verified
against the working tree at 15:37. **Nothing is gated and nothing is committed.** The single next
action is the re-gate the maintainer deferred to a fresh session: `/to-spec` on the PRD, then
readiness-gate round 2 on all eleven briefs, then step 7. Three decisions in this session were made
on the maintainer's behalf and are explicitly revert-on-request — see § "Decisions made under a bare
'agreed'".

## Goal

Break `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md` into independently-grabbable, gated
`ISSUE-*` records. The epic fixes a twice-reproduced flake: the Rust suite fails intermittently under
CPU load because tests assert on wall-clock time and the test seam sits below the process boundary.

The authoritative, non-decaying input is the timing-site audit table under `### Timing-site audit` in
`docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md`. That record also holds the
reproduction itself (§ "The symptom is real and still live") and the panic site (§ Mechanism).

**The maintainer's hard constraint: never invent a rule, tier assignment, or scope boundary to fill a
gap — ask.** This is the thing they pushed back on explicitly in the predecessor session.

## Current state

Everything below is **untracked or modified, none of it committed**. `git status --short` is the
inventory.

| Artifact | State |
|---|---|
| `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md` | **Amended, gate voided.** Frontmatter `gate:` line **removed**; the void is recorded in a blockquote above `## Dimension Scan`. Eight body edits (four agreed in the prior session, four consequential — see § Files touched). `issues:` list now carries `-01` … `-11`. |
| `docs/adr/260902-0312-deterministic-test-tiers.md` | Amended in the **prior** session; untouched this session. It is the authority for the tier rule. |
| `CONTEXT.md` | Amended in the **prior** session; untouched this session. |
| `docs/issues/ISSUE-260902-0747-01..10-*.md` | **All rewritten or repaired.** All carry `status: needs-info` and a `**Readiness gate (cold-reader): FAIL** (round 1)` stamp with that round's findings under `## Triage Notes`. |
| `docs/issues/ISSUE-260902-0747-11-http-target-awaits-the-run-stream.md` | **New this session.** Carries the epic's acceptance. Never gated — minted after the round-1 batch, and its `## Triage Notes` says so. |
| `justfile:55` `test-under-load` | Still untracked-in-HEAD (`git show HEAD:justfile \| rg test-under-load` → exit 1). Now **owned**: `-01` commits it, with an AC on that. |
| Readiness gate round 2 | **Not started.** This is the next action. |
| Step 7 (adversarial breakdown pass) | **Not started.** Blocked on round 2 stamping every brief. |

### Verification state (re-run these if anything is edited)

Every change-introducing acceptance command was re-verified red at baseline at 15:37, and every
present-at-baseline command non-zero. See § Reference snippets for the exact list and figures.

## Key decisions and rationale

The prior session's decisions are recorded in the predecessor handoff and in the ADR; they still
stand. New this session:

- **HTTP endpoint tests are Logic Tier, and that inverts the epic's story.** The reproduced failure
  (`wait_for_run`'s 5s deadline in `tests/http_api.rs:147`) is inside the gate and must be *fixed*,
  not relocated.
  - *Why:* an endpoint test with controlled data and no clock is a component test, and it is
    mechanically reachable — `resync_run_stream_from_journal` (`src/api.rs:1060`) replays
    `db.list_events(run_id)` through a `SeqFilter`, so a test may open the stream *after* the run and
    still get every event. No new production surface.
  - *Consequence, written into both records:* `-01` no longer carries the epic's acceptance, and
    `just test-under-load` is **expected to stay red** after `-01` lands. Acceptance moved to `-11`.

- **Marking is explicit for the Integration Tier only; Logic Tier is the unmarked default.**
  - *Why:* the maintainer chose it over ~518 annotations against a boundary that keeps moving.
  - *Cost, accepted and named in the PRD:* a new process-spawning test with no marker lands silently
    in the gate. `-10` is the mitigation, which is why it is sequenced last.

- **`cargo test -- --list` cannot witness any exclusion.** Verified empirically:
  `cargo test --locked --test docs_catalog -- --list` prints `regeneration_writes_to_disk`, which is
  `#[ignore]`d (`tests/docs_catalog.rs:2705`).
  - *Consequence:* every listing-based AC across the batch was replaced with "what the command
    reports it executed", and `-01`'s selector gained a fifth constraint forbidding an
    `#[ignore]`-shaped tier marker.

- **`-03` sizes against 10790ms, not 8500ms.** The `~8.2–8.5s` figure in the round-1 finding is the
  *timing-out* observation; the same test recorded **10.79s** at a 60s budget (§ M2 of
  `docs/issues/code-reviews/issue-260826-0520-01-code-review-20260831-021621.md`). Two provenance
  caveats are written into the brief: that observation was at load average ~620 on 8 cores and its
  own record warns the seconds may be inflated, and the test has since been restructured to ~156ms.
  Kept as the floor anyway — a cap sized under an observed duration is the defect.

### Decisions made under a bare "agreed" — revert-on-request

The maintainer replied `agreed` twice to messages that contained questions carrying **no**
recommendation. Rather than re-ask a third time, this session formed recommendations and implemented
them, telling the maintainer each is revertible. **A fresh session should treat these three as
provisional until the maintainer confirms.** Each record carries its own rationale in `## Triage
Notes`, so reverting is local.

1. **`-09` split executed as a three-way, not the agreed four-way.** `-09`'s HTTP half was folded
   into the new `-11` rather than becoming a twelfth record.
   - *Why:* the polling helper and the sleeps live in the same file, share the same seam, and are the
     same defect. Splitting them puts two records on one file with one fix.
   - *Recorded in:* `ISSUE-260902-0747-11` § Triage Notes, "Why the two halves are one slice".
2. **`-06`'s AC3 resolved by a third option neither offered.** Instead of parameterizing the
   supervisor poll interval (new production surface) or dropping AC3 (leaves the select arm
   unasserted), AC3 now asserts the **ordering**: a node-runner double blocking at a test-held
   barrier holds a tick open, and only the select arm can make the abort effect precede the release.
   - *Why:* the ADR already prescribes exactly this (§ Consequences, and § The three exemplars,
     "Synchronize with a barrier, not a sleep"), and the double already exists — `AbortBlockingRunner`
     at `src/runtime.rs:7897`, two `tokio::sync::Barrier`s. **No production surface was authorized.**
   - *Recorded in:* `ISSUE-260902-0747-06` § Triage Notes, "AC3's seam, resolved without new
     production surface".
3. **The three orphaned vacuous skips folded into `-04` rather than an eleventh record.** `-04` is
   widened from "the tmux boundary" to every external-dependency guard; its filename is now
   historical and a banner in the brief says so.
   - *Why:* the honest-absence encoding is one decision applied to every guarded dependency; a
     separate record would re-derive the same ADR citation and risk a second, divergent encoding.
   - *Recorded in:* `ISSUE-260902-0747-04` § Triage Notes, "Scope widened to every external-dependency
     guard".

## Files touched

| File | Change |
|---|---|
| `docs/prd/PRD-260902-0301-01-*.md` | **(a)** HTTP paragraph replaced — endpoint tests are logic-tier component tests, with the journal-replay mechanism cited and the story inversion stated. **(b)** "Modules tested, by tier" — HTTP moved to logic; new "Storage splits per test, not as a module". **(c)** § Solution — logic-tier rule restated as isolation; new "Controlled data counts as isolation"; integration-tier paragraph realigned to "real infrastructure". **(d)** `:memory:` rider separating the rejected proposal from the deferred repository fake. **Plus four consequential:** fake-process fixture shapes (B in / C, D out); "Marking is explicit for the integration tier only"; "Acceptance does not land at the tier split"; user story 6 rider. Also: `gate:` line removed + void blockquote above `## Dimension Scan`; § Out of Scope storage bullet split in two; `issues:` gains `-11`. |
| `ISSUE-…-0747-01-draw-tier-boundary.md` | Brief rewritten. Isolation rule; timing-audit table cited *in the brief*; Integration-only marking with the silent-default cost named; fifth selector constraint (no `#[ignore]` shape); execution-based observables; commits `test-under-load`; **acceptance explicitly out of scope**, pointing at `-11`. Triage note "Why the epic's acceptance lands here" replaced with its retraction. |
| `ISSUE-…-0747-02-consolidate-fake-process-fixtures.md` | Brief rewritten around a **structural invariant** ("no test body writes a script body or sets an executable mode") with a verified genus command; shapes C and D explicitly out of scope with reasons; scale snapshot corrected. |
| `ISSUE-…-0747-03-frontend-generous-test-timeout.md` | Brief rewritten. Floor 10790ms with both provenance caveats; per-test override reconciliation added as a bound and an AC (verified: exactly one override exists, `ui/src/features/editor/GraphEditor.test.ts:60`); frontmatter summary de-counted. |
| `ISSUE-…-0747-04-repair-tmux-boundary-tests.md` | Scope widened to all external-dependency guards + historical-filename banner; AC6 replaced with a dedicated-socket-only assertion carrying a positive control; count-as-polarity fixed; execution-based observable. |
| `ISSUE-…-0747-05-run-lifecycle-event-handle.md` | Narrowed to the in-crate handle. Stale "HTTP stays Integration" paragraph replaced with a pointer to `-11`; database rationale realigned to controlled data + the deferred fake; listing and `test-under-load` ACs removed. |
| `ISSUE-…-0747-06-abort-observable-structurally.md` | AC3 restated as an ordering assertion (see above); fourth elapsed-time assertion added to scope (`abort_and_wait_force_clears_when_drain_never_arrives`); AC1 de-anchored from the `from_millis(500)` literal; new AC forbidding promotion-by-assertion-removal alone. |
| `ISSUE-…-0747-07-inject-event-sink-into-decision-functions.md` | "The sink is very nearly sufficient" — verified the six touch no `ctx.db`; only `apply_join_result` reads `ctx.registry.is_aborted` (`src/runtime.rs:4556`). Tier outcome made conditional on the clock-identity fork; AC2 given a distinguished error identity + positive control; findings pushed to code comments. |
| `ISSUE-…-0747-08-extract-interactive-poll-step.md` | Set corrected to **four** tests; `poll_loop_pattern_scan_uses_capture_coordinates_on_redraw` explicitly excluded as already-pure with the reason; AC3 pinned to the two interactive loops, excluding `query_agent_command` (`src/tmux_exec.rs:1780`) with the reason. |
| `ISSUE-…-0747-09-residual-sleeps-and-latent-retry.md` | Narrowed to the retry shim alone; HTTP half handed to `-11`; frontmatter summary rewritten. |
| `ISSUE-…-0747-10-enforce-tier-rule.md` | Three mechanically-produced positive controls (sleep, elapsed-time assertion, permitted clock-stamp); advisory branch de-self-certified; vacuous scripted-delay clause removed; `blocked_by` corrected to add `-04`, `-09`, `-11`. |
| `ISSUE-…-0747-11-http-target-awaits-the-run-stream.md` | **New.** Owns `wait_for_run`, all three `tokio::time::sleep` sites, the `test_router_with_db()` swap, and the epic's acceptance. |

## Dead ends — things tried that did not work

- **Writing a downgraded `gate:` value into the PRD frontmatter.** First attempt wrote
  `gate: needs re-gate — …`. `~/.claude/skills/to-spec/SPEC-GATE.md:60` closes that enum at
  `passed <date> | waived (hobby) <date>`, so a free-text value invents grammar — the exact class of
  move the maintainer forbids. Reverted: the line is **deleted** and the void is recorded as a
  blockquote above `## Dimension Scan` instead. Deleting alone was rejected too, because it is
  indistinguishable from a PRD that was never gated.
- **`rg -n '0o755' src/` as `-02`'s scope command.** Not authoritative in either direction (the
  round-1 finding). The replacement genus command was validated before being written down:
  `rg -n '#!/bin/sh|#!/usr/bin/env|set_mode\(0o7' src/` → hits only in `src/api.rs`,
  `src/tmux_exec.rs`, `src/runtime.rs`, and **zero** in `src/proc.rs` / `src/model.rs` (which hold
  the out-of-scope shapes C and D) and zero in `src/storage.rs` (whose `set_mode` calls are `0o600`
  production code).
- **Trusting the round-1 finding's orphan count.** It listed `zsh_available()` at
  `src/tmux_exec.rs:3924` *and* an early return at `:3875` as two orphans. `:3924` is the probe's
  **definition**; `:3875` is its only use. One guard, not two. `-04` now states the genus
  qualitatively with a validated command rather than a count.
- **`-04`'s first guard-discovery command** included `exists()`, which pulled in
  `src/runtime.rs:8982` (`if path.exists()`), not a dependency guard. Tightened to
  `rg -n -B4 '^\s+return;\s*$' src/ | rg 'available\(\)|geteuid|is_ok_and'` → six hits, all genuine.

## Conventions and gotchas observed

Everything in the predecessor's § "Conventions and gotchas" still holds — in particular **the display
filter mangles the literal phrase "unit test"** (renders as `n`/`ns`), `docs/issues/` is flat with no
`context:` field, and cold-readers correctly refuse to run `just test-under-load` (it spawns
`2 × nproc` `yes` processes). New this session:

- **`cargo test -- --list` enumerates `#[ignore]`d tests.** Proven, not assumed. Never use a listing
  as evidence that a selector excluded something.
- `AGENT-BRIEF.md`'s **"no enumeration closing an open set"** rule is the one most likely to trip a
  rewrite: a list of known-bad forms never closes a genus. Both `-02` and `-04` had to be restated as
  structural invariants with a genus command instead of a shape list.
- `## Triage Notes` is **context, not a delivery channel** — no consumer downstream of the pass stamp
  reads it. Any binding constraint must live in `## Agent Brief`. This is why `-07`'s "record the
  finding" ACs were re-pointed at code comments.
- Gate stamp grammar and round numbering: `~/.claude/skills/triage/READINESS-GATE.md` § Gate stamp.
  A later `FAIL` voids the brief until a subsequent round stamps `PASS`.
- `just test-under-load` takes `RUNS` (default 3) and `LOAD` (default `2 × nproc`) as arguments.

## User preferences and working style

The predecessor's section stands verbatim and is the more complete statement. The one operational
addition from this session:

- **They answer with a bare `agreed`, including to messages containing questions that carried no
  recommendation.** That is not consent to invent — it is a signal that *the question was
  malformed*. The fix is to supply a recommendation with its rationale and alternatives so `agreed`
  has something to attach to, implement it, and say plainly that it is revertible. Do not ask the
  same unrecommended question twice.

## Open questions

1. Do the three decisions in § "Decisions made under a bare 'agreed'" stand? Each is locally
   revertible and each record carries its own rationale.
2. Green-light for the re-gate cost? It is one `/to-spec` run over the PRD (cold reader + adversary)
   plus eleven `cold-reader` spawns at `model: "opus"`, effort xhigh.

## Next actions

1. **Confirm the three provisional decisions** with the maintainer (Open question 1) before gating —
   a gate round on a brief that then gets reverted is wasted.
2. **Re-gate the PRD**: run `/to-spec` over
   `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md`. It carries **no** `gate:` frontmatter
   line by design; the void blockquote above `## Dimension Scan` states which dimensions (4, 6, 9)
   the amendments touched. On pass, write `gate: passed <date>` back into the frontmatter and delete
   the void blockquote.
3. **Readiness-gate round 2** on all eleven briefs. Use the spawn shape in the predecessor handoff's
   § Reference snippets, unchanged. Note `-11` has never been gated — it is round **1** for that
   record, round **2** for the other ten.
4. **Route findings** per `~/.claude/skills/to-issues/SKILL.md` step 6. On pass, write the stamp and
   promote `status: needs-info` → `ready-for-agent`. No record may carry `ready-for-agent` without a
   pass stamp behind it — that was the defect this session had to repair.
5. **Step 7** — one `/codex-researcher` adversarial pass at `scale: breakdown` over the whole
   approved set, only after step 6 has stamped every brief.
6. **Commit.** Per `~/.claude/skills/to-issues/SKILL.md` § "Commits follow gates" — not before.

## Reference snippets

Baseline polarity, re-verified 2026-09-02 15:37. Change-introducing observables, all **red**:

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
```

Present-at-baseline observables, all non-zero:

```
rg -c 'unwrap_or\(2\)' src/runtime.rs                                 → 2
rg -c 'Duration::from_secs\(5\)' tests/http_api.rs                    → 1
rg -c 'tokio::time::sleep' tests/http_api.rs                          → 3
rg -c 'async fn wait_for_pending_approval' src/api.rs                 → 1
rg -c '#!/bin/sh|#!/usr/bin/env|set_mode\(0o7' src/api.rs             → 36
rg -n --glob '*.test.ts' '\}, [0-9][0-9_]*\);?$' ui/src               → 1 (GraphEditor.test.ts:60)
rg -n -B4 '^\s+return;\s*$' src/ | rg 'available\(\)|geteuid|is_ok_and' → 6
```

Proof that `--list` cannot witness an exclusion:

```
$ cargo test --locked --test docs_catalog -- --list | grep -c regeneration_writes_to_disk
1
$ rg -n '#\[ignore' tests/docs_catalog.rs
2705:#[ignore = "writes docs/workflow-schema.md when SB_REGEN_DOCS=1"]
```

Load-bearing source anchors cited by the rewritten briefs (verify before relying on a line number):

```
src/api.rs:1060      resync_run_stream_from_journal — journal replay; makes -11 race-free
src/api.rs:4826      euid/sudo vacuous skip (folded into -04)
src/runtime.rs:4556  apply_join_result's only ctx.registry read (the -07 finding)
src/runtime.rs:7897  AbortBlockingRunner — the barrier double -06's AC3 now uses
src/tmux_exec.rs:1551,1651  the two interactive poll loops (-08 scope)
src/tmux_exec.rs:1780       query_agent_command — the third loop, explicitly out of -08
src/tmux_exec.rs:3875       zsh_available() use site (folded into -04)
tests/http_api.rs:95        test_router_with_db() — the seam -11 swaps onto
tests/http_api.rs:147       wait_for_run's 5s deadline — the reproduced failure
```
