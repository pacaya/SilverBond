# Task L2 — Migrate `WorkflowNode` to a tagged `NodeKind` enum (standalone `version: 3` PR)

> **Origin:** Finding **L2** from `docs/local/feature-tmux-panes-code-review.md` (branch
> `feature/tmux-panes`). Explicitly **deferred** out of the tmux-panes branch during
> `fix-code-review` because it is a **breaking wire-format change** that must not be folded into an
> unrelated feature branch. This document is the developer-ready spec to implement it as its **own
> PR**.
>
> **Severity:** LOW — maintenance / code-quality, *not* a runtime bug. `validate_workflow`
> (`src/model.rs`) runs before execution (`src/api.rs`) and hard-errors the missing-config case, so
> the "illegal states representable" risk is already neutralized at runtime; the defensive unwraps
> below are unreachable belt-and-suspenders today. This refactor makes the invariant
> *type-enforced* instead of *validation-enforced*, and closes one real validation gap (Capture/Kill,
> see below).
>
> **⚠️ Line numbers below are from the original review (HEAD `e96fbb8`).** The `fix-code-review`
> run that closed M1–M7/L1 has since landed on `src/runtime.rs` and `src/tmux_exec.rs`, so anchors in
> those files have drifted. `src/model.rs` and `ui/src/lib/types/workflow.ts` were **not** touched by
> that run. **Locate code by symbol/grep, not by trusting these line numbers.**

---

## Goal

Replace the `WorkflowNode` "kind tag + 10 parallel `Option<*Config>` fields" shape with a single
tagged `enum NodeKind { Decide(DecideConfig), Spawn(SpawnConfig), … }` so that illegal node/config
pairings become **unrepresentable**, the ~7 scattered `match node.node_type` sites collapse to one
exhaustive `match`, and the 3 defensive `.context("missing …Config")?` unwraps can be deleted.

This necessarily changes the persisted/wire JSON shape, so it ships behind a **`version: 3`**
workflow schema (the canonical schema per `CLAUDE.md`) with a load-time migration from the current
flat `version: 2`-style `{type, decideConfig, …}` shape.

## Scope & owning files

- **`src/model.rs`** — `WorkflowNode` (review anchor :565-632, incl. `agent_config` ~:605,
  `node_type` ~:570); `WorkflowNodeType` enum (review anchor :89-106, 14 snake_case variants);
  `validate_workflow` and helpers including `validate_tmux_node_config` (~:1284) and the **Capture/Kill
  no-op arm** (~:1374-1375).
- **`src/runtime.rs`** — the 3 defensive unwraps `.context("missing …Config")?` for subflow / decide /
  batch (review anchors ~:2344 / ~:2821 / ~:3704 — **drifted, re-grep**); the per-kind accessors
  `prompt_template_for_node` / `agent_name_for_node` / `timeout_for_node` (~:1329 / :1351 / :1376);
  and every `match node.node_type` dispatch site.
- **`src/tmux_exec.rs`** — node dispatch that branches on node kind (**drifted, re-grep**).
- **`ui/src/lib/types/workflow.ts`** — the TS `WorkflowNode` type (review anchor :223-247) and the
  open `RunEvent` type if touched.
- **~15 lockstep frontend consumers** — store / inspector / tests that read/write the flat
  `{type, decideConfig}` node shape (e.g. `ui/src/lib/stores/workflowStore.svelte.ts`,
  `ui/src/lib/components/InspectorPanel.svelte`, and their tests). Enumerate exhaustively before
  starting — grep the codebase for `node_type` / `nodeType` / `*Config` field access.
- **Migration + fixtures** — wherever workflow JSON is versioned/loaded; all `version: 2`-era
  fixtures under `templates/`, `workflows/`, and test data.

## Current state (the problem)

`WorkflowNode` (`src/model.rs`) models node kind as a `WorkflowNodeType` enum **plus** ~10 parallel
`Option<*Config>` fields. So illegal pairings — `node_type == Decide` with `decide_config == None`, or
a stray mismatched config — are type-representable. The kind→config invariant is re-derived across ~7
`match node.node_type` sites plus the 3 defensive `.context()?` unwraps in `src/runtime.rs`, and every
`WorkflowNode` literal must set all 10 configs to `None`. In practice `validate_workflow`
(`src/model.rs`, called from `src/api.rs` before any run) rejects the missing-config case with a hard
error, so this is a **maintenance smell — scattered re-derivation, not a live bug.**

