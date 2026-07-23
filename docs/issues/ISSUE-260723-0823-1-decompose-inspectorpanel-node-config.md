---
id: ISSUE-260723-0823-1
kind: issue
category: enhancement
status: needs-triage
summary: Decompose InspectorPanel.svelte — extract per-node-kind config panels and inspector siblings
---

## Triage Notes

Escalated from the code-review walkthrough smells stage (user-directed).

**Source:** `feature-tmux-panes-code-review-20260720-032528.md` — smell `S3`

**Ledger key:** `ui/src/features/editor/InspectorPanel.svelte:1` + Divergent Change — the SMELLS-LEDGER.md row this refactor closes.

**Acceptance criterion:** Whoever lands the fix MUST close the ledger row on completion by running:

```
user/skills/dual-code-review/scripts/smells-ledger.sh fix docs/issues/SMELLS-LEDGER.md '`ui/src/features/editor/InspectorPanel.svelte:1` + Divergent Change ' <landing-commit-sha>
```

where `<landing-commit-sha>` is the SHA/ref of the commit that lands the refactor. The ledger row currently stays `open` until then.

### Problem

`ui/src/features/editor/InspectorPanel.svelte` is a 1,962-line Svelte component (+1486/−439 on branch feature/tmux-panes) exhibiting a Divergent Change smell — it owns four independently-changing axes:

- Per-node-kind config sub-forms, all inlined in one `{#if selectedNode.kind.type === …}` chain running unbroken from `:709` to `:1583` (ten kinds: task/run_agent at `:709-1140` is the largest arm, plus decide `:1182`, parallel_batch `:1219`, spawn `:1277`, send `:1352`, wait `:1384`, capture `:1452`, kill `:1495`, subflow/call `:1518`).
- Agent-capability interpretation: `agentCaps` (`:293`), `supportedAccessModes` (`:487`), `capBadges` (`:465`).
- Access-mode reconciliation: two write-during-render `$effect`s (`:503`, `:515`).
- Unlock prompting: `unlockPrompt` (`:185`), `runSelectedNodePreview` (`:187`), `<PasswordDialog>` (`:1901`).
- The same file also renders the edge inspector (`:1629-1690`) and the full workflow-level inspector (`:1692-1898`).

Impact is maintainability (any new node kind, capability change, or unlock change edits the same file), not correctness. Advisory severity, MEDIUM cap.

### Chosen approach (fix S3)

Extract the simple per-node-kind panels AND the edge/workflow inspectors into child components; leave task/run_agent inline; do NOT lift unlock to the shell.

- Pull the eight simple arms (spawn/send/wait/capture/kill/decide/parallel_batch/subflow) into per-kind child components, each taking `{ node, capabilities, update }`. Each simple arm already has a clean seam: its accessor (e.g. `spawnConfig`, `:262-290`) is a pure `node → config`, and its writer is the `updateConfig(configKey, field, value)` callback (`:373`). The `updateConfig` switch (`:377-423`) collapses as each panel owns one arm.
- Move the edge inspector (`:1629-1690`) and workflow-level inspector (`:1692-1898`) into sibling components.
- Constraints that MUST be honored:
  - Preserve H9's `?? DEFAULT_*` capture/kill accessors (`:274-280`) verbatim — an earlier fix depends on them.
  - Do NOT lift unlock/password prompting to AppShell: M25 (dismissed) ruled the two unlock sites (run-start at `AppShell.svelte:208` vs node-preview at `InspectorPanel.svelte:196`) are independent concerns, and a context hoist would break the load-bearing standalone-mount test at `InspectorPanel.test.ts:101`/`:117-155` (reaffirmed by L20, dismissed).
  - Do NOT force-extract the task/run_agent arm (`:709-1140`): it shares `agentConfigFields` (`:529`), both access-mode `$effect`s (`:503`, `:515`), `nodeConfig` (`:331`), `agentCaps`, and `openSections` machinery (`:132-165`); extracting it would thread 6+ reactive values through props and fails the deletion test.
- Capability DATA already lives in the shell (`capabilities` prop at `:47`, owned by AppShell `:48`/`:108`); only the interpretation stays in the panel, which is intrinsic to node editing.

This is a sizeable, deliberate refactor to be scheduled — not an inline review-fix.

### Sub-task (folded in) — S6: pane-config field duplication between run_agent and spawn

**Source:** `feature-tmux-panes-code-review-20260720-032528.md` — smell `S6`

**Ledger key:** `ui/src/features/editor/InspectorPanel.svelte:789` + Duplicated Code — a second SMELLS-LEDGER.md row this refactor must close.

**Acceptance criterion:** On completion, also close the S6 ledger row by running:

```
user/skills/dual-code-review/scripts/smells-ledger.sh fix docs/issues/SMELLS-LEDGER.md '`ui/src/features/editor/InspectorPanel.svelte:789` + Duplicated Code' <landing-commit-sha>
```

**Why folded here, not fixed independently:** S6 is a *distinct* duplication that this refactor as scoped does **not** subsume. The run_agent PTY section (`:780-873`) and the spawn section (`:1277-1350`) render four near-verbatim field blocks — Pane name (`:785-792`/`:1302-1309`), Access select (`:793-804`/`:1318-1329`), Working directory (`:805-812`/`:1330-1337`), Extra-args textarea (`:852-863`/`:1338-1349`), ~32 shared lines. Because the chosen approach extracts the *simple* per-kind panels but **leaves task/run_agent inline**, a naive spawn extraction would relocate one copy across a file boundary and leave the run_agent copy behind — making the duplication less visible, not gone.

**Required sub-task:** the spawn extraction MUST emit a shared `PaneConfigFields` snippet/component (name/access/cwd/extraArgs), taking `{ config, update, cwdPlaceholder, accessProfiles }`, consumed by *both* the extracted SpawnPanel and the still-inline run_agent block. run_agent keeps its timeout/idle/until/killAfter inline; spawn keeps agent/command/session inline. This also enables the single-edit point required by the HIGH access-select fix (review doc `:344`), which names both `:782-788` and `:1301-1307` as twins.
