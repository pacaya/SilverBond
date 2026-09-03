---
id: ISSUE-260902-0747-08
kind: issue
category: enhancement
status: needs-info
summary: The interactive agent poll loops interleave capture, decision and sleep, so the decision logic is reachable only by spawning a process and waiting out real poll intervals — one test already had to hand-inline the loop body to reach an invariant without one
prd: PRD-260902-0301-01
adrs: [ADR-260902-0312-01]
terms: [Logic Tier]
blocked_by: [ISSUE-260902-0747-01, ISSUE-260902-0747-02]
---

## Agent Brief

**Category:** enhancement
**Summary:** Extract a pure step function from the interactive poll loops — state plus one capture plus a supplied instant, returning a typed step outcome — and drive the poll-loop tests through it.

**Baseline:** this slice is blocked by `ISSUE-260902-0747-01` and `ISSUE-260902-0747-02`, so every
criterion below is read against the tree **after both have landed** — the tier selector exists and the
fake-process fixtures are behind shared helpers. Read against today's tree the pre-change halves look
already-satisfied, because today the default command executes everything; that is the wrong reading.

**Current behavior:**
The interactive agent poll loops capture the pane, decide what the capture means, act, and sleep, all
inside one loop body. The deciding is not separable from the capturing, so a test that wants to assert
what the loop concludes from a given sequence of screen contents has to stand up a fake tmux process
and let the loop run at its real poll interval. Several tests do exactly that, and their timing
constructs are load-bearing C-sites — the delays between scripted captures encode the orderings the
assertions depend on, so they cannot simply be scaled.

The repository has already written this seam by hand once. One test builds the capture-progress state
directly from the existing pure functions, feeds it observations, and asserts the same invariant as
its process-spawning sibling — without spawning anything, and materially faster. The speed is the
lesser point: the structural one is that the invariant was reachable without a process at all, and
someone had to hand-inline the loop body to get at it.

**Desired behavior:**
A named step function — `poll_step` — carries the decision. It takes the loop's state, exactly one
capture, and a supplied instant, and returns a **typed step outcome** describing what the loop should
do next. The orchestration loops keep only "capture, step, act on the outcome, sleep, feed it back";
they make no decision of their own.

*Time enters as a parameter, never as a clock read.* The step function reads no clock. A test can
therefore drive it across a sequence of supplied instants arbitrarily far apart while no wall-clock
time passes at all, which is what makes the deadline, idle, stability and escalation behaviors
assertable without waiting. This mirrors the repository's own exemplar — the pane-stream pump function,
which is generic over its reader and takes its retry delay as a parameter, and whose tests pass a zero
duration (`ADR-260902-0312-01` § The three exemplars, "Parameterize the duration").

*Prefer the existing seam.* The node-runner port already seals the tmux boundary for most runtime
tests. This extraction adds a seam inside the poll loop because none exists there; it must not add a
second seam anywhere the port already reaches.

*The tests move onto it.* Every test that currently reaches a poll-loop decision through a fake
process and a real interval drives `poll_step` with supplied captures and instants instead. The set is
defined by that property, not by an enumeration here — derive it, and expect it to include the
auto-approve ordering test alongside prompt-echo literal handling, pre-send literal handling,
permission-prompt answering after a redraw, and the subagent-marker absolute-deadline bound. The
auto-approve test asserts a poll-loop decision (which reply the loop sends, and when) while writing a
fake tmux script and driving both interactive loops at their real interval, so it satisfies the
property exactly; an earlier draft named four tests and silently left it out, which would have shipped
the extraction alongside a test still reaching those loops through a process. Derive the set with
`rg -n 'poll_agent_interactive|wait_for_agent_ready_interactive|run_agent_interactive' src/tmux_exec.rs`
attributed to owning test. They spawn nothing, wait on nothing, and become Logic Tier.

One neighbouring test is deliberately **not** in that set. The pattern-scan capture-coordinates test
already calls only the pure helpers — unhandled-match selection, prompt-tail extraction, common-prefix
length — with no fixture, no process and no clock, and carries no row in the timing audit. It is
already Logic Tier and its tier does not change here. It may be rewritten onto `poll_step` for
consistency, but no acceptance criterion turns on it, and a criterion asserting it *becomes* Logic Tier
would be false at baseline.

*The hand-inlined test stops reimplementing the loop.* The escalation test that currently rebuilds the
loop body from the pure functions is rewritten to call `poll_step`, asserting the same invariant
through the real seam rather than a copy of it.

*Behavior is unchanged.* This is an extraction. The loops must conclude exactly what they concluded
before, for the same inputs, including at the boundaries — the deadline, the absolute deadline, the
idle threshold, the stability window, and the destructive-match handling that must not fire twice.

**Key interfaces:**
- `poll_step` — the extracted step function. Parameters: the loop state, one capture, and the current
  instant. Returns a typed outcome the loop matches on; the outcome type enumerates the loop's
  possible next actions rather than being a boolean or an option.
- The interactive poll loops — reduced to orchestration around `poll_step`.
- The existing capture-progress and dispatch types, which `poll_step` composes rather than replaces.

