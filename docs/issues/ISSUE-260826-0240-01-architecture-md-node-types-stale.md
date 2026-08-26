---
id: ISSUE-260826-0240-01
kind: issue
category: bug
status: needs-triage
origin: docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md
summary: ARCHITECTURE.md states four "current first-class node types" where the engine has fourteen, and docs/backend.md names ten
---

## Triage Notes

Found on 2026-08-25 during the gate pass on PRD-260826-0009-01 (`docs-truth` epic of
RDMP-260815-2009-01). Filed rather than folded into that PRD: the epic's named deliverable is
`docs/workflow-schema.md` and `docs/execution-model.md`, and these are different documents. It is
filed rather than merely excluded so that the PRD's exclusion points at a real record.

**The defect.** `ARCHITECTURE.md:289-296` reads:

> ### Node types today
>
> Current first-class node types:
>
> - `task`
> - `approval`
> - `split`
> - `collector`

`NodeKind` (`src/model.rs:695-753`) has fourteen variants: `task`, `approval`, `split`,
`collector`, `decide`, `parallel_batch`, `subflow`, `call`, `spawn`, `send`, `wait`, `capture`,
`kill`, `run_agent`. All fourteen are returned by `GET /api/capabilities` (`src/api.rs:216`).

The phrase "Current first-class node types:" makes this a **false statement about the present**,
not an incomplete one — the same class as the `docs/api-reference.md:24` claim that
PRD-260826-0009-01 does fix. `ARCHITECTURE.md:202` separately names five of the missing kinds
("execute spawn, send, wait, capture, and kill control nodes"), so the document contradicts itself:
nine distinct kinds appear in total, four of them under a heading asserting the list is complete.

**Also in scope for whoever takes this:** `docs/backend.md` has the same shape. `:60` reads
"`WorkflowNodeType` — `Task`, `Approval`, `Split`, `Collector`"; `:105` names `Task`, `RunAgent`,
`Spawn`, `Send`, `Wait`, `Capture`, `Kill`. Ten distinct kinds, and `:60` presents its four as the
type's definition.

**Not urgent, and deliberately deferred once.** `typed-contracts` (RDMP-260815-2009-01) already
pins `ARCHITECTURE.md`'s canonical-version statement for editing at the v5 bump, so there is a
natural moment to fix the node-type list in the same pass rather than touching the file twice.
Triage may reasonably route this to that epic instead of scheduling it standalone.

**Evidence base.** The full documentation drift inventory is
`docs/sources/workflow-schema-drift-260825.md` (item X4 covers cross-document node-kind coverage).
