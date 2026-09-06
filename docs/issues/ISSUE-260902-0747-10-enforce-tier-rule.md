---
id: ISSUE-260902-0747-10
kind: issue
category: enhancement
status: needs-info
summary: The tier rule survives only as prose, so a hurried afternoon can put a sleep or an elapsed-time assertion back into the gate; add a check scoped to the logic tier, or ship the rule advisory if the boundary is not mechanically obvious
prd: PRD-260902-0301-01
adrs: [ADR-260902-0312-01]
terms: [Logic Tier, Integration Tier]
blocked_by: [ISSUE-260902-0747-01, ISSUE-260902-0747-04, ISSUE-260902-0747-11, ISSUE-260902-0747-14]
---

## Agent Brief

**Category:** enhancement
**Summary:** Enforce the Logic Tier's timing rule mechanically where enforcement is cheap and scoped, and ship it advisory rather than as a gate that cries wolf if it is not.

**Current behavior:**
After its sibling issues land, the tier rule is real in the test suite and written in the testing
document and the ADR, but nothing prevents a new Logic Tier test from sleeping to sequence work or
asserting on elapsed time. The convention survives only as long as reviewers remember it.

**Desired behavior:**
The Logic Tier's timing rule is checked mechanically, and the check runs where a developer meets it
before review rather than after.

*Scoped to the tier, never repository-wide.* A repository-wide search for durations would flag exactly
the load-bearing sites this epic exists to protect — the Integration Tier's deliberate temporal
assertions. The check is therefore conditioned on the tier boundary: it inspects Logic Tier tests and
ignores everything else. An earlier draft of this record claimed the scoping did most of the
work by itself, on the ground that the scripted-delay fixtures "sleep, so they are Integration Tier,
so a tier-scoped check never sees them". That was a true statement of an interim state mistaken for a
permanent one. `ISSUE-260902-0747-01` does mark them Integration Tier — they sleep to sequence work
and they end a poll at a wall-clock deadline, two of its forbidden forms — but that is where they sit
*before* the seam work, not where they end up. `ISSUE-260902-0747-13` removes the terminal-run deadline wait these tests actually end at,
`ISSUE-260902-0747-05` removes the progress-wait helpers a few of them additionally hold, and
`ISSUE-260902-0747-12` removes the scripted sleep; together those return them to the Logic Tier, which
is where `PRD-260902-0301-01` § Testing Decisions places the workflow decision logic they test. This
record is blocked by both, so by the time it runs they are Logic Tier tests the check **does** see —
and must not flag, because by then they neither sleep nor wait on a deadline. Scoping therefore does
*not* do this work by itself, and a check written on the assumption that it does would miss the
largest group of tests in the tier.

*What it forbids, in the Logic Tier only:* sleeping to sequence work, asserting on elapsed time,
ending a polling or convergence wait at a wall-clock deadline, and — because the PRD's accepted cost
depends on it — the **mechanically recognizable** isolation violations: writing or chmod-ing an
executable fixture, constructing a process directly, swapping a binary invocation, and binding a
socket. `PRD-260902-0301-01` § Implementation Decisions accepts that an unmarked new test lands
silently in the gate *on the grounds that this check keeps the default honest*; a check that hunts
only clock forms does not discharge that, and the accepted cost would be unmitigated. These shapes are
recognizable in this repository — `ISSUE-260902-0747-02` establishes the same genus command for the
fixture shapes — so the cheap half is worth having even though the general property is undecidable.
What stays out of reach is a process reached indirectly through production code, as the HTTP target's
fixture reached `tmux new-session` through a task node; the check cannot see that, the record says so,
and the testing document carries it as a review obligation rather than pretending otherwise. The third form is the one the epic's
reproduced failure took, so a check that omits it does not cover the defect it exists to prevent. A
bound that fires only on a hang — wrapping an await a happens-before edge ends on the passing path —
is permitted and must not be flagged; `PRD-260902-0301-01` § Testing Decisions draws that line.
Reading the clock to stamp a record is permitted and must not be flagged — a timestamp written as data
gates no control flow. Deriving identity from the clock is not covered by that allowance, but whether
the check can distinguish the two is a judgment for the implementer; if it cannot, forbid the two
clear cases and say so rather than producing false positives on the third.

