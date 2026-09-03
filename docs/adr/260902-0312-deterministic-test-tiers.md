---
id: ADR-260902-0312-01
status: accepted
terms: [Logic Tier, Integration Tier, Performance Check, Port, Double]
prd: PRD-260902-0301-01
---

# Test tiers: tests assert behavior, never elapsed time

SilverBond's Rust suite failed intermittently under CPU load, and the failures were
indistinguishable from real regressions (`ISSUE-260901-0216-03`, reproduced twice). The cause was
not one mis-sized constant. Tests asserted on wall-clock time, and the test seam sat
below the process boundary: `tmux_tools_core::with_invocation` swaps *which binary is exec'd*, not
the mechanism, so each "fake tmux" is a real shell script and every test using one paid a real
fork, exec, pipe drain and reaper poll. Logic that was pure could only be reached by spawning a process, so the tests had to wait for
one.

Raising the constants was rejected on evidence: the timing sites divide into arbitrary headroom,
which is safely raisable, and durations load-bearing for an assertion, where raising deletes the
property under test — and several of those are themselves load-fragile, including a
promptness bound inside one of the two tests originally reported as flaky.

The repository is therefore split into two named tiers.

**Logic Tier.** The large majority of tests, and the gate. A Logic Tier test drives its unit through
that unit's own public contract, with every port either replaced by a **fake** or supplied with
**controlled data**. The unit's altitude varies and the rule does not: it may be a single module or
utility, or a use case reached at an API endpoint. Inside the unit's boundary everything runs real;
doubles appear only at its ports. No spawned processes, no network, no shared mutable OS state, and no
clock — a Logic Tier test never sleeps to sequence work, never asserts on elapsed time, and never lets
a wall-clock bound be the termination condition of a polling or convergence wait. A bound that fires
only on a hang is a different construct; § Liveness below draws that line.

Reading the clock to *stamp* a record is not forbidden: a timestamp written as data gates no control
flow and cannot flake under contention, so decision functions that stamp their log entries stay
eligible, and tests simply never assert on those fields. Deriving *identity* from the clock is not
covered by that allowance — a time-ordered id that enters state or event identity **needs an injected
generator**. There is no second branch: an earlier draft allowed "or its path is not Logic Tier
eligible", and that escape is withdrawn. It was priced when the closure was assumed to be two
functions; the derivation puts it at most of the runtime's decision functions, and a tier that sheds
them is not the gate this decision exists to build. A path that cannot take an injected generator is
therefore not a tier assignment but a **re-decision**: the slice stops and says so, rather than
marking the tests and moving on. This is deliberate — it makes the exception visible instead of
silent, and nothing in this ADR gives such a path a home in the Integration Tier, whose membership is
decided by isolation of the external world, or in a Performance Check, whose subject must be elapsed
time.

**Controlled data counts as isolation.** A dependency the test fully owns and constructs for itself —
a temporary-directory SQLite database, a committed file read read-only — is controlled, deterministic
and refactoring-friendly, and belongs in this tier even though no fake replaces it. This is what keeps
the generated-catalog staleness check and the shipped-template validation inside the gate, and what
lets a decision test use a real per-test store while no port exists for one.

**Integration Tier.** Deliberately small and explicitly labeled. It exercises chains of behavior
end-to-end against **real infrastructure** — real SQLite round-trips against a migrated store, real
subprocesses, real FIFOs, real tmux — because the property under test genuinely is about integration
and testing it any other way would be a lie. What this tier isolates is the **external world**:
dependencies that cost money, mutate data outside the test, or belong to someone else. It carries
generous budgets and runs as the non-gating CI job so its failures are visually distinct from a Logic
Tier regression. The gating job runs the Logic Tier, so that the command a maintainer types is the one
this decision makes trustworthy.

**A third, non-gating category holds performance assertions.** Both tiers above are correctness
tiers, and a test whose subject genuinely is elapsed time belongs to neither: it fails the Logic
Tier's clock rule, and its subject is not real infrastructure, so calling it Integration Tier would
contradict the rule that membership is decided by what a test isolates. Such a test is marked into a
separate non-gating category with its own opt-in switch, alongside the Integration Tier's. The
alternatives were to broaden the Integration Tier to mean "non-gating" — which would spend the
isolation rule, the load-bearing claim of this decision, on a small number of tests — or to move the
assertion out of the suite into a benchmark, which converts a failing test into a number someone has
to read and so deletes the guard. Where such a test also asserts a clock-free correctness property,
that property is separated out and stays in the gate rather than leaving it as a passenger.