**One genuine validation gap:** Capture/Kill nodes currently fall into a **no-op arm** in
`validate_workflow` (`src/model.rs` ~:1374-1375) with **no config-presence check** — those node kinds
are not validated for required config at authoring time. The enum migration closes this for free
(their configs become non-optional fields of the variant); if you want an incremental safety win
*before* the full refactor lands, add the missing presence check there as a tiny standalone first
commit.

## Implementation plan

1. **Define `NodeKind`.** Replace `node_type: WorkflowNodeType` + the 10 `Option<*Config>` fields with
   one tagged `enum NodeKind { Decide(DecideConfig), Spawn(SpawnConfig), RunAgent(...), Subflow(...),
   ParallelBatch(...), Capture(...), Kill(...), … }` covering all 14 current variants. Keep any
   kind-independent `WorkflowNode` fields (id, name, edges, etc.) as siblings of the `kind`.
2. **Collapse dispatch.** Convert the ~7 `match node.node_type { … }` sites and the per-kind accessors
   (`prompt_template_for_node` / `agent_name_for_node` / `timeout_for_node`) into single exhaustive
   `match node.kind { … }`. **Delete** the 3 `.context("missing …Config")?` unwraps in
   `src/runtime.rs` — the config is now guaranteed present by the type.
3. **Validation.** Fold the existing per-config checks (including `validate_tmux_node_config`) into the
   exhaustive match; the Capture/Kill no-op arm disappears (its presence requirement is now structural).
   Keep all *semantic* validation (referential integrity, outcome lists, batch bindings) — only the
   *presence* re-derivation goes away.
4. **Wire format + `version: 3` migration.** Move the persisted JSON from the flat
   `{type, decideConfig, spawnConfig, …}` shape to a nested / internally-tagged form (e.g. serde
   `#[serde(tag = "type")]`-style on `NodeKind`, or `kind: {type, config}` — pick the shape that
   round-trips cleanest with the TS type and document it). Bump the workflow schema to `version: 3`
   and add a **load-time migration** that upgrades existing `version: 2` workflows so they still load.
   Migrate all in-tree fixtures/templates and add a round-trip (serialize→deserialize) test plus a
   v2→v3 migration test.
5. **Frontend lockstep.** Update `ui/src/lib/types/workflow.ts` `WorkflowNode` to the nested shape and
   update every consumer (~15: store, inspector, defaults like `CONFIG_DEFAULTS`/`defaultNodeConfig`,
   tests) to read/write `kind` instead of `node_type` + flat config. Keep backend authoritative for
   validation/traversal/checkpoint per `CLAUDE.md`.
6. **Verify** with `just test` (or `cargo test` + `npm test`), `just typecheck` (`svelte-check`), and
   `cargo check`.

## Acceptance criteria

- [ ] `WorkflowNode` holds a single tagged `NodeKind`; no `node_type` field and no `Option<*Config>`
      kind-fields remain. Illegal kind/config pairings no longer compile.
- [ ] The 3 `.context("missing …Config")?` unwraps in `src/runtime.rs` are deleted; dispatch is a
      single exhaustive `match` per site (no `_ => unreachable!()`/`.context()` fallbacks for the
      kind→config invariant).
- [ ] Capture/Kill node config presence is enforced structurally (no silent no-op validation arm).
- [ ] Workflow schema is `version: 3`; a v2→v3 migration loads all pre-existing workflows; all in-tree
      `templates/`/`workflows/` fixtures are migrated.
- [ ] TS `WorkflowNode` type and all ~15 consumers updated; `just typecheck` clean.
- [ ] `just test` green (Rust + UI), including new round-trip + migration tests.

## Out of scope

- Any behavioral change to workflow execution semantics — this is a pure representation refactor.
- The unrelated below-the-cut cleanups noted in the review (dead code, duplication, efficiency).
- Folding this into the `feature/tmux-panes` branch — it must be its **own PR** off the appropriate
  base, because of the breaking wire change.

## Notes for the implementing agent

- Routing suggestion (per the repo's agent-routing rubric): this is **deep cross-module + architecture
  + a breaking wire migration spanning backend and ~15 frontend consumers** → a **Codex
  (`/codex-developer`)**-class task, not a quick Composer edit.
- `src/model.rs` and `ui/src/lib/types/workflow.ts` are at their reviewed line anchors;
  `src/runtime.rs` / `src/tmux_exec.rs` have drifted post-`fix-code-review` — grep by symbol.
- Land the Capture/Kill presence-check as a small first commit if you want an incremental safety win
  before the larger refactor.
