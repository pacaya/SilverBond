---
id: ISSUE-260808-2022-11
kind: issue
category: enhancement
status: done
summary: Extract the workflow-level inspector, promote the shared agent-config fields, and move the workflow-level access-mode effect
claimed_by: implement-issue@macmini.home
claimed_at: 2026-08-09T13:44:31Z
---

## Agent Brief

**Category:** enhancement
**Summary:** Move the workflow-level inspector out of `InspectorPanel` into its own sibling component, promoting the agent-config fields it shares with the `run_agent` block and taking the workflow-level access-mode effect with it — behavior-preserving.

**Current behavior:**

The `InspectorPanel` editor component renders the workflow-level inspector inline in the branch taken when neither a node nor an edge is selected. That branch carries more than agent defaults: enumerate its sections with `rg -n 'inspectorSection__title|isDrilledIn' ui/src/features/editor/InspectorPanel.svelte` and read the region the extent command below identifies. It renders a **Workflow** section (name, goal, working directory, entry node, orchestrator), a **Limits** section, a **Run As / Sandbox** section with its attach hint and copy action, an **Agent defaults** section, and a **Variables** section. All of them move. That branch changes for entirely different reasons than node editing does, and it is a substantial region of an already-oversized component; locate its extent with `rg -n '^\{#if |^\{:else|^\{/if\}' ui/src/features/editor/InspectorPanel.svelte`.

Several things couple that branch to the rest of the component, and each must be dealt with before it can move.

An `agentConfigFields` snippet defined in the parent is rendered both by the `run_agent` node-level block and by the workflow-level agent-defaults block; enumerate its definition and consumers with `rg -n 'agentConfigFields' ui/src/features/editor/InspectorPanel.svelte`. A Svelte snippet defined in a parent is not visible inside a child component, so the branch cannot simply move.

A write-during-render effect reconciles workflow-level agent defaults when the expanded-agent state changes; that state is toggled only by this branch's UI and is additionally reset by the parent's selection-change effect. Enumerate the effects with `rg -n '\$effect\(' ui/src/features/editor/InspectorPanel.svelte` and the expanded-agent state with `rg -n 'showAgentDefaultsFor' ui/src/features/editor/InspectorPanel.svelte`.

The run-as helpers this branch owns are written in terms of a generic merge helper that lives in the parent's instance script and is also used by the node-kind config writers that stay behind; enumerate its definition and every call site with `rg -n 'mergeConfig' ui/src/features/editor/InspectorPanel.svelte`. Its semantics — defaults-merge with delete-on-`undefined` — are load-bearing for clearing run-as back to unset.

**Desired behavior:**

The workflow-level branch renders through its own sibling component, and the parent's top-level conditional keeps its shape.

**The shared agent-config fields become a component.** The snippet is promoted to a shared component consumed by **both** the extracted workflow inspector and the `run_agent` block that stays in the parent. Passing the snippet down as a prop is rejected: it would leave the parent owning a chunk of the child's rendering. Duplicating it is rejected outright.

**The access-modes helper lifts into a plain module.** The helper that derives supported access modes from a profile list is called both from inside the promoted snippet and from call sites that stay in the parent — enumerate them with `rg -n 'supportedAccessModes' ui/src/features/editor/InspectorPanel.svelte`. A component's instance script is not importable, so the helper cannot travel inside the new component; lift it into a plain shared module that every caller imports. The agent may choose the module's name and its position within `ui/src/features/editor/`, but not somewhere outside that directory — the acceptance criterion below is scoped there and would be unsatisfiable otherwise.

**The generic merge helper lifts into a plain shared module.** The run-as helpers cannot travel with the branch while they depend on a helper defined in the parent's instance script, and that helper's remaining call sites stay in the parent — so it cannot move wholesale either. Lift it into an importable module that both the extracted component and the parent consume. Preserve its semantics exactly, including delete-on-`undefined`, which is what lets run-as clear back to unset. The kind-panels issue later extends the same module with the per-kind config accessors and defaults; creating it here is expected, and that issue's criteria are written to account for it already existing.

**The workflow-level access-mode effect moves with the branch,** together with the expanded-agent state that triggers it and the agent-defaults writer it calls. Splitting a write-during-render effect from the state that triggers it is the most likely way this refactor silently stops preserving behavior, so keep them together.

The suite already exercises this reconciliation for an agent whose access-profile list holds a single entry — locate it with `rg -n 'unrestricted' ui/src/features/editor/InspectorPanel.test.ts` and read the case the hits sit in. That is where a broken reconciliation surfaces first, so run it early rather than waiting on the whole file. It is not a separate acceptance criterion: the file-scoped preservation criterion below already requires it to pass unmodified, and naming it twice would only add a second claim about the suite that decays as cases are added to that file.

**Keep an explicit reset of the expanded-agent state.** The parent's selection-change effect currently resets it, and that reset must survive the move rather than being dropped in favor of unmount semantics.

The reason is that this branch is not always freshly mounted when the reset fires. Any store transition that sets the selection to the workflow kind with a null id leaves both the selected node and the selected edge null, so this branch is taken. Derive that set rather than assuming it: run `rg -n 'selection = \{ kind: "workflow", id: null \}' ui/src/lib/stores/workflowStore.svelte.ts`, open the file at each hit and read the store method enclosing it, then find each of those methods' callers by grepping its name over `ui/src`.

Those transitions do not all behave the same way, and the split is what makes the reset load-bearing. Some are reachable **while this branch is already mounted** with a panel expanded — on those paths component-local state would not reset, and a panel that collapses today would stay open. Others are reachable only from a state where a node is selected, so the branch is unmounted at that moment and remounts fresh; drill-in is one of these, and its entry points are enumerated by `rg -n 'drillIntoSubflow' ui/src --glob '!*.test.ts'`. Work out which transitions fall on which side from those outputs rather than generalizing from any one path.

Reproduce the reset inside the extracted component so the behavior is unchanged on every path. That instruction is unconditional — it does not depend on how the transitions divide, and the division above is why dropping the reset in favor of unmount semantics is unsafe rather than a description of what to implement.

The node-level access-mode effect, which reconciles the *selected node's* access mode, is not this issue's concern and stays in the parent.

Rendered output is unchanged across every section named above — same controls, same labels, same order, and same behavior for the workflow fields, the limits, run-as with its attach hint and copy action, agent defaults, and variables, including whether each section renders at all under the drilled-in conditional.

**Key interfaces:**