Liveness comes from a **happens-before edge**, not from a wall-clock guard: a barrier, a receiver, or
a signal handed out before the work starts — something that cannot be starved into a false failure. A
finite timeout is not equivalent; it is still a race against wall time whenever the producer can be
starved — descheduled, or, as in the reproduced failure, a real process the machine is too loaded to
run promptly — and that is how the failure happened at a bound that normally had ample margin.
What separates a forbidden bound from a permitted one is **what ends the wait on the passing path**.
Forbidden: the clock ends it — a poll or convergence wait that reads a real resource until it changes
and gives up at a deadline. `wait_for_run` is exactly that, and it is the reproduced failure. Note
that this line is *not* the timing audit's A/B/C classification: `wait_for_run` is an A-site, a
liveness guard, so classifying a bound as A does not make it permissible. Permitted, as a last resort:
a happens-before edge ends the wait and the bound fires only on a hang, never participating in the
passing path — `pane_stream_pump_honors_explicit_drain_with_receiver_alive` wraps an already-signalled
drain in a one-second timeout, which a loaded machine cannot close. Such a guard is sized so that
exceeding it means a genuine hang, and carries a comment at the site naming the hang it guards. The
sizing and the comment are review-enforced; only the structural pair is mechanically checkable.
Hangs are otherwise the job timeout's concern. Sleeping to sequence work is forbidden in both tiers;
asserting on elapsed time is forbidden in the Logic Tier, and permitted only as the Integration Tier's
narrow exception below.

Asserting on elapsed time is permitted only in the Integration Tier, and only where the property genuinely *is*
temporal — a promptness guarantee the system owes — with a generous bound and a note saying why.
Convenience bounds do not qualify.

**Tier membership is decided by what a test isolates, never by where it lives.** Location was
considered as the marker, in both directions, and is wrong in both.

It cannot *promote* a test by relocation. `tests/` is a separate crate: `pub(crate)` items and
`#[cfg(test)] mod test_support` are unreachable from an integration target, and such a target
compiles the library **without** `cfg(test)`, which silently flips the build-profile timeout shims
from their test values to their production ones — `pane_stream_owner_wait_timeout()` and
`abort_and_wait_drain_timeout()` both resolve to their production budgets. A relocated liveness test
would hang rather than run
with a generous budget.

Nor does it work in reverse. Out-of-crate targets already exist, and some of them — the
generated-catalog staleness check and the shipped-template validation — are deterministic
first-party invariants that belong in the Logic Tier and in the gate. Location carries exactly one
consequence, and it is not a tier assignment: an out-of-crate target must not depend on the
`cfg(test)` shims, because they do not apply across that boundary.

The selector is left to the implementation, as a constraint rather than a mechanism: the **default
command must run the Logic Tier and nothing else**, the exclusion must operate per test rather than
per target — reaching an individual integration-tier test inside the library harness, and excluding no
logic-tier test, including the out-of-crate targets that are in the gate — one opt-in switch must add
the tier back, and the switch must not collide with the docs-regeneration maintenance writer.
Target-level `required-features` alone does not satisfy this — it cannot exclude individual tests
inside the library harness, which is where most integration-tier tests live.

**A dependency is tested by the repository that owns it.** SilverBond does not test tmux, and does
not test `tmux-tools` on its behalf. Tests that drive a real external binary to observe *that
binary's* behavior belong upstream.

The converse matters as much, and is easy to get wrong: a test that drives a real external binary to
observe **SilverBond's own** behavior is SilverBond's test and stays. The `SessionGuard` and
`PaneGuard` drop tests spawn real tmux sessions, but what they assert is this repository's RAII
cleanup policy — which guard kills what on drop, and what `disarm` suppresses. They belong to the
Integration Tier, not upstream and not the bin.

## Doubles are fakes, and every fake names its port

A double is an **alternative implementation of a port** — the same interface, a different mechanism,
usually in-memory. It is not a recorded expectation. Assert by reading state back through the fake,
never by asserting that a call happened; interaction verification is a last resort, reserved for
outgoing command-type port calls with no observable state, and even then it asserts message content,
never call counts or ordering. Every double names the port it is the second adapter for — a double
that cannot name one is standing in for an internal collaborator, and the test should drive the
enclosing interface instead.

The worked example is already in the tree. `NodeRunner` is a port, and the suite carries eight
implementations of it. That is the boundary where doubles earn their keep in this repository, because
what sits behind it is process execution.

