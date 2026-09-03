---
id: ISSUE-260902-0747-14
kind: issue
category: bug
status: needs-info
summary: Tests that assert nothing but pure fields of the constructed tmux invocation reach them through the resolving constructor, which spawns an interactive login shell — sourcing the developer's own shell configuration into the test environment and costing most of a second per call
prd: PRD-260902-0301-01
adrs: [ADR-260902-0312-01, ADR-260622-0208-01]
terms: [Logic Tier, Integration Tier]
blocked_by: [ISSUE-260902-0747-01]
---

## Agent Brief

**Category:** bug
**Summary:** Move the tests that assert only invocation struct fields onto the existing non-resolving constructor, so they touch no process and become Logic Tier.

**Baseline:** this slice is blocked by `ISSUE-260902-0747-01`, so every criterion below is read
against the tree **after** the tier selector exists. The tier assertions are unobservable before that
and are not meant to be read against today's tree.

**Provenance.** This record was split out of `ISSUE-260902-0747-04` by maintainer decision
2026-09-02, on the gate's class 6 prong (b): its batch shares no test function with either batch that
remains there, is green or red on its own, and its acceptance is two `rg` commands plus one
default-command report. `-04` keeps the socket-isolation and honest-absence batches, which do share
their test functions with each other.

**Current behavior:**
Some tests assert nothing but pure fields of the constructed tmux invocation — the socket, the command
prefix, the user downgrade, the precedence between them — yet reach those fields through the
*resolving* constructor. Resolution runs the tmux binary lookup through an interactive login shell,
which sources the developer's own shell configuration into the test environment and costs most of a
second per call. Nothing in these tests depends on resolution having happened; they assert the struct.

A non-resolving constructor already exists and is already used by at least one neighbouring test, so
the seam this record needs is present — no new abstraction is owed.

Derive the set rather than working from a count. The call sites of the resolving constructor, split
into production and test, are what this record acts on:

```
rg -n 'build_tmux_invocation\(' src/tmux_exec.rs src/api.rs
```

Read each hit: a production call site stays exactly as it is, and a test call site whose assertions
are only about struct fields moves. If a test call site asserts something that genuinely depends on
resolution, it does **not** move — say so and name it rather than forcing it across.

**Desired behavior:**
Every test that asserts only invocation struct fields uses the non-resolving constructor. With the
login shell gone those tests spawn no process, read no clock and touch no shared mutable OS state, so
they satisfy the Logic Tier rule and enter the gate.

Production binary resolution is unchanged. `ADR-260622-0208-01` stands and resolving through an
interactive login shell remains the accepted production behavior; the defect is that these *tests*
went through that path, not that the path is wrong.

**Key interfaces:**
- The resolving and non-resolving tmux invocation constructors — no signature change on either. The
  moving tests change which one they call, nothing more.

**Acceptance criteria:**
- [ ] `rg -n 'build_tmux_invocation\(' src/tmux_exec.rs` returns only the resolving constructor's own
      definition line — no test call sites. Test call sites are present before this change.
- [ ] `rg -n 'build_tmux_invocation\(' src/api.rs` returns only the production node-preview call site.
      A test call site is present before this change.
- [ ] Every moved test asserts the same fields with the same expected values as before. This record
      changes which constructor a test calls; it changes no assertion. Any test that cannot move
      without weakening an assertion is left where it is and named, with the reason.
- [ ] The moved tests are executed by the default command after this change and were not before it.
      Read this from what the command reports it executed, not from `cargo test -- --list`, which
      enumerates `#[ignore]`d tests and so cannot witness an exclusion.
- [ ] The full Rust suite is green before and after this change, with the same set of passing tests.

**Out of scope:**
- Any change to production tmux binary resolution. `ADR-260622-0208-01` is respected, not revisited.
- The real-tmux guard tests, their socket isolation, and the honest-absence encoding —
  `ISSUE-260902-0747-04`.
- Consolidating fake-process fixtures — `ISSUE-260902-0747-02`. These tests drive neither a fake nor
  the real binary after this change; they construct a struct.

## Triage Notes

Minted 2026-09-02 by splitting `ISSUE-260902-0747-04`, on the maintainer's decision the same day. The
round-3 readiness gate on `-04` found class 6 prong (b) firing for this batch specifically: the three
batches in that record were said to share the same four test functions, which is true of the
socket-isolation and honest-absence batches and false of this one.

**Readiness gate:** not yet run on this record.
