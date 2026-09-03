---
id: PRD-260902-0301-01
scale: epic
stakes: internal
terms: [Run, Runtime Event, Workflow, Logic Tier, Integration Tier, Performance Check, Port, Double]
adrs: [ADR-260902-0312-01, ADR-260622-0208-01]
issues: [ISSUE-260902-0747-01, ISSUE-260902-0747-02, ISSUE-260902-0747-03, ISSUE-260902-0747-04, ISSUE-260902-0747-05, ISSUE-260902-0747-06, ISSUE-260902-0747-07, ISSUE-260902-0747-08, ISSUE-260902-0747-09, ISSUE-260902-0747-10, ISSUE-260902-0747-11, ISSUE-260902-0747-12, ISSUE-260902-0747-13, ISSUE-260902-0747-14]
source: grilling 2026-09-02 (conversation-entered); diagnostic evidence ISSUE-260901-0216-03
---

# A deterministic test suite

## Problem Statement

The Rust test suite fails intermittently under CPU load, and the failures are indistinguishable
from real regressions. Three separate closed records sighted this and each declined to file;
`ISSUE-260901-0216-03` reproduced it twice and traced the cause.

For a maintainer this has three costs. `cargo test --locked` is not a trustworthy acceptance gate —
a red run has to be re-run before it can be believed, which is the exact property an acceptance
gate is supposed to provide. The CI job added by `ISSUE-260826-0637-01` runs on a hosted runner
substantially weaker than a developer machine, so it is more exposed to this defect than local runs
are, and its signal is correspondingly more likely to be a false red. And a developer running the suite alongside other work —
which is how all three original sightings arose — cannot tell whether their change broke something.

The cause is not one bad constant. It is two structural properties of how the suite is written:

**Tests assert on wall-clock time.** Deadlines are fixed literals sized for an idle machine, so a
test that normally finishes with margin fails when the machine is loaded — without the code under
test having changed.

**The test seam sits below the process boundary.** `tmux_tools_core::with_invocation` is not a test
double: it swaps which binary is exec'd, not the mechanism, so a "fake tmux" is a real shell script
and every test using one pays a real fork, exec, pipe drain and reaper poll. Logic that is pure is
reachable only by spawning a process, so the test must wait for one.

Raising the deadlines cannot fix this. The timing sites divide into two kinds: arbitrary headroom,
which is safely raisable, and durations that are load-bearing for an assertion, where raising
deletes the property under test. Both kinds are numerous, and several of the load-bearing ones are
themselves load-fragile — including a promptness bound inside one of the two tests originally named
as flaky.

`ISSUE-260901-0216-03` holds the reproduction and the per-site classification — a table keyed by
owning function, marking each timing construct A (raisable headroom), B (poll tick) or C (load-bearing
for an assertion). That table is the sweep's authoritative input and cannot be re-derived
mechanically.

## Solution

Tests assert on behavior, not on elapsed time. Two tiers, named and enforced.

A **Logic Tier** owns the vast majority of tests. Its rule is **isolation**: a logic-tier test drives
its unit through that unit's own public contract, with every port either replaced by a **fake** or
supplied with **controlled data**, and it asserts on no elapsed time. The unit's altitude varies and
the rule does not — it may be a single module or utility, or a use case reached at an API endpoint.
Inside the unit's boundary everything runs real; doubles appear only at its ports. What that rules
out concretely: no spawned processes, no network, no shared mutable OS state, and — the clock rule
stated precisely — **no sleeping to sequence work, no assertion about elapsed time, and no wall-clock
bound serving as the termination condition of a polling or convergence wait**. That third clause is
the one the reproduced failure violates: `wait_for_run` polls persisted state until it changes and
fails at a deadline, so the clock is what ends the wait, racing a producer it cannot be told about.
In the reproduced failure that producer is a real process, not a descheduled task — § Implementation
Decisions states the mechanism in full.
A bound that only ever fires on a hang is a different construct, and § Testing Decisions draws the
line. Reading the
clock to stamp a record is not forbidden; a timestamp written as data gates no control flow and
cannot flake under contention, so decision functions that stamp their log entries stay eligible, and
tests simply never assert on those fields. **Deriving identity from the clock is a different matter**
and is not covered by that allowance. The rule quantifies over **every clock-derived identifier that
enters state or event identity**, and over every function that can reach one — not over a
named mint and not over a list of functions. The slice that cuts the seam derives both levels itself
— the mints, and the functions that reach them — and two derivation hazards are worth naming because
each has defeated a search here before: a mint may be passed as a function *reference* rather than called
(`.unwrap_or_else(…)`), so a call-syntax search misses it; and identifiers minted inline at their use
site do not appear in any helper's call graph at all. Having derived the set, the slice gives every
path in it an **injected generator**. There is no fork: an earlier draft let the slice choose between
injecting and declaring the path "not Logic Tier eligible", and that second branch is withdrawn in
`ADR-260902-0312-01`. The derivation is still owed and still comes first — it is what tells the slice
how much to inject — but it no longer decides *whether*. A path that cannot take an injected
generator is a re-decision the slice raises, not a tier it assigns itself.

**Controlled data counts as isolation.** A dependency the test fully owns and constructs for itself —
a temporary-directory SQLite database, a committed file read read-only — is deterministic and
refactoring-friendly, and belongs in this tier even though no fake replaces it. That is what keeps
the first-party invariant checks — generated-catalog staleness and shipped-template validation —
inside the gate where they belong, and what lets a decision test use a real per-test store while no
port exists for one. The tier tests decision logic through seams above the process boundary, and its
results are reproducible on any machine under any load.

An **Integration Tier** is small, explicitly labeled, and exercises chains of behavior end-to-end
against **real infrastructure** — a migrated SQLite store, real subprocesses, real FIFOs, real tmux —
because some properties genuinely are about integration and testing them any other way would be a
lie. What this tier isolates is the external world: dependencies that mutate data outside the test or
belong to someone else. It carries generous budgets and runs as a separate CI job so its failures are
visually distinct from a logic regression.

Where behavior is currently observable only by waiting, the fix is to make it observable. The
runtime already holds the signals tests need and does not expose them; exposing them replaces
polling with a real happens-before edge and removes the clock as a side effect.

Success is observable: the Logic Tier passes under heavy CPU contention with the same result as an
idle machine, repeatedly.

## User Stories

