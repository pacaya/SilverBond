---
id: ISSUE-260905-2136-05
kind: issue
category: enhancement
status: needs-triage
summary: The Performance Check category is defined in the tier ADR but not built, and with its only prospective member deleted the repository has no performance-testing practice and nobody owning whether it should
---

## Triage Notes

Filed 2026-09-05, when `PRD-260902-0301-01` was narrowed to the reproduced failure plus the tier rule.

**The decision taken 2026-09-05.** The large-workflow test is split by `ISSUE-260902-0747-01` — its
clock-free correctness half stays in the gate — and **the timing assertion is deleted**, not parked.
Leaving it `#[ignore]`d was rejected: an unrun test is not a guard, and under the tier rule an unmarked
test asserting elapsed time is a Logic Tier violation that `ISSUE-260902-0747-10`'s checker is required
to flag, so parking it would have made the rule false on the tree it governs.

**What remains open, and what this record now is.** SilverBond has no performance-testing practice and
no one owning whether it should have one. That is the question here, and it is genuinely open rather
than a deferral of the above:

1. **Build the category with an owner** — a scheduled, non-blocking job with a named owner, a recorded
   build profile and runner, and visible results. Worth doing only if somebody will read the results;
   `ADR-260902-0312-01` rejects hiding the Integration Tier from CI on the grounds that unrun tests
   rot, and an opt-in performance category nobody runs fails the same test.
2. **A maintained benchmark.** The ADR considered and rejected this for the general case, on the
   grounds that it converts a failing test into a number someone has to read. The objection is weaker
   for a small set with a named owner.
3. **Nothing.** Validation performance is not currently a property anyone is defending, and the
   deleted assertion had gone unexamined for long enough that its budget was never tuned. Closing this
   record `wontfix` is a legitimate outcome.

Do not reopen this by re-adding an `#[ignore]`d timing test; that is the state the deletion removed.

**Context.** The assertion pairs a fifteen-second validation budget with a correctness property
(`src/model.rs:6631`, `:6648`, `:6653`). Splitting them is already owned by
`ISSUE-260902-0747-01`; only the timing half is in question here.

**Related.** `ADR-260902-0312-01` § Retained debt, `PRD-260902-0301-01` § Out of Scope and
§ Open Questions, `ISSUE-260902-0747-01` (performs the split), `ISSUE-260902-0747-10` (keeps the
term's `planned` marker in `CONTEXT.md` until this is resolved).
