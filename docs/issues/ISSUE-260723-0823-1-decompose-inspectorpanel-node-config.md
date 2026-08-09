---
id: ISSUE-260723-0823-1
kind: issue
category: enhancement
status: needs-triage
summary: Decompose InspectorPanel.svelte — extract per-node-kind config panels and inspector siblings
blocked_by: [ISSUE-260808-2000-07, ISSUE-260808-2022-04, ISSUE-260808-2022-11]
---

## Agent Brief

**Category:** enhancement
**Summary:** Extract every typed-config node-kind sub-form out of `InspectorPanel` into its own panel component, moving the config-writing plumbing into a shared module — behavior-preserving.

**Current behavior:**

The `InspectorPanel` editor component renders a config sub-form for every workflow node kind inline in one unbroken conditional chain, so the component changes whenever any node kind's editing UI changes. Survey the chain's arms with `rg -n '\{:else if selectedNode\.kind\.type' ui/src/features/editor/InspectorPanel.svelte`.

Most of what those arms need to read and write lives in the component's own instance script: a typed-config accessor per kind, a default-config constant per kind, and one central config-writing function whose switch narrows on a config key. Enumerate them with `rg -n 'const DEFAULT_[A-Z_]*CONFIG|function updateConfig|const update[A-Z][a-zA-Z]* =|^\s*function [a-z][a-zA-Z]*Config\(' ui/src/features/editor/InspectorPanel.svelte`. Because that plumbing is component-local rather than importable, adding or changing a node kind means editing this component even when nothing else about it changes — the Divergent Change smell recorded against it.

**This issue's baseline is not the tree as it stands today.** Its `blocked_by` siblings land first, and one of them — `ISSUE-260808-2022-11` — creates the shared plumbing module and lifts the generic merge helper into it, because its own run-as helpers cannot move without it. So by the time this issue starts, that module exists and the merge helper is already importable; check with `rg -n 'mergeConfig' ui/src/features/editor/` before assuming otherwise, and read where the definition sits rather than trusting the hit count. The sibling is required to export it under the name `mergeConfig`, but not to use any particular declaration form, so do not probe for `function mergeConfig` specifically. This issue extends that module rather than creating one.

A second parent-defined snippet renders a shared inputs-binding list; enumerate its definition and consumers with `rg -n 'inputBindings' ui/src/features/editor/InspectorPanel.svelte`.

**Desired behavior:**

Every node kind that carries a typed config object renders through its own panel component. Those kinds are `decide`, `parallel_batch`, `spawn`, `send`, `wait`, `capture`, `kill`, and `subflow`/`call` — the last pair sharing one panel, as they share one config object today.

**The config plumbing moves into a plain shared module.** The default-config constants and the pure `node → config` accessors lift out of the component's instance script into an importable module. That module already exists when this issue starts — `ISSUE-260808-2022-11` creates it to lift the generic merge helper, which its run-as helpers depend on — so this issue extends it rather than creating it, and the merge helper is already importable. Each panel imports what it needs and performs its own write through the workflow store, so the central config-writing switch loses its per-kind cases. This is the part that actually cures the smell: afterwards, adding a node kind means adding a panel and its default, with no edit to the parent's plumbing. Passing parent-bound update callbacks down instead is rejected — it would keep every kind's case in the parent switch and leave the smell half-cured.

The `run_agent` arm stays inline and keeps its own writer and its case, so the switch does not disappear entirely.

**The inputs-binding snippet becomes a shared component.** Both of its consumers are arms that leave the parent, so promoting it is not about sharing with a surface that stays — it is that a parent-defined snippet is invisible inside a child component, and duplicating it into two panels would create a fresh duplication of exactly the kind this decomposition exists to remove. Promote it to a shared component that both panels consume.

Rendered output is unchanged for every node kind: same controls, same labels, same order, same editing behavior.

**Key interfaces:**

- **Panel dependency surface** — the floor is the selected node, the runtime capabilities, and whatever that arm writes through; it is *not* a ceiling. Derive each panel's real surface from the arm being moved. At least the `parallel_batch`, `spawn`, and `subflow` arms additionally need the active workflow document, `subflow` additionally resolves a referenced subflow and offers drill-in, and `spawn` needs the selected agent's access-profile list.
- **Store access** — the workflow store is a module singleton and sibling editor components already import it directly. Do that rather than introducing Svelte context, which would break the standalone-mount pattern the component's tests rely on.
- **Config accessors** — preserve each accessor's existing fallback semantics exactly when it moves. The `capture` and `kill` accessors in particular fall back to their default-config constants; an earlier fix (H9, the kind-shaped backfill for the two kinds the backend omits) depends on that, and dropping it re-blanks the editor on an ordinary save/reload.
- **Shared per-control components** — the pane name, access, working-directory, and extra-args controls are being extracted into shared components by `ISSUE-260808-2000-07`. The `spawn` panel must consume those shared components rather than re-inlining the controls; that issue lands first.
- **Locally-scoped styles** — the contract-summary rules used by the `subflow` arm move with that panel. Enumerate with `rg -n 'contractBox' ui/src/features/editor/InspectorPanel.svelte`.
- **Destination** — colocate with the existing editor components, matching the convention already used by the sibling components in that directory; panels may go in a subdirectory.

**Acceptance criteria:**

- [ ] Each in-scope kind renders through a dedicated panel component. For each panel name chosen, `rg -n '<Name>' ui/src/features/editor/InspectorPanel.svelte` returns a match — run the command once per name rather than as a single alternation. No such component names exist before this change. Verify by reading that each arm renders its panel, since an import alone would satisfy the grep.
- [ ] The per-kind writers and their switch cases have left the parent. `rg -n 'const update(Decide|Batch|Spawn|Send|Wait|Capture|Kill|Subflow) =' ui/src/features/editor/InspectorPanel.svelte` and `rg -n 'case "(decide|batch|spawn|send|wait|capture|kill|subflow)Config"' ui/src/features/editor/InspectorPanel.svelte` each return no matches; each returns matches before this change.
- [ ] The per-kind config plumbing is importable rather than component-local. After the change, `rg -n 'const DEFAULT_[A-Z_]*CONFIG' ui/src/features/editor/InspectorPanel.svelte` returns no matches for the in-scope kinds and `rg -n 'const DEFAULT_[A-Z_]*CONFIG' ui/src/features/editor/ --glob '*.ts'` shows them in a shared module; before this change they are defined only inside the parent's instance script. The generic merge helper is **not** part of this criterion — `ISSUE-260808-2022-11` lifts it into the same module first, because its run-as helpers cannot move without it, so this issue extends an existing module rather than creating one.
- [ ] The inputs-binding snippet is a shared component. `rg -n '\{#snippet inputBindings' ui/src/features/editor/InspectorPanel.svelte` returns no matches; returns a match before this change.
- [ ] **Preservation** — `npx vitest run --config ui/vite.config.ts ui/src/features/editor/InspectorPanel.test.ts` passes, and every test case present in that file at the baseline still passes with its assertions unmodified. New cases may be added; existing assertions may not be relaxed. Green before and after.
- [ ] **Preservation** — the baseline tests covering `capture` and `kill` default configs pass unmodified. These are the targeted guard on the accessor fallbacks above. Green before and after.
- [ ] **Preservation** — `just typecheck` reports no new errors. Green before and after.
- [ ] The Divergent Change smells-ledger row is closed against the landing commit. `rg -n 'InspectorPanel\.svelte:1' docs/issues/SMELLS-LEDGER.md` shows a `fixed:` verdict rather than `open`; it reads `open` before this change. The helper is `~/.claude/skills/dual-code-review/scripts/smells-ledger.sh fix docs/issues/SMELLS-LEDGER.md '<row key>' <landing-commit-sha>` with row key `` `ui/src/features/editor/InspectorPanel.svelte:1` + Divergent Change ``. Editing the verdict column directly is equally acceptable; the criterion is the closed row, not the mechanism. **This row is a whole-component judgment**: close it only once the sibling issues listed in `blocked_by` have landed, so that the per-kind, edge, workflow, and duplicated-control axes have all been separated.

**Accepted risk (explicit):** the component's test suite covers few of the extracted panels directly, and no test asserts field order for them. Their "renders identically" contract is verified by review and typecheck rather than mechanically, except where a preservation criterion above names a specific guard. This is a deliberate maintainer decision (2026-08-08) to concentrate new coverage where risk concentrates — the config-default fallbacks here, and field order in `ISSUE-260808-2000-07` — rather than to demand fresh coverage for every extracted surface. An agent may add coverage but is not required to.

**Why this is a single slice:**

The plumbing lift is what makes this one piece of work rather than eight. The moment any panel owns its own write, the merge helper, the defaults, and that kind's accessor must already be importable — so the module lift is a precondition of the first panel, not a step after the last. Once it has happened, the parent's switch is mid-migration until the final case leaves, and every panel extracted before that point depends on the same module landing. The criteria are views of one migration: the panels, the plumbing they import, and the switch cases they vacate.

This record makes no claim to be atomic with its siblings. The edge inspector, the workflow inspector, and the shared per-control components were split out precisely because each is independently green.

**Out of scope:**

- **The shared per-control form components belong to `ISSUE-260808-2000-07`.** Do not re-implement the pane name, access, working-directory, or extra-args controls; the `spawn` panel consumes the shared components that issue creates.
- **The edge inspector belongs to `ISSUE-260808-2022-04`** and **the workflow-level inspector, the shared agent-config fields, and the workflow-level access-mode effect belong to `ISSUE-260808-2022-11`.** Do not touch those surfaces.
- **Do not extract the `task`/`run_agent` arm.** It shares the agent-config fields, the node-level access-mode effect, the node-config snapshot, agent capabilities, and the collapsible-section machinery; extracting it would thread six or more reactive values through props and fails the deletion test. It stays inline and keeps its own writer and switch case.
- **Do not hoist unlock/password prompting to the app shell.** M25 (dismissed) ruled that run-start unlock and node-preview unlock are independent concerns, and L20 reaffirmed it. A context hoist would also break the standalone-mount test, which renders this component with plain props and no shell context.
- **Do not extract the `approval`, `split`, or `collector` arms.** Unlike the in-scope kinds, these carry no typed config object, no accessor, and no writer wrapper, so there is no seam to extract along; each would become a near-empty component. They stay inline.
- No change to any node-kind config schema, its defaults, or its validation.
- No backend changes.
- No visual, styling, copy, or layout changes — the decomposition is behavior-preserving.
- Not a fix for any open access-profile or privilege finding; those are tracked separately.

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

> **Superseded (2026-08-08).** This section and `### Chosen approach (fix S3)` below are pre-split history, retained for provenance. They describe a scope that spanned four records and instruct extracting the edge and workflow inspectors, which this record's `## Agent Brief` now places explicitly out of scope. The Agent Brief governs; nothing in these two sections is contractual.

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

### Triage verification (2026-08-08)

Structural claims above were checked against the tree and hold: the node-kind chain, every cited arm boundary, the edge and workflow inspector ranges, the config-writing switch, the pure per-kind accessors, and the unlock sites are all where this record says they are. The three "MUST" constraints were each confirmed independently — the H9 default fallbacks are present on the capture and kill accessors, and the unlock constraint is load-bearing because the test file mounts the component standalone with plain props and no shell context. The pane-config duplication is real and near-verbatim, down to the shared working-directory placeholder expression. The component's unit tests pass at triage time, so the refactor has a green safety net to work against. Both ledger rows still read `open`.

**Corrections to the record, resolved into the Agent Brief:**

1. **Three arms were unaccounted for.** The chosen-approach section names eight simple arms to extract but the kind chain also carries `approval`, `split`, and `collector`, which appear nowhere in this record. Resolved on evidence rather than by asking: those three are the only arms with no typed config object, no accessor, and no writer wrapper — there is no seam to extract along, so the brief scopes them explicitly out and leaves them inline.

2. **The `agentConfigFields` snippet straddles the extraction boundary.** This record cites the snippet only as a reason not to extract `task`/`run_agent`, but the same snippet is also consumed by the workflow-level inspector, which the chosen approach *does* extract. Svelte snippets defined in a parent are not visible inside child components, so the workflow-inspector extraction hits exactly the problem S6 identifies for pane fields. Maintainer decision (2026-08-08): promote the snippet to a shared component consumed by both surfaces — the same remedy S6 mandates for the pane fields — rather than passing the snippet down as a prop or dropping the workflow inspector from scope. Consistency with the S6 decision was the deciding factor; it is the only option that removes the duplication instead of relocating it. Recorded in the brief.

3. **S6 cites a finding that does not exist.** There is no HIGH access-select finding at review doc `:344`; that offset lands in the unhandled-promise-rejection findings. The access-select work is a MEDIUM finding earlier in the same document, and it is already marked FIXED — the populated selects it called for have landed as two copies. So S6's "enables the single-edit point required by…" is retrospective, not a live dependency: nothing sequences ahead of this refactor. The brief does not repeat the citation.

4. **No destination was stated.** The brief pins colocation with the existing editor components. Note for whoever picks this up: `ui/src/components/{inspector,layout,run,workflow}/` are four empty directories that have never been committed to git — scaffolding from an abandoned earlier plan, not a destination. They are not part of this issue's scope.

**Incidental finding (not scope):** the component's scoped `<style>` block defines only rules used by the subflow arm and the workflow inspector; the widely-used field and inspector classes are global. Each scoped rule therefore travels with its own extraction and the parent's style block ends up empty — independent corroboration that the seams are clean.

- Scale snapshot (non-contractual): `wc -l ui/src/features/editor/InspectorPanel.svelte` → ~1.9k lines (2026-08-08)

**Readiness gate (cold-reader): FAIL** (round 1, 2026-08-08)

Classes 1, 2, 3, 8, and 9 clear — class 8 inert on a never-gated record, and all seven change-introducing acceptance observables verified red at baseline. Three classes fired.

**Class 4 — G1, the pane-config group is self-contradictory as specified.** The brief mandates a shared four-field pane-config group (pane name, access, working directory, extra-args) consumed by both the spawn panel and the inline `run_agent` block, *and* mandates unchanged rendered output. Those cannot both hold: the four fields are non-contiguous in both consumers. Verified independently — in `run_agent` they occupy positions 1, 2, 3, and 8 of nine sibling fields, with timeout/idle/ready-stable/until interleaved between; in `spawn` they occupy 3, 5, 6, and 7 of seven, with session name interleaved. A contiguous four-field component reproduces neither order. The design fork — accept a reorder, make the group slot-based so consumers interleave, or shrink it to the contiguous subset — is unresolved and belongs in the brief.

**Class 4 — G4, `$effect` #2 straddles the extraction boundary.** Verified independently: the second access-mode effect reads `showAgentDefaultsFor`, whose only UI owner is the workflow-level inspector (extracted by this brief) and whose reset lives in a separate parent effect; it writes workflow agent-defaults and does not touch `run_agent` at all. The claim inherited from this record's original chosen-approach section — that the `task`/`run_agent` arm shares *both* access-mode effects — is therefore false for the second one, and the brief never states where that effect, its trigger state, its writer, or its reset land after extraction. A write-during-render effect separated from its trigger state is a live behavior-preservation risk.

**Class 4 — G6, the demo seam cannot see most of the change.** The component's test suite covers neither the `decide`, `parallel_batch`, `send`, `wait`, nor `subflow` panels nor the edge inspector — six of the ten surfaces the brief promises are byte-identical — and no test asserts field order anywhere, so a G1 reorder would land green. The brief permits added coverage but never requires it, while its single-slice argument leans on that suite as the sole demo seam.

**Class 5 — G2/G3, delegable.** The stated `{node, capabilities, update}` panel contract is insufficient for three of eight panels: `parallel_batch` and `subflow` need the active workflow document (`subflow` also its resolved reference), and `spawn` needs capabilities plus the selected agent's access profiles. Separately, the brief's line saying the supported-access-modes helper "moves with" the agent-config group is unimplementable as written — a component's instance script is not importable and three of its four call sites stay in the parent; the helper belongs in a shared module. Both are fixable by rewriting the brief lines as explicit delegations with bounds.

**Class 7 kind (a) — three bare figures on brief text under edit.** "Used by exactly two regions", "written twice", and "consumed by both" are occurrence counts over current code stated without discovery commands. All three are true today; the authoring rule binds regardless. Remedy is command substitution, never refreshing the number. Ranked least important of the findings.

**Class 6 — epic-in-issue-clothing, prong (b) FIRES. Proposed remedy: split.** Prong (a) does not fire — the record mints no epic-scale decisions. Prong (b)'s second half does: the acceptance criteria decompose into independently green, independently verifiable batches. The brief's five-bullet single-slice argument was tested against the tree and three bullets failed. The central config-writing switch does *not* collapse when the last arm leaves — its `runAgentConfig` case survives by the brief's own out-of-scope rule — and its cases are textually independent, so no shared mutable state forces atomicity. Ledger row `:789` names a specific line and duplication rather than the component, and closes on the S6 work alone; this record's own heading files S6 as a "Sub-task (folded in)". And the "splitting re-creates the defect" bullet rebuts only an incoherent split, not one that keeps the field group with both its consumers. Two bullets survive: there is genuinely no demoable intermediate, and the work mints no decisions beyond mechanical relocation (G1 and G4 excepted).

Proposed cut lines, for maintainer confirmation: **(A)** edge inspector — depends only on the selected edge, its display name, capabilities, the store, and the condition builder, with zero coupling to the config switch, either field group, agent capabilities, or unlock; **(B)** pane-config group + spawn panel *with both consumers* — closes ledger row `:789` on its own and carries G1; **(C)** agent-config group + workflow inspector — carries G4; **(D)** the remaining six kind panels and writer removal — closes ledger row `:1`.

**What we've established so far:**

- The brief's scope boundaries, hoisted constraints, and citations all verify; the `approval`/`split`/`collector` exclusion is correct, and all four triage corrections hold.
- Every acceptance observable is falsifiable at baseline; the preservation criteria are green (suite passes, typecheck clean).
- Two design questions are now open that were not open before the gate: G1 and G4.

**What we still need from you (@reporter):**

- Confirm or reject the proposed **split** into batches A–D above, or direct that this stay one record with the class-6 finding overridden.
- **G1** — for the non-contiguous pane fields: accept a visual reorder, make the shared group slot-based so each consumer interleaves its own fields, or reduce the shared group to the contiguous subset and leave extra-args duplicated?
- **G4** — when the workflow inspector is extracted, does the second access-mode effect move with it along with its trigger state, writer, and reset, or stay in the parent?
- **G6** — require new test coverage for the currently-uncovered panels and the edge inspector as part of the work, or accept the unverified reorder risk explicitly?

### Gate round 1 resolved (2026-08-08)

All four questions answered by the maintainer; brief revised accordingly and returned to `needs-triage` for re-gating.

**Split — confirmed, as two records rather than the gate's four.** The gate's batch B is now `ISSUE-260808-2000-07` (shared pane-config field components + spawn panel), which owns the `:789` Duplicated Code ledger row. Batches A, C, and D stay here and own the `:1` Divergent Change row. The gate's four-way cut was reduced because A, C, and D share one completion check — the `:1` row names the component as a whole and closes only when it stops owning multiple axes, so none of those three closes it alone. Batch B was conceded because its ledger row names a specific duplication and closes on its own work; the earlier argument that the two were indivisible was wrong and is withdrawn in the brief. Noted openly for the next gate: the edge-inspector extraction remains independently verifiable and could still be carved out.

**G1 — resolved in the sibling record.** The single contiguous four-field group was unbuildable without a reorder. Replaced by one component per field, composed by each consumer in its existing order. Detail lives in `ISSUE-260808-2000-07`.

**G2/G3 — rewritten as explicit delegations.** The panel prop contract now states the three-prop minimum as a floor rather than a ceiling and names the arms that need more; the supported-access-modes helper is now specified as lifting into a shared module, with its call sites enumerated by command instead of asserted.

**G4 — the workflow-level access-mode effect moves with the workflow inspector**, together with its trigger state, its defaults writer, and that state's reset. The brief now also corrects the inherited claim that the `run_agent` arm shares both effects — it shares one. The possibility that the trigger state resets naturally on unmount, since the workflow inspector renders only when nothing is selected, is flagged for confirmation rather than assumed. A new acceptance criterion asserts the trigger state has left the parent, and a preservation criterion pins the existing workflow agent-defaults test as the guard.

**G6 — targeted coverage, remainder accepted explicitly.** Field-order coverage is required in the sibling record, where the reorder risk actually lives; the workflow agent-defaults test is pinned here as the guard on the effect move. Coverage for the remaining uncovered panels and the edge inspector is explicitly *not* required, and the brief now records that accepted risk in the open rather than leaving it implicit.

> **Partly superseded (2026-08-09).** The edge-inspector half of this exclusion was reversed by maintainer decision once the edge work became its own record: as a standalone record it carried no working guard at all, so `ISSUE-260808-2022-04` now requires a minimal edge-render test. The reversal is recorded there. Nothing else in this paragraph changes, and this record's own `**Accepted risk (explicit)**` paragraph is unaffected — it scopes to the extracted kind panels, which after the split are all this record still owns.

**Class 7 kind (a) — the three bare counts are gone**, replaced by discovery commands (`rg` over the component for the concern survey, the snippet's consumers, and the scoped-style usages). The pane-field count moved out with the sibling record.

### Gate round 2 resolved (2026-08-08)

Round 2 fired class 6 prong (b) again, on stronger evidence than round 1, plus two class-4 gaps and a class-7 figure. All are addressed by splitting this record further and renarrowing it to the node-kind panels; it is now round 3's subject.

**Class 6 accepted rather than argued a third time.** Two independent cold readers found this record multi-slice, and the second falsified its remaining atomicity claim outright: the `:1` row cannot close while a sibling record still owns the `spawn` axis, so a completion check shared with a different record cannot be what makes this one atomic. The decomposition is now four records — shared per-control components (`ISSUE-260808-2000-07`), edge inspector (`ISSUE-260808-2022-04`), workflow inspector with the agent-config promotion and the workflow-level effect (`ISSUE-260808-2022-11`), and this one for the kind panels. This record now carries `blocked_by` for the other three and closes the `:1` row last, with the closure condition stated in the criterion itself. The edge-inspector carve-out this record twice declined to make is the second of those records.

**Class 4 — G-A, the second snippet.** The inputs-binding snippet is rendered by two arms this record extracts, and the brief was silent on it. Unlike the agent-config snippet, both its consumers leave the parent, so "promote it so both surfaces share it" does not transfer. Maintainer decision (2026-08-08): promote it to a shared component anyway — duplicating it into two panels would manufacture a fresh duplication of exactly the kind this work exists to remove.

**Class 4 — G-B, writer ownership.** The brief endorsed two incompatible builds: panels owning their writes, and the parent supplying bound callbacks. Maintainer decision (2026-08-08): lift the default constants, the merge helper, and the accessors into a plain importable module, and let panels write through the store singleton. Bound callbacks were rejected because they keep every kind's case in the parent switch and leave the smell half-cured. This also dissolves the not-importable-accessor problem the sibling record hit independently, since the accessors become module exports.

**Class 4 — G-C, ledger semantics.** The `:1` closure test is now stated on the criterion rather than implied by an atomicity argument, and it is explicitly conditioned on the sibling records landing.

**Class 7 — the bare count is gone.** "Of the two write-during-render access-mode effects" asserted a figure no command on the surface derived, and there are three effects, not two. The effect move went to the workflow-inspector record; the surviving prose describes the plumbing qualitatively and hands the reader commands that enumerate it.

**Noted for round 3:** the gate observed that per-name greps are satisfiable by a single barrel import, so the panel criterion now requires reading that each arm renders its panel rather than trusting the grep. Stale pre-split prose earlier in these notes — the folded-in S6 sub-task, its "MUST emit a shared PaneConfigFields component" line, and the "eight simple arms" enumeration — is superseded by the Agent Brief and by the sibling records; it is left as history and is not the contract.

**Readiness gate (cold-reader): FAIL** (round 2, 2026-08-08)

Stamp written retrospectively on 2026-08-08: round 2's findings were recorded above under `### Gate round 2 resolved` but its terminal verdict was never stamped, so the matcher saw only round 1. Round 2's verdict was FAIL — class 6 prong (b), plus the unaddressed inputs-binding snippet, the writer-ownership contradiction, and a bare effect count. Recorded here so the round sequence is complete and the round-3 stamp below is not misread as round 2.

**Readiness gate (cold-reader): PASS** (round 3, 2026-08-08)

Every gap `fine`; class 6 does not fire on either prong; every class-7 surface maps to a non-blocking row; class 8 inert; class 9 does not fire on either arm, with all five change-introducing observables executed and red at baseline.

The reader verified the scope independently rather than accepting it: the eight in-scope kinds are exhaustive against the fourteen kind variants in the workflow types — ten carry a typed config object, minus `run_agent` which stays, with `subflow` and `call` sharing one config and therefore one panel. The three excluded arms were read directly and write their fields inline through the store, which independently corroborates this record's store-writes design. The inputs-binding promotion rationale checks out: exactly two snippets exist, and the inputs-binding one has exactly two consumers, both arms that leave the parent, with zero overlap against the agent-config snippet owned by `ISSUE-260808-2022-11`. Scope across all four records is disjoint with no gap, and only this record claims the `:1` row.

The reader also falsified this record's own precondition argument in a way worth recording: a panel *could* technically land without the module lift, since the merge helper is small and one default is empty, so the lift is a design-relative precondition rather than a code-forced one. The brief survives because it states the claim conditionally and mandates that design — but the conclusion drawn from it establishes ordering, not indivisibility. Prong (b) does not fire on the work regardless: the eight panels are homogeneous instances of one relocation sharing one module, one accessor set, and one completion check, unlike the heterogeneous axes that fired in rounds 1 and 2 and are now separate records.

Non-blocking notes carried forward, none owing a re-gate:

- The plumbing-enumeration command does not match the per-kind accessors it is offered to enumerate, though each sits adjacent to its default constant.
- The chain-survey command matches only `{:else if}` arms and silently omits the chain's opening arm, which is out of scope in two places anyway.
- The accepted-risk coverage claims carry no command; both were verified true.
- `### Problem` and `### Chosen approach (fix S3)` are now marked superseded in place, closing the one Triage-Notes/brief contradiction the reader found.
- The source review document still records S6 as folded into this record; it now belongs to `ISSUE-260808-2000-07`.
- A second per-kind default source exists in the store-side node metadata and diverges for `spawn`; consolidating it is forbidden by this brief's scope rules but not named explicitly.
- The three `blocked_by` siblings are untracked as the tree stands, so a fresh clone of this branch would not yet contain the artifacts this brief cites.

**Readiness gate (cold-reader): REOPENED** (round 4 pending, 2026-08-08)

The round-3 `PASS` above is withdrawn. It was stamped against a version of this brief that had already changed: while round 3 was running, the plumbing criterion and the Desired-behavior plumbing paragraph were rewritten to resolve a merge-helper collision found by the gate on `ISSUE-260808-2022-11`. The round-3 report's acceptance table executed `rg -n 'function mergeConfig' ui/src/features/editor/` as one of its five observables — a criterion this record no longer carries — which is direct evidence it read the pre-edit text. Its round-3 reasoning about the merge helper being "small" also only makes sense against that earlier version.

Nothing in the gate's *findings* was wrong; the defect is that a `PASS` certifies whichever text the reader examined, and that is not the text on disk. Re-gating at round 4 against the current brief. Two things are repaired in the same edit, so round 4 sees one coherent version:

- **Stale premise.** `Current behavior` still listed the generic merge helper among the plumbing living in the component's instance script, and its enumeration command still matched `function mergeConfig`. Once `ISSUE-260808-2022-11` lands — which this record's `blocked_by` guarantees happens first — both are false, leaving the brief self-contradictory against its own criterion. Corrected to describe the plumbing that is still component-local at this record's actual baseline.
- **Sequencing made explicit** rather than implied by `blocked_by` alone.

Process note for the rest of this decomposition: a gate round is only valid against the text it read. No brief in this set should be edited while its own gate is in flight; if a sibling's findings force an edit mid-round, the round is void and must be re-run rather than stamped.
