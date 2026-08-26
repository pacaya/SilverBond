---
id: ISSUE-260826-1648-03
kind: issue
category: bug
status: needs-triage
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
