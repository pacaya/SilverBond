---
id: ISSUE-260902-0747-04
kind: issue
category: bug
status: needs-info
summary: Every external-dependency guard in the Rust suite reports a pass when its dependency is absent, and the real-tmux guard tests additionally run on the user's default socket where they can collide with a developer's live sessions
prd: PRD-260902-0301-01
adrs: [ADR-260902-0312-01, ADR-260622-0208-01]
terms: [Integration Tier]
blocked_by: [ISSUE-260902-0747-01]
---

## Agent Brief

**Category:** bug
**Summary:** Isolate the real-tmux guard tests on a dedicated socket, and stop a missing external dependency reading as success anywhere in the Rust suite.

> The record's filename says "tmux boundary" for historical reasons. Its scope is wider: **every**
> guard in the Rust suite that returns early when an external dependency is absent, wherever it lives.

**Baseline:** this slice is blocked by `ISSUE-260902-0747-01`, so every criterion below is read
against the tree **after** the tier selector exists. The tier assertions in the acceptance criteria
are unobservable before that and are not meant to be read against today's tree.

**Scope note.** The vacuous-skip half of this record covers every external-dependency guard in the
suite, not only tmux's. That scope is authorized upstream by `PRD-260902-0301-01`
§ Implementation Decisions, "a missing external dependency must not read as success", which states
the constraint over the genus precisely because the encoding is one decision rather than one per
dependency. This record names no count of the guards; derive the set yourself with the discovery
command below and read each hit.

**Bounded 2026-09-05.** Deriving the set stays as written — a runnable predicate outlives any list.
What is bounded is the *work per hit*: choose the encoding once, then apply it mechanically to every
hit. A guard that resists the shared encoding — because deciding whether its dependency is "expected"
needs a judgement this slice has no basis to make — is **recorded on this record and left alone**,
not resolved here. The slice is done when every hit either carries the encoding or carries a one-line
note saying why it could not.

This keeps a suite-wide sweep from becoming a suite-wide redesign. The epic was narrowed on
2026-09-05 to the reproduced failure plus the tier rule; this record stayed in scope because a test
that reports green without running defeats the rule, not because the sweep is cheap.

**Current behavior:**
Three defects, two of them at the tmux boundary.

*The guard tests are unisolated and vacuously green.* Several tests construct this repository's
`SessionGuard` and `PaneGuard` and assert its RAII cleanup policy — which guard kills what on drop,
and what `disarm` suppresses — by creating real, long-lived tmux sessions. They create them on the
**user's default socket**, so they can collide with a developer's live sessions and leave long-sleeping
session residue on a shared runner. Each begins by consulting an availability probe and returning
early when it is false, so a skipped test is reported as a pass. The probe itself is weaker than it
looks: it asks whether a `list-sessions` invocation *ran*, not whether it succeeded — the sibling
session-existence helper has to add an explicit exit-code check, which is the tell — so
it amounts to "is the tmux binary spawnable", and only an absent binary takes the early return.

*The same vacuous-skip defect sits on guards that are not tmux's.* Further tests in the suite return
early when a dependency is missing and are therefore reported as passing on any machine lacking it.
One guards an interactive-shell dependency through a shell-availability probe, in the same module as
the tmux guards; it is a single guard, not a family. Another, in the HTTP module, guards a pane-stream
FIFO test on a combination of root privileges, `sudo`, and a system account; it prints a diagnostic line before returning, which is
better than silence but still reports a pass. They are the same defect as the tmux guards', and they
are in scope here because the encoding decision below is **one** decision, not one per dependency.

Discovery command for the genus — an early return whose condition tests for an external dependency.
It over-returns slightly, so read each hit rather than treating the set as closed:

```
rg -n -B4 '^\s+return;\s*$' src/ | rg 'available\(\)|geteuid|is_ok_and'
```

**Desired behavior:**

*Guard tests, isolated.* The guard tests run against a tmux socket dedicated to the test run
rather than the user's default, so they cannot see or disturb a developer's live sessions and leave
nothing behind on a shared runner. They obtain it through a single named helper —
`guard_test_socket` — rather than each test naming a socket itself, and every tmux command they issue
goes through it. Session residue is cleaned up on the way out even when an assertion fails.

Residue is asserted on the **dedicated socket only**. Asserting that the user's default socket is
untouched was considered and dropped: the honest version of that check would have to enumerate a
developer's live sessions, and the property it protects — the guard tests never address the default
socket — is already carried by every tmux command going through the one helper.

*Absence never reads as success — for every guarded dependency, not only tmux.* One encoding is
chosen and applied to every guard the discovery command returns — the tmux guards, the
interactive-shell guard, and the privilege-guarded FIFO test among them. Applying different encodings per dependency is the outcome to avoid;
the point of doing them together is that a reader learns the convention once.