*The escape hatch is the honest outcome, not a failure.* If the tier boundary does not turn out to be
mechanically obvious — if the check cannot tell a Logic Tier test from an Integration Tier one without
guessing — then **ship the rule advisory rather than ship a gate that cries wolf**
(`PRD-260902-0301-01` § Implementation Decisions, "One mechanical check"; `ADR-260902-0312-01`
§ Consequences, "Enforcement is scoped, or advisory"). Advisory means the rule is stated where a
developer writing a test will read it and, where cheap, reported without failing the build. Taking
this branch is a legitimate completion of this issue; record which branch was taken and why.

*No false reds.* Whichever branch is taken, the check must not fail on the suite as it stands when this
issue completes. A check that flags existing, decided Integration Tier assertions is wrong and must be
narrowed, never satisfied by weakening those assertions.

**Key interfaces:**
- The check itself — a repository script or lint invoked from a recipe, and, if it gates, from the
  Logic Tier CI job. It reads the tier marker introduced by `ISSUE-260902-0747-01` to decide what is in
  scope.
- The testing document's tier section, which gains the statement of what is checked and what is left to
  review.

**Baseline:** this slice is the join node **of the narrowed epic** — blocked by `-01`, `-04`, `-11`
and `-14`, the remaining slices that change Rust test code or move a test between tiers — so every
criterion below is read against the tree after those four have landed.

Amended 2026-09-05. This record was originally blocked by all thirteen siblings, on the premise that
the check could only run once every violation was repaired. Eight of those slices were deferred, so
that premise is gone and it is not restored by waiting: **the check enforces the Logic Tier, not a
clean suite.** Known violations that remain live in the Integration Tier as retained debt
(`ADR-260902-0312-01` § Retained debt) and are outside what this check examines. A criterion below
that reads as "the suite is clean" is to be read as "the Logic Tier is clean".

**The check knows two categories.** The **Performance Check** is defined in `ADR-260902-0312-01` but
is **not built in this pass** (`PRD-260902-0301-01` § Out of Scope), so no member exists for the check
to misclassify and no third selector is read. If that category is ever built, teaching the check about
it is that work's problem, not this record's.

**How the controls are produced.** Every criterion below that calls for a "mechanically produced"
working copy differing from the tree in one dimension is discharged instead by a **fixture suite**: a
directory of small, checked-in source examples, each a minimal test file exercising exactly one
accepted or rejected shape, with the expected verdict recorded beside it. The check runs over the
fixture directory as part of its own test.

This is a deliberate simplification, taken 2026-09-05. Mutating a copy of the whole repository per
control was specified when this record was the join node behind thirteen slices; it is expensive to
build, slow to run, and no more convincing than a fixture whose expected verdict is written down. The
guarantee that matters is unchanged and is retained in full: **a clean result counts as evidence only
when the same invocation also reports the paired violation.** A check that examines nothing must not
be able to discharge a criterion by silence. Keep every paired-run requirement below; change only how
the input is produced.

**Acceptance criteria:**
- [ ] Either a check exists and is invoked from a recipe, or the record states that the boundary was
      not mechanically obvious and the rule shipped advisory. `rg -n 'Logic Tier' justfile` returns the
      recipe introduced by this change in the first case; no matches before this change in either.
- [ ] In the gating branch, the check runs clean against the suite as it stands at this issue's
      completion, and its **positive control** is produced mechanically from that same tree: a working
      copy differing in exactly one dimension — one sleep inserted into one Logic Tier test — on which
      the check reports a violation. The violation is identified by a signal the clean run never
      emits: the report names the offending test and the rule it broke, so a non-zero exit from any
      other cause is not mistaken for a detection.
- [ ] A second mechanically-produced control covers the other forbidden form: the same tree with one
      elapsed-time assertion inserted into one Logic Tier test, on which the check reports a violation
      naming that test.
- [ ] A third control covers the third forbidden form: the same tree with one Logic Tier test given a
      loop that reads a resource until it changes and gives up at a wall-clock deadline, on which the
      check reports a violation naming that test.
