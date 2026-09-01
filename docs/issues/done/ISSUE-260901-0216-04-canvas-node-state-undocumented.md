---
id: ISSUE-260901-0216-04
kind: issue
category: bug
status: done
prd: PRD-260826-0009-01
summary: The workflow schema document describes the canvas viewport but never mentions the per-node canvas state map that sits beside it
claimed_by: implement-issue@macmini
claimed_at: 2026-09-01T10:48:40Z
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

## Context Pack — generated at claim (2026-09-01T10:48:40Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01):
- Tier P — hand-written, source cross-referenced: field tables (edges, top-level, variables, subflow catalog, agent config) each cite the source location that owns them; this addition belongs to Tier P, not to the generated catalog.
- The generator emits blocks, not documents: generated content lives only between `<!-- BEGIN GENERATED: … -->` markers, and the generator's only file is `docs/workflow-schema.md`'s node catalog — a hand-written section must sit outside those spans.
- Generated examples cannot state defaults, which is why requiredness/optionality and omitted-when-empty behavior must be written out in a table rather than shown by example.
- `docs/workflow-schema.md` is restructured, not patched: grammar first, then the generated catalog, then the Tier P tables; new prose matches that structure.
- `src/model.rs` is stated as the authority; v4 is canonical (`WORKFLOW_SCHEMA_VERSION = 4`), v5 is a cited forward reference only.
- Honest limits: all Tier P content is hand-written and not machine-checked — requiredness is a serde attribute no check in this epic catches.

**Test seam & Testing Decisions:** observable at `cargo test --locked` — specifically the `tests/docs_catalog.rs` freshness check, which regenerates the marker-delimited blocks in memory and asserts equality against the committed markdown. Testing Decisions that touch it: generation-plus-freshness-diff is the primary guarantee (a hand edit *inside* a generated block fails the diff; prose added outside the markers does not); `cargo test` is side-effect-free by default and only writes under `SB_REGEN_DOCS=1`; the freshness check gates the generated blocks only and is not a general docs linter, so the new section's correctness rests on review against the type, not on a test.

**ADRs:**
- ADR-260815-2009-01 — Typed workflow contracts over a JSON-valued variable store · accepted (forward reference only; v5 is unlanded and out of scope here).

**Terms:**
- `Workflow` — versioned node/edge graph definition (schema `version: 4`; v2–v3 normalize forward at ingest), including its subflow catalog. _Avoid_: pipeline, flow.
- `Node Kind` — the fourteen-variant tagged union in a node's required `kind` object. _Avoid_: step type.
- No glossary entry exists for the canvas or per-node canvas state; the record carries no `terms:`/`adrs:` frontmatter, so these are drawn from the parent PRD's term list. Do not mint new vocabulary — describe the fields in the document's existing table voice.

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md · docs/adr/INDEX.md · CONTEXT.md · docs/workflow-schema.md

## Code Review

Review file: `issue-260901-0216-04-code-review-20260901-111252.md`

- M1 (MEDIUM): Documentation asserts a key/node-id correspondence that nothing enforces — FIXED
- M2 (MEDIUM): Serde default stated twice within the new section (promoted in-diff duplication smell) — FIXED
- L1 (LOW): Map determinism (sorted keys on serialize) not stated — deferred
- L2 (LOW): Member wire types never stated — dismissed (AC2 satisfied by the adjacent prose form)
- L3 (LOW): "via the parent `#[serde(default)]`" phrasing — dismissed (verbatim-parallel to the adjacent pre-existing paragraph)
- L4 (LOW): Non-finite position serializes to `null` and then fails to reload — deferred (out of scope; follow-up candidate)

Smells: 3 advisory (1 promoted to M2). No graduation rows.

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

## Resolution

**Commit:** `fix: document the per-node canvas state map in the workflow schema (ISSUE-260901-0216-04)`

**Route:** `cursor` (`/cursor-developer`) for both the implementation and the single fix round —
single-file documentation prose with no cross-module reasoning, no security surface, and no
architecture decisions. Same route chosen independently for the implementation and the fix batch.

**TDD:** n/a (linear) — documentation work; the rule-based trigger routes mechanical/config/docs
changes away from red-green. The guarding seam (`tests/docs_catalog.rs`) already existed.

**Review telemetry:** dual review (Claude + Codex), cross-verified and adjudicated inline.
6 findings — 0 CRITICAL, 0 HIGH, 2 MEDIUM, 4 LOW.
Outcomes: 2 FIXED (M1, M2), 2 deferred (L1, L4), 2 dismissed (L2, L3).
Fix rounds used: **1 of 4**.
One finding (M2) entered the findings track by the in-diff duplication promotion rule — every cited
site lay inside the issue diff. 3 advisory smells recorded in `docs/issues/SMELLS-LEDGER.md`
(all newly appended); no graduation rows.
Both reviewers independently converged on the same single spec-axis defect (M1), differing only on
severity — Claude MEDIUM, Codex LOW; adjudicated MEDIUM on the ground that a documentation-truth
record anchors severity to the axis rather than the blast radius.

**Suite:** PASS — `just test` (`cargo test --locked` then `npm test`), 659 passed, 0 failed,
1 pre-existing ignored. No pre-existing failures to report.

**Follow-up left open** (deferred L4, not filed): non-finite (`NaN`/infinity) values in
`WorkflowCanvasNodeState` / `WorkflowCanvasViewport` serialize to bare `null` and then fail to
deserialize, so a non-finite position writes a workflow file that no longer loads. Explicitly out of
scope for this record ("Any change to the types themselves, their serialization, or their defaults");
worth its own record against the types.

**Closed:** 2026-09-01 (UTC)
