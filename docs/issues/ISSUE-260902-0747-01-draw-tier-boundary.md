---
id: ISSUE-260902-0747-01
kind: issue
category: enhancement
status: needs-info
summary: The default Rust test command runs process-spawning and clock-bound tests alongside deterministic ones, so a red result cannot be trusted; introduce the tier selector, mark every Integration Tier test explicitly with Logic Tier as the unmarked default, harden the acceptance recipe, and split CI into a gating logic job and a non-gating integration job
prd: PRD-260902-0301-01
adrs: [ADR-260902-0312-01]
terms: [Logic Tier, Integration Tier]
---

## Agent Brief

**Category:** enhancement
**Summary:** Make the Logic Tier / Integration Tier split real: a selector the default `cargo test` honours, an explicit marker on every Integration Tier test, two CI jobs, and the tier rule written down.

**Current behavior:**
The Rust suite is one undifferentiated set. A bare `cargo test` — what the `test-rust` recipe and the
Rust CI workflow both invoke — runs deterministic decision-logic tests in the same pass as tests that
fork real shell scripts, open real FIFOs, create real tmux sessions on the user's default socket, and
wait out fixed wall-clock deadlines. A red run does not distinguish a logic regression from a loaded
machine. Nothing in the tree names a tier and no mechanism selects one. `docs/testing.md` describes a
single "Rust integration tests" layer; `CLAUDE.md` says nothing about timing rules in tests.

This record owns `just test-under-load`, the epic's acceptance seam: `ISSUE-260902-0747-11` runs it as
regression acceptance and `ISSUE-260902-0747-10` closes on it.

**Baseline:** no blockers. Read every criterion against the tip of `feature/tmux-panes`, not `main`.

**Desired behavior:**
The two tiers minted by `ADR-260902-0312-01` are enforced mechanically, and the command a maintainer
habitually types is the gating one.

*Tier assignment.* Membership is decided by **what a test isolates, never by where its file lives**
(`ADR-260902-0312-01` § "Tier membership is decided by what a test isolates"). A Logic Tier test drives
its unit through that unit's own public contract with every port either replaced by a fake or supplied
with **controlled data**, and asserts on no elapsed time. An Integration Tier test exercises a chain of
behavior end-to-end against **real infrastructure**; what it isolates is the external world.

Two consequences of that rule decide cases this issue will meet:

- **Controlled data counts as isolation.** A dependency the test fully owns and constructs for itself —
  a temporary-directory database, a committed file read read-only — is Logic Tier even though no fake
  replaces it. A temp-dir database round-trip is *not* by itself an Integration Tier marker.
- **Storage splits per test, not as a module.** Within the storage module, Integration Tier is the
  subset whose subject genuinely is the real engine or the real filesystem — the migration-on-init
  behaviors and the file- and write-ahead-log permission assertion. The rest assert this repository's
  own persistence logic against a store they fully own and are Logic Tier
  (`PRD-260902-0301-01` § Testing Decisions, "Storage splits per test, not as a module").

*The classification input.* The per-site timing classification is contractual and is not re-derived: it
is the table keyed by owning function under `### Timing-site audit` in `ISSUE-260901-0216-03`, marking
each timing construct A (raisable headroom), B (poll tick) or C (load-bearing for an assertion). Consult
it before assigning any test that touches the clock. It records two hazards a pattern sweep gets wrong:
one cleanup-timeout constant is used with **opposite polarity** at two sites, and the scripted-delay
fixtures encode concurrency orderings as bare integers that no duration pattern matches.

*Marking is explicit for the Integration Tier only.* Every Integration Tier test carries an explicit
marker at the test. The Logic Tier is the **unmarked default**: a test with no marker is Logic Tier and
is in the gate. Known cost: a newly added test that spawns a process and carries no marker lands
silently in the gate. `ISSUE-260902-0747-10` is what keeps that default honest.

*Mark against today's tree.* Assign each test by what it isolates **as it is written now**. Do not leave
a test unmarked on the grounds that some later record will make it deterministic.

Expect the Integration Tier to be **large, and to stay large**. The epic was narrowed on 2026-09-05 and
the seam work that would have promoted these tests is deferred (`ISSUE-260902-0747-05` through `-08`,
`-02`, `-09`, `-12`, `-13`). What they would have repaired is retained debt under `ADR-260902-0312-01`
§ Retained debt. A large Integration Tier is the expected outcome here, not a defect and not a
trajectory to anticipate.

*Three test sets this slice owns.* Each is a marking, not a seam change:

- **The application-host startup test.** `starts_host_on_ephemeral_port_and_serves_health`
  (`src/host.rs:129`) binds a real `TcpListener` (`:33`) and gates on `wait_for_health` (`:80-95`), a
  convergence wait that fails at a deadline. It fails both halves of the Logic Tier rule: Integration
  Tier.
- **The self-exec child fixtures** (`src/model.rs:5221`, `:5301`) fork the test binary itself:
  Integration Tier. **Cost this hazard rather than discovering it:** the child re-invokes the binary
  with `--exact <test name>`, so a selector that excludes the marked parent makes the parent's assertion
  about the child's output unsatisfiable. Whatever selector you choose must keep that pair working when
  the Integration Tier is run; test the selector against this pair.
- **`writer_progresses_while_a_read_connection_is_checked_out`** (`src/storage.rs:2107`), whose subject
  is the real connection pool's concurrency behavior and which bounds its passing path with a 250ms
  timeout. Decide it against the rule — its subject is arguably the real engine, and a 250ms bound on
  the passing path is a forbidden mechanism either way — and record which reading you applied.

*Split the large-workflow test.* `validate_workflow_many_calls_against_large_subflow_within_budget`
(`src/model.rs:6631`) asserts two properties: that validation finishes inside a fifteen-second budget,
and that a large valid workflow produces no error issues. The second is clock-free logic and is Logic
Tier by the ordinary rule — separate it into its own test that stays in the gate.

The timing half has nowhere built to go. The **Performance Check** category is defined in
`ADR-260902-0312-01` but is **not built** by the narrowed epic (`PRD-260902-0301-01` § Out of Scope), so
this record adds no third selector and no third marker. Leave the timing assertion in place marked
`#[ignore]`, with a comment naming `ISSUE-260905-2136-05` as the record that owns deciding its fate.
Do not mark it Integration Tier — that would make a tier mean "non-gating" rather than a statement about
what a test isolates.

*One exception, named and temporary.* Membership is per test, so this exception is over tests, not over
the target. In `tests/http_api.rs`, the tests whose Logic Tier violations `ISSUE-260902-0747-11` repairs
stay **unmarked and in the gate** even though they still wait on a clock and the run-creating ones still
**spawn real tmux sessions**. Derive that set by applying the Logic Tier rule to each test in the target
and intersecting with what `-11` scopes; do not take it from a category label such as "the run-stream
tests", which excludes one of the three tests the reproduction names.

The exception exists because the reproduced failure lives in that target, and marking it Integration
Tier would carry that failure out of the gate — satisfying the epic's acceptance by relocating the
defect (`PRD-260902-0301-01` § Implementation Decisions, "One temporary exception to the marking rule").

One test in the same target is **not** covered: the preview endpoint's task test
(`test_node_accepts_v3_task_node`, `tests/http_api.rs:889`) is Integration Tier by the ordinary rule,
because it genuinely requires task execution and no runner port stands on that path.

The exception **expires at `ISSUE-260902-0747-11`**; from that point none exists and
`ISSUE-260902-0747-10` checks that none does. Record it as a single greppable entry — one reason, one
closing record. No second exception is authorized: a test that appears to need one is a question for the
maintainer, not a judgment call for the implementer. Expect the gate to stay red between this issue and
`-11`; that is the correct signal.