- [ ] Three controls cover the permitted cases and must all come back clean: the same tree with one
      Logic Tier test reading the clock to stamp a record; the same tree with one Logic Tier test whose
      await is wrapped in a hang-only bound of the shape
      `pane_stream_pump_honors_explicit_drain_with_receiver_alive` uses; and the same tree with one
      **Performance Check** member asserting elapsed time. A check that flags any of them is wrong and
      is narrowed, not accepted — the hang-only bound is the hardest of the three and is why the
      forbidden third form is stated as a termination condition rather than as the presence of a
      timeout.

      **Each clean result is only evidence if the check was live when it was produced.** A check that
      reports nothing satisfies every clean-run criterion in this record vacuously. So each of these
      three controls is run in the same invocation as one of the violating controls above, and the
      report must name the violation *and* stay silent on the permitted case. A run producing no
      output at all discharges none of them.
- [ ] No existing Integration Tier assertion was weakened to satisfy the check, and the check reports
      no violation against any of them — demonstrated the same way: the run that reports clean on the
      Integration Tier also reports the violation on a paired dirty variant, so silence is
      distinguishable from a check that examined nothing.
- [ ] **`just test-under-load` passes over the final gate population.** This record is the join node:
      it is blocked by every slice that changes Rust test code or moves a test between tiers, so it is
      the first point at which the default command's contents are settled. `ISSUE-260902-0747-11` runs
      the same recipe when it removes the reproduced failure, and that criterion stays — but `-11` is
      blocked only by `-01`, so `-04` and `-14` still move tests into the gate after it passes. Without
      this criterion no record runs the epic's acceptance against the population the epic actually
      ships. (Before 2026-09-05 this read "seven later slices"; those eight are deferred and the point
      now rests on two.) Run the Integration Tier once as well, so a test promoted out of the gate is
      still known to pass somewhere. Both branches of this record owe this criterion — it is about the
      suite, not about the check.

      **State the acceptance's limit alongside the result.** The reproduced failure is intermittent —
      the reproduction measured two red runs and one green — so a passing sample is not proof of a fix
      and a single failing one is not proof of a regression. Record the repetition count actually run
      and the load actually established, not merely that the recipe exited zero. This is the same
      limit `ISSUE-260902-0747-11` carries for the regression run; it applies with more force here,
      because this is the closing acceptance for the whole epic.
- [ ] The Logic Tier test the tier rule names as its example of a permitted hang-only bound carries the
      comment the rule prescribes — naming the hang it guards — since the rule holds it up as the
      template and it predates the rule. It carries no such comment before this change. This is the
      review-enforced half of the permitted form; the check itself must not demand it.
- [ ] **No tier-rule exception survives.** `ISSUE-260902-0747-01` recorded exactly one — the
      out-of-crate HTTP target, held unmarked and in the gate while it still waited on a clock — and
      `ISSUE-260902-0747-11` closed it. The dependency edges make the state clause true before this
      issue starts, so the state alone is **not** what this criterion tests. In the gating branch the
      check must *treat* an exception as a violation, demonstrated by a fourth mechanically-produced
      control: the same tree with one exception entry reintroduced, on which the check reports a
      violation naming it. The exception entry's form is pinned by `ISSUE-260902-0747-01` — a single
      stable token that `rg` finds — so this control reintroduces that exact form rather than
      inventing one; a control built on a guessed form tests nothing. In the advisory branch the testing document states, at the point a
      developer writing a test will read it, that no tier-rule exception is in force and that a test
      appearing to need one is a question for the maintainer. If an entry is somehow still present,
      this issue does not proceed by widening the check to accept it.
- [ ] A control covers the isolation half: the same tree with one Logic Tier test given an executable
      fixture it writes and chmods, and another with one binding a socket, on each of which the check
      reports a violation naming that test. If the implementer finds either shape not mechanically
      recognizable in this suite, the record says which and why, and the testing document carries it as
      a review obligation — the criterion is discharged by an honest negative finding, not by silence.
- [ ] In the advisory branch, the record names the specific property of the tier marker that defeated
      mechanical scoping — not that it was "not obvious" — and the testing document states the rule at
      the point a developer writing a test will read it. Observable at the testing document's tier
      section and at this record's `## Triage Notes`.