1. As a maintainer, I want `cargo test --locked` to give the same answer under load as it does idle, so that a red run is evidence of a defect rather than a prompt to re-run.
2. As a maintainer, I want a red CI result to tell me about my code rather than about the runner's core count, so that I can act on it directly. (The *first* execution may precede this epic — `ISSUE-260901-0216-05` is deliberately not blocked by it — in which case this story is satisfied for subsequent runs.)
3. As a maintainer, I want to run the suite while other work loads my machine, so that I do not have to idle the laptop to trust a test result.
4. As a maintainer, I want a failing test to name the behavior that broke, so that I do not have to distinguish "timed out" from "asserted wrong".
5. As a developer, I want the Logic Tier to run fast enough to sit in a tight edit loop, so that I run it continuously rather than at the end.
6. As a developer, I want an integration-tier test's tier to be stated explicitly at the test, so that I do not have to infer it from whether it happens to spawn a process. (The Logic Tier is the unmarked default — see "Marking is explicit for the Integration Tier only" under Implementation Decisions.)
7. As a developer, I want a written rule about timers and dependencies in tests, so that I know what is expected before review rather than after.
8. As a developer, I want that rule enforced mechanically where enforcement is cheap, so that the convention survives a hurried afternoon.
9. As a developer adding a test for a decision, I want a seam that lets me call the decision directly, so that I do not stand up a subprocess to assert a boolean.
10. As a developer, I want the abort path to be observable through a signal rather than a database poll, so that my test waits on the event it cares about.
11. As a developer, I want fake-tmux fixtures behind named helpers, so that I am not copy-pasting a five-line `chmod` block for the fifty-ninth time.
12. As a developer, I want tests that assert struct fields not to spawn my login shell, so that my `.zshrc` is not part of the test environment.
13. As a reviewer, I want a red CI job to distinguish a logic failure from an integration failure, so that I know whether to look at the diff or the runner.
14. As a reviewer, I want to see promptness properties asserted structurally, so that a passing test means the ordering holds rather than that the machine was fast.
15. As an operator of the CI pipeline, I want the Integration Tier to be a separate job, so that I can give it a different timeout and a different retry policy.
16. As an operator, I want no test to leave a `sleep 600` tmux session behind on a shared socket, so that runners are not polluted by test residue.
17. As a maintainer, I want a clear line between a test of the dependency and a test of our own policy that happens to use it, so that cleanup-contract coverage is kept while genuine dependency coverage can move upstream.
18. As a developer, I want a test of decision logic not to spawn a process, so that I do not pay a fork and exec to assert a boolean.
19. As a future contributor, I want the reasoning behind the tier split recorded as a decision, so that I can tell whether a rule still applies before working around it.
20. As a future contributor, I want the rules phrased against existing code that already does it right, so that I have a concrete model rather than an abstract principle.
21. As a maintainer, I want the timing-site audit preserved with the commands that produced it, so that a future sweep can re-derive which sites are load-bearing instead of re-judging them.
22. As a maintainer, I want the two mechanical hazards documented, so that nobody scales a constant that is used with two opposite polarities.
23. As a maintainer, I want a latent 2-second retry sleep identified before a test reaches it, so that it does not become the next flake.
24. As a maintainer, I want to know that a virtual clock would not have removed the subprocess cost, so that the decision against adopting one is not revisited on false hopes.

## Implementation Decisions

**How to read a set named in this document.** Sets here are of four kinds and only two of them bind.
A **decision** — one opt-in switch, the tier rule's clauses, the seam constraints — is authoritative
and stays so until amended here. An **observed snapshot** — the tests the diagnostic record lists as
the reproduction, the durations it measured — is a closed fact about a recorded event, cited to its
record, and does not go stale. A **current-tree population** — which tests fail a rule today, which
functions reach a mint, how many call sites a helper has — is **never authoritative in this
document**: it is owned by the slice that acts on it, derived at that slice's own baseline, and
recorded there. Anything else is an **illustration**, explicitly non-exhaustive, and cannot assign
work or carry an acceptance criterion.

So a slice never lifts a count or a membership list out of this document's prose. It derives the
population it needs, states the derivation it used, and states what it found. Where two slices need
the same population they consume the same named derivation rather than each writing their own, so
that a disagreement between them is visible instead of silent. A derivation that a query can only
partly settle — tier membership asks what a test isolates, reachability can hide behind a function
reference, and the diagnostic record's A/B/C classification is judgement its own record explains why
syntax cannot recover — yields candidates and witnesses, and the owning slice records the
classification it made over them.

**Location never decides a tier.** An earlier draft made directory location the tier marker in both
directions; that was wrong twice over. It cannot *promote*
a test into `tests/`, because that is a separate crate: `pub(crate)` items and `#[cfg(test)] mod
test_support` are unreachable from it, and an integration target compiles the library *without*
`cfg(test)`, silently flipping the build-profile timeout shims to production values
(`pane_stream_owner_wait_timeout()` and `abort_and_wait_drain_timeout()` both resolve to the long
production budget), so a
relocated liveness test would hang rather than run generously. And it cannot be read in reverse
either: the repository already carries out-of-crate targets — including the one holding the
reproduced failure and the deterministic committed-file invariant checks — and this epic does not
migrate them inward.

**Tier membership is decided by what a test isolates, never by where it lives.** An earlier draft said
a test under `tests/` is Integration Tier "by construction"; that quietly reinstated location as the
marker and contradicted keeping the committed-file readers in the gate, since both of them live under
`tests/`. Location carries exactly one consequence, and it is not a tier assignment: an out-of-crate
target must not depend on the `cfg(test)` shims, because they do not apply across that boundary.

**The selector is a constraint here, not a mechanism.** Choosing it needs the code in hand, and an
earlier draft that pinned one from reading alone was wrong: target-level `required-features` gates
out-of-crate targets but cannot exclude individual tests inside the library harness, where
most integration-tier tests actually live. What the implementation must satisfy:

- The **default command** — a bare `cargo test`, which is what `just test-rust` and the CI job run
  today — executes the Logic Tier and nothing else. The gating command must be the habitual one.
- The exclusion operates **per test, not per target**. It must reach an individual integration-tier
  test wherever it lives — the pane-stream FIFO tests, the database/WAL-sidecar permission assertion,
  the migration-on-`init` behaviors, the process-group termination test and the tmux guard tests all
  sit inside the library harness — and it must exclude **no** logic-tier test, including the
  out-of-crate targets. A target-level mechanism alone cannot do this: `required-features` on a
  `[[test]]` entry decides whole targets and cannot reach a test inside the harness.
- One opt-in switch adds the Integration Tier back, for the separate CI job and for local use.
- Whatever the switch is, it must not collide with the docs-regeneration maintenance writer, which
  already claims `#[ignore]` for a non-tier purpose.

