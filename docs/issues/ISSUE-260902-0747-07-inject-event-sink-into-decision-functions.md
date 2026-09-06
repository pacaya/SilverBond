---
id: ISSUE-260902-0747-07
kind: issue
category: enhancement
status: needs-triage
summary: Six large runtime functions carry decision logic reachable only through a real database, because they emit events through a context that owns the store; parameterize the sink so the decisions are callable from a test without one
adrs: [ADR-260902-0312-01]
terms: [Run, Runtime Event, Logic Tier]
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

**Known blocking defect, and a changed premise, from the 2026-09-05 adversarial review.**

The premise changed: the ADR's global identity requirement was narrowed on 2026-09-05 to identity that
is *load-bearing* — determining ordering, asserted exactly, or replayed. Much of this brief's identity
work is no longer required by any accepted decision, and reviving it needs the narrowed rule applied
first.

The defect stands regardless: the brief derives its closure from six decision functions, while the
requirement it cites quantifies over every function reaching a clock-derived identity mint. `start_run`
and `restart_from` mint inline (`src/runtime.rs:1408`, `:1564`), initial-checkpoint cursor creation sits
outside the root set (`:2206`), and out-of-crate callers (`tests/http_api.rs:1206`) cannot be served by
replacing an in-crate test constructor. A six-function boundary cannot satisfy a global quantifier.

Also challenged as excess: requiring all six functions to be callable with no database at all does not
follow from determinism, since the brief itself concedes that direct decision tests with a controlled
per-test store already qualify as Logic Tier. If revived, separate identity injection from event-sink
extraction — their callers and outcomes differ — and test the production sink adapter's
commit-before-broadcast ordering separately from a fake sink, since a fake asserting its own recorded
behavior proves nothing about the real adapter (`src/runtime.rs:6985`).

## Agent Brief

**Category:** enhancement
**Summary:** Give the six runtime decision functions an injected async event sink, and whatever else their context needs, so each is reachable from a test without standing up a real store.

**Baseline:** this slice is blocked by `ISSUE-260902-0747-01`, so every criterion below is read
against the tree **after** the tier selector exists.

**Current behavior:**
Six functions in the runtime carry substantial decision logic and can only be called with a runtime
context that owns a real database. They are `select_next_decision`, `apply_join_result`,
`release_collectors_if_ready`, `complete_subflow_if_at_exit`, `handle_approval_resolution` and
`handle_terminal_cursor_status`. Testing any decision they make therefore means standing up storage
and driving the orchestration that calls them, rather than calling the decision and asserting its
outcome. One of the six already returns a plain decision value and is already reached directly from
test code at several call sites, so the seam is half-cut and the shape is not speculative.

**Desired behavior:**
Each of the six is reachable from a test without a real store, by taking its event sink as a
parameter.

*They stay `async`.* An earlier PRD draft said these functions were `async` only because they emit
events and would become synchronous once the sink was parameterized. That diagnosis was wrong and the
target was unimplementable. The event emission path awaits the durable append, propagates its failure,
assigns the durable sequence, and only then broadcasts — and that ordering is load-bearing, because
one of the six propagates an emit failure *before* it mutates the decision log. A synchronous sink
cannot preserve that.

*Ordering and failure semantics are byte-identical afterwards.* Any sink that defers or buffers would
change observable ordering and failure semantics. Preserving append-before-broadcast, and preserving
the existing failure boundary — which failures propagate, and at what point relative to which
mutations — is a requirement of the extraction, not a nice-to-have. Staying `async` costs nothing that
matters: the obstacle to testing these functions was the database, not the `async` keyword.

*Confirm the boundary set before cutting.* The six above are the PRD's list, and this issue owns
confirming it against the code. Neighbouring functions interleave emissions and mutations the same
way, so the set may need to grow or shrink; if it does, say so and why. Do not silently extend it.

*The sink is very nearly sufficient, and no storage fake is needed.* The six do not reach the store
directly: within their own bodies the only context dependency is the event emission, plus a single
abort-state read through the run registry in the join-result function. The database reaches them
through the context type, not through their own code. So parameterizing the sink — plus whatever the
one registry read needs — is expected to discharge the reachability outcome on its own, and a
dictionary-backed store double is neither required nor wanted here (`ADR-260902-0312-01`
§ "A repository fake is considered and deferred").

Confirm that against the code rather than taking it on trust; if a seventh dependency turns up, name
it. The binding constraint is the outcome — **each of the six becomes reachable from a test without
standing up a real store** — and how much of the context must be parameterized to achieve that is this
issue's finding, not something the PRD asserts. Prefer parameterizing what is needed over introducing
a new abstraction: the node-runner port already seals the tmux boundary for most runtime tests, and
the instruction is to use existing seams rather than add another.

*The clock-derived identity question — derive the closure before answering it.* Reading the clock to
*stamp* a record is permitted in the Logic Tier: a timestamp written as data gates no control flow.
**Deriving an identifier that enters state or event identity from the clock is not covered by that
allowance** (`ADR-260902-0312-01`, § Logic Tier). Every path in the closure gets an **injected
generator**. There is no fork: the ADR withdrew the "or its path is not Logic Tier eligible" branch
by maintainer decision 2026-09-02, so this slice does not choose between injecting and marking. If a
path in the closure genuinely cannot take an injected generator, that is a **re-decision** to raise —
stop and report it; it is not a tier this slice may assign.

