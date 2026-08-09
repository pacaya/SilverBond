---
id: ISSUE-260808-2000-07
kind: issue
category: enhancement
status: done
summary: Extract shared per-field pane-config components consumed by both run_agent and spawn, and extract the spawn panel
claimed_by: implement-issue@macmini.home
claimed_at: 2026-08-09T11:52:54Z
---

## Agent Brief

**Category:** enhancement
**Summary:** Give the repeated inspector form controls a single edit point by extracting one component per control, converting every consumer in place — behavior-preserving.

**Current behavior:**

The `InspectorPanel` editor component renders the same form controls in more than one place. Pane name, access-profile select, working directory, and extra-args each appear in both the `run_agent` PTY section and the `spawn` section.

The working-directory control appears again as a per-node setting, inside the collapsible agent-tuning section of the block shared by `task` and `run_agent` nodes — so it renders for `task` nodes as well, and only while that section is expanded.

It also appears in the workflow-level inspector, which is **out of scope here**. That occurrence edits the workflow's own working directory rather than a node's: it binds the value directly, updates on input rather than on blur, and carries no placeholder — because it *is* the value the in-scope occurrences fall back to displaying. It is not de-duplicated by this issue or by any sibling, and it moves wholesale with the workflow-level inspector in `ISSUE-260808-2022-11`.

Survey every occurrence, with surrounding context so the interleaving is visible, using:

```
rg -n -B2 -A6 '<span>(Pane name|Access|Working directory|Extra args)</span>' ui/src/features/editor/InspectorPanel.svelte
```

The copies differ only in which value they read and which writer they call: the two `run_agent`/`spawn` sets read their respective typed config objects, while the per-node working directory reads the selected node's own `cwd` and writes it directly rather than through a config object. The practical cost is that a change to any of these controls needs several edits that are easy to make inconsistently — a prior review finding (M11, in the source review, already fixed) had to replace the free-text access inputs with capability-populated selects in more than one place for exactly this reason.

**Desired behavior:**

Each repeated control becomes its own small component owning that one control's markup and change handling, and every existing occurrence is converted to use it. The four components are consumed by the `run_agent` and `spawn` sections; the working-directory component additionally serves the per-node occurrence, whose writer differs.

**The controls are composed individually, not as a contiguous group, and nothing is reordered.** In neither the `run_agent` nor the `spawn` section are these controls adjacent — other controls are interleaved between them, as the survey command above shows. A single grouped component therefore cannot reproduce either section's field order. Each consumer composes the individual components in the order it uses today and keeps its own controls inline between them, so rendered markup is unchanged in every consumer.

For the avoidance of doubt, the field label order each section must still render after this change is:

- `run_agent` "Agent run" section, in order: **Pane name, Access, Working directory, Timeout (s), Idle seconds, Ready-stable (s), Until marker, Extra args, Kill pane after**.
- `spawn` section, in order: **Agent, Command, Pane name, Session name, Access, Working directory, Extra args**.

These two sequences are the specification for the order test below — the test asserts against *these literal sequences*, not against whatever order happens to exist when the test is written.

**Conversion is in place. No node-kind arm is extracted into a panel component by this issue,** and no config writer or config-writing switch case moves. The `spawn` arm stays inline in the node-kind chain and simply consumes the new components; extracting it belongs to the sibling kind-panels issue. This keeps the change to markup substitution with no plumbing changes.

**Key interfaces:**

- **Field components** — each takes the control's current value and a change callback; the parent passes a callback already bound to whichever writer that occurrence uses today, so no writer, merge helper, config default, or store access moves into the new components. The access-profile component additionally takes the list of available profiles and must keep its "registry default" empty option. The working-directory component additionally takes a placeholder string, which every consumer derives from the active workflow's working directory as it does today. The extra-args component keeps its newline-split-and-trim parsing and its short-textarea styling.
- **Nothing else changes shape** — the typed config accessors, the config-writing switch, the per-kind writer wrappers, and the node-kind chain are all untouched by this issue.
- **Destination** — colocate the new components with the existing editor components, matching the convention already used by the sibling components in that directory.

**Acceptance criteria:**

