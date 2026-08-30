---
id: ISSUE-260826-1648-03
kind: issue
category: bug
status: done
origin: docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md
summary: The node-testing example in docs/api-reference.md passes mockContext in a shape the request type does not carry, so the key is silently dropped and the template variable the example advertises resolves to nothing
---

## Triage Notes

Found on 2026-08-26 during the round-5 and round-7 readiness gates on `ISSUE-260826-0637-02`. Filed
rather than folded into that issue: `-02`'s `## Out of scope` bounds it to the claims it names, and
widening the edit makes the change harder to review against its stated purpose.

**The defect.** The `POST /api/test-node` example passes a `mockContext` whose keys sit at the top
level of the object. The request's context type carries its variables under a nested field instead,
and no struct in the model denies unknown fields — an ignore-blind sweep of `src/` finds no
`deny_unknown_fields` anywhere. So the key deserializes into nothing, is silently dropped, and the
`{{var:…}}` token the example advertises resolves against an empty map.

**Why it is filed separately from the flat-shape fix.** `ISSUE-260826-0637-02` corrects the same
example's node to the canonical nested `kind` form, because the flat shape is *rejected* by the
endpoint and therefore defeats the parent PRD's first success criterion — no reader can copy a
documented example the engine rejects. This defect is different in kind: the engine **accepts** the
request. Nothing is refused, nothing errors, and the reader gets a successful response that quietly
does not do what the example says it does. It is a worse reading experience and a weaker failure
signal, but it does not trip the success criterion that motivated the other fix.

**Scope note.** Derive the correct shape from the request type rather than from this record. The
sibling fields of that example — the working-directory field alongside `mockContext` — were not
audited and may carry their own divergence; check them in the same pass rather than assuming the
one flagged key is the only one.

**Triaged 2026-08-30.** Claim verified against the tree at `22ec2cb`, and the record's open scope
note closed.

The request type is `TestStepRequest.mock_context: Option<NodeTestContext>` (`src/api.rs:653`),
consumed at `src/api.rs:689`. `NodeTestContext` (`src/runtime.rs:1821-1843`) is
`rename_all = "camelCase"` and carries its variables under a nested `variables` map (`:1826`) —
alongside `nodeOutputs`, `previousOutput`, `branchOrigin`, `branchChoice`, every one
`#[serde(default)]`. The documented top-level `{ "topic": "AI safety" }` therefore deserializes to
an all-defaults struct: no field named `topic` exists, and the ignore-blind sweep for
`deny_unknown_fields` across `src/` still returns nothing, so the key is dropped without error.
`run_node_preview` passes `var_map: &mock_context.variables` into the template context
(`src/runtime.rs`, `resolve_template_vars` call), so `{{var:topic}}` resolves against an empty map —
exactly the silent-success failure this record describes.

**Scope note resolved.** The record flagged the sibling working-directory field as unaudited and
possibly divergent. It is not: `cwd: Option<String>` (`src/api.rs:651`) matches the documented
`"cwd": "/workspace"`. The remaining sibling keys of `NodeTestContext` do not appear in the example
at all, so nothing else in it diverges. The one flagged key was the only defect.

Fixed inline during triage rather than routed to an agent: the correct shape is fully derived from
the request type and the edit is one line.

## Resolution

Corrected the `POST /api/test-node` example body in `docs/api-reference.md:146` from
`"mockContext": { "topic": "AI safety" }` to
`"mockContext": { "variables": { "topic": "AI safety" } }`.

- **Changed:** `docs/api-reference.md` (one line).
- **Verified:** `variables` is the field name on the wire — `NodeTestContext` is `camelCase` and the
  field is single-word, so no rename applies; `run_node_preview` feeds it to `var_map`, which is what
  `{{var:topic}}` reads.
- **Not done:** no `deny_unknown_fields` was added. Making the endpoint reject the old shape would
  turn a silent drop into a visible error and is the stronger fix, but it is an engine change with
  blast radius across every request type in `src/` — out of scope for a documentation record, and
  worth deciding as its own issue if wanted.