- **Agent-config component** — its props mirror the current snippet's parameters: capabilities, an access-profile list, the current values, a typed update callback keyed on the agent-defaults shape, and placeholder labels. The callback is generic over the agent-defaults key in its snippet form; either preserve that with Svelte 5's `generics` attribute or widen it, provided no call site loses type safety. Its call sites pass differing placeholder labels and differing update targets — one writes node config, the other writes workflow agent defaults — so both must remain expressible; compare them via the `agentConfigFields` command above.
- **Workflow inspector's own surface.** The run-as helpers, the attach-hint derivation, the clipboard-copy action and its copied-state flag, and the agent-defaults reader belong to this branch and move with it, as do the expanded-agent state and the effect named above. **Do not decide membership by whether a consumer's line number falls inside the branch's template extent.** Most of these members live in the instance script, nowhere near that extent, and the line-number test gives the wrong answer twice over: for members consumed only by each other, and for the expanded-agent state, whose reset lives in a shared parent effect that clears unrelated state and is not itself moving.

  Resolve the remainder by construction rather than by rule. Move the named members, then let the compiler close the set: anything the moved code still needs and cannot reach is either a further member — grep its identifier over `ui/src/features/editor/InspectorPanel.svelte`, and if every remaining consumer is itself moving, take it — or a shared dependency that must be lifted into a module or passed in, as the two helpers above are. Anything left behind with no remaining consumer is dead and should go; unused locals are not a typecheck error here, so nothing will flag it for you.
- **Runtime capabilities.** The moved effect reads them, and the Agent defaults section renders only when they are present; find the uses with `rg -n 'capabilities' ui/src/features/editor/InspectorPanel.svelte` and decide for each whether the code around it is moving — the moved effect's own reads sit in the instance script, well outside the branch's template extent, so do not filter by that extent. Unlike the active workflow, capabilities are a parent prop rather than store state, so the extracted component cannot derive them from the store — it must receive them.
- **Scoped styles — derive the set, do not assume it.** Dump the component's scoped style block with `rg --pcre2 --multiline -n '(?s)^<style>.*?^</style>' ui/src/features/editor/InspectorPanel.svelte` and read every selector it defines. For each of those selectors, find its usages by grepping that class name over the same file. A rule whose usages all fall inside this branch travels with the extracted component, and its definition in the parent's `<style>` block goes; a rule with no usage inside this branch stays and belongs to a sibling issue. Any class this branch uses that no selector in that block defines is global and needs no action either way. Derive which rules fall on which side from that output rather than assuming the split in advance. Note that unused CSS selectors are a warning rather than an error here, so the typecheck will not force the cleanup.
- **The active workflow document, with its fallback.** Nearly every control this branch renders reads the active workflow; derive which ones with `rg -n 'activeWorkflow' ui/src/features/editor/InspectorPanel.svelte` and read every hit, deciding for each whether the code around it is moving — this branch's reads are not confined to its template extent, since several sit in instance-script members that travel with it. Read them rather than counting them: some are nested reads feeding a control's options rather than its value, and those depend on the fallback just as much. In the parent it is the store's drill-aware active document falling back to the component's `workflow` prop, and the store side of that is nullable. If the extracted component derives it from the store rather than receiving it, it must reproduce that fallback exactly. **Assume no preservation criterion would catch dropping it.** Check why for yourself with `rg -n 'setWorkflow|render\(' ui/src/features/editor/InspectorPanel.test.ts` and read how the suite's render helper is built — what it does before mounting is what decides whether any existing case could observe the fallback at all. This is the single most easily-missed dependency in this issue.
- **The drilled-in conditional.** Whether the Limits and Run As / Sandbox sections render at all is gated on the store's drilled-in flag; find it with `rg -n 'isDrilledIn' ui/src/features/editor/InspectorPanel.svelte`. That conditional moves with the sections it guards. Whether the suite already guards it is checkable with `rg -n 'drillIntoSubflow|isDrilledIn' ui/src/features/editor/InspectorPanel.test.ts` — read what any matching case asserts rather than assuming coverage either way. The conditional is easy to drop when relocating the sections it wraps.
- **Store access** — the workflow store is a module singleton and sibling editor components already import it directly. Do that rather than introducing Svelte context, which would break the standalone-mount pattern the component's tests rely on.
- **Destination** — colocate with the existing editor components, matching the convention already used by the sibling components in that directory.

**Acceptance criteria:**

- [ ] The workflow-level branch renders through its own component. `rg -n 'WorkflowInspector' ui/src/features/editor/InspectorPanel.svelte` returns a match; no matches before this change. (Name is illustrative; check against the name chosen.)
- [ ] The agent-config fields are a shared component and the inline snippet is gone. `rg -n '\{#snippet agentConfigFields' ui/src/features/editor/InspectorPanel.svelte` returns no matches; returns a match before this change.
- [ ] Both surfaces consume the shared component. `rg -l 'AgentConfigFields' ui/src/features/editor/` lists both the parent component and the extracted workflow inspector; no matches before this change. (Name is illustrative; check against the name chosen.) Verify by reading that both files render it, not merely import it.
- [ ] The access-modes helper is importable from a plain module rather than a component instance script. After the change, `rg -n 'supportedAccessModes' ui/src/features/editor/` shows its definition in a `.ts` module with the parent importing it; before this change the definition is inside the parent's instance script and no `.ts` module defines it.
- [ ] The workflow-level effect and its trigger state have left the parent. `rg -n 'showAgentDefaultsFor' ui/src/features/editor/InspectorPanel.svelte` returns no matches; returns matches before this change.
- [ ] The generic merge helper is importable from a plain module rather than a component instance script. After the change, `rg -n 'mergeConfig' ui/src/features/editor/` shows it defined and exported from a `.ts` module, with the parent importing rather than defining it; before this change the only definition is inside the parent's instance script. Whatever declaration form the lift uses, **export it under the name `mergeConfig`** — `ISSUE-260723-0823-1` runs a pre-flight check on that name and would read false otherwise.
- [ ] **A test guards the expanded-agent reset.** A test expands an agent's defaults panel in the workflow inspector, triggers a selection-clearing transition, and asserts the panel is collapsed afterwards. **It must drive that transition by calling the store directly, with no node selected.** That shape is required because it is the only one guaranteed to hold the branch mounted across the transition. Some UI routes to a selection-clearing transition — drill-in among them — are reachable only with a node already selected, which unmounts and remounts the branch, so the panel would come back collapsed whether or not the reset survived the extraction; a fixture driven that way guards nothing. Driving the store directly with nothing selected keeps the branch mounted on every path, so the assertion tests the reset rather than a remount. The existing suite already uses this pattern and carries a comment explaining why; find it with `rg -n 'drillIntoSubflow' ui/src/features/editor/InspectorPanel.test.ts`. The test lands in `ui/src/features/editor/InspectorPanel.test.ts`, so the preservation runner below executes it. Establish what the suite asserts about this behavior today with `rg -n 'showAgentDefaults|agentDefaults' ui/src/features/editor/InspectorPanel.test.ts` and read the matching cases; what that returns is why the reset is easy to drop during extraction.
- [ ] **Preservation** — `npx vitest run --config ui/vite.config.ts ui/src/features/editor/InspectorPanel.test.ts` passes, and every test case present in that file at the baseline still passes with its assertions unmodified. New cases may be added; existing assertions may not be relaxed. Green before and after.
- [ ] **Preservation** — `just typecheck` reports no new errors. Green before and after.