- [ ] Each of the four controls has a dedicated component. For each of the four component names chosen, `rg -n '<Name>' ui/src/features/editor/InspectorPanel.svelte` returns matches — run the command once per name rather than as a single alternation. No such component names exist before this change.
- [ ] Every in-scope occurrence is converted, leaving no inline copy behind. After the change, `rg -n -B2 -A6 '<span>(Pane name|Access|Working directory|Extra args)</span>' ui/src/features/editor/InspectorPanel.svelte` no longer shows an `<input`, `<select`, or `<textarea` element inline beneath those labels in the `run_agent`, `spawn`, or per-node occurrences — each renders through a component instead. Before this change every occurrence carries its element inline. **The workflow-level working-directory occurrence is out of scope and will still show its element inline after this change; disregard it when reading the output.** Verifying this criterion is a reading of the command's output, not a bare exit code.
- [ ] **Field order is asserted against the literal sequences above.** A test renders a `run_agent` node and a `spawn` node and asserts the rendered field-label sequence of each section equals the corresponding sequence written in Desired behavior, as a literal expected value in the test. It must not derive its expectation from the rendered output. No test asserts field order before this change.
- [ ] **Preservation** — `npx vitest run --config ui/vite.config.ts ui/src/features/editor/InspectorPanel.test.ts` passes, and every test case present in that file at the baseline still passes with its assertions unmodified. New cases may be added; existing assertions may not be relaxed. Green before and after.
- [ ] **Preservation** — `just typecheck` reports no new errors. Green before and after.
- [ ] **Preservation** — the config-writing switch is untouched. `rg -c 'case "spawnConfig"' ui/src/features/editor/InspectorPanel.svelte` and `rg -c 'const updateSpawn =' ui/src/features/editor/InspectorPanel.svelte` each still return a match after this change, as they do before it. This issue does not move writers; the sibling kind-panels issue does.
- [ ] The Duplicated Code smells-ledger row is closed against the landing commit. `rg -n 'InspectorPanel\.svelte:789' docs/issues/SMELLS-LEDGER.md` shows a `fixed:` verdict rather than `open`; it reads `open` before this change. The helper is `~/.claude/skills/dual-code-review/scripts/smells-ledger.sh fix docs/issues/SMELLS-LEDGER.md '<row key>' <landing-commit-sha>` with row key `` `ui/src/features/editor/InspectorPanel.svelte:789` + Duplicated Code ``. Editing the verdict column directly is equally acceptable; the criterion is the closed row, not the mechanism.

**Why this is a single slice:**

The deliverable is the single edit point, and it exists only once every in-scope occurrence shares the components. Converting one consumer and leaving another inline relocates a copy rather than removing it — the defect this issue exists to cure. The ledger row it closes names this specific duplication and closes exactly on this work.

Stated precisely, since an earlier draft overreached: the two structural criteria are inseparable from each other, because a component consumed by one surface and not the others delivers no single edit point. The order test is a characterization guard that would land green on its own, and the ledger row is bookkeeping — neither is an independently *demoable* subset, and neither is a slice boundary any reasonable reader would draw around four ten-line components. There is no useful decomposition here, but not because every criterion depends on every other.

**Out of scope:**

- Extracting any node-kind arm into a panel component, including `spawn` — that is the sibling kind-panels issue.
- Moving any config writer, config default, merge helper, or config-writing switch case.
- The edge inspector, the workflow-level inspector, and the shared agent-config fields — separate sibling issues.
- Hoisting unlock/password prompting anywhere.
- Any change to config schemas, defaults, validation, or access-profile semantics — this is markup substitution only.
- Any visual, styling, copy, or layout change, including field reordering.

## Context Pack — generated at claim (2026-08-09T11:52:54Z)

**PRD decisions relevant to this slice:** no PRD linked — the record carries no `prd:` frontmatter, and no `docs/prd/` or `docs/decisions/prd/` directory exists in this repo. The governing decisions are stated in the record's own Agent Brief and Triage Notes:

- Maintainer decision (2026-08-08, gate finding G1): emit **one component per control**, not a single contiguous four-field group — the controls are non-contiguous in both consumers, so a group cannot reproduce either section's order.
- Conversion is **in place**: no node-kind arm (including `spawn`) is extracted, and no config writer, default, merge helper, or switch case moves. Markup substitution only.
- Each field component takes current value + a change callback already bound by the parent to that occurrence's existing writer; access-profile also takes the profile list and keeps its "registry default" empty option; working directory also takes a placeholder derived from the active workflow's working directory; extra-args keeps newline-split-and-trim parsing and short-textarea styling.
- The working-directory component has a **third** consumer: the per-node `cwd` control in the collapsible agent-tuning block shared by `task` and `run_agent`.
- The workflow-level working-directory control is **out of scope** — different binding semantics (binds directly, updates on input, no placeholder) and it *is* the value the in-scope occurrences show as placeholder; it moves with `ISSUE-260808-2022-11`.
- Colocate new components with the existing editor components, matching the sibling convention in that directory.

**Test seam & Testing Decisions:** observable at the editor inspector component's vitest suite (`ui/src/features/editor/InspectorPanel.test.ts`, run via `npx vitest run --config ui/vite.config.ts`); the surveying seam is `rg -n -B2 -A6 '<span>(Pane name|Access|Working directory|Extra args)</span>' ui/src/features/editor/InspectorPanel.svelte`. Testing decisions touching it:

- Maintainer decision (2026-08-08, gate finding G6): require **targeted field-order coverage** for these two consumers rather than full coverage of all uncovered surfaces.
- The order test asserts the rendered field-label sequence against the **literal sequences written in the brief** — `run_agent`: Pane name, Access, Working directory, Timeout (s), Idle seconds, Ready-stable (s), Until marker, Extra args, Kill pane after; `spawn`: Agent, Command, Pane name, Session name, Access, Working directory, Extra args. It must not derive the expectation from rendered output.
- Preservation: every baseline test case still passes with assertions unmodified; new cases may be added, existing ones may not be relaxed; `just typecheck` reports no new errors; both green before and after.
- Per-name `rg` greps prove naming, not consumption — the completeness criterion is a **reading** of command output, not an exit code; the order test is the real functional guard.
- Known thin spot (gate round 3, non-blocking): the per-node working-directory writer coerces empty to `null` where config writers coerce to `undefined`; unpinned and unguarded — behavior-preserving governs.
- Ledger closure: `rg -n 'InspectorPanel\.svelte:789' docs/issues/SMELLS-LEDGER.md` must read `fixed:` against the landing commit.

**ADRs:** no ADR index found — no `docs/adr/INDEX.md` and no `docs/model/generated/`. The only decision record in-repo is `docs/decisions/tmux-bin-resolution.md`, which does not touch this slice.

**Terms:** no glossary — the record carries no `terms:` frontmatter and there is no `CONTEXT.md` at the repo root or under a context root.

**Full artifacts:** docs/issues/ISSUE-260808-2000-07-shared-pane-config-fields.md · docs/issues/SMELLS-LEDGER.md · CLAUDE.md

## Resolution

**Commit:** `feat: extract shared per-field pane-config components (ISSUE-260808-2000-07)`

**Route:** cursor (`/cursor-developer`) — well-scoped single-file Svelte component extraction with a detailed spec and pre-defined field-order guards; no cross-module reasoning, security surface, or infrastructure work. Both fix rounds routed to cursor for the same reason.

**TDD:** n/a (linear) — behavior-preserving markup substitution, and the mandated field-order test is a characterization guard that is green at baseline by construction, so red-green does not apply.

**Review telemetry:** 7 findings — 2 MEDIUM, 3 LOW, plus 2 MEDIUM promoted from in-diff duplication smells. 5 FIXED, 1 deferred (L3), 2 dismissed (D1, D2). Fix rounds used: 2 of 4. Codex returned zero findings independently; all findings originated with the Claude reviewer or the promotion rule. 2 advisory smells appended to the ledger; graduation check emitted nothing.

**Suite:** green — 466 Rust tests (449 unit + 17 integration) and 111 frontend tests across 10 files, 0 failures. `just typecheck` clean (593 files, 0 errors, 0 warnings).