**One temporary exception to the marking rule, and it expires at a named slice.** The out-of-crate
HTTP target is a logic-tier member by subject — its unit is a use case reached at its API boundary
against a store the test owns — but on the tree as it stands it satisfies neither half of the rule:
its fixtures spawn real tmux through the production runner, and its waits are clock-bound. A marking
pass applied to the letter of the rule would therefore mark it Integration Tier and carry it out of
the gate.
That is not allowed here, and the reason is the epic's whole point: the reproduced failure lives in
that target, and relocating it would let the epic satisfy its acceptance by moving the failure rather
than fixing it. The tests in that target whose Logic Tier violations `ISSUE-260902-0747-11` repairs
therefore stay unmarked and in the gate while they violate the rule, as a single named exception
recorded by the slice that draws the boundary (`ISSUE-260902-0747-01`). `-01` derives that set from
the target at its own baseline and records what it derived; this document does not name its members,
and a category label such as "the run-stream tests" is not that set. The exception is about those
tests, not about the target as a whole:
membership is per test, so the preview endpoint's task test being marked Integration Tier (decided
below) is not a second exception and does not disturb this one. A criterion phrased as "the HTTP target
carries no Integration Tier marker" is therefore wrong, and the slices that own the marker and the
fixture both need the per-test phrasing instead. The exception is temporary by construction: `ISSUE-260902-0747-11` repairs
**both** halves — the fixture stops spawning tmux and the waits become awaits on the run stream — and
closes it, after which the target satisfies the rule on the letter as well as the spirit and needs no
exception at all. Closing it on the clock half alone would leave the exception standing.
`ISSUE-260902-0747-10`, which is sequenced after `-11`, holds
the check that no exception survives. Between `-01` and `-11` the gate carries the reproduced failure
honestly, and it is expected to go red under load in that window. It will not go red on every run —
the failure is intermittent, and the reproduction measured two red runs and one green — so a green
run there is not evidence the defect is gone. What the exception buys is that the gate is still
*capable* of showing it; marking the target out would have removed even that.

**"One exception" counts decided exceptions, not unmarked rule-failing tests.** The exception count
is a count of decisions this document has made — one, the HTTP target above — and never a count of
tests that fail the Logic Tier rule while carrying no marker. That second thing is a current-tree
population, so a criterion about it states the derivation and its baseline rather than a number, and
the slice that owns the criterion derives it. A test that fails the rule while carrying no marker and
sitting under no decided exception is either a defect to repair or a classification still to decide;
marking it is neither, and a slice that marks one to turn a check green has answered a question by
side effect rather than satisfying the criterion.

**Marking is explicit for the Integration Tier only.** Every integration-tier test carries an
explicit marker at the test; a test with no marker is Logic Tier and is in the gate. The alternative
— annotating every test in the repository — was considered and rejected: it is roughly a
five-hundred-annotation change against a boundary that keeps moving while the seam slices land, for a
signal the unmarked default already carries.

The cost is real and is accepted with its mitigation named, and with the mitigation's limit named too:
a newly added test that spawns a process and carries no marker lands silently in the gate. The
mechanical enforcement check is what keeps the default honest, which is why it is scoped to the tier
boundary and sequenced after the promotions rather than shipped alongside the split. It catches the
recognizable shapes — an executable fixture written and chmod-ed, a process constructed directly, an
invocation swapped, a socket bound. It does **not** catch a process reached indirectly through
production code: the HTTP target spawned real tmux for four rounds of review by starting a task node
over an API, and no grep over the test body would have found it. That residue is a review obligation
stated in the testing document, not a solved problem, and it is the strongest argument for keeping the
Integration Tier small enough that its membership can be reasoned about.

**CI runs both tiers as separate jobs, and the gate is the Logic Tier.** A red integration job and a
red logic job must be distinguishable at a glance. The gating job runs the Logic Tier only —
otherwise the epic could satisfy its acceptance while `cargo test --locked`, which both the
`justfile` and `rust-tests.yml` invoke unfiltered, still went red on a retained integration
deadline. The Integration Tier runs as a separate, non-gating job. Hiding it from CI is rejected:
unrun tests rot.

**The database stays real; the polling goes.** `Database` is an owned, embedded implementation
detail, not an external dependency to be faked. What makes the runtime tests slow and fragile is
that they *poll* persisted state rather than being told it changed. The registry already holds the
signal — a drained-run token cancelled on clear, with no accessor while its sibling abort signal has
one.

Exposing it *symmetrically* is not sufficient, and the naive version is racy: `RunRegistry::clear`
removes the registry entry **before** cancelling the token, and `start_run` spawns the supervised
task before returning the run id — so a run that finishes quickly is already gone when the caller
looks it up, and an `abort_signal`-shaped accessor returns `None` with no edge left to await. A
completion signal must therefore be handed out at registration or be level-triggered, so it stays
observable after the fact.

That signal covers **terminal** waits only. An earlier draft called those a minority of the sites;
they are the larger share, and the slice states the split from a discovery command rather than from a
figure carried here. The drained-run token
is cancelled by `clear`, i.e. at completion, but the polling helpers are used for intermediate states
too — waiting for a pending approval, or for one queued approval then the next before supplying
responses. The general seam is therefore the **runtime event stream**, which already exists and is
already awaited directly in existing tests. Terminal completion is one event among many.

Reaching it race-free needs a production change, not just an accessor. `subscribe` is a live
broadcast with no replay and returns nothing once the run is cleared, and `start_run` returns the run
id only *after* spawning the executor — so a caller cannot subscribe before the first
`approval_queued` or `approval_required` is emitted. Existing direct-event tests dodge this by
registering a known run id and subscribing before spawning their producer, which is precisely the
"obtained before the run starts" constraint. So the entry point hands back the receiver, or accepts a
pre-registered run id. A race-free journal-plus-replay already exists inside the HTTP streaming path
and is the fallback shape if a live subscription proves insufficient.

The constraint applies to **every run-lifecycle entry point that spawns**, not only to starting a
run: resuming a run and restarting from a checkpoint have the same register-spawn-then-return
ordering, and existing tests of both immediately follow them with the clock-bound polling helpers
this epic removes. Covering only the start path would leave the resume and restart tests polling a
five-second deadline, which is the defect verbatim.

**HTTP endpoint tests are logic-tier tests — once their fixtures stop spawning tmux.** They reach the
runtime only over HTTP and so receive a serialized run id and no in-process handle; an earlier draft
concluded from that they must stay in the Integration Tier with bounded waits. That inference was
wrong, but so was the correction that replaced it, which checked only the clock half of the rule.
Under the rule above an endpoint test with controlled data and no clock satisfies it — the unit is a
use case reached at its API boundary, and a per-test temporary-directory store is controlled data, not
real infrastructure — **and today these tests fail the isolation half outright.**

*What the fixtures actually do.* Two functions carry this between them, and a slice that converts
"the shared fixture" without distinguishing them edits the wrong one. `test_router_with_security`
(`tests/http_api.rs:103`) builds the router's state with `RuntimeContext::new` (`:116`), which
installs the production `TmuxNodeRunner` (`src/runtime.rs:1380-1381`); the `with_runner` constructor
that takes a double is `#[cfg(test)]` (`:1391`) and private (`:1392`), so it is structurally
unreachable from an out-of-crate target by two independent barriers. `create_terminal_echo_run`
(`tests/http_api.rs:185`) is the function that
then starts a workflow whose single node is an `echo` **task**, and a task node is a runner node kind
(`src/runtime.rs:2086-2098`), so the run reaches `run_agent_sequence` (`src/tmux_exec.rs:1036`),
`PaneGuard::acquire` (`:1378`), `spawn_pane` (`:439`) and finally the `new-session` invocation
(`:2215`) through `tmux::run_checked_owned` (`:2229`). These tests spawn real tmux sessions. Removing the database poll
changes none of that, and the earlier claim that it did was never checked against the isolation half
of the rule.

