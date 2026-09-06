---
id: ISSUE-260902-0747-02
kind: issue
category: enhancement
status: needs-triage
summary: Executable fake-process fixtures are built in the test body across three modules — inline, or with only the permission step delegated to a mechanism-named helper — so nothing declares what each test is faking or whether it needs a process at all
adrs: [ADR-260902-0312-01]
terms: [Logic Tier]
blocked_by: [PRD-260902-0301-01]
---

## Deferral note

Deferred 2026-09-05, out of `PRD-260902-0301-01`'s delivery scope and into the backlog, following the
maintainer's decision to narrow that epic to the reproduced failure plus the forward-facing tier rule.
The narrowed epic delivers `-01`, `-11`, `-04`, `-03`, `-14` and a scoped `-10`; this record is good
work that is not that task.

Deferring it does not retire the problem it describes. It is retained debt under
`docs/adr/260902-0312-deterministic-test-tiers.md` § Retained debt: the tests it would have repaired
stay in the Integration Tier, stay in the non-gating job, and must not be described as fixed.

**Do not implement this brief as written without re-triage.** It was authored against the pre-narrowing
ADR and PRD, and its `blocked_by` chain assumes slices that are no longer sequenced.

**Known defect, from the 2026-09-05 adversarial review.** Consolidating fixtures ahead of the seam work
was challenged as unnecessary sequencing: this record changes no assertion, process dependency or tier,
and `ISSUE-260902-0747-08` then removes some of the fixtures it would move. If revived, consolidate the
*surviving* shared infrastructure after the seam work rather than before it.

## Agent Brief

**Category:** enhancement
**Summary:** Move every executable fake-process fixture behind a named, shared helper whose call site declares what behavior is being faked.

**Baseline:** this slice has no blockers and is read against the tree at the tip of the branch this epic lands on (`feature/tmux-panes`), not against `main`. It may be picked up
before or after `ISSUE-260902-0747-01`; where that changes how a criterion is observed, the criterion
says so.

**Current behavior:**
Rust test modules stand up fake executables by hand: write a shell script to a temporary path, make
the file executable, and point an invocation at it. The blocks are near-identical, so a reader cannot
tell from a call site what behavior the fake is standing in for, nor whether the test needs a real
process at all or merely inherited one from a copied fixture.

The population is one genus. Three forms are known and all are in scope; derive the full set rather
than working from this list, which is an illustration of the genus and not its roster:

- **Inline** — the script body is written and the executable mode set in the test body itself.
- **Half-extracted** — the script body is still written in the test body, but the permission step is
  delegated to an existing helper that names the *mechanism* (making a file executable) rather than
  the faked behavior. A call site in this form is no more declarative than an inline one; the fixture
  is not behind a helper, only the `chmod` is.
- **Module-local** — a helper inside the test module writes the body *and* sets the mode
  (`src/runtime.rs:8920`, `:8948`, `:11293`; `src/tmux_exec.rs:3406`, `:3518`). These are behind a
  helper but not a *shared* one, so they read as consolidated while each module still owns its own
  fixture construction. A sweep that looks only for inline construction will report them as done.

Discovery command for the genus. It matches the shell shebang written from test code and the
executable-mode literals in both the `set_mode` and `from_mode` spellings:

```
rg -n '#!/bin/sh|#!/usr/bin/env|(set_mode|from_mode)\(0o7' src/
```

It over-returns and under-returns, so read each hit rather than treating the output as the set.

**The exclusion is over lines, not over tests.** Some hits set an executable-bit mode on a temporary
**directory** — traversal setup, or a permission the test asserts is preserved — and those *lines* are
not fake-process fixtures and are not in scope. Do not lift the enclosing test out of scope on that
basis: the tests that do this also build genuine inline fake-process fixtures a few lines away, and
excluding them wholesale leaves those fixtures standing while AC1 reads green. Derive the directory
lines by reading what each mode is applied to, and exclude exactly those. Conversely, a module-local
helper whose body is far from its call sites will not look like a fixture at the call site at all.

