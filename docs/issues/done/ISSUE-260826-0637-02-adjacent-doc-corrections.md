---
id: ISSUE-260826-0637-02
kind: issue
category: bug
status: done
summary: Correct the api-reference capabilities node-type list and its flat-shape node example, and remove the dead docs index link
claimed_by: implement-issue@Mac-mini-4
claimed_at: 2026-08-30T02:13:35Z
pr: null
prd: PRD-260826-0009-01
terms: [Node Kind]
---

## Agent Brief

**Category:** bug
**Summary:** Three false claims in the documentation that are one-line fixes each and depend on nothing.

**Current behavior:**
`docs/api-reference.md` documents the `/api/capabilities` response with a `supportedNodeTypes`
array containing a strict subset of the node kinds the endpoint returns. Compare the documented
array against the handler's own literal — `rg -n 'supportedNodeTypes' docs/api-reference.md src/api.rs`
— and the documented list is short. This is a false claim about a live endpoint's response body, in
the document a client author reads to write against it.

The same document's node-testing section shows a request body whose node uses the legacy flat
shape — a top-level `type` with no `kind` wrapper. The engine refuses exactly that shape at this
endpoint, by name, so the example cannot be copied into a working request. Find it by searching
`docs/api-reference.md` for a node object carrying a top-level `type`, and confirm the refusal from
the endpoint's own node-parsing path rather than from this brief.

`docs/README.md` is the documentation index. Its table links to a document describing an agent
abstraction plan that does not exist in the tree. The index is the entry point to the docs, so a
dead row in it is the first thing a reader hits.

**Desired behavior:**
The capabilities example lists exactly the node-kind wire tags the endpoint actually serializes,
in the order the endpoint emits them, so a reader comparing the document against a live response
sees them agree. Derive the list from the handler rather than copying it from anywhere else —
the frontend carries its own stale copy as a fallback, and `ISSUE-260826-0520-01` covers the fact
that neither is derived from the enum.

The node-testing request body uses the canonical v4 nested `kind` form, so a reader who copies it
gets a request the endpoint accepts. Change the node's shape and nothing else: the surrounding
fields of that example are not this issue's subject.

The documentation index contains no row pointing at a file that does not exist.

**Key interfaces:**
- The capabilities handler in the API module builds the response; the node-type list it
  serializes is the authority for this edit. `NodeKind`'s serde tags are the underlying truth,
  and `CONTEXT.md`'s **Node Kind** entry records that the bare `type` tag is what the endpoint
  publishes.
- `docs/README.md`'s index table — remove the row rather than repointing it at a substitute
  document; no replacement exists.

**Acceptance criteria:**
- [ ] The documented `supportedNodeTypes` array in `docs/api-reference.md` equals what the
      capabilities handler returns, element for element and in the same order. Observable:
      `rg -c 'parallel_batch' docs/api-reference.md` returns a non-zero count; no matches before
      this change.
- [ ] The documented array is set-equal to the handler's. Observable: extract both arrays
      mechanically — the documented one from `docs/api-reference.md`, the handler's from
      `src/api.rs` — and diff them; the diff is empty. Derive the tag set from the handler, never
      from this brief, and do not assert its size anywhere in the doc prose.
- [ ] The node example in the node-testing section deserializes as a valid node — not merely that
      it has stopped using the legacy flat shape. Observable at the seam: derive the node struct's
      required-field set from the source and check the example against it. Rewriting the `kind`
      wrapper alone leaves the example rejected for a second reason, so the flat-shape fix is
      necessary and not sufficient; deriving both the rejection rule and the required-field set
      from the endpoint's node-parsing path rather than from this brief is part of the work.
      Before this change the example does not deserialize at all.
- [ ] `docs/README.md` contains no reference to the missing agent-improvements document.
      Observable: `rg -c 'agent-improvements-plan' docs/README.md` returns no matches; it
      returns a match before this change.
- [ ] Every remaining link target in the `docs/README.md` index resolves to a file that exists.
      Observable at the seam: extract the table's link targets and test each for existence.
- [ ] `just check-v4-docs` still passes.

**Out of scope:**
- **The rest of the capabilities example.** The documented JSON block diverges from what the
  handler serializes in further ways — among them the `features` map, the per-agent entries, and
  key ordering. Treat that divergence set as **underived**, not as enumerated here: obtain it by
  diffing the documented example against the handler's own output, never from a list in this brief.
  All of it is out of scope; within that JSON block this issue corrects the `supportedNodeTypes`
  line only. The flat-shape node example above is a separate claim in a different section and is
  in scope; nothing else in that section is. Widening the
  edit makes the change harder to review against its stated purpose. File the remainder separately
  if you want it fixed, deriving its contents the same way.
- Deriving `supportedNodeTypes` or `supportedEdgeOutcomes` from the enums instead of string
  literals, and the frontend's third stale copy. Both are engine and UI changes owned by
  `ISSUE-260826-0520-01`; this issue corrects documentation only and touches nothing under
  `src/` or `ui/`.