**Why this is a single slice:**

The branch cannot move without the snippet promotion, because a parent-defined snippet is invisible inside a child; the snippet cannot be promoted without lifting the access-modes helper, because a component instance script is not importable from the parent's remaining call sites; and the effect cannot stay behind, because its trigger state leaves with the branch that owns it.

Stated precisely, since earlier drafts overreached: that chain fixes the *order* the work must land in, which is not the same as atomicity. Either helper lift can land on its own with its criterion satisfied and both preservation runners green. What makes this one slice is that no such subset is independently **demoable** — a lifted helper whose consumers have not moved delivers nothing observable, and the only demo seam is the parent rendering this branch through a sibling component with the suite and typecheck green.

**Out of scope:**

- The node-kind panels, the edge inspector, and the shared per-control form components — each is a separate sibling issue.
- Extracting the `task`/`run_agent` arm. It stays inline and becomes a consumer of the promoted agent-config component.
- The node-level access-mode effect, the node-config snapshot, agent-capability interpretation, and the collapsible-section machinery — all stay in the parent.
- Hoisting unlock/password prompting anywhere.
- Closing any smells-ledger row. The `InspectorPanel.svelte:1` Divergent Change row closes on the kind-panels issue once every axis has been separated.
- Any change to agent-defaults semantics, run-as semantics, or access-mode reconciliation behavior.
- Any visual, styling, copy, or layout change.

## Context Pack — generated at claim (2026-08-09T13:44:31Z)

**PRD decisions relevant to this slice:** no PRD linked — this record carries no `prd:` frontmatter and the repo has no `docs/prd/` or `docs/decisions/prd/` tree. The binding decisions below are the maintainer decisions carried verbatim in `## Triage Notes`:
- Promote the shared `agentConfigFields` snippet to a component (2026-08-08) — passing it down as a prop or deferring is rejected; only promotion removes the duplication instead of relocating it.
- Move the workflow-level access-mode effect together with its trigger state, writer, and reset (2026-08-08) — taken after the gate falsified the claim that the `run_agent` arm shares both access-mode effects; it shares one.
- Ownership of the `mergeConfig` lift belongs to this record (gate round 1/2 resolution), because the run-as helpers cannot move without it; `ISSUE-260723-0823-1` was reopened and states the module already exists at its baseline. Export name must literally be `mergeConfig` — that sibling pre-flight-checks the name; declaration form is free.
- The kind-panels record (`ISSUE-260723-0823-1`) later extends the same module with per-kind accessors/defaults and owns the `InspectorPanel.svelte:1` Divergent Change ledger row; close no row here.
- The access-modes module must live within `ui/src/features/editor/` — the delegation is bounded to match its directory-scoped criterion.

**Test seam & Testing Decisions:** observable at `ui/src/features/editor/InspectorPanel.test.ts`, run via `npx vitest run --config ui/vite.config.ts ui/src/features/editor/InspectorPanel.test.ts`; every baseline case must still pass with assertions unmodified (new cases allowed, none relaxed), plus `just typecheck` with no new errors. Testing decisions that touch that seam:
- The reset-guard test must drive the selection-clearing transition by **calling the store directly with nothing selected** — the only shape guaranteed to hold the branch mounted; UI routes such as drill-in unmount and remount it, so a UI-driven fixture guards nothing. The existing suite already uses this pattern and carries a comment saying why.
- The new test must land in `InspectorPanel.test.ts` so the file-scoped preservation runner executes it.
- The single-access-mode reconciliation case (grep `unrestricted` in that file) is where a broken reconciliation surfaces first — run it early; it is guidance, not a separate criterion.
- Accepted-risk (recorded non-blocking at round 6): rendered-output preservation for the Workflow fields and the Variables section rests on review, not on any mechanical guard. The branch's own header markup is likewise unasserted.
- The harness sets a workflow before mounting, so no existing case would observe the active-workflow fallback being dropped — treat that fallback as unguarded.

**ADRs:** no ADR index in this repo — no `docs/adr/INDEX.md` and no `docs/model/generated/`; the only decision doc is unrelated (`docs/decisions/tmux-bin-resolution.md`). Tier skipped.

**Terms:** no glossary — this record carries no `terms:` frontmatter and there is no `CONTEXT.md` at the repo root or under a context root. Tier skipped.

**Full artifacts:** docs/issues/ISSUE-260808-2022-11-extract-workflow-inspector.md (Agent Brief + Triage Notes) · sibling records `ISSUE-260723-0823-1`, `ISSUE-260808-2022-04` under docs/issues/ · CLAUDE.md

## Resolution

**Commit:** `feat: extract the workflow-level inspector and shared agent-config fields (ISSUE-260808-2022-11)`

**Route:** cursor (`/cursor-developer`) for the implementation and both fix rounds — a large but well-specified in-directory Svelte extraction with the coupling set enumerated in advance and every discovery step handed over as a command; no cross-module reasoning, security surface, or infrastructure work.

**TDD:** n/a (linear) — behavior-preserving relocation of a template branch plus its writers and effects. The one falsifiable addition is the reset guard, which was verified red by deletion probe rather than assumed.

