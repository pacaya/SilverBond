---
id: ISSUE-260902-0747-10
kind: issue
category: enhancement
status: needs-info
summary: The tier rule survives only as prose, so a hurried afternoon can put a sleep or an elapsed-time assertion back into the gate; add a check scoped to the logic tier, or ship the rule advisory if the boundary is not mechanically obvious
prd: PRD-260902-0301-01
adrs: [ADR-260902-0312-01]
terms: [Logic Tier, Integration Tier]
blocked_by: [ISSUE-260902-0747-01, ISSUE-260902-0747-04, ISSUE-260902-0747-11, ISSUE-260902-0747-14, ISSUE-260902-0747-15]
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
ignores everything else.

Scoping is not, however, a substitute for the check's own coverage. The scripted-delay fixtures sleep
and end a poll at a wall-clock deadline, so `ISSUE-260902-0747-01` marks them Integration Tier and the
check does not see them — and no record in this epic promotes them back. They are retained debt
(`ADR-260902-0312-01` § Retained debt). Do not design the check's scope around a population that will
move; nothing moves here.

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
gates no control flow. Deriving identity from the clock is constrained only where that identity is
load-bearing (`ADR-260902-0312-01`), which a source scan cannot determine — so the check does not
hunt clock-derived identity at all, and says so. It must still flag every form listed above; none of
them may be dropped for being hard to recognise.

*The escape hatch is the honest outcome, not a failure.* If the tier boundary does not turn out to be
mechanically obvious — if the check cannot tell a Logic Tier test from an Integration Tier one without
guessing — then **ship the rule advisory rather than ship a gate that cries wolf**
(`PRD-260902-0301-01` § Implementation Decisions, "One mechanical check"; `ADR-260902-0312-01`
§ Consequences, "Enforcement is scoped, or advisory"). Advisory means the rule is stated where a
developer writing a test will read it and, where cheap, reported without failing the build. Taking this branch is a **finding to be confirmed, not an implementer's
preference**: state the specific property that defeated the check and stop for maintainer
confirmation before shipping advisory (`PRD-260902-0301-01` § Implementation Decisions;
`ADR-260902-0312-01` § Consequences). The branch condition is **tier decidability** — whether the
check can decide any test's tier from the tree alone with no per-test human judgement. It is not
violation coverage: a check that decides tiers cleanly but cannot see every violation is the gating
branch with a stated limit, not the advisory branch.

*No false reds.* Whichever branch is taken, the check must not fail on the suite as it stands when this
issue completes. A check that flags existing, decided Integration Tier assertions is wrong and must be
narrowed, never satisfied by weakening those assertions.

**Key interfaces:**
- The check itself — a repository script or lint invoked from a recipe, and, if it gates, from the
  Logic Tier CI job. It reads the tier marker introduced by `ISSUE-260902-0747-01` to decide what is in
  scope.
- The testing document's tier section, which gains the statement of what is checked and what is left to
  review.

**Baseline:** this slice is the join node **of the narrowed epic** — blocked by `-01`, `-04`, `-11`,
`-14` and `-15`, the remaining slices that change Rust test code or move a test between tiers — so every
criterion below is read against the tree after those four have landed.

**The check enforces the Logic Tier as it stands, not a clean suite.** It runs once the tier
assignments settle, not once every violation is repaired. Two known violations sit in **Logic Tier** tests whose dependence lives in
production code the test body never names — the unlock throttle's five-second window
(`ISSUE-260905-2136-01`) and ambient registry configuration in driver tests (`ISSUE-260905-2136-02`).
A source scan cannot see either. They are filed, they are not repaired by this epic, and a criterion
below that reads as "the Logic Tier is clean" means clean **of the forms this check inspects** — not
free of every isolation defect. Do not widen the check to chase them; do not claim they are absent.

**The check knows two categories.** The **Performance Check** is defined in `ADR-260902-0312-01` but
is **not built in this pass** (`PRD-260902-0301-01` § Out of Scope), so no member exists for the check
to misclassify and no third selector is read. If that category is ever built, teaching the check about
it is that work's problem, not this record's.

**How the controls are produced.** Every control below is a working copy of the tree under test,
produced **mechanically** from it and differing in exactly one dimension. A hand-authored fixture file
does not satisfy a control: the check must be shown to catch a violation written the way this suite
actually writes one — a helper-wrapped `tokio::time::sleep`, a `with_delay` integer, a
`std::thread::sleep` — and a minimal example tuned to the check's own literal proves nothing about
that.

Each control is run in the same invocation as the clean case it is paired with, and the report must
name the violation while staying silent on the clean case. A run producing no output at all discharges
nothing: a check that examines nothing would otherwise satisfy every clean-run criterion here by
silence.

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
- [ ] Two controls cover the permitted cases and must both come back clean: the same tree with one
      Logic Tier test reading the clock to stamp a record; and the same tree with one Logic Tier test
      whose await is wrapped in a hang-only bound of the shape
      `pane_stream_pump_honors_explicit_drain_with_receiver_alive` uses. A check that flags either is
      wrong and is narrowed, not accepted — the hang-only bound is the harder of the two and is why the
      forbidden third form is stated as a termination condition rather than as the presence of a
      timeout.

      **Each control carries its own witness.** Build each as a two-dimension variant of the tree: the
      permitted shape *and*, in a different Logic Tier test, one forbidden shape. The check must report
      the forbidden one by name and stay silent on the permitted one, in the same run. Do not discharge
      this against another criterion's control — a permitted case whose only witness is elsewhere is
      not itself falsifying.
- [ ] No existing Integration Tier assertion was weakened to satisfy the check, and the check reports
      no violation against any of them.

      **Witness this by moving the boundary, not by dirtying an Integration Tier test.** A forbidden
      shape inserted into an Integration Tier test is silent by design — the check ignores that tier —
      so such a pairing proves nothing. Build the control by taking one Integration Tier test that
      already contains a forbidden shape and removing only its tier marker: the check must then report
      it by name, and must fall silent again when the marker is restored. That varies exactly the
      dimension the check keys on, and it is this criterion's own witness.
- [ ] **`just test-under-load` passes over the final gate population.** This record is the join node:
      it is blocked by every slice that changes Rust test code or moves a test between tiers, so it is
      the first point at which the default command's contents are settled. `ISSUE-260902-0747-11` runs
      the same recipe when it removes the reproduced failure, and that criterion stays — but `-11` is
      blocked only by `-01`, so `-14` still moves tests into the gate after it passes, and `-04` moves
      guard tests out of it. Without this criterion no record runs the epic's acceptance against the
      population the epic actually ships. Run the Integration Tier once as well, so a test moved out of the gate is
      still known to pass somewhere. Both branches of this record owe this criterion — it is about the
      suite, not about the check.

      **Its pass condition is not a green run.** After the honest-absence work, a guard whose
      dependency is genuinely absent does not report a pass, and the root-plus-`sudo`-plus-system-account
      combination is available on no CI runner and few developer machines. Satisfy this criterion by
      recording, per non-passing test, which dependency was missing — read from the diagnostic that work
      requires each guard to emit. A run that is non-green *only* for named absent dependencies
      satisfies it; any other failure does not.

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
      them before it. **Performance Check keeps its planned marker**: the category is defined but not
      built, and removing the marker would assert a practice that is not in force.

      `CONTEXT.md` states that the marker comes off in the epic that makes
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