- Writing a replacement for the missing agent-improvements document.
- Every other document that under-reports node kinds — `ARCHITECTURE.md`, `docs/backend.md`,
  `README.md`, `docs/architecture-overview.md`. `ARCHITECTURE.md`'s four-kind list is owned by
  `ISSUE-260826-0240-01`; the rest are excluded by PRD-260826-0009-01.
- `docs/workflow-schema.md` and `docs/execution-model.md`, which are the epic's main deliverables
  and have their own issues.

## Context Pack — generated at claim (2026-08-30T02:13:35Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01):
- "The adjacent falsehoods are corrected in place": the capabilities node-type list, the node-testing example's legacy flat shape, and the dead `docs/README.md` index row are all one-line doc fixes, not schema changes.
- **No engine code changes.** This epic reads `src/`, writes `docs/`; nothing under `src/` or `ui/` is edited. Deriving the list from the enums instead of string literals is explicitly deferred (owned by ISSUE-260826-0520-01).
- The handler's own literal is the authority for the wire tags; the frontend carries a third stale copy — do not copy from it.
- Verified tag set (recorded in the PRD's serde-harvest decision, order as the derive lists it): `task, approval, split, collector, decide, parallel_batch, subflow, call, spawn, send, wait, capture, kill, run_agent`. Confirm against the handler before editing — the PRD's list is evidence, not the authority.
- The nested `kind` shape is canonical; the flat top-level `type` shape is accepted only by the legacy migration path (`migrate_v2_nodes_to_v3_kind`), which the node-testing endpoint's strict path does not take.
- v4 is canonical (`WORKFLOW_SCHEMA_VERSION = 4`); v5 is decided-and-unlanded — cite, never describe as present.
- Scope discipline: within the capabilities JSON block only the `supportedNodeTypes` line changes; the `features` map, per-agent entries, and key ordering are known-divergent and out of scope.

**Test seam & Testing Decisions:** observable at the document boundary — a documented example must deserialize strictly as a workflow document, not merely look well-shaped. The PRD's Testing Decisions fix this seam as `serde_json::from_value::<WorkflowV3>()` (strict), then `ensure_defaults` + `validate_workflow`, zero error-severity issues — and deliberately **not** via `normalize_workflow_value`, because the unconditional v2→v3 `kind` migration would rescue exactly the flat shape this issue removes. For this slice the seam is applied by hand: derive the node struct's required-field set (fields carrying no `#[serde(default)]`) from the source and check the example against it. Requiredness is the one drift class no automated check catches, so it is a read-and-verify obligation here. `just check-v4-docs` remains a blacklist guard that cannot verify — passing it proves nothing.

**Authorities to read (source, not prose):**
- The `/api/capabilities` handler in `src/api.rs` — builds `supportedNodeTypes`; its literal, element for element and in emission order, is what the doc must equal.
- `NodeKind` in `src/model.rs` — internally tagged (`#[serde(tag = "type")]`); its serde wire tags are the underlying truth beneath the handler's literal.
- The node struct in `src/model.rs` — the required-field set the corrected example must satisfy; a `kind`-wrapper fix alone leaves the example rejected through a different arm with a different message.
- The endpoint's node-parsing path — derive the flat-shape rejection rule from it rather than from the brief.

**ADRs:** none linked on this record (no `adrs:` frontmatter); the parent PRD's `adrs: [ADR-260815-2009-01]` covers the v5 bump, which this slice only cites, never describes.

**Terms:**
- `Node Kind` — the fourteen-variant tagged union in a node's required `kind` object, selecting both what the node does and which config shape it carries; the bare `type` tag alone is what `/api/capabilities` publishes as `supportedNodeTypes`. _Avoid_: step type. (↔ `src/model.rs` `NodeKind`, `WorkflowNodeType`)

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md · CONTEXT.md · docs/sources/workflow-schema-drift-260825.md · docs/api-reference.md · docs/README.md

## Code Review

Review file: `issue-260826-0637-02-code-review-20260830-023050.md`

- L1 (LOW): `mockContext` example has the wrong shape, so the node example cannot demonstrate what it claims — deferred (out of scope per the brief; both reviewers agree on the facts and on exclusion)
- L2 (LOW): "without executing it" is false for `POST /api/test-node` — deferred (pre-existing prose in the section the brief excludes)
- L3 (LOW): drift-survey source carries two now-stale line citations — dismissed (`docs/sources/` is immutable by convention, DOCS-LAYOUT.md:26)

Smells: 1 advisory (Duplicated Code, `docs/api-reference.md:24`; not promoted — only one of its four cited sites lies inside the issue diff). No graduation rows.

Reviewers: Claude + Codex, cross-verified. Codex reported zero findings; Claude reported three, all LOW, all cross-verified by Codex as factually true but out of scope. Claude confirmed all six of Codex's verified-clean claims, contesting none. Fix rounds used: 0 of 4 — no finding was in scope to fix.

## Triage Notes

**Readiness gate (cold-reader): PASS** (round 3)

Round 1 found a decaying count in an acceptance criterion and a scoping gap; round 2 found that
the round-1 remedy had reintroduced a count. Both were fixed. Round 3 ran the full nine-class
rubric, executed every observable, verified the three named capabilities-example divergences
against the handler, and swept the whole record for superseded restatements of any repaired
claim — none survived.

**Readiness gate (cold-reader): REOPENED** (round 4, 2026-08-26, breakdown-adversary finding)

The round-9 breakdown adversary found a copyable flat-shape node example in the node-testing
section of `docs/api-reference.md` that no brief owned, defeating the PRD's first success
criterion. This brief already owned that file, so the surface was folded in here rather than
minted as a ninth slice.

**Readiness gate (cold-reader): PASS** (round 5, full-enumeration)

Round 5 ran as a full-enumeration round over the reopened brief with the folded-in flat-shape
surface promoted as a tracked finding. The refusal was verified from the endpoint's own parsing
path (non-test code), the new criterion was confirmed falsifiable at baseline, and the amended
scope boundary was verified disjoint by section extent — the capabilities JSON block and the
node-testing example do not overlap at this tree. Classes 1-5 swept 31 units, class 7 swept 39
extracted surfaces including the Triage Notes, class 9 arm A executed all six observables (red or
exempt) and arm B found no triggered requirement, class 6 fired neither prong, and class 8 was
applicable but found every Triage-Notes unit to be a non-binding shape. No class fired.

**Readiness gate (cold-reader): REOPENED** (round 6, 2026-08-26, post-gate observation)

Round 5 passed but observed that the flat-shape criterion was weaker than the outcome it serves:
the node struct carries a required field the current example also omits, so an edit satisfying the
criterion verbatim would still produce an example the endpoint rejects. The criterion now asserts
the positive — that the example deserializes — rather than the absence of one bad form.

**Readiness gate (cold-reader): PASS** (round 7, full-enumeration)

Round 7 judged the rewritten criterion from scratch. The premise was confirmed by mechanical
extraction over the node struct: three fields carry no serde default, the example omits two of
them, and after a shape-only fix the request is still refused — through the generic arm rather than
the flat-shape arm, so for a different reason and with a different message. The criterion is red at
baseline, its derivation instruction reaches the struct in two hops, and it still covers the flat
shape both by entailment and by name. The decision not to name the missing field was upheld as
correct anti-decay form. Classes 1-5 swept 35 units, class 7 swept 43 extracted surfaces, class 9
arm A executed all six observables (five red, one exempt) and arm B found no triggered requirement
— the rewrite specifically removed an absence assertion that had been satisfiable while the true
outcome stayed broken. No class fired.

## Resolution

**Commit:** `fix: correct api-reference node-type list and node example, drop dead docs index link (ISSUE-260826-0637-02)`

**Route:** cursor (`/cursor-developer`) — three well-scoped one-line documentation corrections with
explicit observables; no cross-module reasoning, infrastructure, or security surface that would call
for codex.

**TDD:** n/a (linear). Documentation-only work. The acceptance criteria's "observable at the seam"
phrasing names verification commands rather than a test seam, and the context pack fixed the
deserialization seam as applied by hand for this slice — no test file was added or changed.

**Review telemetry:** 2 reviewers (Claude + Codex), cross-verified, adjudicated inline. Findings by
severity: 0 CRITICAL, 0 HIGH, 0 MEDIUM, 3 LOW. Outcomes: 0 FIXED, 2 deferred (L1, L2), 1 dismissed
(L3). Fix rounds used: **0 of 4** — Codex reported zero findings, and all three of Claude's were
cross-verified as factually true but outside the brief's scope. Smells: 1 advisory (Duplicated Code),
not promoted — only one of its four cited sites lies inside the issue diff. Review file:
`issue-260826-0637-02-code-review-20260830-023050.md`

**Deferred work** (both confirmed real by both reviewers, both excluded by this brief's Out of scope —
worth a follow-up issue against the node-testing section of `docs/api-reference.md`):
- `mockContext: { "topic": ... }` deserializes into an empty `variables` map, so the example's
  `{{var:topic}}` never resolves in the preview.
- "without executing it" is false for `POST /api/test-node` — `run_node_preview` launches the agent
  via `run_tmux_oneshot`.

**Suite:** PASS — `just test` green, 595 tests (466 Rust across 4 binaries, 129 vitest in 13 files),
0 failed, 0 skipped. Playwright e2e is not part of the `test` recipe and was not run.

**Verification highlight:** the hardest acceptance criterion — that the corrected node example
actually deserializes rather than merely abandoning the flat shape — was proven by execution during
cross-verification: a throwaway integration test ran
`serde_json::from_value::<silverbond::model::WorkflowNode>` over the literal documented object and
passed with `node_type_str() == "task"`.

**Closed:** 2026-08-30 (UTC)