This is where the reproduced failure's mechanism is stated in full, and § Solution's summary of the
`wait_for_run` shape defers to it: the producer those tests wait five seconds for is a real
`tmux new-session` plus an echo plus a teardown, not merely a descheduled async task. The fixture hides the cost, too — it accepts
`Completed`, `Failed` **or** `Aborted` as terminal and never asserts the echo ran, so a missing or
broken tmux satisfies it. Authorization tests pay for real tmux and validate none of it.

*The fixture is not the only isolation violation in this target, and an earlier draft implied it was.*
That draft named the shared fixture and stopped. Two further sites exist. The first is a duplicate:
`run_control_routes_return_typed_client_errors` builds its own inline `echo`-task workflow rather than
calling the shared fixture, so a fixture-only conversion leaves it spawning tmux — which is why the
reproduction names three tests while the shared fixture is called by two. The second is a different
genus and is the one that matters: **`POST /api/test-node` spawns unconditionally, and no port stands
in front of it.** `test_node` builds its invocation through `build_tmux_invocation`, which resolves the
tmux binary through an interactive login shell (`zsh -lic`, the 729–998ms cost `ISSUE-260901-0216-03`
records), and then `run_node_preview` reaches `run_tmux_oneshot` directly. The node-runner port is not
on that path, so there is nothing to substitute and no fixture change reaches it. `test_node_accepts_v3_task_node` asserts
that the preview **succeeded**, so the execution is the subject of the test, not an accident of its
setup.

**The preview endpoint's task test is Integration Tier**, by the rule stated two paragraphs below
rather than by a new one: it genuinely requires task execution, and this epic has not authorized a
runner port across the crate boundary. Marking it relocates nothing — it is not one of the tests the
diagnostic record lists as the reproduction — and membership is per test, not per target, so the rest
of the target stays in the gate. The consequence for `ISSUE-260902-0747-11` is that its positive control
is scoped to the target's Logic Tier members rather than to the whole target, and that its isolation
criterion covers the inline duplicate as well as the shared fixture.

*The fix needs no new production surface, because the shape already exists in the target.* These
tests need a run in a terminal state and its stream token; they do not need a task to execute. An
approval-only workflow — a single `approval` node with no edges — is not a runner node kind, so it
reaches a pending approval, terminalizes on `POST /api/runs/{id}/approve`, and never touches the
runner. `creates_and_approves_runs` (`tests/http_api.rs:1184`) already drives exactly that workflow
over HTTP today. Converting the shared fixture to that shape is what makes the target logic-tier on
the isolation half; awaiting the run stream is what makes it logic-tier on the clock half. Both land
in `ISSUE-260902-0747-11`. If an endpoint test genuinely requires task execution — as the preview
endpoint's task test does today — it is
Integration Tier, or it needs an injectable runner port reachable across the crate boundary — new
production surface, and a decision this epic has not taken.

It is also mechanically reachable with no new production surface, because the run stream is
journal-backed rather than live-only: `stream_run` subscribes first and then reads
`db.list_events(run_id)` unconditionally (`src/api.rs:892-893`), so nothing emitted between the two
is lost; it yields the whole journal through a `SeqFilter` before draining the receiver, and returns
outright if that journal already carries the run's `done` event. A run that has completed and been
cleared has no live receiver at all, and the journal alone still carries it — so a test may start a
run and open the stream *afterwards* and still receive every event.
(`resync_run_stream_from_journal` at `src/api.rs:1060` is a different thing: the recovery path taken
on a resync signal or a lagged receiver. It is not what makes post-hoc subscription safe.) The bounded wait is replaced by awaiting the stream, not
by a shorter deadline.

This inverts the epic's story, and deliberately. The reproduced failure of `ISSUE-260901-0216-03`
lives in the out-of-crate HTTP target; an earlier framing let the epic satisfy its acceptance by
moving that failure out of the gating tier. It stays in the gate and gets fixed in place.

In-memory SQLite is rejected, on three grounds: the measured cost is sleeping rather than I/O; the
connection pool would give each pooled connection its own private database, so a write through one
checkout is invisible to the next; and the connection manager is not file-agnostic — it secures the
database file and its write-ahead-log sidecars before and after connect, which a permission test asserts
on directly. Revisit once the sleeps are gone and I/O is actually the dominant cost.

What is rejected there is **in-memory SQLite specifically** — swapping the database file for
`:memory:`. A dictionary-backed **fake** behind the `Database` interface is a different proposal, and
none of those three objections transfers to it. That proposal is considered and separately deferred
in `ADR-260902-0312-01`, on cost against benefit rather than impossibility: `impl Database`
(`src/storage.rs:154-888`) carries twenty-one methods — eighteen `pub`, two `pub(crate)` and one
private helper — so a fake owes nineteen implementations and a constructor (the private helper is an
implementation detail no second adapter is obliged to reproduce). (An earlier draft said
"some 28"; that figure counted the whole storage module, `WorkflowStore` and `TemplateStore`
included.) The fake is therefore a standing maintenance surface whose drift from real SQLite
semantics — notably the durable sequence the event append assigns, whose ordering is load-bearing —
would fail green rather than red. Controlled data covers the gap meanwhile.

**Production changes are in scope where they make behavior observable.** Three are decided:

- The drained-run signal becomes observable without a registry lookup — handed out at registration or level-triggered, per the seam constraint stated above.
- Every run-lifecycle entry point that spawns hands back the receiver, or accepts a pre-registered
  run id, so that a subscription can be obtained before the run starts — stated in full under "The
  database stays real; the polling goes", which is where the constraint and its reach are decided.
- The abort cancellation token becomes an additional arm of the supervisor's select loops. Today
  the only abort check is at the top of the loop, so worst-case abort latency is one poll tick.
  Adding the arm removes the tick and reduces abort latency; it stands on those merits alone.

  It does **not** dissolve the wall-clock promptness assertions on the abort path, and an earlier
  draft claimed it would. **This document states no count of them**, and the two earlier drafts that
  did — three, then four — were both short. The set is derived, from two sources that disagree and
  must both be consulted: the timing audit's classification table, and an independent sweep for
  elapsed-time assertions across the runtime's test code. The audit's A/B/C classification is *not*
  the filter — an A-classified assertion is still an assertion, and the audit's own prose names a site
  its table scores A as a promptness assertion. Deriving from the table alone is how the previous count
  came up short, and it is how the sweep would miss the one assertion that sits on the supervisor path
  this select-arm change alters. Each assertion the sweep finds needs its own structural replacement,
  decided per path, or an explicit decision to keep it as a timing assertion in the Integration Tier —
  including any that the audit classifies A.

  Because poll and push differ only in latency, the arm itself is asserted **structurally** — the
  supervisor obtains the abort token and selects on it — rather than through a behavioral ordering.
  An ordering assertion cannot separate the two without either parameterizing the poll interval or
  pausing the clock, and this epic authorizes neither.