The derivation is still owed and still comes first, because it is what tells you how much to inject.
Derive two levels: every clock-derived identifier that enters state or event identity, and every one
of the six functions that can reach one. Two hazards have each defeated a search here before, so plan
for them:

- A mint may be passed as a function **reference** rather than called — `.unwrap_or_else(new_cursor_id)`
  — so a call-syntax pattern such as `name\s*\(` misses it. Match the bare name.
- An identifier minted inline at its use site appears in no helper's call graph at all.

An earlier draft named two functions here. Do not carry a number forward from it, from the PRD, or
from these notes; the tier consequence below is sized by what you derive.

*Tier outcome.* A test that calls one of these decisions directly, spawns no process and waits on no
clock is Logic Tier and belongs inside the gate. With the fork withdrawn there is no second outcome to
write into the tree, and no function in the closure leaves the gate as a result of this slice.

Note that this issue does not create the Logic Tier's first direct call sites. Some already exist —
tests that call a decision directly while constructing a real per-test store, which is controlled data
and therefore already Logic Tier under `ADR-260902-0312-01`. What this issue adds is reachability
*without* a store. Derive which of the six already have a direct call site and which do not — the
`rg` over each function's name is the derivation, and no count is carried here — and note that
"already has a direct call site" and "already reachable without a store" are different questions: the
context type owns the database unconditionally, so today **none** of the six is reachable without one,
which is what AC1 changes.

**Key interfaces:**
- The six named functions' signatures — each takes its event sink as a parameter rather than reaching
  it through a context that owns the store.
- The event sink itself — an async interface whose contract is the current emission path's: append
  durably, propagate the append's failure, assign the durable sequence, then broadcast, in that order.
- The runtime context type — parameterized to whatever degree the reachability outcome requires.
- The generators for the clock-derived identifiers in the closure you derive — one per mint kind, not
  one generator assumed in advance. The runtime mints more than one kind, and the closure decides
  which of them this slice injects.

**Acceptance criteria:**
- [ ] Each of the six functions is called directly from a test that constructs **no** database at all
      — not a temporary-directory one, not any. Observable at each function's own signature: the test
      supplies a sink and whatever else the context requires, and never a store. Before this change no
      such call is possible for any of the six; the direct call sites that exist today all construct a
      real per-test store.
- [ ] A test drives the injected sink to fail its durable append and asserts that the failure
      propagates out of `select_next_decision` **before** the decision log is mutated. The failure is
      identified by a signal no other path emits — the sink returns a distinguished error the
      production append cannot produce, and the assertion matches on that identity rather than on the
      result merely being an error. It is paired with a **positive control** on the same fixture: with
      the sink succeeding, the same drive mutates the decision log. Without the control the criterion
      is satisfied by any earlier failure, which also leaves the log unmutated.
- [ ] The emission ordering is unchanged for every one of the six. Observable at the injected sink,
      by reading back the order in which it recorded what it was handed — never by asserting that a
      call happened, or on call counts or ordering through a spy, which `ADR-260902-0312-01`
      § "Doubles are fakes" forbids. Stated as the two branches the ordering actually has rather than
      as one four-step sequence:
      - *Success:* durable append, then assignment of the sequence the append returned, then
        broadcast. The broadcast carries the assigned sequence; a broadcast before assignment, or one
        carrying a sequence the store did not return, is a change in observable ordering.
      - *Failure:* the append error propagates out of the function before any decision-log mutation,
        with no sequence assigned and no broadcast at all.
      These are mutually exclusive: on the failure branch there is no assignment and nothing to
      broadcast, so no implementation can perform all four steps in sequence. An earlier draft of this
      criterion demanded exactly that and was unsatisfiable; AC2 above already carries the correct
      failure-branch shape, and this criterion must agree with it rather than contradict it.
- [ ] The boundary set is confirmed against the code, and the confirmation is written **at the sink
      interface's own definition** as a comment naming the set and why it stops where it does —
      either the six as listed, or a stated different set with the reason it differs. The bounding
      property is **not** "contains an `emit_event` call": many more functions in the module do, and
      the implementer will find them. It is the PRD's own criterion — a decision function whose
      result a test wants to assert directly, which today can only be reached through a context that
      owns the store. Where a neighbouring function that emits does *not* meet that criterion, the
      comment says so in a clause rather than leaving the reader to re-derive it. A finding recorded
      only in this record's notes does not satisfy this: the notes are not delivered to the next
      reader of the code.
- [ ] The clock-identity closure is derived and recorded **beside the injected generators, in the
      code** — the identifiers, the functions reaching them, and the search that found each, with the
      set stating which of the six are in it and which are not. The destination is the same contract
      surface AC4 uses and for the same reason: a closure recorded under `## Triage Notes` on this
      record reaches no later reader of the code. A closure asserted without the derivation, or
      copied from this brief or the PRD, does not satisfy this.