Rust's default harness has no dynamic skipped outcome, so "skip loudly" is not directly expressible.
The honest encoding is the implementer's choice among a static opt-in, a hard failure where the
dependency is expected, or a reporter that surfaces the condition
(`ADR-260902-0312-01` § Consequences, "A test that needs a real external dependency gets an isolated
one, and its absence never reads as success" — the ADR says *dependency*, and the genus matters here:
root plus `sudo` plus a system account is not a binary). Whichever is chosen, two bounds hold **for every guarded dependency, not only tmux**: a run on a
machine missing that dependency must not be indistinguishable from a run where the guarded tests were
exercised and passed; and if the encoding is a failure, its message must name the missing dependency
specifically, so the condition is distinguishable from a genuine regression in the contract under
test. If the probe is kept in any
form, it checks the exit code and not merely that a process ran.

*Tier.* Every guard test is Integration Tier — they spawn processes and touch shared mutable OS
state. They are **kept and repaired, not deleted**: they assert SilverBond's own cleanup contract, not
tmux's behavior, so the "a dependency is tested by the repository that owns it" rule does not reach
them (`ADR-260902-0312-01` § "A dependency is tested by the repository that owns it", second
paragraph).

**Key interfaces:**
- `guard_test_socket` — the new helper pinning the dedicated socket for the guard tests.
- The availability probe — either removed, or corrected to check the exit code and to make absence
  observable.

**Acceptance criteria:**
- [ ] `rg -n 'guard_test_socket' src/tmux_exec.rs` returns the helper and its call sites introduced by
      this change; no matches before it.
- [ ] `rg -n 'if !tmux_available\(\)' src/tmux_exec.rs` returns no matches; it returns the early-return
      guard at each guard test before this change.
- [ ] With the Integration Tier switch on and tmux absent from `PATH`, the guard tests do not report
      as passed, and the reported outcome names the missing tmux dependency — a signal the passing
      path never emits. With tmux present they run and pass. Observable at the Integration Tier
      command introduced by `ISSUE-260902-0747-01`.
- [ ] The same holds for every other guarded dependency in the suite: with the interactive shell
      absent, and separately with the privilege/`sudo`/system-account combination unavailable, the
      guarded tests **do not report as passed**, and the outcome **names the missing dependency on the
      non-pass path**. Both halves are required and neither alone suffices. A message is not enough on
      its own: the privilege-guarded FIFO test already prints a line naming its missing dependency on
      the very path where it reports a pass, so a criterion satisfied by a message is green before the
      change. A non-pass outcome is not enough on its own either: deleting the guards satisfies it —
      the unguarded body panics on the absent dependency — while building none of the honest encoding
      and producing a failure indistinguishable from a real regression. The acted-on complement: with
      each dependency **present**, the same tests run and pass. The discovery command in Current
      behavior returns no guard that reports a pass on absence after this change.

      For the privilege guard specifically, "present" means root plus `sudo` plus the system account,
      which the CI job does not provide. Satisfy the complement for it wherever that combination is
      actually available and say where; do not silently treat the complement as discharged by the two
      dependencies CI does install.
- [ ] All guarded dependencies use the **same** encoding. A reader can state the convention after
      reading one of them.
- [ ] Residue on the dedicated socket is asserted by an automated fixture, and that fixture carries a
      **positive control**: the same residue check, run against a copy of the guard-test fixture
      differing in exactly one dimension — a session deliberately left un-dropped — reports residue,
      while the real run reports none. Without the control the criterion is vacuously green, because a
      socket that was never created holds no sessions and a run that never happened leaves no residue.
      The check enumerates sessions on the dedicated socket by name after the run, so it distinguishes
      "cleaned up" from "never ran".
- [ ] The guard tests are not executed by the default command after this change. Read this from what
      the command reports it executed, not from `cargo test -- --list`, which enumerates `#[ignore]`d
      tests and so cannot witness an exclusion.

**Out of scope:**
- Any change to production tmux binary resolution. `ADR-260622-0208-01` is respected, not revisited.
- Deleting the guard tests, or moving them upstream to the `tmux-tools` repository. They assert an
  owned contract.
- The process-group termination test, which is explicitly irreducible and stays as written in the
  Integration Tier. It is unguarded — it needs no absent-dependency encoding.
- Changing what any guarded test asserts. This issue changes how absence is reported, not the
  contract under test.
- Taking the struct-field tests off the login-shell path — `ISSUE-260902-0747-14`. That batch shares
  no test function with either batch here, so it was split out by maintainer decision 2026-09-02 on
  the gate's class 6 prong (b); the no-split decision recorded below stands for the two batches that
  remain, which do share their four test functions.
- Consolidating fake-process fixtures — `ISSUE-260902-0747-02`. These tests drive the real binary, not
  a fake.
- The `tmux-tools` tokio unification, tracked by `ISSUE-260902-0445-01` and sequenced after this epic.

## Triage Notes

