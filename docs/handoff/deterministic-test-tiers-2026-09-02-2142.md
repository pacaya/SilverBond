# Handoff: deterministic-test-suite breakdown — twelve briefs gated, PRD failing round 8 on a recurring defect genus

> Prepared 2026-09-02 21:42 from a Claude Code session in `/Users/Shared/Data/work/Programming/SilverBond`,
> branch `feature/tmux-panes`, HEAD `70a878b` (unchanged — **nothing committed**).
> Audience: a fresh Claude instance with no memory of the prior conversation. Read top to bottom.
> **Supersedes `docs/handoff/deterministic-test-tiers-2026-09-02-1915.md`.** From that document what still
> stands: its § "Traps a fresh instance will otherwise fall into" (every entry, plus three new ones below)
> and the maintainer-constraint statements in its § Goal. What is now stale in it: its § "Next actions"
> (all done), its § "Current state" table (superseded below), its claim that the briefs are ungated (they
> are stamped), and its round-numbering claim that `-11` was on round 2 (it had never been gated).
> The `-1537` handoff's § "Conventions and gotchas observed" and § "User preferences and working style"
> still stand; the `-1419` handoff's § "User preferences" is still the fullest statement.

## TL;DR

All twelve briefs have been readiness-gated (**2 PASS, 10 FAIL**) and stamped; the PRD has been through
two more spec-gate rounds (7 and 8, both FAIL). Round 8's findings are **not applied** — that is the open
work. The session's central finding is that this PRD keeps failing one defect genus: it asserts a **closed
set where a derivation is owed**, five times across five rounds, most recently *inside the fix written for
that exact failure mode*. The maintainer has been asked to choose among three remedies (a/b/c in § Open
questions) and has not answered. Do not start editing until they do.

## Goal

Break `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md` into independently-grabbable, gated
`ISSUE-*` records. The epic fixes a twice-reproduced flake: the Rust suite fails intermittently under CPU
load because tests assert on wall-clock time and the test seam sits below the process boundary.

