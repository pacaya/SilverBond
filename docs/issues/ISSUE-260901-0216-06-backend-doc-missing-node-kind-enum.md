---
id: ISSUE-260901-0216-06
kind: issue
category: bug
status: ready-for-agent
prd: PRD-260826-0009-01
summary: The backend module reference's core-enums list names the plain node-type discriminant but not the tagged node-kind enum that carries each kind's configuration
---

## Agent Brief

**Category:** bug
**Summary:** List the tagged node-kind enum among the workflow model's core enums in the backend
module reference.

**Current behavior:**
The backend module reference has a core-enums list for the workflow model module. It names the
plain node-type enum with all fourteen of its variants, alongside the response-format, edge-outcome
and split-failure-policy enums.

It does not name the tagged node-kind enum. That type is the one that actually defines the
canonical document shape: the plain enum is the bare discriminant, while the tagged enum carries
each kind's configuration payload and is what a workflow document serializes as a nested object
with a type tag. A reader using this document to orient in the model module is pointed at the
discriminant and never told the type it discriminates exists.

**Desired behavior:**
The core-enums list names the tagged node-kind enum, and distinguishes it from the plain node-type
enum it sits beside — the plain one being the discriminant, the tagged one carrying per-kind
configuration.

The entry matches the list's existing presentation rather than introducing a new one.

**Key interfaces:**
- The tagged node-kind enum and the plain node-type enum — two distinct types in the workflow model
  module. The reference currently names only the second.

**Acceptance criteria:**
- [ ] The core-enums list names the tagged node-kind enum.
      `rg -n '^- .NodeKind.' docs/backend.md` returns the list entry; no matches before this change.
- [ ] The entry states what distinguishes it from the plain node-type enum, so a reader can tell
      which of the two to reach for.
- [ ] The entry sits inside the workflow model module's core-enums list, not elsewhere in the
      document. `rg -n -A 8 '^\*\*Core enums:\*\*' docs/backend.md` shows the entry within that
      list's extent.

**Out of scope:**
- The per-node canvas state gap in the schema document, which is ISSUE-260901-0216-04.
- Enumerating the tagged enum's variants or their configuration payloads. The schema document owns
  that; this entry is a pointer, matching how the list treats its other entries.
- Any other section of the backend module reference.
- Adding test coverage for `docs/backend.md`. No test guards this document today, and establishing
  one is a separate decision.

## Triage Notes

Filed 2026-09-01, split out of ISSUE-260901-0216-04 at the readiness gate. The gate found the two
gaps had no shared seam — different documents, different types, no ordering dependency — and that
no test guards `docs/backend.md` at all, so the suite could not bind them into one slice.

Originally deferred out of `ISSUE-260826-0240-01`, whose resolution says the gap belonged in "a
separate record in the PRD-260826-0009-01 docs-truth family". No such record was minted at the
time. Confirmed still open at audit time by reading the enum declarations against the document.

**Readiness gate (cold-reader): PASS** (round 2, 2026-09-01)

Gate note, carried for the implementer (non-binding): the third criterion's `-A 8` window runs past
the core-enums list into the neighbouring agent-configuration list, so the command alone does not
prove placement. The prose clause carries that requirement — read the output, do not just check the
exit status.
