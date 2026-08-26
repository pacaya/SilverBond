---
id: ISSUE-260826-1648-01
kind: issue
category: bug
status: needs-triage
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