**Acceptance criteria:**
- [ ] `rg -n 'fn poll_step' src/tmux_exec.rs` returns the step function; no matches before this change.
- [ ] `poll_step` reads no clock: time enters only through its instant parameter. A test drives it
      across supplied instants spanning longer than any deadline in the loop while performing no
      deliberate wait of its own and reading no clock — every temporal input is a supplied instant.
      State it that way rather than as "consumes no measurable wall-clock time": every executed test
      consumes measurable time, and proving otherwise would need an elapsed-time assertion, which is
      the very thing this epic forbids. The structural property is reviewable; the temporal one is
      not.
- [ ] Both **interactive agent** poll loops — the readiness loop and the prompt-response loop — obtain
      their next action from `poll_step` and make no decision of their own beyond capturing, acting on
      the returned outcome, and sleeping. The agent-command query loop is **not** in scope: it decides
      only idle-versus-deadline against its own query timeout, dispatches nothing, and handles no
      destructive match, so folding it into the same outcome type would widen that type for a case it
      does not share.
- [ ] Every test in the derived set drives `poll_step` directly, spawns no process,
      and is executed by the default command after this change; the default command does not execute
      them before it. Read this from what the command reports it executed, not from
      `cargo test -- --list`.
- [ ] The escalation test that currently rebuilds the loop body from the pure functions calls
      `poll_step` instead, and asserts the same invariant it asserts today.
- [ ] The full suite is green across both tiers. The loops' conclusions are unchanged for the same
      inputs, including at the deadline, absolute-deadline, idle-threshold and stability boundaries,
      and a destructive match is still handled exactly once.

**Out of scope:**
- The six runtime decision functions and their event sink — `ISSUE-260902-0747-07`.
- The scripted-delay fixtures elsewhere in the suite whose bare integers encode concurrency orderings
  — `ISSUE-260902-0747-12` owns those; only the tests in the derived set move onto the step
  function.
- Changing what the poll loops conclude, or their poll interval, or the interactive escalation policy.
- The `tmux-tools` tokio unification, which is the root fix for the process boundary and is tracked by
  `ISSUE-260902-0445-01`, sequenced after this epic. This issue leaves the pinned revision untouched.

## Triage Notes

Minted 2026-09-02 from `PRD-260902-0301-01`; breakdown approved by the maintainer the same day.
Blocked by `ISSUE-260902-0747-01` for the tier assignments, and by `ISSUE-260902-0747-02` because the
fake-process fixtures these tests use must be behind named helpers before it is visible which of them
need a process at all.

**Scale snapshot (non-contractual):** `rg -c '0o755' src/tmux_exec.rs` → roughly twenty fake-process
fixture sites in this module (2026-09-02); the poll-loop tests named above are a subset. Their A/B/C
classification is in `ISSUE-260901-0216-03` § "Timing-site audit" — they are predominantly C-sites,
which is why the remedy is a seam rather than a raised bound.

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

- `poll_loop_pattern_scan_uses_capture_coordinates_on_redraw` (`src/tmux_exec.rs:4803-4838`) is **already pure** — it calls only `next_unhandled_pattern_match`, `extract_after_prompt` and `common_prefix_len`; no fixture, no clock, no audit-table row. The set is four tests, not five.
- AC3's "Every interactive poll loop" is unpinned: three loops exist (`src/tmux_exec.rs:1551`, `:1651`, `:1792`), and `query_agent_command` has different semantics.
- Count-as-polarity and broken listing template (systematic defects 1–2).

### Readiness gate round 2 — findings

**Readiness gate (cold-reader): PASS** (round 2)

Every gap `fine` or an explicit delegation; class 6 does not fire; 42 class-7 surfaces all non-blocking, with no kind (a) figure on brief text under edit; class 8 inert; class 9 fires on neither arm.

The promoted risk did not materialise: AC3 states the loop boundary structurally by role and carries no line numbers, and the three production polling loops are exactly the three the brief describes.

**One ruling the maintainer should see.** AC4's pre-change half ("the default command does not execute them before it") is false against the tree at gate time, because no tier selector exists until `-01` lands, and true at this record's declared post-`-01` baseline. The reader ruled class 9 arm A does not fire, on the declared-baseline reading, and flagged that the rubric's literal default would fire. **Settled upstream 2026-09-02:** a slice's baseline is the tree after its blockers land. This record must state that baseline in its brief, which is a post-stamp edit requiring `REOPENED` and a re-gate.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`

### Post-PASS edit 2026-09-02

**Readiness gate (cold-reader): REOPENED** (round 2, 2026-09-02 — the round-2 PASS was reopened by a post-stamp edit; REOPENED is a state change on that round, not a round of its own)

Edited after an authoritative `PASS`, so the stamp above no longer stands and a re-gate is owed. The
edit is the one the round-2 report itself called for: the baseline sentence naming this record's
blockers, which is what makes AC4's pre-change half the check it is meant to be rather than a
statement already false at gate time. Nothing else in the brief moved.

### Round 3 findings — 2026-09-02

**Readiness gate (cold-reader): FAIL** (round 3)

**The set that moves onto `poll_step` is defined by its property, not by an enumeration.** A fifth
test satisfies that property and was silently outside the named four: it writes a fake tmux script,
drives both interactive loops at their real interval, and asserts which reply the loop sends. Leaving
it out would have shipped the extraction alongside a test still reaching those loops through a
process. Out of scope no longer re-closes the set by enumeration.

The `REOPENED` stamp is rewritten in canonical form; it previously carried two verdict words on one
line and admitted a reading under which this record was gated.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round3.md`.
Awaiting re-gate.
