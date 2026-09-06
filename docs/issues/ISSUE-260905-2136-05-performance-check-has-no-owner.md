---
id: ISSUE-260905-2136-05
kind: issue
category: enhancement
status: needs-triage
summary: The Performance Check category is defined in the tier ADR but is not built, has no scheduled run and no named owner, leaving the large-workflow timing assertion ignored and rotting
---

## Triage Notes

Filed 2026-09-05, when `PRD-260902-0301-01` was narrowed to the reproduced failure plus the tier rule.

**The state this record exists to resolve.** `ADR-260902-0312-01` defines a third, non-gating
category for tests whose subject genuinely is elapsed time. It is the right shape and it is not
built: the narrowed epic declines to add a third selector switch for a single member
(`PRD-260902-0301-01` § Out of Scope). So `ISSUE-260902-0747-01` splits the large-workflow test —
its clock-free correctness half stays in the gate — and leaves the timing assertion `#[ignore]`d
with a comment pointing here.

An ignored test does not run. It will rot. That is the honest consequence of not building the
category, recorded rather than hidden, and it is the thing to fix or delete.

**The decision to make.** Three options, and the third is respectable:

1. **Build the category with an owner.** A scheduled, non-blocking job with a named owner, a recorded
   build profile and runner, and visible results. A failed threshold is a diagnostic alarm, not a
   merge condition. This is only worth doing if somebody will actually read the results — the ADR
   rejects hiding the Integration Tier from CI on the grounds that unrun tests rot, and an opt-in
   performance category nobody runs fails the same test.
2. **Convert it to a maintained benchmark.** The ADR already considered and rejected this for the
   general case, on the grounds that it converts a failing test into a number someone has to read.
   That objection is weaker for a single assertion with a named owner.
3. **Delete the timing assertion.** If no one will own running it, the guard is already gone and the
   `#[ignore]`d test is only pretending otherwise. Deleting it is more honest than keeping a test
   that never runs, and the correctness half stays in the gate either way.

Do not resolve this by leaving the test ignored indefinitely; that is the current state and it is
what this record is against.

**Context.** The assertion pairs a fifteen-second validation budget with a correctness property
(`src/model.rs:6631`, `:6648`, `:6653`). Splitting them is already owned by
`ISSUE-260902-0747-01`; only the timing half is in question here.

**Related.** `ADR-260902-0312-01` § Retained debt, `PRD-260902-0301-01` § Out of Scope and
§ Open Questions, `ISSUE-260902-0747-01` (performs the split), `ISSUE-260902-0747-10` (keeps the
term's `planned` marker in `CONTEXT.md` until this is resolved).
