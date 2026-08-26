---
id: ISSUE-260826-0520-01
kind: issue
category: bug
status: needs-triage
origin: docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md
summary: /api/capabilities hardcodes supportedNodeTypes and supportedEdgeOutcomes as string literals instead of deriving them from NodeKind and WorkflowEdgeOutcome, so the endpoint can drift from the enums silently
---

## Triage Notes

Found on 2026-08-26 during the glossary session for PRD-260826-0009-01 (`docs-truth` epic of
RDMP-260815-2009-01), while grounding the **Node Kind** and **Edge Outcome** terms against `src/`.
Filed rather than folded into that PRD: `docs-truth` is scoped to make no engine changes and edit
nothing under `src/` (PRD § Dimension Scan, architecture shape), and this needs a code change.

**The defect.** `src/api.rs:216-217` publishes both lists as literal string arrays:

```rust
"supportedNodeTypes": ["task", "approval", "split", "collector", "decide", "parallel_batch", "subflow", "call", "spawn", "send", "wait", "capture", "kill", "run_agent"],
"supportedEdgeOutcomes": ["success", "reject", "branch", "loop_continue", "loop_exit"],
```

Neither is derived from its enum. `WorkflowNodeType` (`src/model.rs:159-174`) already carries
`#[serde(rename_all = "snake_case")]` and an `as_str()` (`:176`), and `WorkflowEdgeOutcome`
(`src/model.rs:215-221`) carries the same derive. Adding a variant to either enum compiles clean
and leaves the endpoint stale — the same failure class as the documentation drift the parent epic
exists to delete, in the one surface that is supposed to be the machine-readable truth.

Both lists happen to be **correct today**. This is a missing pin, not a live falsehood.

**Why it will bite on a known schedule.** `condition-ast` (RDMP-260815-2009-01) adds a sixth edge
outcome, `failure` (ADR-260815-2009-02), and `script-node` adds a fifteenth node kind
(ADR-260815-2009-03). Both epics have to remember to edit a string literal in `src/api.rs` that
nothing points them at. `typed-contracts` lands the `script` variant in the v5 grammar earlier
still, widening the window in which the enum and the endpoint disagree.

**Also in scope for whoever takes this.** `ui/src/features/editor/GraphEditor.svelte:240-241`:

```js
const supportedNodeTypes = $derived(
  capabilities?.supportedNodeTypes ?? ["task", "approval", "split", "collector"],
);
```

A third hardcoded copy, and this one is already the stale four-kind list — so if the capabilities
fetch fails or has not resolved, the editor's palette silently offers 4 of 14 node kinds rather
than failing visibly. That is a worse outcome than an empty palette, because it looks like a
complete list.

**Relation to the parent epic.** PRD-260826-0009-01 fixes `docs/api-reference.md:24`, which
advertises a four-kind list where the endpoint returns fourteen (audit item X2). That corrects the
*document* against the endpoint. This issue is the other half: nothing pins the *endpoint* against
the enums. The epic's success criterion 3 — a change to the set of node kinds fails CI rather than
shipping — is served by the generated doc catalog and its set-equality check, and by nothing at all
for `/api/capabilities`.

**Suggested shape, for triage to accept or replace.** Derive both arrays from the enums (a
`WorkflowNodeType::ALL` / `WorkflowEdgeOutcome::ALL` slice plus the existing `as_str()`), or, if the
literals are wanted for wire stability, add a test asserting set-equality between each literal and
its enum's serde tags — the same technique PRD-260826-0009-01 adopts for the node catalog in
`tests/docs_catalog.rs`. The frontend fallback wants deleting or narrowing to an explicit
loading/error state rather than a silent short list.

**Evidence base.** `docs/sources/workflow-schema-drift-260825.md` item X2 covers the doc-side half;
the endpoint-side gap is not in that audit and was found separately.