Minted 2026-09-02 from `PRD-260902-0301-01`; breakdown approved by the maintainer the same day.
Blocked by `ISSUE-260902-0747-01` because the tier assignments in the acceptance criteria need the
selector to exist.

The evidence for both defects, including the exit-code tell in the availability probe and the
measured cost of the login shell, is in `ISSUE-260901-0216-03` § "Related defects found during the
audit". That record also carries two corrections to earlier drafts of the probe diagnosis; the
description above reflects the corrected reading.

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

**Scope widened to every external-dependency guard (2026-09-02).** The round-1 gate found the
vacuous-skip defect on three orphaned guards outside this record's stated scope — a shell-availability
probe in the same module, and a privilege/`sudo`/system-account guard in the HTTP module. The
maintainer was offered an eleventh record or leaving them unowned; neither was taken. They are folded
in here because the honest-absence encoding is **one** decision applied to every guarded dependency,
and a separate record would re-derive the same ADR citation and risk a second, divergent encoding.
The filename's "tmux boundary" is therefore historical. One correction to the round-1 finding: it
listed the shell guard as two sites, but those are the probe's definition and its single use — one
guard, not two.

**AC6 replaced (2026-09-02).** The maintainer chose an automated fixture assertion on the dedicated
socket only, dropping the default-socket half: the honest version of "the user's default socket is
untouched" would have to enumerate a developer's live sessions, and the property it protects is
already carried by every tmux command going through the one helper. The replacement carries a
positive control, because a socket that was never created holds no sessions — without it the
criterion is vacuously green exactly as the original was.

- AC6 (socket residue) names no observation seam and is vacuously green at baseline — no dedicated socket exists yet — so it cannot distinguish "cleaned up" from "never ran".
- Orphaned siblings carrying the identical vacuous-skip defect, with no owner: `zsh_available()` at `src/tmux_exec.rs:3924` / early return `:3875`, and a euid/sudo self-skip at `src/api.rs:4826-4835`. **Empirically confirmed** — all four guard tests report `ok` in under 10ms with tmux off `PATH`.
- AC5's failure-signal requirement passed.
- Count-as-polarity and broken listing template (systematic defects 1–2).

### Readiness gate round 2 — findings

**Readiness gate (cold-reader): FAIL** (round 2)

- **Class 9 arm B req. 2 on AC6 — not waivable.** The signal AC6 names ("the reported outcome names the missing dependency") is already emitted on today's *passing* path: `src/api.rs:4834` prints "different-UID pane stream test requires root, sudo, and the daemon user" and then returns `ok`. AC5 carries both "a signal the passing path never emits" and an acted-on complement; AC6 carries neither, so an implementation that fails those tests unconditionally satisfies it.
- **Class 7 kind (a) — seven rows.** "Four tests…", "Five tests…" and their restatements are load-bearing counts with no deriving command. Both figures are accurate today; the remedy is substitution by command, never a refreshed number.
- **Class 6 prong (b) fires.** Three independently demoable batches. **Maintainer decision 2026-09-02: do not split** — the guard and socket batches share the same four test functions, so the merge seam costs more than the split buys.
- Two `delegable` gaps owe explicit bounds: socket granularity (one per run or one per test, which AC8's residue assertion races) and the tmux-scoped wording of the honest-absence bounds paragraph, which the section generalises to every dependency.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`

### Round 2 repairs applied 2026-09-02

AC6 now turns on a non-pass outcome with an acted-on complement, rather than on a diagnostic message
the passing path at `src/api.rs:4834` already emits. The wider scope — every external-dependency
guard, not only tmux's — is authorized upstream in the PRD rather than claimed by this record, and the
counts in the brief are marked as a snapshot with the derivation command carrying the set. Baseline
stated.

**This record does not split**, decided by the maintainer 2026-09-02. Its batches share the same four
test functions, so a split would put a merge seam through them and cost more than the separation
buys. `ISSUE-260902-0747-05` does split, on the opposite finding.

Awaiting round 3.

### Round 3 findings — 2026-09-02

**Readiness gate (cold-reader): FAIL** (round 3)

**This record split.** The struct-field batch is now `ISSUE-260902-0747-14`, on the maintainer's
decision 2026-09-02: the round-3 gate found class 6 prong (b) firing for that batch, which shares no
test function with the socket-isolation or honest-absence batches. The earlier no-split decision
rested on all three sharing four test functions — true of the two that remain, false of the third.

AC6 now requires the absence outcome to *name the missing dependency on the non-pass path*, because
"does not report as passed" alone is satisfied by deleting the guards. The honest-absence bounds are
stated over the genus rather than over tmux. The privilege guard's acted-on complement is bounded
explicitly, since CI provides no root/`sudo`/system-account combination. Every guard count is replaced
by its derivation, and the ADR quotation is corrected from "binary" to "dependency" — the genus
matters where the guard is a privilege combination.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round3.md`.
Awaiting re-gate.
