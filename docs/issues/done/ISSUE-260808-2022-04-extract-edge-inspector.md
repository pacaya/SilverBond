---
id: ISSUE-260808-2022-04
kind: issue
category: enhancement
status: done
summary: Extract the edge inspector out of InspectorPanel into its own sibling component
claimed_by: implement-issue@macmini
claimed_at: 2026-08-09T12:43:25Z
---

## Agent Brief

**Category:** enhancement
**Summary:** Move the edge-inspector branch of `InspectorPanel` into its own sibling component — behavior-preserving.

**Current behavior:**

The `InspectorPanel` editor component renders several different inspectors from one top-level conditional — a node inspector when a node is selected, an edge inspector when an edge is selected, and a workflow-level inspector when neither is. Survey that conditional's branches with `rg -n '^\{#if|^\{:else|^\{/if\}' ui/src/features/editor/InspectorPanel.svelte`.

The edge branch is rendered inline in that conditional rather than in its own component, so it contributes to the component's size and to the Divergent Change smell recorded against it, even though it changes for entirely different reasons than node editing does.

**Desired behavior:**

The edge branch renders through its own sibling component. The parent's conditional keeps its shape — when an edge is selected it renders that component instead of the branch's markup.

Rendered output and edit behavior are unchanged. The branch renders four controls and a delete action, all of which move: an **Outcome** select, a **Label** text input, a **Branch id** input, a **Condition** editor built on the existing condition-builder component, plus the branch's delete button. All four controls keep their labels, their order, and their existing edit semantics; the delete button keeps its existing label verbatim — do not rename it.

**Key interfaces:**

- **Dependency surface.** Derive it from the branch being moved. It consists of the selected edge, its computed display name, the edge-outcome list, the workflow store's update and edge-removal operations, and the existing condition-builder component. Two dependencies are easy to miss and both silently change behavior if dropped:
  - **The active workflow document.** Both the selected-edge derivation and the display-name derivation read the active workflow, which is the store's drill-aware active document falling back to the component's `workflow` prop, and which can be null when no workflow is loaded. If the component derives these rather than receiving them, it must reproduce that fallback exactly.
  - **The outcome-list fallback.** The outcome select does not read the runtime capabilities alone — it falls back to a hardcoded outcome vocabulary when capabilities are absent, and capabilities are optional on this component. Preserve the fallback and its exact vocabulary; find it with `rg -n 'supportedEdgeOutcomes' ui/src/features/editor/InspectorPanel.svelte`.
- **What the branch does *not* touch** — the node-kind chain, the config-writing switch, either parent-defined snippet, agent-capability interpretation, unlock prompting, and the access-mode effects.
- **Scoped styles — derive the set, do not assume it.** Dump the component's scoped style block with `rg --pcre2 --multiline -n '(?s)^<style>.*?^</style>' ui/src/features/editor/InspectorPanel.svelte` and read every selector it defines. For each of those selectors, find its usages by grepping that class name over the same file. A rule whose usages all fall inside the edge branch travels with the branch; a rule with no usage inside the branch stays behind. Any class the branch uses that no selector in that block defines is global and needs no action either way. Derive the answer from that output rather than assuming in advance which way it comes out.
- **Parent-side cleanup.** Moving the branch strands whatever it was the last user of in the parent. Enumerate the candidates with `rg -n 'WorkflowEdge|edgeDisplayName|removeEdge' ui/src/features/editor/InspectorPanel.svelte` and remove what goes dead once the branch is gone; the typecheck will not flag it, since unused locals are not an error in this project's configuration.
- **Store access** — the workflow store is a module singleton and sibling editor components already import it directly. Do that rather than introducing Svelte context, which would break the standalone-mount pattern the component's tests rely on.
- **Props vs direct access** — the agent may choose which dependencies arrive as props and which the component derives itself, provided rendered output and edit behavior are unchanged.
- **Destination** — colocate with the existing editor components, matching the convention already used by the sibling components in that directory.

**Acceptance criteria:**

- [ ] The edge branch renders through its own component. `rg -n 'EdgeInspector' ui/src/features/editor/InspectorPanel.svelte` returns a match; no matches before this change. (Name is illustrative; check against the name chosen.) Verify by reading that the conditional renders it, not merely that it is imported.
- [ ] All four controls have left the parent. `rg -n '<span>(Outcome|Label|Branch id|Condition)</span>' ui/src/features/editor/InspectorPanel.svelte` returns no matches; returns a match for each of the four before this change. These four labels occur nowhere else in the parent, so this covers the whole control set rather than a sample of it.
- [ ] **A test renders the edge inspector.** A test builds a workflow containing at least one edge, selects that edge, and asserts that the Outcome, Label, Branch id, and Condition controls all render, and that editing the outcome persists to the workflow. It lands in `ui/src/features/editor/InspectorPanel.test.ts` so the preservation runner below executes it. `rg -n 'selectEdge' ui/src/features/editor/InspectorPanel.test.ts` returns no matches before this change — the suite exercises no edge-selection path, so nothing today would notice if the edge branch stopped rendering. The store's edge-selection method is available to the existing standalone-mount harness; find it with `rg -n 'selectEdge' ui/src/lib/stores/workflowStore.svelte.ts`.
- [ ] **Preservation** — `npx vitest run --config ui/vite.config.ts ui/src/features/editor/InspectorPanel.test.ts` passes, and every test case present in that file at the baseline still passes with its assertions unmodified. New cases may be added; existing assertions may not be relaxed. Green before and after. Note this criterion guards the *rest* of the component, not the edge branch — the criterion above is what guards the edge branch.
- [ ] **Preservation** — `just typecheck` reports no new errors. Green before and after.

**Why this is a single slice:**

One branch moves into one component. The criteria are views of that single move plus the guard that makes it verifiable; no subset lands green on its own.

**Out of scope:**

- The node-kind panels (`ISSUE-260723-0823-1`), the shared per-control form components (`ISSUE-260808-2000-07`), and the workflow-level inspector with its shared agent-config fields (`ISSUE-260808-2022-11`) — each is a separate sibling issue.
- Any change to edge semantics, edge validation, condition-builder behavior, or the outcome vocabulary — including the hardcoded fallback, which is preserved as-is rather than corrected.
- Broadening the test harness beyond what the edge-render criterion needs.
- Closing any smells-ledger row. The `InspectorPanel.svelte:1` Divergent Change row is a whole-component judgment and closes on `ISSUE-260723-0823-1` once every axis has been separated; this issue closes no row on its own.
- Any visual, styling, copy, or layout change.

## Context Pack — generated at claim (2026-08-09T12:43:25Z)

**PRD decisions relevant to this slice**: no PRD linked — the record carries no `prd:` frontmatter and the repo has no `docs/prd/` or `docs/decisions/prd/` tree; slice-governing decisions live in the Agent Brief and Triage Notes only.

- Coverage exclusion reversed (maintainer, 2026-08-09): `ISSUE-260723-0823-1` excluded the edge inspector by name from targeted coverage; that exclusion does not hold for this standalone record, so an edge-render test is a required criterion.
- The extraction is behavior-preserving: no edge semantics, validation, condition-builder, or outcome-vocabulary change — including the hardcoded outcome fallback, preserved as-is rather than corrected.
- The delete control keeps its label verbatim; copy, styling, and layout changes are out of scope.
- Store access is via the module singleton import, not Svelte context, so the standalone-mount test pattern keeps working.
- This record closes no smells-ledger row; the `InspectorPanel.svelte:1` Divergent Change row is owned by `ISSUE-260723-0823-1`.
- Scoped styles and parent-side dead code are derived by the brief's commands, never assumed in advance.

**Test seam & Testing Decisions:** observable at `ui/src/features/editor/InspectorPanel.test.ts` via the existing standalone-mount harness, which drives the workflow store singleton directly (`selectEdge`) and accepts an edges patch on the fixture; the new case must build a workflow with an edge, select it, assert Outcome/Label/Branch id/Condition all render, and assert an outcome edit persists. Testing decisions that touch it: the test must land in that exact file because the preservation runner (`npx vitest run --config ui/vite.config.ts ui/src/features/editor/InspectorPanel.test.ts`) is file-scoped; every baseline case must still pass with assertions unmodified — new cases may be added, existing ones may not be relaxed; harness broadening beyond what the edge-render criterion needs is out of scope; the guard is a characterization net (it passes against the un-extracted branch), so the four-labels completeness criterion is what proves the extraction. Second preservation gate: `just typecheck` reports no new errors — note unused locals are not an error here, so it will not flag stranded parent code.

**ADRs:** no ADR index reachable — the record carries no `adrs:` frontmatter, and the repo has neither `docs/adr/INDEX.md` nor `docs/model/generated/` (the only decision doc, `docs/decisions/tmux-bin-resolution.md`, is unrelated to this slice).

**Terms:** no glossary reachable — the record carries no `terms:` frontmatter and no `CONTEXT.md` exists at the repo root or under any context root.

**Full artifacts:** docs/issues/ISSUE-260808-2022-04-extract-edge-inspector.md · CLAUDE.md

## Resolution

**Commit:** `feat: extract the edge inspector into its own component (ISSUE-260808-2022-04)`

**Route:** cursor (`/cursor-developer`) — well-scoped single-file Svelte branch extraction with an explicit dependency surface and derived-not-assumed style and dead-code procedures; no cross-module reasoning, security surface, or infrastructure work. Both fix rounds routed to cursor for the same reason.

**TDD:** n/a (linear) — behavior-preserving markup and handler relocation, and the mandated edge-render criterion is a characterization guard that is green at baseline by construction, so red-green does not apply to the extraction itself. The one test that *is* falsifiable arrived during fix round 1: the H1 regression guard was proven red against reconstructed broken semantics rather than merely asserted.

**Review telemetry:** 9 findings — 1 HIGH, 4 MEDIUM, 4 LOW. 7 FIXED, 2 dismissed (L2, L3) on the brief's own contract. Fix rounds used: 2 of 4; round 2 closed with no new findings from either reviewer. Every acceptance criterion independently confirmed satisfied by both reviewers. 3 smells reported: 2 promoted into the findings track as M3/M4 under the in-diff duplication rule, 1 advisory (`EdgeInspector.svelte:14` Middle Man, borderline) appended to the ledger. No ledger row closed by this issue, per Out of scope — the `InspectorPanel.svelte:1` Divergent Change row is owned by `ISSUE-260723-0823-1`.

**The HIGH finding was a real regression, and its guard is real.** H1: the extraction weakened the parent's branch predicate from an edge-existence check to a selection-kind check, so undoing an edge-add rendered a blank inspector instead of falling back to the workflow inspector. Caught empirically by baseline-vs-worktree render comparison, fixed by restoring `{:else if selectedEdge}`, and pinned by a new case in `InspectorPanel.test.ts`. Re-verified at close by reintroducing the broken predicate in a throwaway worktree: the suite went to 1 failed / 15 passed, with the failure landing on `renders workflow inspector when edge selection is stale after undo`. This test is the main durable addition beyond the extraction.

**Suite:** green — 466 Rust tests (449 unit + 17 integration) and 113 frontend tests across 10 files, 0 failures. `just typecheck` clean (594 files, 0 errors, 0 warnings).

**Bundle:** the embedded frontend bundle in `public/` was rebuilt and is committed with the source change, per the "Frontend bundle freshness" convention in `CLAUDE.md`. Verified at close by running `npm run build` against the parked tree and confirming `git status public/` came back empty — the committed bundle is byte-identical to a fresh build. This mattered more than usual because `core.hooksPath` is unset in this clone, so the freshness hook was guarding nothing.

**Environment note (not a code defect) — the park was avoidable.** This record parked at `ready-for-human` on 2026-08-09 because `just test` aborted at `cargo test` with `command not found`, and the parking note concluded no Rust toolchain existed on this machine. That diagnosis was wrong in a way that cost two records: `ISSUE-260808-2000-07` had already hit the identical failure hours earlier and established that the toolchain *was* present at `~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/cargo` (1.96.0) with only the `~/.cargo/bin` shim missing. The maintainer subsequently installed the toolchain properly (`cargo`/`rustc` 1.97.1 on `PATH`), and the full gate was re-run against the parked branch with **no code change** — the green result above. Nothing about the repo or this change caused the park.

**Open, non-blocking inaccuracy in the brief (maintainer call, deliberately left standing).** Under "Why this is a single slice", the sentence "no subset lands green on its own" is false: the edge-render criterion is a characterization guard that passes against the un-extracted branch. Gate rounds 2, 3, and 4 each recorded this. It is immutable under the round-4 PASS stamp, prong (b) does not depend on it, and nothing an implementer does changes because of it, so correcting it would have cost a `REOPENED` and a round 5 disproportionate to a self-description with no contract effect.

**Pre-existing repo defect, confirmed independently (not fixed here, out of scope).** `ISSUE-260808-2000-07`'s Resolution reported that `.githooks/pre-commit` cannot pass as written on any commit touching `ui/src`: it symlinks `node_modules` into a `git checkout-index` snapshot, and resolving through that symlink changes Svelte's scoped-class hashes for vendored `svelte-flow` components, so the snapshot build never matches the staged `public/` byte-for-byte. The same mechanism was hit again while verifying this record — a worktree with a symlinked `node_modules` failed all 10 test files at `Cannot find module '/@fs/…/@testing-library/svelte/src/vitest.js'`, and cloning `node_modules` for real made them pass 113/113 unchanged. Two independent encounters with the same root cause; the hook stays disabled until it compares normalized output or gives the snapshot a real `node_modules`.

**Date:** 2026-08-09 (UTC)

## Triage Notes

Minted 2026-08-08 as one of four records the `ISSUE-260723-0823-1` decomposition was split into, after that record's readiness gate fired class 6 prong (b) twice. This is the carve-out both gate rounds identified as cleanly separable: the edge branch couples to nothing else in the component. The parent record had conceded its separability while keeping it in scope; the maintainer accepted the split rather than the concession.

Deliberately closes no ledger row — see Out of scope. The `:1` Divergent Change row is a whole-component judgment and is owned by the kind-panels record, which lands last.

- Scale snapshot (non-contractual): `rg -n 'selectedEdge' ui/src/features/editor/InspectorPanel.svelte` → the branch and its derivations, a small fraction of the file (2026-08-08)

**Readiness gate (cold-reader): FAIL** (round 1, 2026-08-08)

Classes 1, 2, 6, 8, and 9 clear — the record is correctly issue-scale on both prongs, and both change-introducing observables were verified red at baseline. Three things blocked the stamp, all brief-level.

**Class 5 — the named preservation guard is blind to the work.** The record's only behavioral guard was the existing test suite passing, and that suite cannot observe the edge inspector at all: its render harness selects a node and takes no edge-selection path, and its workflow fixture carries no edges. The suite would stay green if the edge branch were deleted outright. So the brief's contract — same controls, same labels, same order, same edit behavior — had zero mechanical verification, and the record neither required coverage nor stated an accepted risk.

**Classes 3 and 4 — the completeness criterion was unreachable and covered half the controls.** Its described post-state could never be produced, because the selected-edge derivation must stay in the parent to drive the conditional and would always appear in the command's output. Separately, the branch renders four controls — Outcome, Label, Branch id, Condition — and both the criterion and the Desired-behavior prose named only condition and outcome, so nothing required the other two to move and nothing would have noticed.

**Class 7 kind (a) — a bare count.** "Renders three different inspectors" asserted a figure with no discovery command on the surface; true, but the authoring rule binds regardless.

**Noted, not blocking:** the claimed dependency surface omitted the active workflow document and the hardcoded outcome-vocabulary fallback — the two places a careless move silently changes behavior, and precisely what the blind suite would not catch. The claim that nothing else in the parent needs to change is also false in a small way: the edge-type import and the display-name derivation go dead, and unused locals are not a typecheck error here.

### Gate round 1 resolved (2026-08-08)

**Coverage now required — the earlier exclusion is reversed, not reinterpreted.** `ISSUE-260723-0823-1` excluded the edge inspector *by name* when it recorded its targeted-coverage decision, under `### Gate round 1 resolved`. **Maintainer decision (2026-08-09): that exclusion is reversed for this record.** So a minimal edge-render test is now a required criterion: build a workflow with an edge, select it, assert all four controls render and that editing the outcome persists.

The reason the exclusion no longer holds is that it was taken while the edge work sat inside a larger record carrying other guards. Transplanted into a standalone record it left this record with no working guard at all — the Agent Brief's edge-render criterion carries the command over the test file that establishes this.

Recorded as a reversal deliberately. An earlier draft of this note claimed the new criterion merely *applied* the maintainer's targeted-coverage decision rather than reversing it; that framing was withdrawn on maintainer direction, because re-reading a named exclusion as an instance of the general rule it was carved out of is a provenance claim this record cannot support. The substance stands without it.

Note what this criterion does and does not buy: it is a characterization guard — see the round-2 findings below — so it is a regression net rather than proof the extraction happened. The completeness criterion is what proves that.

**Completeness criterion rewritten onto the control labels.** All four control labels occur nowhere else in the parent, so their joint absence is a sound, reachable post-state covering the entire control set. The unreachable derivation-based reading is gone.

**Control set enumerated.** Desired behavior now names all four controls and the delete action explicitly instead of gesturing at two of them.

**Class 7 resolved by command substitution.** The branch count is replaced with a command that enumerates the conditional's branches.

**Omissions closed.** The active workflow document and the outcome-vocabulary fallback are now named in Key interfaces as the two behavior-silent dependencies, and the parent-side dead-code cleanup replaces the false claim that nothing else changes. Sibling issue IDs are now cited in Out of scope, which previously named the siblings only in prose.

**Readiness gate (cold-reader): FAIL** (round 2, 2026-08-08)

Round 1's substantive findings are repaired: the completeness criterion's post-state is now reachable and covers all four controls — the reader independently confirmed the four labels occur nowhere else in the parent, checking near-misses rather than trusting the claim — and the record now carries a guard that is not blind to the edge branch. Classes 1–6, 8, and 9 all clear; class 6 does not fire on either prong; class 9 does not fire on either arm, and arm B specifically does not trigger on the new test because it asserts the system acts on the planted edge rather than leaving it alone.

**Class 7 kind (a) — three inventory claims with no command deriving them.** All three are true today; the authoring rule binds regardless. The test-suite claim ("no edge-selection path at all, fixture carries no edges, would stay green if the branch were deleted") is an inventory over a file no command in the brief touched, and it decays as siblings add tests to that same file. The style claim ("uses none of the component's locally-scoped style rules") is a zero-occurrence inventory. The parent-cleanup claim ("leaves the edge-type import and display-name derivation with no remaining users") is another.

**Non-blocking notes the reader raised:** the new test had no stated home, and the preservation runner is file-scoped, so a test in a new file would have been run by no criterion; the brief called the delete control a "Delete edge" button where the code renders `Delete`, inviting a copy change that Out of scope forbids; the render criterion lacked the read-not-just-import caveat both siblings carry; and control order is contracted in prose but guarded by nothing. The reader also corrected the record's own single-slice claim — "no subset lands green on its own" is overstated, since the new test is a characterization guard that would pass against the un-extracted branch today.

### Gate round 2 resolved (2026-08-08)

**All three class-7 surfaces replaced with commands, not refreshed assertions.** The test-suite claim now carries a grep over the test file with qualitative polarity and points at the store's edge-selection method so the fixture is writable as specified. The style claim now cites the command that enumerates every scoped rule and its usages. The parent-cleanup claim now hands over an enumeration command and asks the agent to remove what goes dead, rather than asserting in advance what that is.

**Non-blocking pickups taken while the brief was open.** The test is pinned to the file the preservation runner executes. The delete button's label is no longer restated — the brief says to keep it verbatim, which is what Out of scope required anyway. The render criterion gained the read-not-just-import caveat. The overstated single-slice sentence is left standing as-is only where accurate; the guard's independent greenness is recorded here rather than argued away.

**Readiness gate (cold-reader): FAIL** (round 3, 2026-08-09)

One blocking surface. Every other class clears, and the reader re-derived them from the tree rather than inheriting rounds 1–2: classes 1–5 clear, class 6 does not fire on either prong, class 8 is inert on a never-gated record and materially clean besides, and class 9 does not fire on either arm — the change-introducing observables executed red at baseline, the preservation criteria executed green, and the edge-render criterion triggers neither arm-B requirement because it asserts the system *acts on* the planted edge rather than leaving it alone.

**Class 7 kind (a) — the scoped-style bullet in Key interfaces.** The bullet's confirmation command hardcodes the rule families the author already had in hand, so it presupposes the inventory it is offered to derive: a scoped rule outside that pattern is invisible in its output, and the bullet's exhaustiveness qualifier — "enumerates every scoped rule" — therefore cannot come from executing it. Two further assertions ride on the same bullet: that none of the scoped rules travel with this branch, which is a zero-occurrence inventory, and that the classes the branch does use are global, which is a stylesheet inventory carrying no command at all. The reader verified all three true against the tree. Truth does not discharge a kind (a) row on brief text under edit; the remedy is command substitution — a command that derives the set of selectors the component's style block defines — never a refreshed assertion.

**Non-blocking, carried forward:**

- The single-slice sentence "no subset lands green on its own" is false: the edge-render criterion is a characterization guard that passes against the un-extracted branch today. Round 2 recorded this sentence as left standing "only where accurate", but it in fact still stands in full.
- Round 2's resolution claimed the read-not-just-import caveat is carried by both siblings. Only `ISSUE-260808-2022-11` carries it in a criterion; `ISSUE-260808-2000-07` carries an equivalent remark under Triage Notes only.
- The assertion sitting beside the test-file command — that the suite exercises no edge-selection path — is not derived by that command, which finds a token rather than a path. Judged not load-bearing, since the criterion's required action is unconditional either way. The reader confirmed it independently with an ignore-blind repo-wide sweep.

### Gate round 3 resolved (2026-08-09)

**The style bullet now derives its set instead of presupposing it.** The hardcoded rule-family pattern is gone. The bullet is split onto its own line and hands over a command that dumps the component's entire scoped style block, followed by the rule for deciding which side of the extraction each selector falls on: usages lying entirely inside the branch travel with it, a rule with no usage inside stays, and a class the block does not define is global. Nothing about the outcome is asserted up front, so the exhaustiveness qualifier, the zero-occurrence inventory, and the global-classes claim are gone rather than refreshed — the remedy the class prescribes.

**The same construction was corrected in `ISSUE-260808-2022-11` in the same pass**, verbatim as it stood there, so that record's own round 3 does not burn on a row already adjudicated here. `ISSUE-260723-0823-1` was deliberately left alone: its command is narrower and is authoritative for the set it names, so it is not this defect. Both edited records were ungated at the time — authoritative verdict FAIL — so no `REOPENED` stamp was owed on either.

**The three non-blocking items are recorded above, not edited away.** In particular the false single-slice sentence is left standing: what was wrong is this record's account of its own history, and correcting the sentence would bury that rather than show it. Flagged to the maintainer as an open, non-blocking inaccuracy in the brief.

**Readiness gate (cold-reader): PASS** (round 4, 2026-08-09)

Every gap `fine` or an explicit delegation; class 6 does not fire on either prong; every class-7 surface maps to a non-blocking row; class 8 inert on a never-gated record and materially clean besides; class 9 does not fire on either arm. The reader re-derived the record rather than inheriting rounds 1–3, and reported executing every change-introducing observable red at baseline and both preservation criteria green.

The round-3 remedy holds. The reader ran the substituted style procedure to completion rather than accepting that it would terminate: it dumped the component's scoped style block, enumerated the selectors defined there, checked each one's usages, and reached the same disposition the old hardcoded pattern had merely asserted — with the difference that the answer now comes out of the output. It separately confirmed the classes the branch uses are defined in the global stylesheet, which is the step the removed assertion had skipped.

Two verifications worth recording because they were done independently rather than taken from the brief. The reader attempted to falsify the four-labels-occur-nowhere-else claim under both a narrow and a broad reading, surfacing only a differently-worded label and a comment, so the completeness criterion covers the whole control set as claimed. And it confirmed the test seam is actually workable — the standalone-mount harness drives the store singleton directly, the fixture accepts an edges patch, and existing cases already assert persistence the way the new criterion requires.

**Non-blocking, carried forward — none owes a re-gate:**

- **Third round running, the single-slice sentence is still false.** "No subset lands green on its own" is contradicted by the record's own edge-render criterion, which passes against the un-extracted branch today. Rounds 2, 3, and 4 have each recorded it. Prong (b) does not depend on it and nothing an implementer does changes because of it. It is now immutable under this stamp; correcting it would cost a `REOPENED` and a round 5, which is disproportionate to a self-description with no contract effect. Maintainer call, deliberately left open.
- **The parent-cleanup command is a hardcoded token list** — structurally the same shape round 3 blocked on for the style bullet, minus the two features that made that one blocking: it carries no exhaustiveness qualifier and no inventory assertion rides on it, and it instructs the agent to remove what goes dead rather than naming the outcome. The reader derived the true dead set independently from the branch and found it inside the pattern. Flagged for visibility rather than as a defect.
- **The does-not-touch list** is the nearest surviving analogue to the zero-occurrence inventory round 2 blocked on. The reader ruled it non-blocking on a stated line — every item is answerable from the branch the agent is moving, under the adjacent instruction to derive the dependency surface from that branch, whereas the style question required a cross-reference the agent had no prompt to make. Recorded so the line can be overruled.
- The round-3 note describing the sibling correction as made "verbatim as it stood there" is ambiguous: the two style bullets are not textually identical, and the sentence is true only under the reading that each was corrected in its own wording. Journal-only.

**Disclosure — journal structure repaired before this stamp.** The round-3 stamp had been inserted into the middle of `### Gate round 2 resolved`, which orphaned that section's second paragraph under the round-3 heading and misattributed round-2 remediation to round 3; the reader caught it. The paragraph was moved back under its own heading before this stamp was written. No wording changed, no paragraph was added or removed, and no text in `## Agent Brief` was touched — the repair is paragraph placement within the journal only. Recorded here rather than left silent, since a stamp certifies the text it covers.

### Parked by `/implement-issue` — full-suite gate could not run (2026-08-09)

**Status: `ready-for-human`. The implementation is complete and reviewed; the blocker is environmental, not the work.**

**What was attempted.** Claimed at 2026-08-09T12:43:25Z after a clean claim-time tripwire (`SCALE: PASS`, `CONSTRAINTS: CLEAN`). Routed to `cursor`, implemented as a linear (non-TDD) pure refactor. Dual review (Claude + Codex) cross-verified and adjudicated inline produced 9 findings; 7 were fixed across 2 fix rounds (cap 4) and 2 were dismissed on the brief's own contract. Round 2 closed with no new findings from either reviewer. Every acceptance criterion was independently confirmed satisfied by both reviewers.

**What blocks.** Step 6's full suite returned `SUITE: ERROR`. This project's full suite is `just test` = `cargo test && npm test`, and **no Rust toolchain exists on this machine** — `cargo`, `rustc`, and `rustup` are all absent from `PATH`, and `~/.cargo/bin` contains only unrelated binaries (`cargo-nextest`, `claude-history`, `tmux-tools`). The Rust half aborted at `cargo: command not found` (exit 127). An infrastructure error is never judged against the diff, so this parks rather than closes — even though the diff is entirely frontend and the frontend half ran fully green (10 files / 113 tests, including all 16 `InspectorPanel.test.ts` cases). `just typecheck` and `just build` both succeed.

**Maintainer decision needed.** This is not specific to this issue: no issue in this repo can pass the full-suite gate on this machine. Either provision the Rust toolchain, or decide whether frontend-only slices may gate on `just test-ui` + `just typecheck` alone. Re-running `cargo test` on a machine with Rust is the only outstanding step before this can close.

**Where the work is.** Parked on branch `wip/ISSUE-260808-2022-04` (commit `8f8cb90`), which carries the six implementation files: the new `EdgeInspector.svelte`, the reworked `InspectorPanel.svelte` and `InspectorPanel.test.ts`, and the rebuilt `public/` bundle (`just build` output, verified byte-identical to a fresh build by both reviewers, so the frontend-freshness pre-commit hook is satisfied). The working branch was left clean of implementation changes.

**Review artifact.** Full findings, evidence, and fix notes in `issue-260808-2022-04-code-review-20260809-130831.md`.

**One thing worth a human's eye regardless of the toolchain question.** The review's HIGH finding (H1) was a genuine behavior regression the extraction introduced — the parent's branch predicate was weakened from an edge-existence check to a selection-kind check, so undoing an edge-add left the inspector blank instead of falling back to the workflow inspector. It was caught empirically (baseline-vs-worktree render comparison), fixed, and pinned by a new regression test that was itself proven red against reconstructed broken semantics. That test is the main durable addition beyond the extraction.

## Code Review

Dual review (Claude + Codex, cross-verified, adjudicated inline) — full evidence in `issue-260808-2022-04-code-review-20260809-130831.md`.

- H1 (HIGH): Stale edge selection renders a blank inspector instead of the workflow inspector — FIXED
- M1 (MEDIUM): Embedded frontend bundle under `public/` not rebuilt — FIXED
- M2 (MEDIUM): Condition assertion checks only the label text, not that the condition editor rendered — FIXED
- M3 (MEDIUM): Edge-selected predicate decided twice, in two components — FIXED
- M4 (MEDIUM): Four handlers repeat the same update-workflow / locate-edge / mutate sequence — FIXED
- L1 (LOW): Persistence assertion indexes `edges[0]` instead of finding the edge by id — FIXED
- L4 (LOW): Fix round 1 reintroduced a stranded `WorkflowEdge` type import in the parent — FIXED
- L2 (LOW): Delete button, header, and control order unasserted — dismissed
- L3 (LOW): Active-document resolution runs twice per render pass — dismissed

L4 was raised during round-1 fix verification and orchestrator-verified before entering the file. Two fix rounds used of a cap of 4; round 2 closed with no new findings from either reviewer.

Smells: 3 reported, 2 promoted into the findings track as M3/M4 under the in-diff duplication rule, 1 advisory (`EdgeInspector.svelte:14` Middle Man, borderline) merged into the ledger. No ledger row closed by this issue, per Out of scope.
