---
id: ISSUE-260902-0747-12
kind: issue
category: bug
status: needs-info
summary: The scripted runner double sleeps for real to encode the concurrency orderings its tests assert, so the workflow decision logic at the heart of the gate is deterministic only while the machine is quiet
prd: PRD-260902-0301-01
adrs: [ADR-260902-0312-01]
terms: [Run, Logic Tier, Integration Tier, Port, Double]
blocked_by: [ISSUE-260902-0747-01, ISSUE-260902-0747-05, ISSUE-260902-0747-06, ISSUE-260902-0747-13]
---

## Agent Brief

**Category:** bug
**Summary:** Replace the runner double's scripted sleep with a happens-before edge the test controls, so the orderings these tests assert hold by construction instead of by winning a race.

**Baseline:** this slice is blocked by `ISSUE-260902-0747-01`, `-05`, `-06` and `-13`, so every
criterion below is read against the tree **after all four have landed** — the tier selector exists,
the terminal-run and predicate polling helpers are gone and replaced by awaited signals, and the abort
path no longer asserts elapsed time.

**Current behavior:**
The runtime's scripted runner double takes a per-step delay and, when that delay is non-zero, sleeps
for it inside the double before returning the step's result (`src/runtime.rs:7587-7588`). The delay is
not decoration: it is how these tests express *which branch finishes first*, and the assertions read
that ordering back out of the checkpoint.

The shape is **not** uniform, and assuming it is will send you looking for a slow/fast edge that some
of these tests do not have. The commonest shape is a split dispatching two branches, one scripted slow
and one scripted fast, with the test asserting that the policy under test — fail-fast cancellation,
best-effort continuation, drain-then-fail, collector barrier scoping, epoch invalidation on restart —
did the right thing with the branch that had not finished yet. Others encode something else entirely:
equal delays across three branches standing in for *simultaneous overlap* against a concurrency cap,
a single delay racing a sibling that is not a scripted step at all, and several delays spread across
two subflow frames. Derive the shape per test before converting it; what each set of integers is
standing in for is the thing you have to replace, and it differs. `fail_fast_cancel_stops_slow_siblings` (`src/runtime.rs:12760`)
is the representative case: a slow branch and a fast failing branch, and an assertion that the slow
branch's result never lands. Nothing in the test says "the fast branch completes first" — two integers
say it, and only while the scheduler cooperates.

This is the largest surviving clock dependency in the suite, and it sits on the subject matter the
gate exists to protect. Under contention the two durations can converge or invert: the slow branch
completes before the cancellation reaches it, the result lands, and a correct implementation reports a
failure. That is the epic's defect in its purest form — a test that passes because the machine was
fast rather than because the behavior was right — and it differs from the reproduced HTTP failure only
in that no one has yet caught it red.

