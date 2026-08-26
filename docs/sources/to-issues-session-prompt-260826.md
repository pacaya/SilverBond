# to-issues session — prompt for a fresh Claude Code session

Paste everything below the line into a new session in this repo.

---

The `docs-truth` epic's glossary blocker is resolved and `/to-issues` is unblocked. This session
does two things: a scoped re-gate, then the slicing.

**Read first, in this order:**

1. `CONTEXT.md` — 43 terms. **Read the two-line header before the term list**; it states the rule
   that governs every entry, and that rule was settled by a dedicated session on 2026-08-26.
2. `docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md` — the whole thing, but especially
   `## Implementation Decisions` (tier model, marker-block grammar, generator home),
   `## Testing Decisions`, `## Out of Scope`, and the sequencing paragraph in `## Further Notes`.
3. `docs/sources/workflow-schema-drift-260825.md` — 160 findings, cited on both sides against
   commit `5f2f2c7`. This is the epic's work order.
4. `/Users/agent/.claude/skills/to-spec/SPEC-GATE.md` — the dimension matrix, marker law, and
   cold-reader procedure.

## State

Nothing is committed. Branch `feature/tmux-panes`. Modified: `CONTEXT.md`,
`docs/adr/260815-2009-typed-workflow-contracts.md`,
`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md`. Untracked: all of `docs/prd/`,
`docs/issues/ISSUE-260826-*`, `docs/sources/*-260826.md`, `docs/sources/workflow-schema-drift-260825.md`.

## Task 1 — scoped re-gate on `domain terms`

The `domain terms` row flipped `deferred` → `decided`, the `[OPEN: glossary-minting]` anchor and its
ledger entry were both cleared, and four body passages were rewritten (the Terminology decision, the
version-framing sentence, user story 58's rationale, and the `## Further Notes` paragraph on why the
glossary and the docs disagreed). SPEC-GATE says to re-run for changed dimensions.

Spawn **one** `cold-reader` (opus, xhigh) scoped to `domain terms` only — not another full pass. The
rubric item most likely to bite is (d), body text taking a position on a dimension the table calls
deferred, since that is exactly the text that changed. The mechanical pre-check already passes:
`gate:` present, zero `[OPEN:]` anchors, zero `[ASSUMED:]`.

If it passes, leave `gate: passed 2026-08-26` as is. If it finds something, fix and re-run that
dimension.

## Task 2 — `/to-issues` on the PRD

**Sequencing is constrained, not free.** Per `## Further Notes`: the marker-bearing scaffold for
`docs/workflow-schema.md` must precede the generator, because the generator rewrites only the span
between `BEGIN GENERATED`/`END GENERATED` markers and today's file contains none — it has no legal
output span until the scaffold exists. The schema reference is then mostly assembly. The execution
model is the large hand-written piece and wants uninterrupted authorship. Consistency of voice
argues one author per document rather than parallel section writers.

**Do not re-litigate these.** They are decided and their evidence is in
`docs/prd/adversary-reports/` (eight rounds) or in the glossary session:

- The glossary rule: an unmarked entry's definition is true of `src/` at HEAD; an entry marked
  `_(planned — ADR-…)_` is defined by that ADR and the engine does not accept it yet. `grep
  '(planned' CONTEXT.md` is the outstanding set. Each schema-changing epic owns the terms its ADRs
  name — installed in the roadmap's `docs-truth` entry.
- `docs-truth` makes **no engine changes** and edits nothing under `src/`.
- v4 is canonical; v5 is decided (ADR-260815-2009-01) and delivered by `typed-contracts`.
- The node catalog is generated from constructed Rust values; the execution model is hand-written
  and nothing in it is generated.

## Findings that are inputs to the execution-model issue, not work of their own

Discovered while grounding the glossary; none are in the drift audit, and the execution model's
author will want them:

- **Node results are shared between split siblings at root but private inside a subflow.** Split
  children clone the parent's `call_stack` wholesale (`src/runtime.rs:5859`) and each `CallFrameState`
  carries its own `subflow_results`; `results_for_cursor` (`:2553`) returns one map or the other,
  never both, and `all_results_for_cursor` (`:2564`) is a plain clone with no merge. D4 and D6 each
  hold half of this.
- **The runner/immediate dispatch split has exactly one crossing.** A runner-kind node whose
  `skipCondition` fires is resolved inline inside `process_immediate_cursors`
  (`src/runtime.rs:3170-3179`); otherwise that arm falls through and the cursor is dispatched into
  the `JoinSet` later. D32 presents the split as clean.
- **Representative selection is first-still-live-in-arrival-order** (`:6041-6044`), not "whichever
  branch happened to win" as the PRD's Solution section phrases it.
- **`ExecutionLog` is accumulated on the checkpoint throughout the run** (`:607`, written at `:2439`,
  `:6403`, reset by restart at `:1567-1572`) and *separately* persisted at finalize — not "written at
  finalize".
- **"Write-once node outputs" is false.** `insert_result_for_cursor_index` (`:2639`) is a plain
  `BTreeMap::insert` with no occupancy guard, so a `loop_continue` revisit replaces the node's own
  prior result. ADR-260815-2009-01's body still uses the phrase; worth a wording fix whenever
  `typed-contracts` next touches that ADR.

**One thing verified only halfway, for whoever writes the resume section.** `active_panes` lives on
the in-memory `ActiveRun` (`src/runtime.rs:874`) and appears nowhere in `RuntimeCheckpoint`'s field
list (`:560-608`), so a resumed run starts with an empty registry and `resolve_active_pane`
(`:1221-1245`) returns `None` for every input on an empty map. What was **not** traced is what the
`target` call sites do with that `None` — fall back to the raw string, or fail. Trace it before
writing that section.

## Untriaged issues (three, all `needs-triage`)

- `ISSUE-260826-0004-01` — unreachable orchestrator branch fallback plus the silent
  first-branch-edge default.
- `ISSUE-260826-0240-01` — `ARCHITECTURE.md` claims four "current first-class node types" where the
  engine has fourteen.
- `ISSUE-260826-0520-01` — `/api/capabilities` hardcodes `supportedNodeTypes` and
  `supportedEdgeOutcomes` as string literals instead of deriving them from the enums; the frontend
  carries a third, already-stale copy as a fallback.

Slicing does not depend on these being triaged, but `ISSUE-260826-0004-01` records behavior the
schema reference must document (the real branch-routing default), so the relevant issue should cite
it rather than re-deriving it.