**Desired behavior:**
Every executable fake-process fixture is constructed through a named helper in one shared test-support
location, and **nothing outside that shared surface creates the file** — writes it and sets an
executable mode on it. That is the structural invariant, and it is what the acceptance criteria check
— not the absence of a particular literal, which a single helper move would satisfy while leaving
half-extracted fixtures untouched. Note what the invariant does *not* forbid: a call site may still
pass a scripted body to a shared helper, so a shebang appearing at a call site is not itself a
violation. The helper creating the file is what matters. An earlier draft phrased this as "no test
module writes a script body", which contradicted that allowance and left the implementer choosing
between one parameterized helper and a helper per faked behavior.

Each helper names the behavior it fakes rather than the mechanism — what the fake process *does* when
invoked, not that it is a script that gets made executable — so a call site reads as a declaration of
the test's dependency on process behavior. Where several call sites want the same fake with a
different scripted body, the helper takes the body (or the behavior it should exhibit) as a parameter
rather than being duplicated per call site.

This is prefactoring for the seam extractions, not a cleanup for its own sake
(`PRD-260902-0301-01` § Implementation Decisions, "Fake-process fixtures get consolidated behind
named helpers"): once every process dependency is declared at its call site, the sibling seam issues
can see which tests genuinely need a process. Record what that reveals under `## Triage Notes` on
this record; **acting** on it belongs to `ISSUE-260902-0747-05` through `-08`, not here.

**Key interfaces:**
- A shared test-support surface holding the fake-process helpers, reachable from the test modules of
  every module that currently builds fixtures inline or half-extracted. It is `#[cfg(test)]`-scoped
  and in-crate, so out-of-crate targets are unaffected.
- Each helper's signature — it returns whatever the call site needs to point an invocation at the
  fake (a path, or a configured invocation), and takes the faked behavior as its parameter.
- The existing mechanism-named permission helper, which is subsumed: after this change it is either
  private to the shared surface or gone, and no test calls it directly.

**Acceptance criteria:**
- [ ] No code outside the shared test-support surface **writes a file and sets an executable mode on
      it**. That is the structural invariant; it is not a claim about where a literal appears. A call
      site that passes a scripted body *to* a shared helper satisfies this even though the body's
      shebang is written at that call site; a test module or a module-local helper that creates the
      file itself does not. Before this change the discovery command returns such sites, spread over
      more than one module; read its output for the set rather than a list here.
- [ ] Every module-local fixture helper is either moved into the shared surface or deleted, and no
      test module retains a private one. This is the form a literal-based sweep reports as already
      consolidated, so check it by reading the modules, not by grepping for a shebang.
- [ ] Every remaining call site names the faked behavior in the helper it calls; no call site passes
      a raw permission mode.
- [ ] The full Rust suite is green before and after this change, with the same set of passing tests —
      this issue changes no assertion and no test's tier. Observe the **whole** suite, recording the
      executed-test set before and after and comparing them. Which invocation gives you the whole
      suite depends on whether `ISSUE-260902-0747-01` has landed: before it, a bare
      `cargo test --locked`; after it, that plus the Integration Tier and Performance Check switches.
      Use whichever covers everything at the tree you are working against — this record is independent
      of `-01` and may be picked up before or after it, so the criterion is about the coverage of the
      observation, not about a fixed command.

**Out of scope:**
Other ways a test reaches a real process are not fake-executable fixtures and are not in scope. Two
are named here because they are the ones a sweep will trip over; if you find a further shape, it is
out of scope on the same grounds — it is not a written-and-chmod-ed fake — and worth recording:

- **Inline `sh -c` command strings**, where the test passes a shell program as an argument rather
  than writing an executable. These live in the process-supervision module and their subject *is*
  process and descendant behavior — timeout bounds, output-tail capture — so they are Integration
  Tier by the rule in `ADR-260902-0312-01` and consolidating their bodies would not change that.
- **Self-exec child fixtures**, where the test re-invokes the test binary itself as a child process
  under a marker environment variable. The child is not a fake; it is the same binary, and the
  mechanism has nothing to consolidate into a script helper.

Also out of scope:

- Removing any test's dependency on a real process, or promoting any test between tiers. Deciding
  which tests can lose their process is the finding this issue produces; acting on it is
  `ISSUE-260902-0747-05` through `-08`.
- Changing what any fake script does. The scripted bodies move behind helpers unchanged.
- The real-tmux guard tests, which drive the real binary rather than a fake and are
  `ISSUE-260902-0747-04`. Derive them with `rg -n 'if !tmux_available\(\)' src/tmux_exec.rs`
  attributed to owning test rather than carrying a count.
- Out-of-crate test targets.

## Triage Notes

Minted 2026-09-02 from `PRD-260902-0301-01`; breakdown approved by the maintainer the same day.
Independent of `ISSUE-260902-0747-01` — it changes no tier assignment — but sequenced alongside it
because both touch the same test bodies, and it blocks `ISSUE-260902-0747-08`.

**Scale.** Derive it with the discovery command in the brief and read the hits; do not carry a number
here. An earlier snapshot recorded "~110 sites" taken from `rg -c`, which prints a count **per file**
rather than a total, so the figure was the sum of two lines misread as one. `ISSUE-260901-0216-03`
§ "Root cause: the seam is below the process boundary" describes the same population qualitatively —
distinct fake shell-script fixtures serving a large share of the suite, few of them shared.

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

- `rg -n '0o755' src/` is not authoritative for "every inline fake-process fixture": it over-includes six helper-body lines and misses three shapes — (B) inline script delegating chmod to `make_executable` (`src/runtime.rs:10080`, `:10093`), (C) `sh -c` inline strings (`src/proc.rs:179`, `:212`), (D) self-exec child (`src/model.rs:5242`, `:5319`). Moving `make_executable` alone turns both ACs green while shape-B fixtures survive.
- The hole is open upstream too: the PRD says only "*Most* fake-process script fixtures are written inline".
- Count-as-polarity (systematic defect 1).

### Readiness gate round 2 — findings

**Readiness gate (cold-reader): FAIL** (round 2)

- **Class 3/7 — a third fixture form.** Module-local helpers that write the script body *and* set the mode (`src/runtime.rs:8920`, `:8948`, `:11293`; `src/tmux_exec.rs:3406`, `:3518`) are neither inline nor half-extracted. These are the ones the diagnostic record counts as "already behind extracted helpers"; the two-form reading treated them as consolidated when they are only un-shared. Fixed upstream in the PRD.
- **Class 4 — AC1 and AC2 contradict each other.** AC2 blesses a call site passing a scripted body to a shared helper; AC1 requires the shebang literal to match only inside the shared surface. Every body carries `#!/bin/sh` as its first line, so the construction AC2 permits leaves AC1 red.
- **Class 3 — the executable-mode conjunct oversweeps.** Two tests set executable-bit modes on temp *directories* and assert on them (`src/api.rs:4237`, `:4842`); neither is a fake-process fixture, neither is excluded, and the discovery command cannot see them (`from_mode`, not `set_mode`). The command also misses 16 `from_mode(0o755)` sites.
- **Class 3 — the out-of-scope list closes an open set.** "Two further ways a test reaches a real process exist" is falsified in-tree.
- **Class 4 — AC4's whole-suite observation.** After `-01`, `cargo test --locked` *is* the tier selector, so "deliberately not through the tier selector" is unexecutable as written.
- Class 6 does not fire. Class 8 inert. Class 9 does not fire.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`

### Round 2 repairs applied 2026-09-02

Third fixture form named, with the list stated as an illustration and the set left to be derived.
AC1 restated as the structural invariant — nothing outside the shared surface writes a file and sets
an executable mode — which removes its contradiction with the call-site form AC2 permits. Directory-
permission tests excluded by name; discovery command widened to `from_mode`. Out-of-scope list no
longer closed. AC4's observation stated as coverage rather than a fixed command. Scale count removed.
Baseline stated. Awaiting round 3.

### Round 3 findings — 2026-09-02

**Readiness gate (cold-reader): FAIL** (round 3)

Baseline ref corrected: it named `main`, a ref 87 commits behind the tree every claim in this brief
was measured against, at which the module-local fixture form does not exist and AC2's set is empty.
The directory exclusion is restated over *lines* rather than tests — the two tests it named each build
an in-scope inline fixture a few lines from the directory chmod — and its false "assert on them"
qualifier is gone. Desired behavior no longer forbids a script body at a call site while AC1 permits
one; the invariant is now "nothing outside the shared surface creates the file". AC1's pre-change file
inventory, the guard-test count and the directory count are all replaced by their derivations.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round3.md`.
Awaiting re-gate.
