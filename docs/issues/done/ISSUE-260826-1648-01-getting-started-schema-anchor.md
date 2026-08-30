---
id: ISSUE-260826-1648-01
kind: issue
category: bug
status: done
origin: docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md
summary: docs/getting-started.md deep-links a workflow-schema.md subsection that ISSUE-260826-0637-03 deletes, so the anchor dies when the schema scaffold lands
---

## Triage Notes

Found on 2026-08-26 during the round-4 readiness gate on `ISSUE-260826-0637-03`, and confirmed
independently in that record's round-5 gate. Filed rather than folded into `-03`: that issue is
already the largest scaffold pass in the `docs-truth` epic, this is a one-line link edit in a file
the epic otherwise does not touch, and `-03`'s `## Out of scope` now excludes the file explicitly
and directs the repoint here.

**The defect.** `docs/getting-started.md` links into `docs/workflow-schema.md` with an anchor
pointing at the `Run As (runAs)` subsection. `ISSUE-260826-0637-03` replaces that document's
structure with eight pinned `## ` headings, and its deletion rule removes the subsection the anchor
targets, so the link resolves to the file but lands nowhere once `-03` lands.

**Blast radius is exactly one link.** An ignore-blind sweep of the whole tree — including hidden
directories and `node_modules`, and covering the relative, `docs/`-prefixed, `./`-prefixed, HTML
`href`, reference-style, percent-encoded and alternate-spelling forms — found this to be the **only**
anchored deep link into `docs/workflow-schema.md` anywhere in the repository. Unanchored references
to the file are unaffected.

**No sibling repairs it.** `ISSUE-260826-0637-02`'s link criterion is scoped to `docs/README.md` and
tests file existence rather than anchors. `ISSUE-260826-0637-06` owns the section the content moves
into and documents `runAs` as a table row rather than re-creating a subsection, so the old slug is
not regenerated under a different owner.

**Sequencing.** This is only actionable once `ISSUE-260826-0637-03` has landed and the eight
headings exist — before that there is no correct target to repoint at. Whoever picks it up should
derive the new target from the document as it then stands rather than from this record.

**Triaged 2026-08-30.** Claim verified against the tree at `22ec2cb`. `ISSUE-260826-0637-03` has
landed: `docs/workflow-schema.md` now carries exactly the eight pinned `## ` headings, and no
`Run As (runAs)` subsection survives. The anchor `#run-as-runas` at `docs/getting-started.md:145`
resolves to the file and lands nowhere — the defect is live, as filed.

The blast-radius claim was re-checked rather than trusted: an ignore-blind sweep for
`workflow-schema.md#` across the tree (excluding only `.git` and `node_modules`) returns exactly
one hit, `docs/getting-started.md:145`. Unchanged from the original audit.

Repoint target derived from the document as it now stands, per this record's own instruction:
`runAs` survives as a table row at `docs/workflow-schema.md:1360`, inside `## Document-level
fields` (`:1346`). That heading is the nearest pinned ancestor of the content the old anchor
targeted, so `#document-level-fields` is the correct successor slug. `ISSUE-260826-0637-06`
did not regenerate a `runAs` subsection under any other owner, confirming this record's
"no sibling repairs it" finding.

Fixed inline during triage rather than routed to an agent: a one-line link edit with a
derived target is below the cost of minting a brief and claiming a record.

## Resolution

Repointed the single anchored deep link in `docs/getting-started.md:145` from
`workflow-schema.md#run-as-runas` to `workflow-schema.md#document-level-fields`.

- **Changed:** `docs/getting-started.md` (one line).
- **Verified:** target heading `## Document-level fields` exists at `docs/workflow-schema.md:1346`;
  the `runAs` row it now leads to is at `:1360`.
- **No engine change**, no test change — the repository has no anchor-resolution check, so nothing
  guards this class of link. Pinning anchors mechanically was considered and not done here: it is a
  repo-wide concern, not this record's, and there is exactly one anchored deep link to pin.