**Environment note (not a code defect):** the first full-suite run returned `SUITE: ERROR` because the `cargo` shim in `~/.cargo/bin` is missing from this machine, so `just test` aborted at `cargo test` with `command not found`. The toolchain itself is present at `~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/cargo` (1.96.0); re-running with that directory on `PATH` produced the green result above. Nothing about the repo or this change caused it, but the shim is worth repairing — every `just test` on this machine will hit it.

**Bundle:** the embedded frontend bundle in `public/` was rebuilt and is committed with the source change, per the "Frontend bundle freshness" convention in `CLAUDE.md`. Its delta is confined to one JS chunk swap plus the `index.html` script reference; the CSS artifact is byte-identical to baseline, independently corroborating that this refactor produced no styling change.

**Pre-existing repo defect found while verifying the bundle (not fixed here, out of scope):** `.githooks/pre-commit` cannot pass as written, on any commit touching `ui/src`. It snapshots the staged tree with `git checkout-index` and symlinks `node_modules` into it (line 30), then diffs a build of that snapshot against the staged `public/`. Resolving `node_modules` through a symlink changes Svelte's scoped-class hashes for vendored `svelte-flow` components, so the snapshot build never matches an in-repo build byte-for-byte and the hook aborts at line 35 regardless of freshness. Verified: normalizing `svelte-[hash]` tokens away makes the committed bundle and the snapshot build **identical** in both JS and CSS — there is no content difference, only hash churn. This is likely why `core.hooksPath` is unset in this clone and why most recent `ui/src` commits carry no `public/` rebuild. A fix would make the hook compare normalized output, or give the snapshot a real `node_modules` rather than a symlink.

**Date:** 2026-08-09 (UTC)

## Code Review

Review file: `issue-260808-2000-07-code-review-20260809-123056.md`