- [ ] **The glossary stops calling this practice planned, for the terms this epic makes real.**
      After this change, `grep '(planned' CONTEXT.md` returns no `_(planned — ADR-260902-0312-01)_`
      entry for **Logic Tier**, **Integration Tier**, **Port** or **Double**, and returns every one of
      them before it. **Performance Check keeps its planned marker**: the narrowed epic defines the
      category but does not build it (`PRD-260902-0301-01` § Out of Scope), and removing the marker
      would assert a practice that is not in force. Amended 2026-09-05; before that this criterion
      covered every term the ADR names. `CONTEXT.md` states that the marker comes off in the epic that makes
      a practice real and that the epic owns the terms its ADR names, so the removal belongs to the
      slice at which the practice actually comes into force — this one, the join node behind every
      slice that changes Rust test code. Removing a marker is not editing a definition: if an entry's
      text is no longer true of the tree, that is a finding to report, not a rewrite to make here.

- [ ] **The check states what it cannot see, and the testing document repeats it.** This check reads
      test sources. It therefore cannot detect a Logic Tier test whose timing or isolation dependence
      lives in production code reached from the test body, and two such cases are already known and
      filed: the unlock throttle's five-second production window, reached from endpoint tests that
      contain no timing construct at all (`ISSUE-260905-2136-01`), and driver capability tests reading
      the developer's own `agents.toml` through a process-global cache (`ISSUE-260905-2136-02`). Name
      both as the concrete evidence that call-path isolation is a **review obligation** rather than a
      checked property. `ADR-260902-0312-01` § Enforcement is scoped, or advisory requires an honest
      negative finding to name the property that defeated the check; these are it, and a record that
      claims the boundary is fully mechanical is wrong.

**Out of scope:**
- Changing any test's tier. This issue observes the boundary; it does not move it.
- Detecting dependence on a clock or on ambient configuration reached through production code. Known
  unreachable, filed as `ISSUE-260905-2136-01` and `-02`, and stated as a review obligation above.
- Deciding isolation in general. The check hunts the timing forms and the mechanically recognizable
  isolation shapes named above; a process reached indirectly through production code is out of reach
  and is a review obligation, not a check failure.
- The frontend suite, whose timing policy is already asserted deterministically under fake timers.
- Enforcing the rule on out-of-crate targets if the tier marker is not readable there; say so rather
  than guessing.

## Triage Notes

Minted 2026-09-02 from `PRD-260902-0301-01`; breakdown approved by the maintainer the same day. Last in
the epic and blocked by every slice that moves a test across the boundary, because a check written
before the promotions land would encode a boundary that is still moving.

This is the one slice whose acceptance admits two genuinely different outcomes. The advisory branch is
not a fallback taken on running out of time — the PRD and the ADR both prefer it to a noisy gate, and
an implementer who takes it should not feel they have under-delivered.

### Readiness gate round 1 — findings

**Readiness gate (cold-reader): FAIL** (round 1)

All ten `ISSUE-260902-0747-*` briefs were gated in parallel on 2026-09-02 (`cold-reader`, `model: opus`,
one per record, rubric `~/.claude/skills/triage/READINESS-GATE.md`). All ten returned FAIL. The reports
were not written to disk; the compressed findings below and in
`docs/handoff/deterministic-test-tiers-2026-09-02-1419.md` are the sole surviving record of them.

`status:` is `needs-info` because two upstream things are owed. `PRD-260902-0301-01` was amended after
this round (HTTP endpoint tests moved to the Logic Tier, storage split per-test, the Logic Tier rule
restated as isolation with controlled data) and is itself awaiting a re-gate; and
`ADR-260902-0312-01` now decides tier membership by **what a test isolates**, not by what it touches,
which every brief in this batch predates.

**Systematic defects across the batch** (fix in one sweep, not per record):

1. **Count-as-polarity** — pre-change polarity written as a count beside a discovery command
   ("It returns four test call sites as well before this change"). `AGENT-BRIEF.md`
   § *Qualitative polarity vs decaying state* requires qualitative phrasing, never a count.
   Present in `-02`, `-04`, `-06`, `-07`, `-08`, `-09`; the correct form is already used in `-01`,
   `-03`, `-05`, `-10`.