*Selector.* The mechanism is the implementer's finding, subject to five constraints. The first three are
from `PRD-260902-0301-01` § Implementation Decisions ("The selector is a constraint here, not a
mechanism") and restated in `ADR-260902-0312-01`; the last two are this record's:

- A bare `cargo test` runs the Logic Tier and nothing else. The gating command is the habitual one.
- The exclusion operates **per test, not per target**. It must reach an individual Integration Tier test
  living inside the library harness — the pane-stream FIFO tests, the storage permission and migration
  tests, the process-group termination test, the tmux guard tests — and must exclude no Logic Tier test,
  including out-of-crate targets. Target-level `required-features` alone does not satisfy this.
- Exactly one opt-in switch adds the Integration Tier back, for the non-gating CI job and local use.
- The switch must not collide with the `#[ignore]`-gated docs-regeneration maintenance writer, which
  already claims `#[ignore]` for a non-tier purpose. Running the Integration Tier must not run the docs
  writer, and running the docs writer must not require running the Integration Tier.
- **The selector must not be `#[ignore]`-shaped.** `cargo test -- --list` enumerates `#[ignore]`d tests
  alongside the rest, so an `#[ignore]`-based tier marker would leave tier membership invisible to the
  listing and make every listing-based check in this epic unfalsifiable. Excluded tests must be
  genuinely not executed by the default command, and the exclusion must be observable in what that
  command reports it ran.

*Location constraint (a consequence of location, not a tier assignment).* An out-of-crate target must not
depend on the `cfg!(test)` build-profile timeout shims: an integration target compiles the library
without `cfg(test)` and those shims silently resolve to their production budgets across that boundary.

*Commands.* `test-rust` keeps invoking the default command and becomes the Logic Tier gate with no edit
to its body. A sibling recipe `test-integration` runs the Integration Tier via the opt-in switch.
`test-under-load` is already tracked (committed 2026-09-05); this record **owns, verifies and hardens
it** — it does not create it. Its body needs no change for the tier split, because narrowing the default
narrows what it measures; it does need the argument hardening in the criteria below.

*CI.* The Rust workflow carries two jobs: a **gating** job running the Logic Tier via the default
command, and a **separate, non-gating** job running the Integration Tier via the switch, so a red
integration result and a red logic result are distinguishable at a glance and can carry different
timeouts and retry policies. Hiding the Integration Tier from CI is rejected — unrun tests rot.

*Documentation.* `docs/testing.md` gains a section naming both tiers, stating the membership rule, and
giving the command for each. `CLAUDE.md` gains a thin pointer to `ADR-260902-0312-01` as the source of
the tier and timing rules — a pointer, not a copy.

**Key interfaces:**
- The tier marker as it appears **at an Integration Tier test** — the single form every such test
  carries, and what a reader checks to know a test's tier.
- The tier selector — the mechanism both the default command and the opt-in switch resolve through. It
  consumes the marker; it is not itself readable at the test.
- The `test-rust`, `test-integration` and `test-under-load` recipes, and the Rust CI workflow's job list.
- The testing document's tier section, and the `CLAUDE.md` pointer.

**Acceptance criteria:**
- [ ] Marker membership is decided by `ADR-260902-0312-01`'s tier rule — **what a test isolates** — not
      by a list of mechanisms. Every test that fails the Logic Tier rule carries the Integration Tier
      marker, except the tests covered by the named exception; no test that satisfies the rule carries
      it. Two sufficient conditions for marking, neither subsuming the other:
      - *Forbidden mechanisms.* Spawning a process, opening a FIFO, creating a tmux session, joining a
        process group, binding a socket, writing or chmod-ing an executable fixture, sleeping to
        sequence work, asserting on elapsed time, or ending a polling or convergence wait at a
        wall-clock deadline.
      - *Real-infrastructure subject.* A test whose subject genuinely **is** the external world, even
        with none of those mechanisms — the database and WAL-sidecar permission assertion and the
        migration-on-`init` behaviors are the worked examples, required marked by
        `PRD-260902-0301-01` § Testing Decisions.
      A bound that fires only on a hang, wrapping an await that a happens-before edge ends on the
      passing path, is **not** a forbidden mechanism and does not earn the marker:
      `pane_stream_pump_honors_explicit_drain_with_receiver_alive` (`src/api.rs:5220`) is the named
      example of a Logic Tier test that keeps such a guard. The marker is a single greppable form, so
      the marked set can be compared against the mechanism sweep as one audit input among two — a
      divergence is a question to answer against the rule, not automatically a defect. Before this
      change no marker exists and the comparison has an empty left side.
- [ ] The tests covered by the exception carry **no** Integration Tier marker and are executed by the
      default command, even though at this issue's completion they still wait on a clock **and the
      run-creating ones still spawn real tmux sessions**. State which tests those are by derivation:
      apply the Logic Tier rule to each test in `tests/http_api.rs`, and record the set together with
      the derivation used. A criterion phrased as "the HTTP target carries no Integration Tier marker"
      is wrong — membership is per test, and `test_node_accepts_v3_task_node` in that same target is
      correctly marked. The exception is recorded in one greppable place naming the tests, the reason,
      and `ISSUE-260902-0747-11` as the record that closes it; an unmarked rule-failing test with no
      such entry does not satisfy this criterion. **Pin the entry's form**: choose one stable token that
      `rg` finds and that appears nowhere else, and state it in this record — `ISSUE-260902-0747-10`
      builds a control that reintroduces an exception entry, and can only do so against a form this
      record fixed.
- [ ] The default command excludes the Integration Tier and the switch adds it back, demonstrated in one
      run through the same seam: the default command's own report of what it executed does **not**
      include `session_guard_kills_session_on_drop_without_disarm`, and the same report taken with the
      opt-in switch **does**. Before this change the default command executes it, so the first half is
      red at baseline; the second half is the acted-on witness that a selector excluding everything
      cannot satisfy. `cargo test -- --list` is not an acceptable observable — it enumerates
      `#[ignore]`d tests and cannot distinguish "excluded" from "listed but skipped".
- [ ] `validate_workflow_many_calls_against_large_subflow_within_budget` (`src/model.rs:6631`) is split:
      a test asserting only that a large valid workflow produces no error issues is executed by the
      default command, and a test carrying only the fifteen-second budget assertion is `#[ignore]`d and
      executed by neither the default command nor the Integration Tier switch. That test's comment names
      `ISSUE-260905-2136-05`. Before this change one test carries both assertions and the default
      command executes it.
- [ ] The application-host startup test, the self-exec child fixtures, and
      `writer_progresses_while_a_read_connection_is_checked_out` each carry a recorded tier decision.
      The self-exec pair passes when the Integration Tier is run through its switch — the `--exact`
      re-invocation still resolves the child — which is the case that falsifies a selector excluding
      marked tests from the child's own invocation.
- [ ] The default command does not execute the pane-stream FIFO tests, the storage permission and
      migration tests, or the process-group termination test — the Integration Tier module behaviors
      named in `PRD-260902-0301-01` § Testing Decisions, "Modules tested, by tier". It does execute the
      generated-catalog staleness check, the shipped-template validation, the HTTP endpoint round-trips,
      and the storage tests that assert this repository's own persistence logic against a per-test store.
- [ ] `rg -n 'Logic Tier' docs/testing.md` returns the tier section introduced by this change; no
      matches before it.
- [ ] `rg -n 'ADR-260902-0312-01' CLAUDE.md` returns the pointer introduced by this change; no matches
      before it.
- [ ] `rg -n 'integration' .github/workflows/rust-tests.yml` returns the second job introduced by this
      change; no matches before it. The integration job is not required for the workflow to report
      success.
- [ ] `rg -n '^test-integration' justfile` returns the recipe introduced by this change; no matches
      before it.
- [ ] `git show HEAD:justfile | rg -n '^test-under-load'` returns the recipe. This is a **precondition to
      verify, not work to do** — it has been tracked since 2026-09-05. If it does not hold, stop and
      report rather than re-adding the recipe.
- [ ] The recipe rejects a non-positive repetition count instead of reporting success. `just
      test-under-load 0` exits non-zero after this change; before it the loop body never runs and the
      recipe reports zero failures.
- [ ] The recipe **verifies the load it claims to apply**: after starting its workers it confirms each
      is alive, exits non-zero if fewer are running than were requested, and reports the count actually
      established alongside the count requested.
- [ ] Running the Integration Tier through the opt-in switch does not execute the docs-regeneration
      maintenance writer, and the `regen-docs` recipe still regenerates the catalog blocks without the
      switch.
- [ ] No out-of-crate test target reads a `cfg!(test)`-shimmed timeout helper.

**Out of scope:**
- Cutting any seam, removing any wall-clock wait, or promoting any test into the Logic Tier. This issue
  records where each test stands today. The seam work is deferred (`ISSUE-260902-0747-02`, `-05`
  through `-09`, `-12`, `-13`) and its subject is retained debt under `ADR-260902-0312-01`
  § Retained debt.
- **Making the gate green.** This issue draws the boundary; it does not fix the reproduced failure. That
  failure is in the HTTP target, which is Logic Tier and therefore inside the gate, so
  `just test-under-load` is expected to stay red after this issue lands. Regression acceptance for that
  failure binds to `ISSUE-260902-0747-11`; the epic's **closing** acceptance binds to
  `ISSUE-260902-0747-10`.
- **Building the Performance Check.** No third selector, no third marker, one `#[ignore]`d assertion.
  Owned by `ISSUE-260905-2136-05`.
- The mechanical enforcement check for the tier rule — `ISSUE-260902-0747-10`, sequenced last because
  the tier assignments must settle before a check can read them.
- Rewriting the testing document's stale claims about the HTTP suite's transport and its test-case
  inventory. Documentation drift owned by the docs-truth family; this issue adds only the tier section.
- The frontend suite, which is `ISSUE-260902-0747-03`.

## Triage Notes

Minted 2026-09-02 from `PRD-260902-0301-01`, first of ten slices; breakdown approved by the
maintainer the same day. This is the epic's spine: every other slice's promotions are expressed
against the boundary this one draws.

**Amended 2026-09-05.** The epic was narrowed to the reproduced failure plus the forward-facing tier
rule, and eight slices were deferred. Read the sentence above as history: the promotions it refers to
are now retained debt (`ADR-260902-0312-01` § Retained debt), and the boundary this record draws is
where the suite stays rather than a waypoint. The Agent Brief above was rewritten the same day — its
prior version carried the working tree's state as a requirement, argued cases already decided, and
specified a Performance Check selector the narrowed epic does not build. Gate-round findings below are
left as they were recorded; several of the defects they raise were repaired by that rewrite rather
than by the round they belong to.

**The epic's acceptance does not land here.** HTTP endpoint tests are Logic Tier, so the reproduced
failure stays inside the gate and has to be fixed rather than relocated. A red gate after this issue
is the expected outcome. Regression acceptance binds to `ISSUE-260902-0747-11` and closing acceptance
to `ISSUE-260902-0747-10`.

**Scale snapshot (non-contractual):** `rg -uu -o '#\[(tokio::)?test[\](]' src/ tests/ | wc -l` →
~500 test attributes (2026-09-02). The per-site timing classification the sweep consumes is the
table keyed by owning function under `### Timing-site audit` in `ISSUE-260901-0216-03`, which is
contractual and does not decay.

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

- Membership rule written as an **if-and-only-if** the ADR never states, and an asserted clause — *"real per-test SQLite round-trips … stays in the Logic Tier"* — that appeared in no artifact. Both invented; both now superseded by the amended `ADR-260902-0312-01`.
- The brief never cites the `ISSUE-260901-0216-03` timing-audit table; it sat only in `## Triage Notes`, which is not delivered to the implementing agent.
- **No acceptance criterion checks the tier marking at all.**
- `## Summary` ("a tier decision recorded at every Rust test") contradicts `## Key interfaces` ("readable at the test") — a ~518-annotation difference in scope.
- AC3 cites the PRD as *naming tests* where it names module behaviors.
- `just test-under-load` is consumed but is not in version control (`git show HEAD:justfile | rg test-under-load` → exit 1).

### Readiness gate round 2 — findings

**Readiness gate (cold-reader): FAIL** (round 2)

- **Class 3/4 — the exception count cannot be stated yet.** AC1 marks every test that fails the Logic Tier rule, listing "asserting on elapsed time" as sufficient, and AC1/AC2 close the set at one exception. The performance assertion (`src/model.rs:6631`) asserts elapsed time and this brief forbids marking it, so two unmarked rule-failing tests exist during the epic. The PRD now distinguishes decided exceptions from undecided classifications; this brief's criteria still count the wrong thing, and cannot be phrased correctly until `[OPEN: perf-test-tier]` closes.
- **Class 1/3 — `writer_progresses_while_a_read_connection_is_checked_out` is unowned here.** The PRD assigns it to "the slice that draws the boundary" twice; this brief never names it, while AC1's rule and its own storage paragraph classify it oppositely. It bounds its passing path with a 250ms timeout.
- **Class 4 — done-ness while `perf-test-tier` is open is undefined.** No acceptance criterion covers the consult instruction, so every criterion can be green with the question never raised.
- **New scope, assigned by the maintainer 2026-09-02:** the application-host startup test and the self-exec child fixtures are Integration Tier markings this slice owns. Neither appeared in any brief through seven gate rounds. The self-exec pair carries a `--exact` selector hazard to cost, not discover.
- Class 6 does not fire. Class 7: 66 rows, none blocking — the only record in the batch with a clean class-7 sweep alongside `-03`, `-08` and `-10`. Class 8 inert. Class 9 does not fire.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`

### Round 2 repairs applied 2026-09-02

`[OPEN: perf-test-tier]` is closed upstream — a third non-gating **Performance Check** category, with
the perf test split so its correctness half stays in the gate — so the consult instruction is gone and
the criteria no longer count an undecided classification as an exception. The exception is now stated
and gated per test rather than per target, with its membership derived here rather than named
upstream. The host-startup test, the self-exec pair and
`writer_progresses_while_a_read_connection_is_checked_out` are named in the brief body with the
`--exact` selector hazard costed. The baseline is stated. Awaiting round 3.

### Edited 2026-09-02, not gated this round

This record was held out of the round-3 wave while two decisions that could have landed on it were
put to the maintainer. Both went elsewhere, and the edits made here are consequences of findings on
siblings. The record's authoritative verdict is still `FAIL` (round 2), so no `REOPENED` is owed.

- **Baseline ref corrected.** It named the tree at `main`, which is 87 commits behind the branch this
  epic is written against; three records carried that phrasing and all three are fixed.
- **Acceptance seam attribution corrected.** `test-under-load` is run by `ISSUE-260902-0747-11` as
  regression acceptance and closed on by `ISSUE-260902-0747-10`; this record had named `-11` alone.

Awaiting round 3.