These are treated as design gaps the tests merely exposed — a fire-and-forget spawn with no
completion signal is under-specified regardless of how it is tested.

**Seams go above the process boundary.** Two extractions are decided:

- The interactive poll loop gets a step function — state plus one capture plus a supplied instant,
  returning a typed step outcome — leaving only "exec, sleep, feed it back" in the orchestration
  loop. The repository has already written this seam by hand once: one test reimplements the loop
  body from the existing pure functions and asserts the same invariant as its sibling without
  spawning anything, and runs materially faster for it. The speed is the lesser point; the
  structural one is that the invariant was reachable without a process at all, and someone had to
  hand-inline the loop body to get at it.
- Six large runtime functions carry decision logic reachable only through a database. They take an
  **injected async event sink** rather than becoming synchronous. An earlier draft said they were
  "`async` only because they emit events" and would become sync once the sink was parameterized;
  that diagnosis was wrong and the target was unimplementable. `emit_event` awaits the durable
  append, propagates its failure, assigns the durable sequence, and only then broadcasts — and the
  ordering is load-bearing, since `select_next_decision` propagates an emit failure *before* it
  mutates the decision log. A synchronous sink cannot preserve that.

  Staying `async` costs nothing that matters: the obstacle to testing these functions was the
  database, not the `async` keyword. An injected sink removes the database while leaving ordering
  and failure semantics byte-identical. One of the six already returns a plain decision value and is
  already reached directly from test code (four call sites across two tests), so the seam is
  half-cut. Any sink that defers or buffers would change observable ordering and failure semantics,
  so preserving append-before-broadcast and the existing failure boundary is a requirement of the
  extraction. The six are `select_next_decision`, `apply_join_result`, `release_collectors_if_ready`,
  `complete_subflow_if_at_exit`, `handle_approval_resolution`, and `handle_terminal_cursor_status`;
  the slice's issue owns confirming that boundary set against the code before cutting, since
  neighbouring functions interleave emissions and mutations the same way.

  The sink is necessary but **not sufficient** to remove the database: `apply_join_result` also reads
  abort state through the registry, and the context type carries the database directly, so a
  database-free context is its own dependency question. The constraint is that each of the six becomes
  reachable from a test without standing up a real store; how much of the context must be parameterized
  to achieve that is the slice's finding, not this PRD's assertion.

Prefer existing seams over new ones. The node-runner port already seals the tmux boundary for the
majority of runtime tests; the work is using it rather than adding another.

**Fake-process fixtures get consolidated behind named helpers.** Executable fake-process fixtures are
overwhelmingly built in the test body rather than behind an extracted helper — `ISSUE-260901-0216-03`
§ "Root cause: the seam is below the process boundary" counts only a handful of them already behind
one — in three forms, not the two an earlier draft named. **Fully inline**: body and mode both set in
the test body. **Half-extracted**: the script body still written in the test while only the permission
step is delegated to a helper that names the mechanism rather than the faked behavior. And
**module-local helpers** that write the body *and* set the mode themselves — the very ones the count
above refers to, which the two-form reading treated as already consolidated when they are only
un-shared. All three are in scope, and the contract is structural: no test body and no module-local
helper writes a script body or sets an executable mode outside the shared surface. The slice derives
the population from a command rather than from this description, because a three-item list closes an
open set no better than a two-item one did.
Consolidation is a precondition for the extractions, not a separate cleanup: it is what reveals which
tests need a process at all.

Two adjacent shapes are **not** fake-executable fixtures and are out of scope. Inline `sh -c` command
strings in the process-supervision module pass a shell program as an argument rather than writing an
executable, and their subject genuinely is process and descendant behavior — timeout bounds,
output-tail capture — so they are Integration Tier regardless of how their bodies are written.
Self-exec child fixtures re-invoke the test binary itself under a marker environment variable; the
child is not a fake, and there is no script to consolidate.

**The four real-tmux tests are kept and repaired, not deleted.** An earlier draft proposed deleting
them as coverage of tmux's own behavior. That was wrong: they construct SilverBond's `SessionGuard`
and `PaneGuard` and assert **this** repository's RAII cleanup policy — which guard kills what on
drop, and what `disarm` suppresses. Deleting them would drop real coverage of an owned contract.

Their hazards are real and must be fixed: they move to the Integration Tier, and they use a dedicated
socket rather than the user's default so they cannot collide with live sessions or leave `sleep 600`
residue on a shared runner.

The third hazard is stated as a constraint because the obvious phrasing does not compile: **a missing
external dependency must not read as success.** The constraint is over the genus, not over tmux: any
test in the Rust suite that returns early because a dependency it needs is absent reports a pass on
every machine lacking that dependency, and every such guard is in scope. Rust's default harness has
no dynamic skipped outcome, so "skip loudly" is not directly expressible; the implementation must
find the honest encoding — a static opt-in, a hard failure where the dependency is expected, or a
reporter that surfaces it — and must not leave absence indistinguishable from success. That encoding
is **one** decision taken once and applied to every guard, not one decision per dependency, which is
why the constraint is stated at this altitude and why a single slice owns it.

The tmux guards are the worked example and show why the probe is weaker than it looks:
`tmux::run(&["list-sessions"]).is_ok()` is true for any process that *ran*, whatever its exit code —
the sibling `tmux_session_exists` has to add an explicit `exit_code == 0` check, which is the tell —
so it amounts to "is the tmux binary spawnable", and only an absent binary takes the early return.
The owning slice derives the full set of guards rather than working from the examples named here.

**The `tmux-tools` tokio unification is out of this epic**, and tracked by
`ISSUE-260902-0445-01`. It is the root fix for the process boundary — SilverBond's sync/async split
is downstream of that dependency exposing a blocking API that busy-polls process completion — and it
remains sequenced *after* this epic's seam work, so that tests are not migrated and then rewritten.
It is not carried here because this epic's acceptance never observes it: the Logic Tier's
determinism under load is delivered by the seam and observability work alone. This epic therefore
leaves the pinned revision untouched.

**`ADR-260622-0208-01` is respected, not revisited.** Resolving the tmux binary through an
interactive login shell is an accepted production decision. The defect is that tests asserting only
struct fields go through that path; the fix is that they use the existing non-resolving constructor,
not that production changes.

**The frontend gets a test timeout that is actually generous.** No suite-wide `testTimeout` is
configured (`ui/vite.config.ts`), so every test without its own override runs on vitest's implicit
default — and that default is the cap the recorded frontend flakes exceeded under full-suite load.
Two tests have exceeded it. One still runs on the default and so is still exposed: the
`InspectorPanel` unlock-secret test, recorded timing out at 5000ms (reported 6160ms) under
full-suite load (`ISSUE-260826-0240-01` § Suite). The other, the `GraphEditor` canvas mount test,
was recorded at ~8.2–8.5s while timing out and has since been given its own `}, 20_000)` override
(`ui/src/features/editor/GraphEditor.test.ts:60`), so it no longer runs on the implicit default —
but it is the source of the largest loaded duration this repository has recorded, **10.79s**, timed
while granted a 60-second budget (code review
`issue-260826-0520-01-code-review-20260831-021621` § M2, under `docs/issues/code-reviews/`).