**Review telemetry:** 6 findings, **all LOW** — no CRITICAL, HIGH, or MEDIUM. 4 FIXED, 2 deferred (L1, L5). Fix rounds used: 2 of 4. Codex returned zero findings and zero smells independently; all six originated with the Claude reviewer, which also audited Codex's ten clean claims in reverse and could falsify none. Round 1's re-verify surfaced L6 **inside the comment L2's fix had just written**, so the same-site escalation rule applied: L2 and L6 are one defect, and round 2 removed the mechanism — a hand-maintained enumeration of store transitions in a comment, duplicating knowledge the store's seven `selection = { kind: "workflow", id: null }` sites own — rather than correcting the list. 5 advisory smells appended to the ledger, none promoted (the one Duplication smell's second site is pre-existing code outside the diff, so the in-diff promotion rule does not fire).

**Accepted risk — L1, recorded explicitly because the round-6 gate noted this record left it implicit.** `showAgentDefaultsFor` moved from the always-mounted parent into the branch-scoped child, so it is now destroyed on unmount *in addition to* being reset by the moved effect. The mandated reset is faithfully reproduced; the extra destruction is new behavior. It is observable only where the rendered branch flips without `store.selection` changing: `undo`/`redo` (`workflowStore.svelte.ts:564-577`) replace `workflow` without touching `selection`, so a `{kind:"node"}` selection can resolve to `null` and back. Repro: select a just-added node → undo → expand an agent-defaults panel → redo → undo. The old code kept the panel expanded; this code shows it collapsed. **No fix exists inside this slice's constraints** — strict preservation requires the state to live above the branch, which the Agent Brief mandates against ("the expanded-agent state … move[s] with it"), so this is a structural consequence of the brief rather than an implementation defect. Both reviewers confirmed the mechanism at both cited sites. Accepted on the grounds that the path is contrived, untested before and after, and the collapse is arguably the better behavior. If the drift is ever judged unacceptable, the fix is a brief-level decision on where the state lives, not a patch here.

**Deferred to a follow-up record — L5.** `update("toolToggles", checked ? { webSearch: true } : undefined)` writes a whole-object replacement in both directions, so any sibling toggle the shape later grows is discarded on either transition. Latent, not live: Codex confirmed `workflow.ts:45-47` defines no sibling key today. Moved verbatim by this extraction and squarely inside this brief's Out of scope ("any change to agent-defaults semantics"), so it was not fixed here — but promotion into a shared component doubles the blast radius once a second key is added. Filed as `ISSUE-260809-1545-01`.

**Suite:** green on the rebased tree — 466 Rust tests (449 unit + 17 integration) and 114 frontend tests across 10 files, 0 failures. `just typecheck` clean (598 files, 0 errors, 0 warnings).

**Reset guard verified red, twice and independently.** The Claude reviewer's deletion probe during review turned `InspectorPanel.test.ts:385` failing at 14/15 and green on restore. Re-probed at close against the rebased tree: removing only the `showAgentDefaultsFor = null` assignment from the reset `$effect` produced 1 failed / 16 passed, with the failure landing on `collapses expanded agent defaults when selection clears while workflow inspector is shown`. The guard is falsifiable, not a characterization net.

**Rebase onto `ISSUE-260808-2022-04`.** This record was implemented and reviewed against a baseline where the edge inspector was still inline in the parent. It was rebased onto the landed `-04` extraction before closing; the rebase was conflict-free, and `-04`'s H1 fix — the edge-*existence* branch predicate, as opposed to a selection-kind check — was confirmed intact afterwards. The full suite above was run on the rebased tree, not on the pre-park verification, which was deliberately not reused.

**Bundle.** The park commit deliberately shipped **without** rebuilding `public/`, so the bundle was stale at close and was rebuilt before landing. The CSS artifact's content hash changed, which for a behavior-preserving refactor warranted proof rather than assumption: the rebuilt bundle carries **427 CSS rules before and after, none added, removed, or altered**, and all **243 global (unscoped) rules appear in identical sequence**. Only Svelte-scoped rules reordered, which is expected because they moved into `WorkflowInspector.svelte` and `AgentConfigFields.svelte`; scoped rules carry unique per-component hashes and cannot collide, so the cascade is unaffected. This check mattered because `core.hooksPath` is unset in this clone and the frontend-freshness hook is guarding nothing.

**Environment note (not a code defect) — the park was avoidable.** This record parked at `ready-for-human` because `just test` aborted at `cargo test` with `command not found`, and the parking note concluded no Rust toolchain existed on this machine. The same wrong diagnosis cost `ISSUE-260808-2022-04` a park on the same day, and `ISSUE-260808-2000-07` had already established the true cause hours earlier: the toolchain was present under `~/.rustup/toolchains/`, only the `~/.cargo/bin` shim was missing. The maintainer subsequently installed the toolchain properly (`cargo`/`rustc` 1.97.1 on `PATH`). Nothing about the repo or this change caused the park; no code changed between the park and this close beyond the rebase and the bundle rebuild.

**Date:** 2026-08-09 (UTC)

## Code Review

Review file: `issue-260808-2022-11-code-review-20260809-141744.md`

Dual review (Claude + Codex, cross-verified, adjudicated inline). Codex returned zero findings and zero smells; all five findings originated with the Claude reviewer, which also audited Codex's ten clean claims in reverse and could falsify none. No CRITICAL, HIGH, or MEDIUM findings.

- L1 (LOW): Expanded-agent panel state no longer survives a branch flip that leaves `store.selection` untouched — deferred
- L2 (LOW): The load-bearing reset effect lost its explanatory comment during the move — FIXED
- L3 (LOW): New test's explanatory comment states an over-general rationale — FIXED
- L4 (LOW): Constant lookup maps are rebuilt per component instance — FIXED
- L5 (LOW): Unchecking Web search replaces the whole `toolToggles` object instead of clearing one key — deferred
- L6 (LOW): The new reset-effect comment names a transition that does not exist and omits one that does — FIXED

**ACCEPTED** after 2 fix rounds of a 4-round cap; every finding terminal (4 FIXED, 2 deferred). Round 1 fixed L2/L3/L4, both reviewers verifying. Its re-verify surfaced L6 **inside the comment L2's fix had just written**, so the same-site escalation rule applies: L2 and L6 are one defect, and round 2 redesigns the region to remove the mechanism (a hand-maintained enumeration of store transitions in a comment, duplicating knowledge the store's seven `selection = { kind: "workflow", id: null }` sites own) rather than correcting the list.

