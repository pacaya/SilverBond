# Readiness gate over eleven briefs — 2026-09-02. **All FAIL.**

> Eleven `cold-reader` agents (`model: opus`, effort xhigh), one per record, per
> `~/.claude/skills/triage/READINESS-GATE.md` § Protocol. Round 3 for `-02`…`-09`; round 2 for `-11`
> and `-12`; round 1 for `-13`. `-01` and `-10` were held out of the wave pending two maintainer
> decisions that could have landed on them. Working tree at branch `feature/tmux-panes`, HEAD
> `70a878b`, artifacts uncommitted.
> **Every finding acted on was independently re-derived by the gate runner first.**
> The PRD's round-9 spec gate is in the file beside this one.

## Outcome

| Record | Round | Verdict | Blocking |
|---|---|---|---|
| `-02` | 3 | FAIL | 6 |
| `-03` | 3 | FAIL | 4 |
| `-04` | 3 | FAIL | 3 |
| `-05` | 3 | FAIL | 3 |
| `-06` | 3 | FAIL | 3 |
| `-07` | 3 | FAIL | 2 |
| `-08` | 3 | FAIL | 1 |
| `-09` | 3 | FAIL | 1 |
| `-11` | 2 | FAIL | 3 |
| `-12` | 2 | FAIL | 3 |
| `-13` | 1 | FAIL | 4 |

Class 6 fired once (`-04`, prong (b)). Class 9 fired twice (`-03` arm A, `-11` arm B). Class 8 was
inert or did not fire everywhere. The failures concentrate in one genus, described below.

## The finding that changed the epic

**The clock-identity fork had no destination, and the escape was priced for a closure a third its
real size.** `ADR-260902-0312-01` said a clock-derived identifier entering state or event identity
"needs an injected generator, **or** its path is not Logic Tier eligible". Nothing in the ADR, the
PRD or `CONTEXT.md` said where such a path goes. `-07` answered by fiat — Integration Tier — which is
the broadening the PRD had explicitly refused, and `CONTEXT.md` decides that tier's membership by
isolation of the external world, which a decision function called through an injected sink does not
do.

The gate derived the closure: five of the six decision functions reach a clock-derived mint, one of
them only through `.unwrap_or_else(new_cursor_id)` — the function-reference form a call-syntax search
misses. Taking the escape would have moved five of six out of the gate.

**Maintainer decision 2026-09-02: the fork is withdrawn.** Injection is required for every path in
the closure; a path that cannot take one is a re-decision the slice raises, not a tier it assigns.
Applied in the ADR, the PRD and `-07`. **This voids the PRD's round-9 stamp and owes round 10.**

## The recurring genus, at instance seven

A brief asserts a set over the current tree where a derivation is owed. Five instances were recorded
before this round; this round found two more, both *inside* a round-2 fix for the same defect:

- `-06` was rewritten to derive its elapsed-time set through two reconciled passes. The instruction
  works — the gate ran both passes and they disagreed by exactly the predicted member — but the prose
  around it still said "all four" and "the four tests", which is the table-only answer the derivation
  exists to correct.
- `-12` was rewritten to replace call-site counts with attribution to owning tests, and still opened
  with "the shape is uniform". Five of thirteen in-scope tests match that shape.

The scoped population rule in the PRD holds up: every reader applied it, and it is what let them
classify these as defects rather than as arguable prose.

## Findings that were not prose

| Record | Finding |
|---|---|
| `-02`, `-03`, `-01` | The baseline sentence named the tree at `main`, 87 commits behind the branch. At that ref the fixture population is 3 lines against 128, `-03`'s discovery command returns nothing, and two acceptance criteria are vacuously green. |
| `-05` | A caller passing every promotion check reaches process execution through *production* code — `run_as` with no invocation, resolved at `start_run`. It survives only because the path it would exec does not exist. Left marked. |
| `-11` | AC5's control was green before the change: the shared fixture accepts `Failed` and `Aborted` as terminal, so a broken tmux path already satisfies it — a fact the brief states two paragraphs above the criterion. |
| `-08` | A fifth test satisfies the property that defines the set moving onto `poll_step`, and sat outside the enumeration. The brief answered the question in both directions. |
| `-12`/`-06` | Three `with_delay` sites nobody obliged: `-12`'s AC1 is absolute over the file, `-06` obliged only the calibrated two-second call, and `-12` is forbidden from reaching in. |
| `-05`/`-13` | Two `wait_for_event("done")` sites each record disclaimed to the other. |
| `-13`/`-06` | Two timeout-wrapped terminal waits both records claimed, with no ordering between them. |
| `-13` | `wait_for_path` is converted by no slice of the epic — the tests holding one stay marked past its end. Now stated. |
| `-04` | Class 6 prong (b): the struct-field batch shares no test function with the other two. Split to `ISSUE-260902-0747-14`. |

## Maintainer decisions applied

1. Baseline ref is the tip of the branch the epic lands on, not `main`.
2. The clock-identity fork is withdrawn; injection is required. Decided in the ADR.
3. Ownership: the excluded test's `with_delay` calls → `-06`; the `done` event sites → `-05`;
   `-13` gains `blocked_by: -06`; the population shared by `-13` and `-05` is derived once in `-13`
   and consumed by name in `-05`.
4. `-04` splits; the struct-field batch is `-14`.
5. `-05` leaves the indirect-spawn caller marked rather than adding a check the PRD says cannot be
   written.
6. `-08` includes the fifth test.
7. Naming a preservation set by role is durable specification, not an uncited inventory. `-03`'s AC5
   stands.
8. The `CONTEXT.md` `_(planned — …)_` markers come off in `-10`, as a new acceptance criterion.

## Stamp grammar

`-03` and `-08` carried `**Readiness gate: REOPENED** (after round 2 PASS)` — two verdict words on
one line, with a round number outside the matcher's grammar. Two readers independently found it
parsed three ways, one of which read the record as gated. Both are rewritten as
`REOPENED (round 2, …)`: a reopening is a state change on the round it reopens, not a round of its
own. This keeps the rounds just run numbered 3, and puts `-02`…`-09` at authoritative round 3 — so
their next round is a **full-enumeration round** under D4.

## Next

Re-gate: PRD round 10; `-02`…`-09` round 4 (full enumeration); `-11`/`-12` round 3; `-13` round 2;
`-01`/`-10` round 3; `-14` round 1.