Writing the implicit value out explicitly would satisfy the letter of this decision while changing
nothing, so the contract is behavioural, and it is sized against that largest recorded duration
rather than against either timing-out observation: the configured value must **exceed 10790 ms** by
a margin the slice states, and must not equal the implicit default. Sizing to a duration observed
*while a test was timing out* would set the cap below one this repository has already seen a passing
run take.

The per-test override is reconciled, not stranded: the slice brings it and the configured value into
one relationship rather than leaving two independent budgets. That is an acceptance criterion of
`ISSUE-260902-0747-03`, not an aside. The change is configuration rather than restructuring, and
cannot disturb the fake-timer assertions, which consume no wall clock.

**Rust does not adopt a virtual clock.** `tokio`'s `test-util` feature stays disabled and
`tokio::time::pause()` stays unused. The structural fixes above remove what virtual time would have
bought: the event-stream seam removes the database polling, and the abort select-arm removes the
poll ticks on the abort path. What virtual time would have been aimed at next is the scripted-delay
fixtures, which encode *concurrency orderings* — a barrier expresses that correctly, whereas virtual
time would only make the wrong expression faster. Those fixtures are not merely an argument against
virtual time; they are work, and `ISSUE-260902-0747-12` owns converting them, deriving their extent
at its own baseline. They are not the only wall-clock users left in the suite and this document does
not enumerate the rest; the decision here does not turn on that set. They do sit on the workflow
decision logic this document places in the Logic Tier, so leaving them unconverted would leave that
logic deterministic only on a quiet machine. Enabling a dependency feature to solve a problem the structural work
already removes is how unused machinery accumulates; the option stays available if the seam work
leaves a real gap.

**The reasoning is recorded in `ADR-260902-0312-01`**, phrased against three places in the codebase
that already do it correctly: a pump function taking its retry delay as a parameter, a timeout
shimmed by `cfg!(test)`, and a test double that synchronizes with a real barrier instead of a sleep.
That ADR also mints five terms in `CONTEXT.md` § Testing: the Logic Tier and Integration Tier terms,
the two the tier rule quantifies over — Port and Double — and Performance Check, the third,
non-gating category. A thin pointer goes in
`CLAUDE.md`. The AI-rules harness migration is filed separately and does not block
this work; when it lands, the operative rules get path-scoped and the ADR remains their source.

**One mechanical check**, scoped to the Logic Tier only. A repository-wide grep would flag the
load-bearing sites this epic exists to protect, so the check is conditioned on the tier boundary. If
the boundary does not end up mechanically obvious, ship the rule advisory rather than ship a gate
that cries wolf.

That fallback needs a test, or it discharges every criterion by declaration. **The boundary is
mechanically obvious when a check can decide any test's tier from the tree alone, without a human
judgement per test** — which the marking decision above is designed to deliver, since Integration Tier
membership is carried explicitly in a single greppable form and Logic Tier is the unmarked default. So
the expected outcome is the gate, not the fallback. Taking the fallback is therefore a **finding**, not
a preference: the slice that would take it states which specific property defeated the check, and the
maintainer confirms before it ships advisory. An implementer may not take that branch on its own
judgement.

## Testing Decisions

A good test here asserts an observable behavior of the unit under test and nothing about how long
it took. Sleeping to sequence work is forbidden; asserting on elapsed time is forbidden here, and
permitted only as the Integration Tier's narrow exception below.

**Liveness comes from a happens-before edge, not from a wall-clock guard.** A logic-tier test awaits
a barrier, a receiver, or a signal handed out before the work starts — something that cannot be
starved into a false failure. A finite timeout is *not* equivalent: it is still a race against wall
time whenever the producer can be starved — descheduled, or, as in the reproduced failure, a real
process the machine is too loaded to run promptly — and that is how the failure happened at a
five-second bound that normally returns in three.

