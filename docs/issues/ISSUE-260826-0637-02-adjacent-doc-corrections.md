---
id: ISSUE-260826-0637-02
kind: issue
category: bug
status: ready-for-agent
summary: Correct the api-reference capabilities node-type list and its flat-shape node example, and remove the dead docs index link
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
