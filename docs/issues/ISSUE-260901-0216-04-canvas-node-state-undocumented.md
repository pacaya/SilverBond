---
id: ISSUE-260901-0216-04
kind: issue
category: bug
status: ready-for-agent
prd: PRD-260826-0009-01
summary: The workflow schema document describes the canvas viewport but never mentions the per-node canvas state map that sits beside it
---

## Agent Brief

**Category:** bug
**Summary:** Document the per-node canvas state map in the workflow schema document.

**Current behavior:**
The workflow document's editor-state object carries two fields: a canvas viewport, and a map from
node id to per-node canvas state. The schema document describes the viewport field and its
required members. It does not mention the per-node map at all — neither the field, nor its key
meaning, nor the shape of its values. A workflow author reading the schema document sees an
editor-state object with one field, and a round-tripped document carrying the second field looks
undocumented rather than optional.

**Desired behavior:**
The schema document describes the per-node canvas state map alongside the viewport: that it is
keyed by node id, that it is optional and omitted when empty, and what its value shape is,
covering each member the way the viewport's members are covered.

The addition matches the surrounding conventions of the schema document rather than introducing a
new presentation.

**Key interfaces:**
- The workflow canvas UI type — an object with a viewport member and a map member from node id to
  per-node canvas state, the latter skipped on serialization when empty.
- The per-node canvas state type — the value shape of that map, whose members are what the schema
  document must enumerate.

**Acceptance criteria:**
- [ ] The schema document documents the per-node canvas state map.
      `rg -n 'ui\.canvas\.nodes' docs/workflow-schema.md` returns the field's documentation
      anchor; no matches before this change.
- [ ] Every member of the per-node canvas state type appears in that documentation. Derive the
      member set with `rg -n -A 12 'pub struct WorkflowCanvasNodeState' src/model.rs`, reading only
      as far as that struct's closing brace — the window overruns into the neighbouring type, whose
      members are not in scope here — and confirm each member of the per-node state type is
      described in the document.
- [ ] The documentation states the map's key meaning and its omitted-when-empty behavior.
- [ ] `cargo test --locked` passes, so the generated-block guard remains green against the edited
      document.

**Out of scope:**
- Regenerating or restructuring any `BEGIN GENERATED` block. This addition is a hand-written
  section.
- Documenting editor behavior — how the canvas produces this state, or how the editor consumes it.
  This record documents the persisted shape only.
- `docs/backend.md`'s core-enums list, which is ISSUE-260901-0216-06.
- Any change to the types themselves, their serialization, or their defaults.

## Triage Notes

Filed 2026-09-01 during a completeness audit of the `docs-truth` epic (PRD-260826-0009-01), from a
deferral recorded in `ISSUE-260826-0637-06` and never given a record.

Confirmed still open at audit time by reading the type against the document.

Originally filed together with the `docs/backend.md` core-enums gap. Split at the readiness gate:
the two have no shared seam — different documents, different types, no ordering dependency — and
critically no test guards `docs/backend.md` at all, so `cargo test --locked` cannot bind them. The
sibling is now ISSUE-260901-0216-06.

**Readiness gate (cold-reader): PASS** (round 2, 2026-09-01)

Gate note, carried for the implementer (non-binding): the upstream deferral this record cites
(`ISSUE-260826-0637-06` L8) reads "`ui.canvas` **and** `ui.canvas.nodes` are undocumented". The
`ui.canvas` container is already partly covered by the `ui` row in the document-level table. This
record scopes itself to the per-node map only; the container remainder is not claimed here and is
not stranded by the 04/06 split.