The line between a forbidden bound and a permitted one is **what ends the wait on the passing path**.
A bound is forbidden where the clock is the termination condition — a poll or convergence wait that
reads a real resource until it changes and gives up at a deadline. `wait_for_run` is that shape
exactly, and the genus is named in `ISSUE-260901-0216-03` § Mechanism ("Convergence waits — the test
polls a real resource until it changes") — outside the audit's classification table, which is the
contractual sweep input and classifies timing constructs only. This is deliberately **not** the audit's A/B/C
classification: `wait_for_run` is classified A, a liveness guard, and it is still the reproduced
failure, so the A/C split does not draw this line and must not be used to.

A bound is permitted, as a last resort, where a happens-before edge ends the wait and the bound fires
only on a hang — it never participates in the passing path, so a loaded machine cannot close it.
`pane_stream_pump_honors_explicit_drain_with_receiver_alive` (`src/api.rs:5220`) is that shape: the
drain signal is sent before the await begins, and the one-second `tokio::time::timeout` around it is
hang insurance rather than sequencing. Such a guard is sized so that exceeding it means a genuine hang
rather than a loaded machine, and it carries a comment at the site naming the hang it protects
against. Those last two conditions — the sizing and the comment — are **review-enforced, not
machine-checked**: a mechanical check decides only the structural pair (what ends the wait, and whether
the bound is on the passing path), because neither "generous enough" nor "carries a useful comment" is
decidable by a checker, and a check that demanded them would flag the tier's own template. Hangs are
otherwise caught by the job timeout, which is the right place for that concern.

Elapsed time is an assertion subject only in the Integration Tier, and only where the property under
test genuinely is temporal — a promptness guarantee, not a convenience bound. Such an assertion
carries a generous bound and is documented as deliberate.

**The seam is the unit's own interface, taken as high as possible.** Prefer calling a decision
function over driving the orchestration that calls it; prefer awaiting a signal over polling the
state the signal guards; prefer an injected reader over a spawned process.

**Prior art, all already in the repository:**

- The pump function generic over its reader and taking its retry delay as a parameter. All three of
  its tests pass `Duration::ZERO`; two drive it with a scripted reader and no clock at all, and the
  third uses an in-memory duplex with a one-second outer guard. This is the template.
- The frontend pane-stream client tests, which assert reconnect and backoff policy to the
  millisecond under fake timers, with zero wall-clock. They are the proof that timing *policy* can be
  asserted deterministically. The Rust equivalent is not a virtual clock — see the decision against
  one — but the same instinct: assert the rule, never the elapsed time.
- The abort-blocking test double that synchronizes through a real barrier rather than a sleep.
- The integration-tier storage subset — the migration-on-`init` behaviors and the database/WAL-sidecar
  permission assertion (`src/storage.rs:1812`) — which is the model for that tier: real resources and
  no clock at all. One storage test is deliberately excluded from this exemplar rather than covered by
  it: `writer_progresses_while_a_read_connection_is_checked_out` bounds its *passing* path with a
  250ms timeout (`src/storage.rs:2130`), a C-site the audit flags and the boundary slice owns.

**Modules tested, by tier.** Logic Tier: workflow decision logic, the interactive poll step, capture
delta and prompt extraction, request/response validation, the header-predicate security functions,
schema normalization, the HTTP endpoint round-trips against a per-test
store, and the deterministic first-party invariant checks that read committed files —
generated-catalog staleness and shipped-template validation, and the storage tests that assert this repository's own
persistence logic against a store the test constructs. Integration Tier: pane-stream setup and
teardown over real FIFOs, both tests of the process-supervision module — process-group termination
semantics and the capture-tail bound, since each drives a real `sh -c` child and asserts a duration —
the preview endpoint's task test, which executes a task node through a path with no runner port in
front of it, the `SessionGuard`/`PaneGuard`
drop-policy tests on a dedicated socket, and the storage subset whose subject is the real engine or
the real filesystem — the migration-on-`init` behaviors and the database/WAL-sidecar permission
assertion; the application-host startup test, which binds a real socket and polls a live server to a
deadline; and the self-exec child fixtures, which fork the test binary itself.

**This inventory is not the roster, and no slice may treat it as one.** It names subject areas to make
the rule concrete. The roster is what the marking pass derives by applying the tier rule to every test
in the tree; this paragraph is an illustration of the rule, and where the two disagree the rule wins.
A slice whose acceptance quotes this list by name has miscited it. The self-exec pair carries a marking hazard the
marking slice must cost rather than discover: the child re-invokes the test binary with `--exact
<test name>`, so a selector that excludes the marked parent makes the parent's assertion on the child's
output unsatisfiable.

The capture-tail reader (`read_capture`, `src/proc.rs:149`) is *eligible* for the Logic Tier — it
takes a file handle and could be driven with controlled data and no clock — but no test exercises it
that way today, and creating one is coverage growth rather than flake removal. It is therefore not
claimed by this epic in either direction: the module's two existing tests are Integration Tier per
the paragraph above, and a future direct test of the reader would be Logic Tier by the ordinary
rule, needing no exception.

**Storage splits per test, not as a module.** Nearly every test in `src/storage.rs` works against a
store it constructs itself — a temporary-directory database in most, and in
`bundled_templates_are_all_valid` (`src/storage.rs:1746`) the committed `templates/` tree read
read-only — and both of those are controlled data, so a temp-dir round-trip is not by itself an
integration-tier marker. What is Integration Tier there is the subset whose subject genuinely *is*
the real engine or the real filesystem: the migration-on-`init` behaviors and the database/WAL-sidecar
permission assertion (`src/storage.rs:1812`). The rest assert this repository's own persistence logic
against a store they fully own, and stay in the gate. One further candidate —
`writer_progresses_while_a_read_connection_is_checked_out` (`src/storage.rs:2107`), whose subject is
the real pool's concurrency behavior — is left to the slice that draws the boundary, along with the
rest of the per-test calls.

**A performance assertion belongs to neither correctness tier, so the model gains a third category.**
`validate_workflow_many_calls_against_large_subflow_within_budget` (`src/model.rs:6631`) asserts that
validating a large workflow finishes inside a fifteen-second budget. The timing audit classifies it C
— load-bearing, so raising the budget deletes the property. It is pure validation: it spawns nothing,
touches no external world, and isolates nothing. So it fails the Logic Tier rule (it asserts elapsed
time) while also failing the Integration Tier's definition (its subject is not real infrastructure).

The decision is a **third, non-gating category for performance assertions**, outside both correctness
tiers, marked at the test and added back by its own opt-in switch alongside the Integration Tier's.
The two alternatives were rejected for what they cost. Broadening the Integration Tier to mean
"non-gating" as well as "real infrastructure" would contradict the rule this whole epic rests on —
that membership is decided by what a test isolates — for the sake of one test. Moving the assertion
out of the suite into a benchmark turns a failing test into a number a person has to read, which
deletes the guard the C classification says is load-bearing.

**The test splits, and its correctness half stays in the gate.** It asserts two properties: that
validation finishes inside the budget, and that a large valid workflow yields no error issues. The
second is clock-free logic and is Logic Tier by the ordinary rule; only the timing assertion moves to
the performance category. A slice that relocates the test whole would carry a correctness property
out of the gate as a passenger.

The cost is that both the tier selector and the mechanical tier check have to know about a third
category rather than two. That falls on `ISSUE-260902-0747-01`, which draws the boundary, and on
`ISSUE-260902-0747-10`, which enforces it.

**Explicitly irreducible.** The process-group termination test observes an operating-system
contract — that killing a process group reaches descendants holding inherited descriptors — and
cannot be observed without a real process group. It stays as written, in the Integration Tier.

**Acceptance for the epic** is the success property stated in `## Solution`, and it is a command
rather than a description: `just test-under-load` runs the gating tier repeatedly under CPU
contention and fails if any run fails. The recipe carries the load generator, its cleanup, and the
repetition count, so two people measuring acceptance measure the same thing. A passing suite on a
quiet machine is not evidence.

The recipe reproduced the defect on the current tree twice, which is what makes it a usable gate: it
must go from failing to passing, unchanged, for the epic to be done. Its limit is worth stating,
because the acceptance rests on it — the failure is intermittent, so a passing three-run sample is not
proof of a fix and a failing one is not proof of a regression. The recipe is the resilience check; the
per-slice structural observables are the deterministic half, and neither substitutes for the other.
The recipe also verifies the load it claims to apply, failing rather than reporting a load it did not
establish. It is committed by the slice that
draws the tier boundary, `ISSUE-260902-0747-01`. It cannot remain an uncommitted working-tree change
while a slice's acceptance depends on it.

**The recipe is run at two points, and they accept different things.** `ISSUE-260902-0747-11` runs it
as the *regression* acceptance for the reproduced failure: it is the slice that removes the HTTP
target's clock waits, and a passing run there says that failure is repaired. It is not the epic's
closing acceptance, because `-11` is sequenced behind `-01` alone while later slices go on promoting
tests into the gate after it passes — so a run at `-11` says nothing about the population the epic
actually ships. The **closing** acceptance is `ISSUE-260902-0747-10`, the join node blocked by every
slice that changes Rust test code or moves a test between tiers, and therefore the first point at
which the gate's contents are settled. It runs the recipe over the final gate population, and runs
the Integration Tier once as well so that a test promoted out of the gate is still known to pass
somewhere. Both criteria stand; neither substitutes for the other.

**A slice's baseline is the tree after its blockers land.** Most of this epic's slices are sequenced
behind `ISSUE-260902-0747-01`, and several state acceptance criteria whose pre-change half is only true
once the tier selector exists — "the default command does not execute this test before the change" is
false today, because today the default command executes everything. Read against the tree at the moment
of authorship those criteria look already-satisfied; read against the slice's own baseline they are
exactly the check they are meant to be. The convention for this breakdown is the second reading, and
each slice states its baseline in its brief rather than leaving it to the reader to infer from
`blocked_by:`. Where a criterion is genuinely satisfied at the slice's own baseline, that is a defect in
the criterion and not a licence.

**Acceptance does not land at the tier split.** An earlier framing had it do so: the reproduced
failure is the HTTP target's run-poller, and while HTTP-only tests were Integration Tier, drawing the
boundary honestly would have carried the acceptance by itself. With HTTP endpoint tests in the logic
tier that reading is gone — the reproduced failure sits inside the gate, and regression acceptance
binds no earlier than the slice that removes the HTTP target's clock waits, with closing acceptance
at the join node as stated above. The gate is expected to stay red until the clock waits go.

Acceptance binds to the **gate**, not to a tier in isolation: the gating CI job and the default local
command must both run the Logic Tier, so that satisfying this criterion actually makes the command a
maintainer types trustworthy. An epic that made the Logic Tier deterministic while the gate still
invoked an unbounded integration deadline would not have solved the stated problem.

## Out of Scope

- **Raising timeout constants as a remedy.** Rejected on evidence: it cannot address the
  load-bearing sites, and several of those are themselves load-fragile. No stopgap is taken.
- **Moving storage to in-memory SQLite.** Rejected on three grounds under Implementation Decisions;
  reconsider only after the sleeps are gone and I/O is actually the dominant cost.
- **A dictionary-backed `Database` fake.** A separate proposal, considered and deferred in
  `ADR-260902-0312-01` on cost against benefit. Controlled data — a per-test temporary-directory
  store — covers the gap for this epic.
- **Rewriting `docs/testing.md`'s stale content.** That document claims the HTTP suite binds an
  ephemeral port when it uses an in-process service call, and lists 4 test cases where there are 17.
  This is documentation drift and belongs to the docs-truth family, not here. This epic adds the
  tier documentation it owns and leaves the rest.
- **The AI-rules harness migration.** Its own record (`ISSUE-260902-0306-01`); not a blocker.
- **The `tmux-tools` tokio unification.** Its own record (`ISSUE-260902-0445-01`), sequenced after
  this epic. The root fix for the process boundary, but nothing this epic's acceptance can observe.
- **End-to-end Playwright tests.** Untouched.
- **Any change to production tmux binary resolution.** `ADR-260622-0208-01` stands.
- **Frontend test restructuring.** The frontend already asserts timing policy deterministically
  under fake timers; its tests are not reorganized by this epic. The one frontend change that *is*
  in scope is recorded under Implementation Decisions.

## Open Questions

None. `perf-test-tier` is resolved in § Testing Decisions: a third, non-gating performance category
outside both correctness tiers, with the assertion's correctness half staying in the gate.

## Dimension Scan

> **The round-9 gate stamp is voided and the artifact is awaiting round 10.** This PRD passed round 9
> and carried `gate: passed 2026-09-02`; the stamp is voided by the amendment that withdrew the
> clock-identity fork, decided by the maintainer 2026-09-02 and owned by `ADR-260902-0312-01`.
> Round 9's report and the rounds before it are in `docs/prd/adversary-reports/`; read those rather
> than reconstructing the history here.
>
> **How to gate this document from here.** A finding blocks only if it changes an implementer's
> allowed behavior, the ownership of a slice, or an acceptance criterion. Prose that is merely stale
> or imprecise is corrected as documentation and does not fail the round. Read every set against
> § Implementation Decisions, "How to read a set named in this document": a current-tree population
> stated as authoritative here is a defect whatever its count, and a count that happens to be right
> does not make it one.
>
> The scan rows below are re-verified at each round and this blockquote is deleted when a round passes.

| Dimension | Verdict | Citation |
|---|---|---|
| problem/success | decided | ## Problem Statement; acceptance in ## Testing Decisions |
| scope boundary | decided | ## Out of Scope |
| domain terms | decided | CONTEXT.md § Testing (Logic Tier, Integration Tier, Performance Check, Port, Double — all five minted planned against ADR-260902-0312-01, which carries all five in its own `terms:`); remaining terms resolve to existing CONTEXT.md entries. "Controlled data" is deliberately **not** minted: it is a clause of the Logic Tier definition, stated there in full, and names no thing in the system |
| architecture shape | decided | ## Implementation Decisions; ADR-260902-0312-01; ADR-260622-0208-01 ratified |
| stack | decided | ## Implementation Decisions ("Rust does not adopt a virtual clock"); brownfield otherwise — Rust/tokio/vitest ratified; no new dependency, and the tier selector is left as a constraint rather than a prescribed mechanism |
| data/schema | decided | ## Implementation Decisions ("The database stays real; the polling goes"; the `:memory:`-vs-fake separation); ## Testing Decisions ("Storage splits per test, not as a module") |
| contracts/integrations | decided | ## Implementation Decisions ("The `tmux-tools` tokio unification is out of this epic") — the pinned revision is left untouched; migration tracked by ISSUE-260902-0445-01 |
| UX (if UI) | n/a | no user-facing surface |
| testing/seams | decided | ## Testing Decisions (the tier rule, the seam constraints, and the third non-gating performance category); ADR-260902-0312-01 |
| NFRs | n/a | stakes: internal; no perf, security, compliance or a11y bar is owed |
| ops envelope | decided | ## Implementation Decisions ("CI runs both tiers as separate jobs") |

## Further Notes

The diagnostic evidence, the full per-site classification, and the two mechanical hazards live in
`ISSUE-260901-0216-03`, which is closed as the record that produced this PRD. That audit should be
consulted before any sweep: `TMUX_CLEANUP_TEST_TIMEOUT` is used with opposite polarity at two sites,
and the scripted-delay fixtures encode concurrency orderings in bare integers that no duration
pattern will match.

Two findings from that audit are restated here because each needed an owner, and both now have one:
the retry sleep belongs to `ISSUE-260902-0747-09`, and the HTTP target's sleeps and its polling
helper belong to `ISSUE-260902-0747-11`, the slice that makes that target await the run stream.
Neither originates here — both are recorded in `ISSUE-260901-0216-03` § "Related defects found
during the audit". `run_cursor_task` holds a retry sleep
of `retry_delay` seconds defaulting to **2**, which no current test reaches: the retry loop's
attempt count comes from `retry_count`, and no runtime test sets it, so the loop breaks before the
sleep. The latent cost appears the moment a test sets `retry_count` while leaving `retry_delay`
unset — setting a delay is what would *avoid* the default, not what triggers it. And `creates_and_approves_runs` uses two unconditional sleeps only because
its router helper discards the database handle, leaving it nothing to await.

`ISSUE-260901-0216-05` — reading the result of the CI job's run on the pushed branch — is not blocked by
this epic and does not block it, but the two interact. The first CI run has an elevated chance of
being red for the reason diagnosed here rather than for a defect in the CI workflow definition. Reading a red
first run against this PRD will be quicker than triaging it cold.