The timing audit classifies these deliberately and warns against sweeping them: the delays are
concurrency orderings written as bare integers, which no duration pattern matches and no mechanical
scaling can preserve (`ISSUE-260901-0216-03` § "Timing-site audit", the paragraph beginning "Two
hazards would corrupt any mechanical sweep"). Raising them buys margin and deletes nothing; lowering
them inverts the orderings. Neither is the fix.

Discovery commands:

```
rg -n '\.with_delay\(' src/runtime.rs
rg -n -B2 -A2 'delay_ms > 0' src/runtime.rs
```

The first returns the call sites, the second the sleep they reach. Both are present before this change
and the second returns nothing after it.

**Attribute every call site to its owning test function** rather than counting hits — several tests
carry more than one `with_delay`, so a hit count is not a test count and the two have been confused
here before. `rg -c` compounds that: it prints a count **per file**, not a total.

**Desired behavior:**

*The ordering is expressed as a happens-before edge, not as a duration.* The double gains a way for
the test to hold a step and release it — a barrier, a receiver, or a signal handed out before the run
starts — so that "the fast branch completes while the slow branch is still running" is a fact the test
establishes rather than a race it wins. The repository already carries this exact pattern in the same
test module: `AbortBlockingRunner` (`src/runtime.rs:7897`) blocks a node runner at a real
`tokio::sync::Barrier` the test awaits, and `ADR-260902-0312-01` names it as the tier's exemplar
(§ The three exemplars, "Synchronize with a barrier, not a sleep"). This issue generalizes that
pattern to the scripted runner rather than inventing a second mechanism.

*The assertions are unchanged.* Each test asserts the same property about the same policy afterwards.
What changes is why the property holds: by construction rather than by scheduling luck. A test whose
assertion has to be weakened to survive the conversion has had its ordering misread, and that is a
finding to surface rather than an outcome to accept.

*The delay parameter goes, rather than being set to zero everywhere.* A double that can still sleep
will sleep again. If a step genuinely needs to model duration as *data* — a reported wall time in a
result — that is a field on the result, not a wait.

*Tier outcome.* These tests drive the runtime through the node-runner port against a
temporary-directory database, spawn no process, and assert on no elapsed time. Once the sleep is gone
they satisfy the Logic Tier rule and stay in the gate, which is what makes this issue worth doing:
`PRD-260902-0301-01` § Testing Decisions places workflow decision logic in the logic tier, and that is
exactly the set at stake here. One member of the set,
`parallel_batch_abort_cancels_pending_items_after_next_completion`, is **excluded from this record
entirely** — delay included. Its `with_delay(2000)` is calibrated against its half-second promptness
bound (`ISSUE-260901-0216-03` § "Timing-site audit" records the calibration), so removing the delay
here would leave `ISSUE-260902-0747-06` an assertion whose premise is gone: with an instantaneous
item the batch can terminalize before the abort lands, and the bound goes green having tested
nothing. `-06` owns both halves and this record is blocked on it, which is what lets AC1 below stay
absolute. That test carries more than one `with_delay` call, and `-06` owns **all** of them, not just the
calibrated two-second one — its Out of scope now says so explicitly, because AC1 here is absolute over
the file and any delay left standing in that test would block the field's deletion while this record
is forbidden from reaching in. Derive them at your baseline with
`rg -F -n '.with_delay(' src/runtime.rs` attributed to that owning function, and confirm they are gone
before starting. If any remain, stop and report it: it is `-06` incomplete, not work for this record.

**Key interfaces:**
- The scripted runner double and its step type — the delay parameter is replaced by a
  test-controlled release. It is a double at the node-runner port, so `ADR-260902-0312-01`'s rule
  applies: it names the port it stands in for and holds state a test reads back.
- The split, collector, parallel-batch, subflow-exit and failure-policy tests that script delays. Note
  that this set is **not** enumerated by the timing audit's classification table: that table names only
  `parallel_batch_abort_cancels_pending_items_after_next_completion`, because it classifies elapsed-time
  constructs and these delays are fixture data rather than assertions. The audit covers the fixtures as
  a *sweep hazard*, not as rows. Derive the set from the discovery command and the categories named
  above, and read the audit for the hazard warning rather than for membership.
- No production code changes. This issue touches the test module's double and its callers.

**Acceptance criteria:**
- [ ] `rg -n 'delay_ms' src/runtime.rs` returns no matches; the field, its builder and the sleep it
      reaches are all present before this change.
- [ ] No test in the runtime's test module expresses a concurrency relationship through scripted
      durations — not as a pair standing in for an ordering, not as equal delays standing in for
      overlap, not as a single delay racing a non-scripted sibling. Every relationship an assertion
      depends on is established by a signal the test controls, and a reader can name which one from
      the test body alone.
- [ ] Each converted test asserts the same property about the same policy as before, named test by
      test. An assertion that was weakened or deleted to make a conversion pass is a defect of this
      change, not a permitted outcome; if a conversion cannot preserve one, stop and report it rather
      than landing a weaker test.
- [ ] The converted tests are executed by the default command and carry no Integration Tier marker,
      **because they satisfy the Logic Tier rule in full** — not because the sleep is gone. Removing
      the scripted delay is necessary and not sufficient: check each converted test for every other
      forbidden form, in particular a wait that still ends at a wall-clock deadline. At this record's
      baseline `ISSUE-260902-0747-05` and `ISSUE-260902-0747-13` have replaced the polling helpers
      these tests reached, so that condition should be satisfied — confirm it per test rather than
      assuming it, and report any test where it is not instead of marking it promoted.
      Read execution from what the command reports it executed, not from `cargo test -- --list`.
      `parallel_batch_abort_cancels_pending_items_after_next_completion` is not among them: it is
      excluded from this record entirely and `ISSUE-260902-0747-06` converts and places it.
- [ ] The double holds state the tests read back and records no expectations — no assertion that a
      call happened, no call counts, no ordering of calls asserted against the double itself
      (`ADR-260902-0312-01` § "Doubles are fakes, and every fake names its port").
- [ ] Run outcomes are unchanged: the same runs reach the same terminal states with the same
      `all_results` membership as before this change.

**Out of scope:**
- `parallel_batch_abort_cancels_pending_items_after_next_completion` in its entirety — its delay as
  well as its elapsed-time assertion — and everything else on the abort path. `ISSUE-260902-0747-06`
  owns that test whole, because its delay and its bound are one calibration and converting either
  alone breaks the other. This record is blocked on `-06` for exactly that reason.
- The fake-process fixtures and their permission steps — `ISSUE-260902-0747-02`. The scripted runner
  is an in-process double and writes no script.
- Changing any split, collector, batch or failure policy. This issue changes how a test establishes
  an ordering, never what the runtime does with it.
- Adopting a virtual clock. `tokio`'s test-util feature stays disabled; a happens-before edge is the
  mechanism here, and `PRD-260902-0301-01` § Implementation Decisions is explicit that virtual time
  would only make the wrong expression of an ordering faster.

## Triage Notes

Minted 2026-09-02 from `PRD-260902-0301-01`, after the PRD's round-4 spec gate found this set owned by
no slice.

**Why it was missed for four rounds.** The PRD names the fixtures twice — § Implementation Decisions
("The remaining wall-clock users are the scripted-delay fixtures, which encode *concurrency orderings*
— a barrier expresses that correctly") and § Testing Decisions — but it names them as a *reason not to
adopt a virtual clock*, never as work. Two records then excluded them by name
(`ISSUE-260902-0747-06`, `-08`), which reads as deliberate scoping rather than as a gap, and
`ISSUE-260902-0747-10` asserted they "sleep, so they are Integration Tier, so a tier-scoped check
never sees them" — an assignment no other record makes and which contradicts the PRD's placement of
workflow decision logic in the logic tier. Three records, three answers, no owner. That premise in
`-10` is corrected as part of minting this record.

**Scale.** Derive it with the discovery commands in the brief, attributing each call site to its
owning test. Do not carry a number here: an earlier snapshot took its figure from `rg -c`, which
prints a count per file rather than a total, and call sites outnumber owning tests because several
tests carry more than one delay.

**Why this is the batch's most delicate slice.** Every other record removes a wait whose only job was
to wait. These waits carry meaning: the integers *are* the specification of the concurrency scenario,
and they are not written down anywhere else. Recovering the intended ordering from a pair of durations
is the work, and it is why `-06` and `-08` both pushed the fixtures away rather than absorbing them.
Expect to read each test's assertions to determine what ordering it needs before touching its
scripting.

**Readiness gate:** not yet run on this record.

### Readiness gate round 1 — findings

**Readiness gate (cold-reader): FAIL** (round 1)

Round 1 — this record was minted last and had never been gated.

- **Class 7 kind (a), build-changing — removing the sleep is necessary and not sufficient.** All fourteen owning functions end their wait at `wait_for_terminal_run` → `wait_for_run`, a 5-second deadline over a 20ms poll (`src/runtime.rs:9862`) — the ADR's forbidden convergence-wait shape. The brief says that once the sleep is gone these tests satisfy the Logic Tier rule and stay in the gate. `-05` and `-10` state the two-part condition correctly; this record commits the shorthand, and AC4 rests on it.
- **Class 7 kind (b) — the discovery command needs a required pin.** The set is an inventory the work acts over *and* a set `-05`, `-06`, `-08` and `-10` reference; either condition makes a pin required.
- **Class 3 — the per-test ordering is the specification and is written nowhere.** No criterion requires the record to state, per test, which ordering was encoded and which signal now establishes it, so a conversion that makes two previously-ordered events concurrent satisfies every criterion. Four tests script a single delay and one scripts three equal delays, where "a pair of durations" does not describe the fixture.
- **Class 3 — no criterion pins the surviving set**, so deleting the thirteen in-scope tests turns every criterion green.
- **Class 7 kind (a) — the excluded test carries four `with_delay` calls** (`:11432`, `:11436`, `:11440`, `:11444`), not one; AC1 is absolute over the file, so `-06` must clear all four for it to be reachable, and neither record says so.
- **Class 7 kind (a) — "the shape is uniform"** is false for at least six of thirteen.
- **Class 1 — the Triage Notes call the audit's classification table "the contractual input"** for this record's set; the table names one of the fourteen, and the brief's own Key interfaces says so correctly.
- Class 6 does not fire. Class 9 does not fire on either arm.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`

### Round 2 repairs applied 2026-09-02

The brief no longer says that losing the scripted delay makes these tests Logic Tier. Every owning
function ended its wait at the terminal-run helper, itself a 5-second deadline over a 20ms poll —
the forbidden convergence-wait shape — so the sleep was never the only violation. `blocked_by` now
carries `ISSUE-260902-0747-13` alongside `-05`, which is what actually removes those waits, and the
promotion criterion requires confirming the full rule per test.

The excluded test's multiple `with_delay` calls are named, since AC1 is absolute over the file and
`ISSUE-260902-0747-06` has to clear all of them. Call-site counts are replaced by attribution to
owning tests. Baseline stated. Awaiting round 2 — this record was gated for the first time in the
last batch.

### Round 2 findings — 2026-09-02

**Readiness gate (cold-reader): FAIL** (round 2)

"The shape is uniform" is false — five of thirteen in-scope tests match the slow/fast pair; others
encode simultaneous overlap with equal delays, or race a sibling that is not a scripted step. The
brief now instructs deriving the shape per test, and AC2 forbids expressing a concurrency
*relationship* through scripted durations rather than only a pair standing in for an ordering.

The positional inventory of the excluded test's call sites is replaced by its derivation, and `-06`
now explicitly owns all of them — previously it obliged only the two-second one, which would have left
three sites standing that this record can neither delete nor touch.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round3.md`.
Awaiting re-gate.