- [ ] Every function in the derived closure reaches its clock-derived identifiers through an injected
      generator, and a test supplies a deterministic one. No test in the closure is marked out of the
      gate to satisfy this criterion — the fork that once permitted that is withdrawn, so a marker
      standing in for an injection is a failure of this criterion, not an alternative reading of it.
      If a path in the closure cannot take an injected generator, the slice stops and reports it as a
      re-decision.
- [ ] The full suite is green across every category the selector knows, with no behavioral change to
      any run.

**Out of scope:**
- Making these functions synchronous. Rejected on evidence; they stay `async`.
- Faking or replacing the database in production, and in-memory storage. The database stays real; this
  issue removes the *dependency of the decision functions* on reaching one, not the store.
- The run-lifecycle handle and the polling helpers — `ISSUE-260902-0747-05`.
- The interactive poll loop's step function — `ISSUE-260902-0747-08`.
- Extending the extraction to neighbouring functions beyond whatever the confirmed boundary set
  contains.

## Triage Notes

Minted 2026-09-02 from `PRD-260902-0301-01`; breakdown approved by the maintainer the same day.
Blocked by `ISSUE-260902-0747-01` for the tier assignments in its acceptance criteria. It shares no
code with `ISSUE-260902-0747-08`, which cuts the equivalent seam in the tmux module.

The PRD's own record of the corrected diagnosis is in § Implementation Decisions, "Seams go above the
process boundary", second bullet — worth reading before starting, because the earlier wrong target
(make them synchronous) is the intuitive one and the reason it fails is not obvious from the
signatures alone.

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

- AC6 unfalsifiable: two tests already call `select_next_decision` directly with `TempDir` + `EchoRunner`, and `--list` prints names only, so nothing identifies which tests "call these decisions directly".
- AC2's failure signal is generic — `assert!(result.is_err())` satisfies it, and any earlier error also leaves the log unmutated.
- AC4/AC5 end in "recorded" with no destination named.
- The unconditional *Tier outcome* sentence contradicts branch 2 of the clock-identity fork in the same brief.
- **Useful positive finding:** the six decision functions never touch `ctx.db` directly — only `emit_event`, plus one `ctx.registry.is_aborted` in `apply_join_result`. Injecting the sink is sufficient; no storage fake is needed.
- Count-as-polarity, broken listing template, templated closing AC (systematic defects 1, 2, 4).

### Readiness gate round 2 — findings

**Readiness gate (cold-reader): FAIL** (round 2)

- **Class 7 kind (a), build-changing — the clock-identity closure is larger than two.** The brief names one function that mints a time-ordered cursor id and "another" that reaches it transitively; five of the six sink functions reach the mint, and only `select_next_decision` does not. The mint is reached through `.unwrap_or_else(new_cursor_id)` — a function *reference* — so a call-syntax search misses it, which is how the figure survived re-drafting. Branch 2 of the clock-identity fork carries a tier consequence, so the wrong closure marks five tests Integration where the brief says two.
- Corrected upstream and generalised: the PRD now quantifies over every clock-derived identifier entering checkpoint state or event identity, states no membership set, and names both derivation hazards. The runtime mints four such identifier kinds, not one.
- `delegable`: AC3 names no seam; the confirmed boundary set should name `finish_cursor` and `record_transition_for_cursor` as expected members; AC4's "property that bounds the set" is written nowhere, and 22 functions in `src/runtime.rs` share the mechanical property.
- Class 6 does not fire. Class 8 inert. Class 9 does not fire on either arm.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`

### Round 2 repairs applied 2026-09-02

The clock-identity closure is no longer stated as two functions; the brief now instructs deriving it,
names both hazards that have defeated a search here (the function-reference mint and the inline mint),
and sizes the tier consequence against what the implementer derives. A new criterion requires the
derivation itself to be recorded. The rule's quantifier is "state or event identity", matching
`ADR-260902-0312-01` and `CONTEXT.md`, which the PRD's narrower "checkpoint state" phrasing had
diverged from. Baseline stated. Awaiting round 3.

### Round 3 findings — 2026-09-02

**Readiness gate (cold-reader): FAIL** (round 3)

**The clock-identity fork is withdrawn**, by maintainer decision 2026-09-02, and the decision is
`ADR-260902-0312-01`'s. Every path in the derived closure gets an injected generator; a path that
cannot take one is a re-decision to raise, not a tier to assign. The escape was priced for a
two-function closure, and the derivation puts it at most of the runtime's decision functions.

The gate found the branch it removed was unsound anyway: it sent tests to the Integration Tier, whose
membership `CONTEXT.md` decides by isolation of the external world, and a decision function called
through an injected sink isolates nothing external. Nothing upstream named a destination for such a
path. AC5's destination is pinned to the code, AC4 now supplies the bounding property instead of
demanding one, AC3 names its seam and the doubles rule, and the singular "cursor-id generator" is
plural because the runtime mints more than one kind.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round3.md`.
Awaiting re-gate.