2. **Broken listing template** — "appears in the Logic Tier listing (`cargo test --locked -- --list`)
   after this change and did not before it" is false wherever a test is already deterministic.
   Confirmed false in `-07` and `-08`. Present in `-05` through `-09`.
3. **`cargo test -- --list` includes `#[ignore]`d tests** — proven by `regeneration_writes_to_disk`
   appearing in the listing. Any `#[ignore]`-shaped selector makes every listing-based criterion in
   the batch unfalsifiable. This constrains `-01`'s selector choice.
4. **Templated `just test-under-load` closing AC** in `-05`, `-06`, `-07`, `-09`, `-10` is already
   green at each record's declared baseline (post-`-01`); only the unexecutable qualifier carries
   content. `just test-under-load` is also not in version control and has no owning slice.

**This record:**

- Failure class 9 arm B fires on AC2: no positive control, and "reports a violation" names no unique signal.
- The scripted-delay clause is vacuous — those fixtures sleep, so they are Integration Tier and a tier-scoped check never sees them.
- The advisory branch is self-certifying.
- No seam echo on any criterion.
- **`blocked_by` is wrong**: `-04` and `-09` also promote tests and are not listed.
- Templated `just test-under-load` closing AC already green at the declared baseline (systematic defect 4).

### Readiness gate round 2 — findings

**Readiness gate (cold-reader): FAIL** (round 2)

- **Class 9 arm B req. 1 on AC5 and AC6 — not waivable.** Both are leave-alone assertions: AC5's two permitted-case controls "must both come back clean", AC6's check "reports no violation against any of them". A check that reports nothing at all satisfies both, and neither carries an acted-on witness of its own. AC2/3/4's controls cannot discharge them — the anti-borrowing ban is explicit — and neither condition is shape-bound.
- **Class 4 — AC7 omits the acceptance's stated limit.** `rg 'intermittent|three-run|resilience'` returns nothing in this record; `-11` carries it twice. AC7 makes a `just test-under-load` pass the witness with no deterministic companion, at the recipe's own `RUNS="3"` default the PRD says proves nothing.
- **Class 3 — AC9's control is unbuildable.** `-01` pins the exception entry only as "a single greppable entry"; its form and location are that slice's choice, so "the same tree with one exception entry reintroduced" names no artifact.
- **Class 4 — the advisory branch had no decision procedure.** Settled upstream: the boundary is mechanically obvious when a check can decide any test's tier from the tree alone; taking the fallback is a finding to be argued and confirmed, not an implementer's preference. Applied in the PRD and in `ADR-260902-0312-01` § Consequences.
- **Class 3/4 — ACs 3, 4, 5, 6 and 10 are unscoped by branch**, and meaningless without a check.
- Class 6 does not fire; the ten `blocked_by` edges are all real and `-03`'s absence is correct. Class 7: 37 rows, none blocking.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`

### Round 2 repairs applied 2026-09-02

The clean-run criteria are no longer satisfiable by a check that reports nothing: each is now paired
with a violating control in the same invocation, so silence is distinguishable from a check that
examined nothing. The closing-acceptance criterion carries the reproduction's intermittency limit and
requires the repetition count and established load to be recorded. The exception-entry control now
depends on a form `-01` pins rather than one this record guesses. The check's scope covers three
categories, since the Performance Check asserts elapsed time by design. `blocked_by` gains
`ISSUE-260902-0747-13`, the completion-signal half of the `-05` split. Baseline stated.
Awaiting round 3.

### Edited 2026-09-02, not gated this round

Held out of the round-3 wave pending the maintainer's decision on who owns the glossary markers. The
record's authoritative verdict is still `FAIL` (round 2), so no `REOPENED` is owed.

- **New criterion: the glossary stops calling this practice planned.** `CONTEXT.md` states that the
  `_(planned — ADR-…)_` marker comes off in the epic that makes a practice real, and that the epic
  owns the terms its ADR names. No slice owned that removal. It lands here by maintainer decision
  2026-09-02, because this is the slice at which the practice actually comes into force.
- **`blocked_by` gains `ISSUE-260902-0747-14`**, the struct-field slice split out of `-04`.
- **Attribution corrected.** The scripted-delay tests end their wait at the terminal-run helper, which
  `ISSUE-260902-0747-13` removes; this record credited `-05` alone.

Awaiting round 3.