Smells: 5 advisory (all LOW, none promoted — the one Duplication smell's second site is pre-existing code outside the issue diff, so the in-diff promotion rule does not fire). All five appended as new ledger rows; no graduation rows emitted.

## Triage Notes

Minted 2026-08-08 as one of four records the `ISSUE-260723-0823-1` decomposition was split into, after that record's readiness gate fired class 6 prong (b) twice. Both gate rounds identified this branch as separable; the second round demonstrated that its writers and scoped styles are used nowhere else in the component.

Two decisions the parent record had already settled are carried here verbatim rather than re-opened. Promoting the shared snippet to a component, rather than passing it down as a prop or deferring the extraction, was a maintainer decision on 2026-08-08 — it is the only option that removes the duplication instead of relocating it. Moving the workflow-level access-mode effect together with its trigger state, writer, and reset was a maintainer decision on the same date, taken after the gate falsified the inherited claim that the `run_agent` arm shares both access-mode effects; it shares one.

Deliberately closes no ledger row — see Out of scope. The `:1` Divergent Change row is a whole-component judgment and is owned by the kind-panels record, which lands last.

- Scale snapshot (non-contractual): `rg -c 'showAgentDefaultsFor|runAsAttachHint|updateAgentDefault' ui/src/features/editor/InspectorPanel.svelte` → low-double-digit occurrences concentrated in the branch (2026-08-08)

**Readiness gate (cold-reader): FAIL** (round 1, 2026-08-08)

Classes 1, 2, 4, 5, 6, and 9 clear; class 8 inert. All three mandated moves were verified against the code and hold: the snippet has exactly the two claimed consumers with differing update targets, the access-modes helper genuinely has call sites that stay behind, and the expanded-agent state is written only by this branch. All five change-introducing observables verified red at baseline. Class 6 does not fire on either prong, though the reader noted the single-slice argument proves an ordering rather than atomicity — a precondition chain can land as three commits — and rested its verdict instead on the enablement half delivering nothing demoable on its own.

**Class 3 — the merge helper is a third coupling, unnamed, and collides with a sibling.** The run-as helpers this brief tells the agent to move are written in terms of a generic merge helper defined in the parent's instance script, whose remaining call sites stay in the parent. A component instance script is not importable — the brief's own argument for the access-modes helper — so the run-as helpers cannot travel as written, and the brief said nothing. Worse, lifting it is claimed as a deliverable by `ISSUE-260723-0823-1`, which is `blocked_by` this record and therefore lands afterwards; its criterion asserting the helper is defined only inside the parent beforehand would already be green at its own baseline.

**Class 7 kind (a) — three figures on brief text under edit.** "Two things couple that branch" was a count with no command deriving that set, and was also false — the merge helper is a third. "The two call sites pass different placeholder labels" was a count. "The largest single region of the component after the `run_agent` arm" was an unbacked magnitude ranking, true only under an unstated qualifier restricting it to template regions.

**Premise falsified, non-blocking:** the brief's suggestion that component-local state "would reset on unmount" is false. Drilling into a subflow, jumping to a breadcrumb level, and clearing the selection all set the selection to the workflow kind with a null id, leaving both the selected node and the selected edge null — so this branch stays mounted across exactly those transitions. The reader graded the delegation acceptable because the outcome was pinned either way, but noted the confirmation was unguarded: no criterion went red if the agent confirmed only against the node-select path and dropped the reset.

### Gate round 1 resolved (2026-08-08)

**Class 3 resolved by assigning the lift here.** This record now lifts the merge helper into the shared module, because it is the first record that needs it — the run-as helpers cannot move without it. The brief states that the kind-panels issue later extends the same module. That issue's plumbing criterion has been rewritten to name the per-kind accessors and defaults it actually introduces, rather than the merge helper it would have inherited already-green; both records were edited together so the collision is closed on both sides rather than papered over on one.

**The reset is now stated as fact, not delegated as a question.** Since the falsified premise is settled by evidence, the brief no longer asks the agent to confirm it — it states that the branch stays mounted across the selection transitions that matter, names them, and requires the reset to be reproduced in the extracted component. A new criterion guards it: expand an agent-defaults panel, drill into a subflow, assert it collapses. That test does not exist today, so it is falsifiable, and it asserts an action rather than an absence of one.

**Class 7 resolved by command substitution.** The coupling inventory is now prose naming each coupling with its own enumeration command, the call-site count is gone, and the magnitude ranking is replaced with a command that locates the branch's extent.

**Readiness gate (cold-reader): FAIL** (round 2, 2026-08-08)

Round 1's remedies held. Class 7 is fully discharged — six discovery commands executed clean, five kind (a) candidates examined and cleared, none remaining. Class 6 does not fire on either prong, judged on the work rather than this brief's ordering argument, which the reader agreed proves sequence rather than indivisibility; the verdict rests instead on each enabling piece delivering nothing demoable alone. Class 9 does not fire on either arm: seven change-introducing observables executed red, and the new reset-guard criterion is self-witnessing because it asserts the panel *becomes* collapsed rather than asserting it is left alone.

**Class 3 — the merge-helper resolution was purchased by editing a brief that had already passed its gate.** The carve-out written into `ISSUE-260723-0823-1` does make the two records non-colliding as its text now stands, and the reader confirmed that record's criteria stay red at their own baseline. But that text is not what its round-3 gate examined, and no `REOPENED` stamp accompanied the change. Measured against the version its `PASS` actually certified, this record's merge-helper lift pre-satisfies that record's plumbing criterion — the exact collision round 1 found. The reader declined to pick a disposition, correctly: honouring immutability means the collision is unclosed, while accepting the rewrite means the sibling owes a reopen and a fresh round.

**Also found, non-blocking:** the mounted-branch claim this brief asserts as fact is two-thirds true. Breadcrumb jumps and pane clicks do reach this branch while it is already mounted, but both drill-in entry points require a node to be selected first, so on that path the branch is unmounted and remounts fresh — and drill-in is the very path the reset-guard test pins to. Separately, the brief's inventory of the branch omitted the Workflow and Limits sections entirely, the drilled-in conditional that decides whether Limits and Run As render, and the active-workflow fallback that nearly every control in the branch depends on and that no preservation criterion would catch being dropped.

### Gate round 2 resolved (2026-08-08)

**The sibling has been reopened, not worked around.** `ISSUE-260723-0823-1` now carries a `REOPENED` stamp and returns to `needs-triage` for a round-4 gate against its current text. Its stale premise — which still listed the merge helper among the plumbing living in the component's instance script — is corrected in the same edit, and its baseline is now stated explicitly as the tree after its `blocked_by` siblings land. Ownership of the lift stays here, since the run-as helpers genuinely cannot move without it.

**The mounted-branch claim is now accurate per path**, naming which transitions reach a mounted branch and explicitly warning not to generalize from the drill-in path.

**The omissions are closed.** Current behavior now enumerates all five sections of the branch with a command that finds them, and Key interfaces names the active-workflow fallback as the most easily-missed dependency along with the reason no test would catch it, the drilled-in conditional, the agent-defaults reader, the copied-state flag, and the parent-side style cleanup.

### Pre-emptive correction carried over from the sibling's round-3 gate (2026-08-09)

`ISSUE-260808-2022-04` failed its round-3 gate on class 7 kind (a), against a scoped-style bullet carrying the identical construction this record carried: a confirmation command hardcoding the rule families the author already had in hand, offered as enumerating *every* scoped rule. A pattern that names its own answer cannot establish exhaustiveness — a scoped rule outside the pattern is invisible in its output, which is exactly the case the qualifier exists to rule out.

This record's Key-interfaces style bullet is corrected in the same pass rather than leaving its own round 3 to re-find the row. The bullet is split off on its own, dumps the component's entire scoped style block, and states the rule for which side of the extraction each selector falls on — travels with this branch, stays with a sibling, or is global — instead of naming the expected split in advance. The parent-side cleanup instruction and the unused-selector warning note are preserved.

Nothing else changed. This record remains at FAIL (round 2) and is due a round-3 gate against the corrected text. It was ungated at the time of the edit, so no `REOPENED` stamp was owed.

**Readiness gate (cold-reader): FAIL** (round 3, 2026-08-09)

Class 7 only. Classes 1–5 clear, class 6 does not fire on either prong, class 8 is inert and materially clean, and class 9 does not fire on either arm — every change-introducing grep observable executed red at baseline, both preservation runners executed green, and the reset-guard criterion triggers neither arm-B requirement because it asserts a panel *becomes* collapsed rather than asserting the system leaves something alone. The round-2 style-bullet correction holds: the reader executed the substituted procedure and derived the selector set from its output.

This was the first round to gate this record's substance end to end, round 2 having died on a cross-record ownership question. It cleared that question too — see below.

**Class 7 kind (a) — seven surfaces, all on brief text under edit.** Two of them are not merely uncommanded; the sets they assert are wrong.

- **The transition enumeration is materially incomplete.** Desired behavior names drill-in, breadcrumb jump, and pane click as the transitions that set the selection to the workflow kind with a null id. The store carries considerably more than that; `rg -n 'selection = \{ kind: "workflow", id: null \}' ui/src/lib/stores/workflowStore.svelte.ts` derives the real set, and it includes workflow load, workflow creation, node removal, and edge removal alongside the three named.
- **The reachable-while-mounted subset is also incomplete.** The brief names the breadcrumb jump and the pane click as the transitions that reach this branch while it is already mounted. New, open, and duplicate workflow reach it the same way from the app shell, and none of the three is named.
- The remaining five are true but assert inventories no command on the surface derives: the drill-in entry-point count; the claim that the run-as helpers, attach-hint derivation, copy action, copied-state flag, and agent-defaults reader are used only by this branch; the claim that the test harness always sets a workflow first so no preservation criterion would catch dropping the active-workflow fallback; the claim that an existing test guards the drilled-in conditional; and the reset-guard criterion's claim that no test asserts the collapse today. The reader verified all five against the tree. Truth does not discharge a kind (a) row on brief text under edit, and the last of these decays as the sibling records add tests to the same file.

Note the shape: the used-only-by-this-branch claim and the no-test-asserts-this claim are the same genus that failed `ISSUE-260808-2022-04` at its round 2, whose accepted remedy was a grep over the target file carrying qualitative polarity rather than a refreshed assertion.

**Round 2's cross-record objection is discharged.** The reader confirmed from both records' current text that the merge-helper ownership is coherent and non-colliding: this record claims the lift, `ISSUE-260723-0823-1` states the module already exists at its baseline and explicitly excludes the helper from its own plumbing criterion, and no criterion in either record claims the same deliverable. Immutability is satisfied because that record's authoritative stamp is `REOPENED`, so its text was legitimately editable. Its criteria were re-verified red at its own stated baseline, and this record's criteria are red at this one.

**Non-blocking findings worth acting on while the brief is open:**

- **The reset-guard criterion is at risk of being blind.** It requires drilling into a subflow, and this brief's own parenthetical says both drill-in UI entry points require a node selected first — which unmounts the branch and remounts it fresh, so a UI-driven fixture would pass whether or not the reset survived. The guard bites only when drill-in is driven by a direct store call with nothing selected. The existing suite already uses exactly that pattern and carries a comment saying so.
- **The reset-guard criterion names no home file**, while the preservation runner is file-scoped — so a test written into a new file would be executed by no criterion in this brief. The sibling edge record pins its test for precisely this reason.
- **Both this record and `ISSUE-260723-0823-1` probe the lifted helper with the literal `function mergeConfig`.** An arrow-const lift would satisfy neither, and would leave the sibling's pre-flight check reading false.
- **Key interfaces never names `capabilities`**, though the moved effect reads it and the agent-defaults section is gated on it, and the bullet names the other easily-missed dependencies explicitly.
- The single-slice sentence is false in the same way the edge record's was: the merge-helper lift alone lands with its criterion satisfied and both preservation runners green. Rounds 1 and 2 both noted the ordering-versus-atomicity confusion; the sentence still overstates.
- Round 2's own claim that no class-7 candidates remained is contradicted by this round, which found seven. Recorded rather than deferred to.

### Gate round 3 resolved (2026-08-09)

**All seven class-7 surfaces replaced with derivations, not refreshed assertions.** The transition enumeration is gone: the brief now hands over a command that finds every store site setting the selection to the workflow kind with a null id, instructs the agent to read the enclosing method for each and grep its callers, and asks it to work out which transitions leave the branch mounted from that output. This closes the incompleteness in the same stroke — the command returns the whole set rather than the three the prose had named, and the mounted/unmounted split is now derived instead of asserted. The drill-in entry-point count is replaced by its own enumeration command. The four remaining inventories — the branch-owned helper set, the harness's workflow setup, the existing drilled-in coverage, and what the suite asserts about the reset today — each now carry a command over the file they concern, with qualitative polarity, matching the remedy accepted on `ISSUE-260808-2022-04`.

**The instruction did not change, and the brief now says so.** Reproducing the reset in the extracted component was always unconditional; the transition split is why dropping it in favour of unmount semantics is unsafe, not a description of what to build. That distinction is now stated, so a corrected transition set cannot be read as a changed requirement.

**The reset guard would not have guarded anything.** The criterion required drilling into a subflow, while the brief's own text says drill-in requires a node selected first — which unmounts and remounts the branch, so the panel returns collapsed whether or not the reset survived. The criterion now requires the transition to be driven by a direct store call with nothing selected, states why the UI route is blind, and points at the existing case that already uses that pattern. It is also pinned to the file the preservation runner executes, which it previously was not.

**Non-blocking pickups taken while the brief was open.** Runtime capabilities are now named in Key interfaces alongside the other easily-missed dependencies, with the reason they cannot be derived from the store. The merge-helper criterion no longer probes for `function mergeConfig`; it requires the export name and leaves the declaration form free, and `ISSUE-260723-0823-1`'s pre-flight check was corrected in the same pass so an arrow-const lift cannot leave it reading false. That record was ungated at the time of the edit, so no `REOPENED` stamp was owed. The single-slice paragraph now separates the ordering argument from the atomicity claim it was overstating, and rests the conclusion on the absence of an independently demoable subset — the ground two prior readers said it should have rested on all along.

Due a round-4 gate against this text.

**Readiness gate (cold-reader): FAIL** (round 4, 2026-08-09)

Class 7 only, and down to two surfaces from round 3's seven. Classes 1–5 clear with no `decision-needed` fork; class 6 does not fire on either prong; class 8 is inert and materially clean on the counterfactual; class 9 does not fire on either arm.

**The round-3 remedies were verified by execution, not by inspection.** The reader ran the transition derivation and confirmed it terminates and yields the full set of store methods, then ran the second stage over their callers. It ran the style procedure and confirmed it produces a clean split with nothing pre-stated. Most importantly it answered the question the rewrite was meant to settle: the reset guard **can now go red**. It confirmed each transition assigns a fresh selection object so a child-local effect re-runs, that the branch is not remounted when nothing is selected, that the assertion target still renders after the transition because the Agent-defaults section is gated on capabilities rather than the drilled-in flag, and that the suite already proves the pattern. A child omitting the reset leaves the panel expanded and the assertion fails — which the pre-rewrite UI-driven form would not have caught.

**Class 7 kind (a) — two surfaces, both the same omission.** Round 3's remedy gave a command to every bullet in Key interfaces except one, and to every coverage claim except one.

- **The active-workflow bullet's read inventory.** It enumerates what the branch reads from the active workflow and carries two counts, with no command over the surface it concerns; the bullet's only command derives the test-harness sub-claim instead. The reader confirmed the list is true but already glosses one read — the entry-node dropdown's option list.
- **The pinned baseline test in the effect-move preservation criterion.** A decaying existence claim about the suite with no command locating the case. True today, and the reader noted this is precisely the genus round 3 predicted would decay as the sibling records add cases to that same file — which both siblings and this record's own guard criterion do.

**Cross-record ownership: fully closed on both sides.** The reader re-derived the sibling's stamp chain rather than inheriting round 3's finding, confirmed that record is ungated at `REOPENED` so its text was legitimately editable, and confirmed no criterion in any of the four records claims the same deliverable. It also checked the property the round-3 remedy was meant to establish: every permitted declaration form of the lift satisfies both this record's criterion and the sibling's pre-flight check, and no permitted implementation can place the module where either check would fail.

**Non-blocking findings, several of them defects in the round-3 remedy itself:**

- **The branch-ownership membership rule is wrong as written.** "A helper whose consumers all lie inside the branch travels with it" gives the wrong answer for helpers consumed only by other branch-owned helpers — the run-as value check, the attach-hint derivation, and the copied-state flag all have consumers outside the branch extent and would be left behind, contradicting the same bullet's named set. Self-correcting through typecheck, but the rule needs consumers that are themselves branch-owned to count as inside.
- **The reset guard's stated rationale is over-general.** Requiring a node to be selected first holds for drill-in but not for the pane click, the breadcrumb jump, or the app-shell transitions the brief's own derivation returns. Not build-changing, since the mandated fixture shape is stronger and safe on every path, but the reason given is not the true reason on most paths.
- **The transition-derivation command's context window is exactly at its boundary** for one store method today; a single added line there would truncate the enclosing method it is meant to reveal.
- The effect-move preservation criterion asserts nothing the file-scoped preservation runner does not already require.
- One acceptance criterion caveats its illustrative component name and another does not.
- The cross-record claim that the sibling's criteria account for this module already existing is true against that record's current text, which is ungated and may legitimately change before its own gate.

### Gate round 4 resolved (2026-08-09)

**Both class-7 surfaces closed, by opposite remedies.** The active-workflow bullet's read inventory is replaced by a command over the component with an instruction to read the hits inside the branch extent rather than count them — with a note that some are nested reads feeding a control's options rather than its value, which is the case round 4 found the old list had glossed. The pinned baseline test is handled by **removing the criterion** rather than commanding it: round 4 established it asserted nothing the file-scoped preservation criterion did not already require, so a command would have preserved a redundant claim that decays as cases are added to that file. Its navigational value — where a broken reconciliation surfaces first — is folded into the effect-move paragraph with a command locating the case, and stated there explicitly as guidance rather than a criterion.

**Three defects in the round-3 remedy are repaired.** The branch-ownership rule now states ownership is transitive and tells the agent to work outward from the branch's markup until the set stops growing, with the warning that several members are consumed only by each other in instance-script code far from the extent — the literal reading would have stranded exactly those. The reset guard's rationale no longer claims all UI entry points require a node selected; it says some routes do, names drill-in as one, and rests the requirement on the direct-store-call shape being the only one guaranteed to hold the branch mounted on every path. The transition-derivation command drops its fixed context window in favour of opening the file and reading the enclosing method, so it cannot be silently truncated by growth in a store method.

**Symmetry pickup.** The shared-component criterion now carries the same illustrative-name caveat its sibling criterion already had.

Due a round-5 gate against this text.

**Readiness gate (cold-reader): PASS** (round 5, 2026-08-09) — **superseded before promotion; see below.**

The round returned PASS on a 64-row class-7 ledger with no blocking rows, arm A red on every change-introducing observable, arm B triggering no requirement, class 6 clear on both prongs, class 8 inert and clean on the counterfactual, and the cross-record merge-helper property closed on both sides. It confirmed independently that the reset guard can go red, and that removing the effect-move criterion was sound: the file-scoped preservation criterion is a strict superset of it, and the effect move is doubly covered because deleting the effect turns the single-access-mode baseline case red on its own.

**The stamp was not acted on.** Two of its non-blocking findings were defects in the round-4 remedy itself, and the maintainer chose to fix them rather than promote and lock them in (2026-08-09). Recorded here so the round's work is not lost and the supersession is visible rather than silent; the authoritative verdict for promotion purposes is the round-6 stamp, not this one.

### Gate round 5 resolved — root cause fixed rather than patched (2026-08-09)

Round 4 found the branch-ownership rule wrong for helpers consumed only by each other, and the repair made ownership transitive. Round 5 then found the repaired rule still wrong for the expanded-agent state, whose reset lives in a shared parent effect that clears unrelated state and is therefore not branch-owned — so the rule told the agent to leave behind the very state the Desired behavior and an acceptance criterion both mandate moving. Round 5 separately found the same defect in the active-workflow bullet, which filtered its derivation to the branch's template extent and so missed instance-script reads.

The common error was treating "the branch" as its template extent when most of what it owns lives in the instance script. Patching the membership rule a third time was rejected. **The rule is gone.** The bullet now names the members, says explicitly that line-number containment is the wrong test and why it fails in both directions, and resolves the remainder by construction — move the named members, let the compiler close the set, lift or pass what it cannot reach, delete what is left with no consumer. The active-workflow and capabilities bullets no longer filter by extent either.

**The brief's one internal contradiction is closed.** The access-modes module delegation was unbounded while its criterion is directory-scoped; the delegation is now bounded to the same directory, with the reason stated.

**Readiness gate (cold-reader): REOPENED** (round 6 pending, 2026-08-09)

The round-5 `PASS` above is withdrawn, and this stamp is what makes the withdrawal load-bearing rather than a note. Left standing, that `PASS` would be the authoritative verdict under the matcher — highest effective round — which would mark the brief gated and immutable over text round 5 never read, and would make the four edits recorded above invalid under P3 atomicity. The edits themselves owed no `REOPENED` at the time they were made, since the authoritative verdict was then `FAIL` (round 4); the defect is only that the later `PASS` would certify the wrong text.

This is the same failure `ISSUE-260723-0823-1` hit at its own round 3, and the same remedy. Recording the earned `PASS` and withdrawing it explicitly is preferred over not writing it, so the round's findings stay in the record and the supersession is auditable.

Due a round-6 gate against this text.

**Readiness gate (cold-reader): PASS** (round 6, 2026-08-09)

Every gap `fine` or an explicit delegation; class 6 does not fire on either prong; an 88-row class-7 ledger with no blocking rows and no kind (a) figures surviving; class 8 does not fire; class 9 does not fire on either arm.

**Stamp resolution, since this record's chain is unusual.** This stamp ties at round 6 with the `REOPENED` above, which named itself "round 6 pending" and so consumed the number. The matcher breaks a tie by document order, so this stamp — being later — is authoritative, and the reader confirmed that resolution independently before relying on it. The round-5 `PASS` is history, not authority. Future rounds start at 7.

**The round-5 remedy holds where two previous remedies did not.** The reader executed the membership construction against every identifier the moved code reaches and found no member for which it returns the wrong answer — including both shapes that defeated the earlier rules: helpers consumed only by each other now resolve correctly through the "every remaining consumer is moving" clause, and the expanded-agent state is defused by being pre-named, so the clause that would still have stranded it never runs. It also verified the compiler genuinely closes the set there, since the parent's reset line stops compiling once the state leaves. The style derivation was executed to a clean partition, and the transition derivation was falsified for completeness against every selection assignment in the store rather than only the ones the command returns.

**Class 8 was live this round, and four earlier rounds were wrong to call it inert.** The class fires only on records stamped `PASS`/`WAIVED` at some round — and this record *has* been, at round 5. Withdrawing that stamp by `REOPENED` moves authority but does not un-stamp the fact, so the class became applicable from this round on. The reader judged it on the merits rather than declaring it inert, walked every note in `## Triage Notes` through the materiality test, and found each binding one already hoisted into the brief. It does not fire — but it was judged, not waived.

**Non-blocking, carried forward and now immutable under this stamp:**

- **No accepted-risk statement for the uncovered sections.** Rendered-output preservation for the Workflow fields and the Variables section rests on review rather than any mechanical guard. `ISSUE-260723-0823-1` records the equivalent exposure as an explicit accepted-risk paragraph; this record leaves it implicit. The most consequential of these notes.
- **The component-render criterion is the weaker of the two of its kind** — its grep is satisfiable by an unused import, where the shared-component criterion carries a read-don't-merely-import clause. Not a class-9 fire, since the observable is red at baseline.
- The branch's own header markup sits inside the extent the brief says to move but is not one of the five enumerated sections, so the rendered-output-unchanged sentence does not literally reach it, and no test asserts it.
- The transitions reachable only from a selected state are described as requiring a *node* selected; edge removal requires an *edge*. Same side of the mounted/unmounted split, and the instruction is unconditional.
- The merge-helper module's location is bounded only by its criterion's directory scope, where the access-modes bullet states the bound in prose. Asymmetric but unambiguous.
- The snippet also reaches two access-mode label constants the brief does not name; both are snippet-exclusive, so they travel and the compiler forces it.
- The cross-record claim about the sibling's criteria is true against that record's current ungated text, which may legitimately change before its own gate. Not fixable from this side.

### Parked by `/implement-issue` — full suite unrunnable in this environment (2026-08-09)

**Status: `ready-for-human`. The work is complete and reviewed; only the suite gate is unsatisfiable here.** Code parked on **`wip/ISSUE-260808-2022-11`** (commit `58d02ee`). Review file: `issue-260808-2022-11-code-review-20260809-141744.md`.

**What blocks:** `test-runner` returned `SUITE: ERROR`. `just test` (= `cargo test && npm test`) aborts at the Rust half with `sh: cargo: command not found` (exit 127). No Rust toolchain exists in this environment — `which -a cargo rustc rustup` finds nothing, and `~/.cargo/bin` holds only `cargo-nextest`, `claude-history`, and `tmux-tools`. This is a standing environment gap, not a transient error and not a code failure: **zero Rust tests executed**, so nothing can be attributed to this diff, and the same `SUITE: ERROR` was equally true at claim time. `SUITE: ERROR` routes to fallout by rule — infrastructure is never judged against the diff — so this record parks rather than closing, even though the failure cannot implicate the work.

**What was attempted, and how far it got.** Claim-time tripwire clean (`SCALE: PASS`, `CONSTRAINTS: CLEAN`). Route `cursor` for the implementation and both fix rounds. Every acceptance criterion was verified satisfied by both reviewers independently, including the two that most needed it: the reset guard was confirmed to **go red** by deletion probe (effect removed → 14/15, restored → 15/15), and the active-workflow fallback — the record's flagged most-easily-missed dependency — was confirmed preserved at `WorkflowInspector.svelte:21`. Dual review found 6 findings, all LOW, no CRITICAL/HIGH/MEDIUM: 4 FIXED across 2 fix rounds of a 4-round cap, 2 deferred with reasons. `InspectorPanel.test.ts` 15/15; full UI unit suite 112/112 across 10 files; `just typecheck` 597 files / 0 errors / 0 warnings. **The entire half of the suite that this frontend-only diff could affect ran and matches baseline exactly.**

**Two things the resumer needs to know.** First, the parked branch's `public/` bundle was deliberately **not** rebuilt — the park commits implementation files only, so `just build` must run and `public/` be staged before any real commit (this clone has no active hook to enforce it: `core.hooksPath` is unset and `.git/hooks/` holds only samples, so `just setup` has not been run here). Second, `L1` and `L5` are deferred, not dismissed; `L1` records genuine accepted behavior drift on an undo/redo path, and the round-6 gate had already noted this record lacks the explicit accepted-risk paragraph its sibling carries — `L1` is that exposure made concrete.

**Maintainer decision needed (this is why it is `ready-for-human`, not retried).** Either install a Rust toolchain and re-dispatch, after which this should close immediately with no code change, or decide that a frontend-only slice's gate is the UI suite plus typecheck and amend the loop's suite step accordingly. The second is a policy question about the project's gate that `/implement-issue` cannot settle on its own. Until it is settled, **every** issue in this repo will park on the same `SUITE: ERROR`, so this is a loop-level condition rather than a fact about this record.
