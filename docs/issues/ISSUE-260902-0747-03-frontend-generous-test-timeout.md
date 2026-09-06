---
id: ISSUE-260902-0747-03
kind: issue
category: bug
status: needs-info
summary: The frontend suite configures no test timeout, so it runs on vitest's implicit default — a cap that recorded frontend flakes have exceeded under full-suite load
prd: PRD-260902-0301-01
adrs: [ADR-260902-0312-01]
---

## Scope note

Amended 2026-09-05. This record stays what it is: a small configuration patch raising the outer
Vitest per-test timeout, with its existing justification intact.

**It is not a determinism claim, and must not be written as one.** Testing Library's
`asyncUtilTimeout` is a separate deadline defaulting to 1000ms, consumed by `waitFor` and the
`findBy*` queries, and `ui/src/test/setup.ts` does not override it — so raising the Vitest timeout
leaves the convergence-wait budgets in the same test untouched
(`ui/src/features/editor/InspectorPanel.test.ts:145`, `:150`). Reconciling that nested wait policy is
`ISSUE-260905-2136-04` and is deliberately not in this patch.

Two consequences for the wording of this record: state that it buys resilience headroom rather than
determinism, and drop any claim that the frontend suite is uniformly deterministic under fake timers.
It is not — the pane-stream timing-policy tests use fake timers; this InspectorPanel test uses real
Testing Library waits.

## Agent Brief

**Category:** bug
**Summary:** Configure an explicitly generous vitest test timeout, sized against the slowest frontend test duration recorded anywhere under load.

**Baseline:** this slice has no blockers and is read against the tree at the tip of the branch this epic lands on (`feature/tmux-panes`), not against `main`. It touches only the
frontend suite, so nothing in the Rust tier work moves it.

**Current behavior:**
The vitest configuration sets no `testTimeout`, so every frontend test runs against the implicit
default of the installed vitest version. That default is the cap recorded frontend load failures have
exceeded. The suite is not uniformly fast under contention, and a test that passes comfortably on an
idle machine can exceed the implicit default when the machine is loaded — a false red about the
runner rather than about the code, which is the defect `PRD-260902-0301-01` exists to remove.

The suite carries per-test timeout overrides, added ad hoc when a test was observed to exceed the
implicit default. State no count for them and do not assume there is more than one — derive the set
and act on what it returns. Discovery command:

```
rg -n --glob '*.test.ts' '\}, [0-9][0-9_]*\);?$' ui/src
```

**Desired behavior:**
The vitest configuration sets `testTimeout` explicitly, at a value chosen so that exceeding it means
a genuine hang rather than a loaded machine.

