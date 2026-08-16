---
id: ADR-260815-2009-05
status: accepted
terms: [Assignment, Collector]
---

# Cursor-local variable writes; cross-branch data flows only through collectors

Mid-run variable assignment (`assignVars` on nodes) is binding-only — values come from engine-evaluated bindings, never arbitrary expressions — and strictly cursor-local: a parallel branch's writes live and die with its cursor, so two concurrent branches can never race on a shared variable by construction. Cross-branch data crosses only at collector joins, aggregated as parsed JSON values (real lists, not escaped strings). Aggregation rides the existing declared paths, Value-typed under ADR-260815-2009-01 — the ordered-list contract is `parallel_batch.collectorVar`'s (`src/model.rs:461`); the generic collector keeps its `{inputs, summary}` object shape (`src/runtime.rs:6009`) — no new aggregation surface is introduced. At collector release the surviving scope is the pre-split variable map plus those aggregates — a representative branch's local assignments never leak through survivor selection (the existing representative-state behavior, `src/runtime.rs:6091`, is constrained accordingly). Restart-from-node is valid only when the checkpoint carries no active split families and no call frames — a decidable, checkpoint-level predicate (split bodies are not identifiable from the flat node graph) — and only at a target node that requires no arrival context: `parallel_batch` body nodes (which receive `itemVar` only from the batch scheduler), collector nodes (which wait on inbound arrivals), and any node whose prompt or input bindings reference arrival-scoped values (`previous_output`, `branch_origin`, `branch_choice` — statically detectable from its templates and bindings, since a restart cursor cannot reconstruct the original arrival edge) are invalid targets, rejected by validation; otherwise the restart is rejected with a clear error, since cursor-local scopes carry no restartable identity. So that restart never loses cursor-local assignments, whenever a persistence point finds exactly one root-scope cursor — and at terminal completion, before the final cursor is removed — that cursor's variable map is mirrored into the checkpoint-level map restart seeds from (`src/runtime.rs:1480`). Per-variable and per-checkpoint byte caps are enforced at write time with loud errors — accumulated state multiplies across the checkpoint root, cursors, and call frames, and without caps SQLite serialization becomes the first enforcement mechanism. Consequence accepted deliberately: a branch cannot signal a sibling mid-flight through a variable — anything cross-branch flows through join points, which is what keeps checkpoint/resume semantics well-defined (all state is in `RuntimeCheckpoint`, `src/runtime.rs:560`).

## Considered Options

- **Shared mutable variables with a write policy (last-writer-wins / append / error)** — rejected: every surveyed engine that shipped shared mutable scope discovered its semantics in production (n8n static-data races, GH Actions matrix output collisions); making the race unrepresentable beats adjudicating it.
- **Filesystem-mediated state as the primary mechanism** — rejected for run state: invisible to checkpoints, collides across parallel runs unless authors namespace paths, and desyncs on restart-from-node. The filesystem remains correct for durable domain state (e.g. the issues directory), which outlives any run.

## Evidence

- Concurrency and aggregation failure modes for mutable run state across Argo, Temporal, n8n, GH Actions.
  Provenance: [docs/sources/prior-art-workflow-engines.md](../sources/prior-art-workflow-engines.md) §4; 2026-08-15.