Authoritative non-decaying input: the `### Timing-site audit` **classification** table in
`docs/issues/done/ISSUE-260901-0216-03-flaky-tests-undermine-ci-gate.md`. Contractual; outranks any prose
count. The rest of that record (§ Mechanism, § Root cause, § Related defects, § "The symptom is real and
still live") is evidence, not contract — several rounds have confused the two.

**Maintainer's hard constraints, both reconfirmed this session:**
1. Never invent a rule, tier assignment, or scope boundary to fill a gap — ask.
2. A question put to them with no recommendation attached is malformed. Supply a recommendation with its
   alternatives rather than asking bare.

## Current state

| Artifact | State |
|---|---|
| `PRD-260902-0301-01` | **Failing spec gate round 8.** No `gate:` line; void blockquote above `## Dimension Scan` records the history. Rounds 6 and 7 applied in full. **Round 8's six findings are unapplied.** |
| `ADR-260902-0312-01` | Amended this session — mechanism correction, `nineteen` implementations, tier-term casing, and the enforcement-fallback criterion. Not separately gated. |
| `CONTEXT.md` | Unchanged this session. Carries the four minted terms in § Testing. |
| `ISSUE-260902-0747-01` … `-12` | **All twelve stamped.** `-03` and `-08` `PASS` (round 2); the other ten `FAIL`. All still `status: needs-info`; **none is `ready-for-agent`**, correctly. |
| Brief repairs | **Not started.** This is the main unspent work after the open question is answered. |
| Step 7 (adversarial breakdown pass) | Not started. One has run over the *unapproved* set; its report is on disk. |
| Commit | Not done, and blocked: `~/.claude/skills/to-issues/SKILL.md` § "Commits follow gates". |

### Gate reports on disk — read these rather than reconstructing

- `docs/prd/adversary-reports/PRD-260902-0301-01-spec-gate-260902-round6-report.md`
- `docs/prd/adversary-reports/PRD-260902-0301-01-spec-gate-260902-rounds7-8-report.md` ← **round 8 is the open work**
- `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`
- Each brief's `## Triage Notes` carries its own stamp and findings.

## Key decisions and rationale

Each was put to the maintainer in prose with a recommendation and alternatives, and approved.

1. **Capture-tail reader dropped from the Logic Tier inventory.** *Why:* both `src/proc.rs` tests spawn
   `sh -c` and assert a duration, and § Implementation Decisions already ruled that module's tests
   Integration Tier. *Alternative rejected:* writing a new clock-free test of `read_capture` — coverage
   growth, not flake removal, and nobody owned it.
2. **`testing/seams` stays `deferred`, citation trimmed to the bare ledger slug.** *Why:* SPEC-GATE's
   payload for `deferred` **is** the slug, so a live ledger entry is what the verdict encodes. The
   over-stuffed citation is what made the row look self-contradictory. *Note:* round 8 F7 re-raises this;
   it is a judgement, not an error.
3. **"Controlled data" not minted as a term.** *Why:* fully defined inside CONTEXT.md's Logic Tier entry
   and names no thing in the system. Recorded in the `domain terms` scan row so it is not re-raised.
4. **The preview endpoint's task test is Integration Tier** (decision "A"). *Why:* the PRD already stated
   that an endpoint test genuinely requiring task execution is Integration Tier — the rule existed and had
   only ever been applied to hypothetical future tests. Nothing relocates: it is not in the reproduction.
   *Alternatives rejected:* an injectable runner port (new production surface, explicitly unauthorized);
   rewriting the test to assert the error path (deletes real coverage).
5. **`-05` splits; `-04` does not** (decision "B"). *Why:* `-05` has two production surfaces, two seams and
   two independently demoable halves, and its only cohesion argument was a factually false claim. `-04`'s
   three batches share the same four test functions, so the merge seam costs more than the split buys.
6. **A slice's baseline is the tree after its blockers land** (decision "C"), stated once in the PRD and to
   be stated per brief. *Why:* every post-`-01` criterion is false against today's tree because no selector
   exists yet; the literal gate-time baseline would fire class 9 arm A on `-08`, `-05`, `-07` and `-12` and
   make sequenced work inexpressible.
7. **"One exception" counts decided exceptions, not unmarked rule-failing tests** (decision "D"). *Why:*
   the HTTP target is a decided temporary exception; the perf assertion is an *undecided classification*,
   and marking it would answer `[OPEN: perf-test-tier]` by side effect. **Round 8 F1 shows this paragraph
   is still wrong on the count** — see Open questions.
8. **`src/host.rs`'s startup test and the self-exec pair are assigned to `-01`** as Integration Tier
   markings, no thirteenth record. *Why:* markings, not seam changes. *Cost stated:* the self-exec child
   re-invokes the binary with `--exact <test name>`, so a selector excluding the marked parent breaks it.
9. **Structural remedy over per-instance fixes** for the recurring pattern. *Why:* four rounds of
   per-instance fixes each produced the next instance. **Round 8 shows the remedy was applied to two
   passages rather than to the class, and the fifth instance landed inside it.**

## Files touched this session

| File | Change |
|---|---|
| `docs/prd/PRD-260902-0301-01-deterministic-test-suite.md` | Round 6 repairs (mechanism, citations, `nineteen`, attributions, frontend floor, provenance, casing); round 7 repairs (the `/api/test-node` paragraphs, baseline convention, exception/perf reconciliation, enforcement criterion, clock-identity derivation, abort derivation, inventory disclaimer, 9B rephrase); void blockquote rewritten twice |
| `docs/adr/260902-0312-deterministic-test-tiers.md` | Mechanism correction; `twenty`→`nineteen` implementations; tier-term casing (20 sites); enforcement-fallback criterion in § Consequences |
| `docs/issues/ISSUE-260902-0747-01-*.md` | "untracked"→"uncommitted"; round-2 `FAIL` stamp |
| `docs/issues/ISSUE-260902-0747-02..12-*.md` | Round-2 (or round-1) stamps with per-record findings |
| `docs/prd/adversary-reports/…-round6-report.md`, `…-rounds7-8-report.md`, `…-briefs-round2.md` | New — the three gate reports |

## Dead ends — things tried that did not work

- **Per-instance fixes for the closed-set pattern.** Applied at rounds 6 and 7, each time to the specific
  passage the round flagged. Round 8 found the fifth instance *inside* the round-7 rewrite: rephrasing
  "the target stays unmarked" to "the run-stream tests stay unmarked" replaced a too-large set with a
  too-small one, excluding `run_control_routes_return_typed_client_errors` — one of the three tests in the
  reproduction. **Insight:** the author knowing the pattern is not sufficient protection against it; the
  remedy has to be a document-wide rule, not a better sentence.
- **A call-syntax regex for the clock-identity closure.** I wrote `re.search(r'\b'+name+r'\s*\(')` and got
  "0 of 6", contradicting the `-07` gate. The mint is reached via `.unwrap_or_else(new_cursor_id)` — a
  function *reference*, not a call. Bare-name matching gave the correct 5 of 6. This is exactly why the
  figure survived several drafts.
- **Grepping a hard-wrapped phrase as one line.** `rep()` on `"…through\n`tmux::run_checked_owned`…"`
  failed with count 0 because the phrase spans a newline at ~100 columns. The predecessor handoff warns
  about this and it still bit. Always `rg -U` or reproduce the wrap.

## Conventions and gotchas observed

Everything in `-1915` § "Traps" still holds. New this session:

- **Verify every agent claim before acting.** Across 15 cold-reader spawns, claims that did not survive:
  one that graded a load-bearing `stream_run` ordering error as a "nit"; one that routed a brief-only
  defect upstream to the PRD (`-02`'s AC4 — the PRD states the selector constraint correctly, the brief
  misread it). Two readers independently found the same `src/runtime.rs:11070` assertion, which raised
  confidence in it.
- **`rg -c` prints per-file counts, not a total.** Several records carry "~110 sites" from a `rg -c` whose
  output was two lines.
- **Gate stamps:** canonical shape `**Readiness gate (cold-reader): PASS** (round N)` at column 0. A
  heading like `### Readiness gate round 1 — findings` carries no verdict word and is *skipped as
  malformed* by the matcher — which is correct and intended. `**Readiness gate:** not yet run` is likewise
  inert.
- **P3 Atomicity:** editing a brief that carries an authoritative `PASS` costs a `REOPENED` stamp before or
  with the edit, plus a re-gate. `-03` and `-08` are now in that state.
- Cold readers correctly refuse `just test-under-load`, `cargo test`, and `tests/http_api.rs` — all spawn
  real tmux on the user's default socket. Do not ask them to.

## User preferences and working style

The `-1419` handoff's section is still the fullest statement. Confirmed again this session:

- They answer with a bare **`agreed`**. When every question in the message carried a recommendation, that
  attaches to the recommendations and authorizes the stated plan. When a question carried none, `agreed`
  means *the question was malformed* — supply a recommendation and proceed, saying it is revertible.
- They authorize spend explicitly and in units ("one PRD gate round, then twelve cold-reader spawns"), and
  they want a report at the boundary before the next tranche.
- They predicted the recurring failure mode before it was found again ("Assume the same failure mode is
  still live somewhere else"). That instinct has been right four times; take it seriously.
- They want honest attribution of mistakes, including the assistant's own. F2 is a self-inflicted defect
  and was reported as such.

## Open questions

1. **Which remedy for the closed-set pattern?** Put to them with recommendations, unanswered:
   **(a)** keep patching instances (evidence: five rounds, five instances, one inside the fix);
   **(b) *recommended*** — one normative sentence saying the document asserts no populations, every set is
   derived by the slice that acts on it, and any population named is illustrative and loses to the rule;
   then sweep round 8's 100-sentence ledger once. The round-7 inventory disclaimer is the evidence this
   works — round 8 confirmed it held;
   **(c)** stop gating the PRD's prose and put the effort into the briefs, where class 7 forces derivation
   with a required pin per record. Cheapest path to `ready-for-agent`; but wrong sets do propagate (`-01`
   already inherited the false "two unmarked tests" count).
   **F2 must be fixed under any option** — it is not an inventory nit; it would let the epic relocate one of
   its three reproduced failures out of the gate.
2. **`[OPEN: perf-test-tier]`** — still theirs, still `resolve-by: during-triage`. `-01` cannot have its
   AC1/AC2 phrased correctly until this closes, and `-01` is the root of the dependency graph, so this is
   the critical path to promoting anything.

## Next actions

1. **Ask the maintainer to pick (a), (b) or (c)** from Open question 1 — do not start editing first.
2. **Apply round 8's F1–F6** from `…-rounds7-8-report.md` regardless of that choice; F2 and F1 are
   build-changing. F1's wrong count has already propagated into `ISSUE-260902-0747-01`, so fix both.
3. **Re-gate the PRD (round 9)** using the round-8 spawn prompt shape — its priority instruction to sweep
   set-asserting sentences mechanically is what found F2, and should be kept.
4. **Repair the ten failed briefs** from their `## Triage Notes` findings. Write `REOPENED` on `-03` and
   `-08` before touching them (both carry authoritative `PASS`). `-05` splits per decision 5; `-04` does
   not. Every brief gains a baseline sentence per decision 6.
5. **Re-gate every changed brief.** Note `-01`…`-10` will be round 3, `-11`/`-12` round 2 — and
   READINESS-GATE § "Round escalation (D4)" makes the *next* round after authoritative round 3 a
   full-enumeration round, so `-01`…`-10` hit that at round 4.
6. **Step 7** — one `/codex-researcher` adversarial pass at `scale: breakdown` over the approved set,
   framed as a follow-up (one has already run over the unapproved set).
7. **Commit**, per `~/.claude/skills/to-issues/SKILL.md` § "Commits follow gates" — not before.

## Reference — verified this session

Facts re-derived from the tree; each cost a round to find.

```
tests/http_api.rs:889   test_node_accepts_v3_task_node asserts OK + preview["success"]==true
src/api.rs:680          spawn_blocking(build_tmux_invocation(…))  — unconditional
src/tmux_exec.rs:121    Command::new("zsh").args(["-lic", …])     — real login shell
src/runtime.rs:1749     run_node_preview → run_tmux_oneshot       — NO NodeRunner port on this path
src/runtime.rs:3436     run path returns run_scoped_tmux_invocation when run_as is None — shell-free
src/host.rs:33,:80-95   real TcpListener + a deadline-terminated poll; in the audit table, in no brief
src/runtime.rs:11070    started.elapsed() < TMUX_CLEANUP_TEST_TIMEOUT — the 2nd reproduction's own test
src/runtime.rs:6043     .unwrap_or_else(new_cursor_id) — a reference; call-syntax searches miss it
src/runtime.rs:9862     wait_for_run's 5s deadline over a 20ms poll — all 14 with_delay owners reach it
src/api.rs:892-893      subscribe THEN list_events — the PRD said this backwards until round 6
src/api.rs:4834         eprintln naming the missing dependency on the PASSING path (-04's AC6 defect)
tests/docs_catalog.rs:1820-1831  check_citation → DiscriminantOutsideSymbol (-09's shim hazard)
```

Populations (re-derive rather than trust; these decay):

```
9 of 14 wait_for_pending_approval callers write no fixture, no mode, no socket
5 of 6 decision functions reach new_cursor_id; only select_next_decision does not
42 wait_for_terminal_run call sites vs 36 for the other four helpers combined
5 module-local script-writing helpers: runtime.rs 8920/8948/11293, tmux_exec.rs 3406/3518
4 Uuid::now_v7 identity mints in runtime.rs + 3 more in api.rs/tmux_exec.rs
32 .with_delay( sites across 14 owning functions
```

Spend this session: **15 `cold-reader` spawns** (PRD rounds 6, 7, 8; twelve briefs), all `model: opus`,
effort xhigh.