The contract is **behavioural, not cosmetic**. Writing the implicit default out explicitly would
satisfy the letter of "configure a timeout" while changing nothing, so three properties bind
(`PRD-260902-0301-01` § Implementation Decisions, "The frontend gets a test timeout that is actually
generous"):

- The configured value must be strictly greater than **10790** milliseconds, and by a margin the
  implementer chooses and states. That figure is the slowest frontend test duration recorded under
  load anywhere in this repository's records: the graph-editor canvas mount test, timed at 10.79s
  when granted a 60-second budget, recorded under § M2 of the code review
  `issue-260826-0520-01-code-review-20260831-021621` under `docs/issues/code-reviews/`. Two lower
  observations exist and are **not** the basis — the inspector-panel unlock-secret retry test at
  6160ms (`ISSUE-260826-0240-01`, § Suite) and the same graph-editor test at 8.2–8.5s while timing
  out — because a suite-wide cap sized to either of them still sits under a duration this repository
  has already observed.
- The configured value must not equal the implicit default the installed vitest version applies when
  `testTimeout` is unset. Determine that default from the installed version rather than assuming it.
- Whatever per-test overrides the command returns must be **reconciled, not stranded**. An override that is now shorter
  than the suite-wide default is a per-test *tightening* that no record asked for; each surviving
  override either gets a comment saying why it is deliberately tighter, or is removed so the test
  inherits the new default. Adding new overrides is out of scope.

The chosen value and the margin are delegated to the implementer within those bounds. Record the
reasoning where a later reader will find it: a comment at the configuration site naming the observed
duration that set the floor, the record it comes from, and the margin.

Two provenance caveats belong in that comment or beside it, because a later reader who re-derives the
floor without them will think it is over-sized. The 10.79s observation was taken at an extreme load
average on an eight-core machine, and its source record says so explicitly and warns the absolute
seconds may be inflated; and the test that produced it has since been restructured and now completes
in milliseconds. The figure is retained as the floor anyway — it is the largest duration this
repository has actually observed, and a cap chosen to sit under an observed duration is the defect
this issue exists to remove. Re-measuring under load is welcome and may raise the floor, but is not
required.

Fake-timer assertions consume no wall clock, so raising this cap cannot disturb the frontend's
existing deterministic timing-policy tests. It must not.

**Key interfaces:**
- The `test` block of the vitest configuration — the `testTimeout` field, alongside the existing
  `environment`, `setupFiles`, `css` and `exclude` settings.
- Every per-test timeout override in the frontend suite, as the reconciliation set.

**Acceptance criteria:**
- [ ] `rg -n 'testTimeout' ui/vite.config.ts` returns the setting introduced by this change; no
      matches before it.
- [ ] The configured value is **at least twice** the 10790ms floor, and is not equal to the installed
      vitest version's implicit default. A bare strict-greater-than is not sufficient: 10791ms with a
      comment claiming a one-millisecond margin would satisfy it while providing no headroom at all
      over an observation the record's own provenance caveats say may itself be understated. The
      multiple is the falsifiable form of "generous"; an implementer choosing a larger value needs no
      permission, and one choosing a smaller one is not meeting this criterion.
- [ ] A comment at the configuration site names the observed duration that set the floor, cites the
      record it comes from, states the margin, and carries the two provenance caveats.
- [ ] Every per-test override still present after this change sits beside a comment saying why it is
      deliberately tighter than the suite default; overrides without such a justification are gone.
      The discovery command above enumerates the set to check, and returns overrides carrying no
      such comment before this change.
- [ ] The frontend suite is green, with the same set of passing tests as before this change, and the
      fake-timer assertions in the pane-stream client and validation suites are untouched. Observable
      at `just test-ui`.

**Out of scope:**
- Any restructuring of the frontend tests. The frontend already asserts timing policy
  deterministically under fake timers; `PRD-260902-0301-01` § Out of Scope keeps its tests
  unreorganized by this epic.
- Adding new per-test timeout overrides. Reconciling the existing ones is in scope; minting more is
  not — a test that needs its own budget after this change is evidence for a different record.
- The Rust suite's tiers and timing rules, which are `ISSUE-260902-0747-01` and its siblings.
- Playwright e2e tests.

## Triage Notes

Minted 2026-09-02 from `PRD-260902-0301-01`; breakdown approved by the maintainer the same day. The
smallest slice in the epic and the only one on the frontend stack. Unblocked — it shares no code with
any sibling.

The maintainer was offered the option of folding this into `ISSUE-260902-0747-01` as a configuration
rider and kept it standalone.

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

- *"the one recorded frontend flake"* is an uncommanded occurrence count **and is falsified**: `docs/issues/code-reviews/issue-260826-0520-01-code-review-20260831-021621.md:87-104` records a second frontend timeout at ~8.2–8.5s under load, plus a second load failure of the same `InspectorPanel.test.ts` unlock test.
- An existing per-test `}, 20_000)` override at `ui/src/features/editor/GraphEditor.test.ts:60` goes unmentioned.
- Sizing at >6160ms could land under an already-recorded duration.

### Readiness gate round 2 — findings

**Readiness gate (cold-reader): PASS** (round 2)

Every gap `fine` or an explicit delegation; class 6 does not fire; 52 class-7 surfaces all non-blocking; class 8 inert; class 9 fires on neither arm, with all four change-introducing observables executed red at the gate-time tree.

The round-1 class-1 defect is discharged: the >10790 ms floor and its source are now carried by `PRD-260902-0301-01` § Implementation Decisions, so the brief's citation resolves.

Non-blocking notes, recorded rather than acted on:
- "Two lower observations exist" is not exhaustive — the same source line records a third, 7.3s at a 20s budget. It sits below the floor and changes nothing.
- The two-suite fake-timer preservation set is correct but uncited; a reader running `rg -l useFakeTimers` finds three files and cannot see why `AppShell.runActions` is excluded. `ISSUE-260901-0216-03` § Timing-site audit is the authority.
- `hookTimeout` stays at its 10000 ms default by design and becomes the tighter budget once `testTimeout` exceeds it.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`

### Post-PASS edit 2026-09-02

**Readiness gate (cold-reader): REOPENED** (round 2, 2026-09-02 — the round-2 PASS was reopened by a post-stamp edit; REOPENED is a state change on that round, not a round of its own)

Edited after an authoritative `PASS`, so the stamp above no longer stands and a re-gate is owed. The
edit is one addition: the baseline sentence every brief in this epic now carries
(`PRD-260902-0301-01` § Testing Decisions, "A slice's baseline is the tree after its blockers land").
This record has no blockers, so its baseline is unchanged from what round 2 read; nothing else in the
brief moved.

### Round 3 findings — 2026-09-02

**Readiness gate (cold-reader): FAIL** (round 3)

Baseline ref corrected from `main`, at which this brief's own discovery command returns nothing and
AC4 is vacuously green. The `REOPENED` stamp is rewritten in canonical form — it carried two verdict
words on one line and a round number outside the matcher's grammar, and parsed three ways, one of
which read the record as gated. The plural "overrides" is replaced by the derivation.

**Ruled by the maintainer 2026-09-02:** naming a preservation set by role — "the pane-stream client
and validation suites" — is durable specification, not an uncited inventory. The gate fired class 7
on it and argued the counter-case itself; firing there would penalise compliance with the brief-text
ban on file paths. AC5 stands as written.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round3.md`.
Awaiting re-gate.