- M1 (MEDIUM): Embedded `public/` bundle not rebuilt for the changed `ui/src` — FIXED
- M2 (MEDIUM): Per-node `WorkingDirectoryField` call site diverges from its two siblings — FIXED
- L1 (LOW): Order test asserts label text only, never that a control renders — FIXED
- L2 (LOW): Two scenarios packed into one `it` with a mid-test `cleanup()` — FIXED
- L3 (LOW): Per-node `WorkingDirectoryField` consumer has no test coverage — deferred (maintainer decision G6 scoped coverage to the two named consumers; gate round 3 recorded this gap as non-blocking)
- D1 (MEDIUM): Placeholder derivation repeated at all three call sites — dismissed (mandated by the brief's Key interfaces)
- D2 (MEDIUM): `PaneNameField` and `WorkingDirectoryField` structurally identical — dismissed (contradicts maintainer decision G1; fails the deletion test)

Smells: 2 advisory (Mysterious Name, Speculative Generality), both appended to `docs/issues/SMELLS-LEDGER.md`. Three further Duplication smells were promoted to findings by the in-diff duplication promotion rule and appear above as M2, D1, and D2. Graduation check emitted nothing.

## Triage Notes

Split out of `ISSUE-260723-0823-1` on 2026-08-08 after that record's readiness gate fired class 6 prong (b). The parent record's decomposition was subsequently split further; this record owns the `InspectorPanel.svelte:789` Duplicated Code ledger row.

The gate's class-4 finding G1 is resolved here: the original S6 specification called for a single contiguous four-field group, which is unbuildable without reordering, because the controls are non-contiguous in both consumers. Maintainer decision (2026-08-08): emit one component per control so each consumer composes them in its existing order. This preserves rendered output while still delivering one edit point per control, which is what S6 actually required.

The gate's class-4 finding G6 is partly resolved here: the component's test suite asserts no field order anywhere, so a reorder would land green. Maintainer decision (2026-08-08): require targeted field-order coverage for these consumers rather than full coverage of all uncovered surfaces — it guards the one user-visible risk at a fraction of the cost.

- Scale snapshot (non-contractual): `rg -c '<span>(Pane name|Access|Working directory|Extra args)</span>' ui/src/features/editor/InspectorPanel.svelte` → ~10 label occurrences spread across the node-kind arms and the workflow-level inspector (2026-08-08)

**Readiness gate (cold-reader): FAIL** (round 2, 2026-08-08)

One blocking surface, and both round-1 remedies held. Classes 1–6, 8, and 9 all clear: class 6 does not fire on either prong judged on the merits, and class 9 arm B does **not** fire on the field-order criterion — the reader reproduced both literal label sequences character-for-character against the code and confirmed the criterion is now self-witnessing, since a reorder or a subset makes a literal comparison fail. The round-1 circularity is cured. All four change-introducing observables verified red at baseline.

**Class 7 kind (a) — one sentence.** "The working-directory control appears a third time" is an ordinal count over decaying repo state that the brief's own survey command does not derive: the command returns four working-directory occurrences, not three, the fourth being in the workflow-level inspector. The same sentence placed the per-node occurrence "in the `run_agent` arm's agent-tuning section", which is a qualifier the command does not express and is inaccurate — that block is shared by `task` and `run_agent` nodes and is collapsible, so the control also renders for `task` nodes and only while the section is expanded.

**Noted, not blocking:** the single-slice paragraph overstated — the order test is a characterization guard that would land green alone, so "none is complete without the others" is too strong; the destination directory was unpinned here although all three siblings pin it; and after the whole decomposition lands, the workflow-level working-directory control remains un-de-duplicated by any record.

### Gate round 2 resolved (2026-08-08)

**Class 7 resolved by rewriting the sentence, not refreshing the number.** Current behavior now describes the occurrences qualitatively, states the per-node one's real enclosing conditions, and accounts explicitly for the workflow-level occurrence — including why it is out of scope: it edits the workflow's own working directory with different binding semantics and no placeholder, and it is the value the in-scope occurrences fall back to displaying. The completeness criterion now warns that this occurrence will still show inline markup afterwards and must be disregarded when reading the command output, so the reader no longer has to re-derive that.

**Non-blocking notes taken.** The single-slice paragraph now separates the two structural criteria, which genuinely are inseparable, from the order test and the ledger row, which are not — the conclusion stands on the absence of a sensible slice boundary rather than on an overclaim. The destination is pinned to match the siblings. That the workflow-level control stays un-de-duplicated is now stated in the brief rather than left silent; it is a deliberate accepted outcome, since that control is arguably not the same control.


**Readiness gate (cold-reader): FAIL** (round 1, 2026-08-08)

The central factual claim survived falsification — the four controls are genuinely non-contiguous in both consumers, and a normalized diff of the four pairs leaves exactly one differing line each, so one-component-per-field can reproduce both consumers' markup. Class 6 does not fire on either prong. Four things block the stamp.

**Class 9 arm B — the field-order criterion is circular and has no control.** It pins the expected order to "the order it has at the baseline" while writing that order down nowhere and pinning no baseline tree. The test is authored after the refactor by the same agent, so it snapshots whatever order then exists and is green whether or not a reorder landed. The brief calls it the only mechanical guard against the one user-visible risk; as written it is not a guard. Not waivable; remedy is a brief edit.

**Class 4/5 — the accessor and writer placement is self-contradictory.** The brief says the spawn panel takes an update callback bound to the spawn config key, and also that the spawn writer and its switch case leave the parent. Both cannot hold: if the parent supplies the bound callback the case must stay, and if the panel owns the write it needs the merge helper, the spawn defaults, the store, and the selected-node lookup — all parent-local. Compounding it, the brief has the panel reuse the existing `spawn` accessor, which lives in the component instance script and is therefore not importable — the same defect this record's sibling brief names for its own helper.

**Class 3 — a third instance of the same control falls through the gap between both records.** The `run_agent` arm's agent-tuning section carries a per-node working-directory control that is near-verbatim to the two this record de-duplicates — same label, same wrapper, same placeholder expression — differing only in writing the node's own `cwd` rather than a typed config field. Neither this record nor its sibling claims it, and this record's own enumeration command returns it.

**Class 7 kind (a) — four surfaces on brief text under edit.** "Renders pane-configuration controls twice", "four controls are duplicated", "the two copies are identical apart from…", and the interleaving claim are figures stated without an authoritative discovery command. The enumeration command offered alongside them returns ten label occurrences across three regions rather than the eight across two the prose asserts, so it is not authoritative for the figure; and it prints only the four shared labels, so it cannot show the interleaving it is offered to verify.

**Also noted, not blocking:** the single-slice argument is unsound as written. The field extraction plus the order test plus the ledger row is independently green and closes this record's only completion check, while the spawn-panel extraction and writer departure close nothing — both consumers could be converted in place with the spawn arm still inline, delivering the single edit point and closing the row without any panel extraction. The gate's conclusion is that the record is still issue-scale, but the stated reasoning does not establish it.

### Gate round 1 resolved (2026-08-08)

All findings addressed; brief rewritten and returned for round 2.

**Scope narrowed to in-place conversion.** Acting on the gate's non-blocking observation, the spawn-panel extraction and the writer/switch-case departure are removed from this record entirely — they close nothing here and were the sole source of the class-4 contradiction. The `spawn` arm now stays inline and simply consumes the new components; its extraction moved to the kind-panels record, which is where the `:1` row's per-kind axis is separated. This dissolves the accessor-importability problem too: nothing is imported into a panel because there is no panel.

**Class 9 arm B resolved by specification, not by snapshot.** Both expected field-label sequences are now written into the brief literally, and the criterion requires the test to assert against those literals and forbids deriving the expectation from rendered output. The order is now a specification the fixture checks, so the fixture goes red if a reorder lands.

**Class 3 resolved by inclusion.** The per-node working-directory control is folded in as a third consumer of the working-directory component rather than left stranded between records. Excluding it would have shipped a "single edit point" with a copy still outside it.

**Class 7 resolved by command substitution.** The counts are gone. The survey command now carries context flags so it shows the interleaving it is offered to verify, and the prose describes the occurrences qualitatively rather than asserting how many there are.

**Noted for round 2:** the gate observed that per-name greps are satisfiable by a single barrel import and so do not prove consumption. The completeness criterion is therefore written as a reading of command output rather than an exit code, and the order test is the real functional guard.

**Readiness gate (cold-reader): PASS** (round 3, 2026-08-08)

Every gap `fine`; class 6 does not fire on either prong; no class-7 kind (a) surface remains; class 8 inert; class 9 does not fire on either arm, with all four change-introducing observables executed and red at baseline.

(Stamp ordering note: the round-2 stamp sits above the round-1 stamp in document order because it was appended beside the scale snapshot. Authority follows the round number, so this round-3 stamp is authoritative regardless of position.)

The reader re-derived the field-order criterion independently rather than inheriting round 2's verdict, and reproduced both literal label sequences against the code — nine spans in the `run_agent` "Agent run" section and seven in the `spawn` section, exact match in exact order, with no conditional inside either section that could vary them. It confirmed the criterion is self-witnessing: a reorder, a dropped field, or an empty render all fail the literal comparison. It also verified the four control pairs differ in exactly two lines each — the value read and the writer called — so one component per control reproduces both consumers' markup.

The workflow-level exclusion was checked on all four stated grounds and holds: that control binds a non-nullable workflow field directly, updates on input rather than blur, carries no placeholder, and is literally the string the other three render *as* their placeholder. The reader added a fifth differentiator — its writer targets the workflow document rather than a node's typed config — and judged the "single edit point" claim not overstated, since the brief scopes it to in-scope occurrences and states the exclusion in the open.

Non-blocking notes carried forward, none owing a re-gate:

- The per-node occurrence's render condition also requires agent capabilities to be present, which the brief does not state; the stated condition is therefore wider than reality, which is immaterial to what gets built.
- The per-node working-directory writer coerces an empty value to `null` where the config writers coerce to `undefined`. That difference is unpinned by the brief and unguarded by any existing test — the thinnest spot in this record's preservation net. Behavior-preserving is the governing instruction, but nothing mechanical enforces it.
- The kind-panels sibling is referenced in prose only, while the other two siblings are cited by ID.
- Because the two label sequences are specification rather than a snapshot, code drift before this lands will surface as an order-test failure rather than silently — deliberate, and worth knowing.