**A repository fake is considered and deferred.** A dictionary-backed implementation behind the
`Database` interface would put storage under the same discipline. It is not taken now, and the reason
is cost against benefit rather than impossibility. `impl Database` (`src/storage.rs:154-888`) carries
twenty-one methods — eighteen `pub`, two `pub(crate)`, one private helper — so a fake owes nineteen
implementations and a constructor, the private helper being an implementation detail no second
adapter is obliged to reproduce; the "28" an earlier draft used counted the whole storage module,
`WorkflowStore` and `TemplateStore` included. The fake would be a standing maintenance surface whose
drift from real SQLite semantics — notably the durable sequence the event append assigns, whose
ordering is load-bearing — would fail green rather than red;
and the measured cost of the real store is sleeping rather than I/O, so the speed argument is weak.
Controlled data covers the gap meanwhile. Revisit when storage becomes the bottleneck, or when the
semantics under test stop depending on the real engine.

This supersedes nothing in `PRD-260902-0301-01`. That record rejected **in-memory SQLite** — swapping
the database file for `:memory:` — which is a different proposal, and its objections about the
connection pool handing each checkout a private database and the connection manager not being
file-agnostic do not transfer to a fake.

## The three exemplars

This decision is not invented. Three places in the codebase already do it correctly, and they are
the normative reference — a rule phrased against working code is harder to argue with than a
principle.

- **Parameterize the duration.** `pump_pane_stream` is generic over its reader and takes
  `retry_delay: Duration` as a parameter; its tests pass `Duration::ZERO`, and most drive it with a
  scripted reader and no clock at all. This is the template for the Logic Tier.
- **Shim the timeout by build profile.** `pane_stream_owner_wait_timeout()` returns a short budget
  under `cfg!(test)` and the production one otherwise, so the production budget never leaks into a
  test's wall time.
- **Synchronize with a barrier, not a sleep.** `AbortBlockingRunner` coordinates through a real
  `tokio::sync::Barrier` awaited by the test, rather than sleeping long enough for the other side to
  probably be ready.

## Consequences

**Wall-clock assertions become structural ones.** Where a test asserted that an operation was
prompt, it asserts the ordering that promptness was standing in for. Where production behavior was
observable only by polling persisted state, the production code gains the signal that makes it
observable. The general seam is the runtime event stream, which already carries the intermediate
states the polling helpers waited on; a drained-run signal covers terminal waits only, and the abort
select-loop arm removes the poll tick. Any such handle must be obtained before the work starts — at every
run-lifecycle entry point that spawns, not only at run creation — or be level-triggered — a registry lookup after the fact returns nothing for a run that has already been
cleared, which is the defect that made polling look necessary in the first place.

These are treated as design gaps the tests exposed, not as testability hacks: a fire-and-forget
spawn with no completion signal is under-specified regardless of how it is tested.

**The tier is a contract about what a test isolates, not about where its file sits.** A test is in the
Logic Tier because every port it depends on is faked or its data controlled, and because it asserts on
no elapsed time — not because of its path. Marking is explicit precisely because the structural signal
was unavailable.

**A test that needs a real external dependency gets an isolated one, and its absence never reads as
success.** Real-tmux tests use a dedicated socket rather than the user's default, so they cannot
collide with a developer's live sessions or leave residue on a shared CI runner. A silent `return`
that reports a pass is worse than no test: it passes vacuously on every machine where the dependency
is absent, and nothing distinguishes that from a real pass. This holds for every such guard in the
suite, not only tmux's — the honest encoding is one decision applied across them, not one per
dependency. Rust's default harness has no dynamic
skipped outcome, so the honest encoding — a static opt-in, a hard failure where the dependency is
expected, or a reporter that surfaces it — is the implementation's to choose; what it may not do is
leave absence indistinguishable from success.

**Some tests are irreducible and stay real.** The process-group termination test observes an
operating-system contract — that killing a process group reaches descendants holding inherited
descriptors — and cannot be observed without a real process group. Being in the Integration Tier is
the correct outcome for such a test, not a failure to migrate it.

**Enforcement is scoped, or advisory.** A repository-wide grep for durations would flag the
load-bearing sites this decision exists to protect. Any mechanical check is conditioned on the tier
boundary; if that boundary is not mechanically obvious, the rule ships advisory rather than as a
gate that cries wolf. The boundary is mechanically obvious when a check can decide any test's tier
from the tree alone with no per-test human judgement — which explicit Integration Tier marking against
an unmarked Logic Tier default is designed to deliver, so the gate is the expected outcome. Taking the
advisory fallback is a finding to be argued and confirmed, naming the property that defeated the
check; it is not an implementer's preference.

**`ADR-260622-0208-01` is unaffected.** Resolving the tmux binary through an interactive login shell
remains the accepted production behavior. The defect this decision addresses is that tests asserting
only struct fields went through that path; they use the existing non-resolving constructor instead.

Delivery is tracked by `PRD-260902-0301-01`.
