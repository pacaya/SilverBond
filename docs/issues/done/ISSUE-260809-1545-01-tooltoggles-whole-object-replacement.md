---
id: ISSUE-260809-1545-01
kind: issue
category: bug
status: done
summary: Web search control cannot express the tri-state webSearch field — it destroys an explicit "off" and replaces the whole toolToggles object on every write
claimed_by: implement-issue@Mac-mini-4
claimed_at: 2026-08-15T18:00:46Z
---

## Agent Brief

**Category:** bug
**Summary:** Replace the two-state web-search checkbox in the shared agent-config fields with a three-state control, so an explicit "off" can be written, is distinguishable from "inherit", and survives editing — and so writing it merges into `toolToggles` instead of replacing it.

**Current behavior:**

`ToolToggles.webSearch` is an optional boolean on both sides of the wire, so it carries three states, each with distinct runtime behavior:

- **unset** — the agent's own default; no driver adds a flag
- **`true`** — force on; the Codex driver adds its web-search flag
- **`false`** — force off; the Claude driver adds disallow flags for its web-search and web-fetch tools

Confirm the toggle's shape with `rg -n 'pub web_search: Option<bool>' src/driver.rs` — note that the same file separately declares a `web_search` capability `bool`, a different field. Read the driver arms that consume the toggle with `rg -n 'if let Some\(.*\) = config\.tool_toggles\.web_search' src/driver.rs`. A further guard rejects any set value when the agent lacks the capability; find it with `rg -n 'web_search\.is_some' src/driver.rs`.

The shared agent-config fields component — the Svelte component rendering the per-agent tuning form, used both for a `task`/`run_agent` node's `agentConfig` and for a workflow's `agentDefaults` entry, gated on the agent's `webSearch` capability — renders web search as a **two-state checkbox**. That produces three defects:

1. **`false` is unwritable.** The control emits only the toggles object `{ webSearch: true }` or `undefined`. No user action produces `webSearch: false`, so the force-off state the Claude driver acts on is unreachable from the editor.

2. **Unset and off render identically.** The checkbox's checked state falls back to `false` when the field is absent, so a stored explicit `false` displays exactly like "inherit". A workflow authored outside the editor that sets `webSearch: false` therefore round-trips through the editor as a silent behavior change: the value survives load, renders as an unchecked box, and one toggle of that box replaces it with `undefined` — the disallow flags stop being emitted and nothing tells the user.

3. **Every write replaces the whole `toolToggles` object.** Both the check and the uncheck path assign a freshly built object (or `undefined`) rather than merging into the stored one, so any sibling key the `ToolToggles` shape later grows is discarded on either transition.

Defect 1 also makes a two-level configuration inexpressible. The backend resolves a node's toggles against the workflow defaults by whole-object fallback — read `rg -n 'merge!\(tool_toggles\)' src/model.rs` and the macro above it. When an agent's workflow defaults set `webSearch: true`, clearing the control on a node emits `undefined`, which falls through to the inherited `true`: the user turns web search off on that node and it stays on. The value that expresses "off here" is exactly the one the control cannot write.

A correct precedent for the merge half already exists in this codebase: the workflow-level `runAs` editor merges each field into the stored object through the shared merge helper and collapses the result to `undefined` when no field is set. Find it with `rg -n 'mergeConfig' ui/src/features/editor/`.

**Desired behavior:**

The web-search control offers three selectable states — inherit (the agent default), on, and off — mirroring the reasoning-level control already in the same component, which renders an optional enum as a select whose empty option means "inherit". Specifically:

- Selecting **on** stores `webSearch: true`; selecting **off** stores `webSearch: false`; selecting **inherit** removes the `webSearch` key.
- The rendered state is a function of the stored value with three distinct outcomes: absent → inherit, `true` → on, `false` → off. Absent and `false` must not render alike.
- Writes **merge** into the stored `toolToggles` object rather than replacing it: any other key already present is carried through unchanged. Use the existing shared merge helper rather than introducing a second one.
- When the merged object has no keys set, the whole `toolToggles` value becomes `undefined`, so an unconfigured toggles object is never persisted. The two callers already delete a key whose value is `undefined` and collapse the emptied parent, so the component's obligation is to emit `undefined` for the value, not to prune the containing config.
- Both hosts of the shared component — the node-level tuning form and the workflow-level agent-defaults form — get the three-state control. It is one control in the shared component, not duplicated per host.
- The predicate deciding whether a node's agent-tuning section "has values" treats an explicit `webSearch: false` as configured. It currently tests the field for truthiness, so a node whose only tuning is force-off would report as unconfigured once `false` becomes writable.

**Key interfaces:**

- `ToolToggles` (frontend type mirroring the backend struct) — no shape change; the fix is that the editor can now reach all three states of its optional boolean field.
- The shared agent-config fields component's `update` prop — a `(key, value)` callback typed over the agent-defaults keys. Its `undefined` value already means "clear this key" to both callers; the component must use that to express inherit.
- The shared merge helper used by the `runAs` editor — takes defaults, a base object, a field name, and a value, and deletes the field when the value is `undefined`. Reuse it for `toolToggles`.
- The exported section-visibility predicate over a workflow node and a section id — its agent-tuning arm needs a presence test rather than a truthiness test for the web-search field.

**Test seam:**

The acceptance behavior is observable at two seams, both under `just test-ui`:

1. **The shared agent-config fields component, rendered directly.** It takes its capabilities, values, and `update` callback as props, so a component test can render it with a chosen `toolToggles` value and a spy `update`, drive the control, and assert on the emitted `(key, value)` pair — no store, workflow document, or parent inspector needed.
2. **The exported section-visibility predicate**, a pure function of a node and a section id, unit-testable directly.

**Acceptance criteria:**

- [ ] A component test renders the shared agent-config fields component directly with a spy `update` prop. `rg -n 'AgentConfigFields' ui/src -g '*.test.ts'` returns that test's import of the component; no matches before this change.
- [ ] Driving the control to **off** emits the web-search key with value `false`. `rg -n 'webSearch: false' ui/src -g '*.test.ts'` returns the assertion; no matches before this change.
- [ ] Driving the control to **on** emits the web-search key with value `true`, and driving it to **inherit** emits a `toolToggles` value of `undefined` when no other key is set. Both asserted on the spy's arguments in the same test.
- [ ] Rendering the component with a stored `toolToggles` of `{ webSearch: false }`, of `{ webSearch: true }`, and with the field absent produces three distinct rendered control states, asserted as three distinct values in the same test — the absent case and the `false` case must not assert equal.
- [ ] Given a stored `toolToggles` carrying an additional key beyond `webSearch`, driving the control to each of the three states emits a `toolToggles` value **deep-equal to** the full expected object including that additional key (for inherit: the object with the web-search key absent and the additional key present). Asserted by whole-object equality on the spy's argument, not by a containment check.
- [ ] A unit test over the exported section-visibility predicate asserts that a node whose only agent config is a `toolToggles` of `{ webSearch: false }` reports the agent-tuning section as having values. `rg -n 'sectionHasValues' ui/src -g '*.test.ts'` returns that test; no matches before this change.
- [ ] `just test-ui` passes.
- [ ] `just typecheck` passes.
- [ ] `cargo test` passes — the backend already implements the tri-state, and this change must not require touching it.

**Out of scope:**

- **Any backend or driver change.** The tri-state is already implemented and validated on the Rust side; this issue only makes the editor able to reach it.
- **The driver asymmetry.** The Claude driver acts only on force-off and the Codex driver only on force-on, while the web-search capability is a single flag both declare. A three-state control will therefore let a user select a state their chosen agent ignores. That is backend semantics and belongs in its own issue — do not "fix" it here, and do not suppress options to paper over it.
- **Per-level labelling of the inherit option.** The component's `placeholders` prop already distinguishes "workflow default" from "agent default" for the text fields, but the reasoning-level control hardcodes a single inherit label at both levels. Match that existing idiom; threading a per-level inherit label through `placeholders` is a separate consistency sweep across both controls.
- **Every other field in the shared component** — access mode, model, reasoning level, system prompt, max turns, max budget. Their behavior is unchanged.
- **The `runAs` editing path.** It already merges correctly and is referenced here only as the precedent to follow.
- **Adding a second key to `ToolToggles`,** or any change to the workflow schema or its version.

## Context Pack — generated at claim (2026-08-15T18:00:46Z)

**PRD decisions relevant to this slice:** no PRD linked (record carries no `prd:` frontmatter; no `docs/prd/` or `docs/decisions/prd/` in this repo).

**Test seam & Testing Decisions:** observable under `just test-ui` at two seams named by the brief — (1) the shared agent-config fields component rendered directly with props (capabilities, values, spy `update`), asserting the emitted `(key, value)` pair; (2) the exported section-visibility predicate as a pure function of node + section id. No PRD Testing Decisions available (no PRD linked).

**ADRs:** no `adrs:` frontmatter on this record; nothing selected from `docs/adr/INDEX.md`.

**Terms:** no `terms:` frontmatter; no repo-root `CONTEXT.md` glossary present.

**Slice-relevant decisions carried from the record's own triage (not from a PRD):**
- Three-state control (inherit / on / off) over `toolToggles.webSearch`, mirroring the existing reasoning-level select whose empty option means inherit; the reviewers' `checked || undefined` fix was rejected as not reaching force-off.
- Writes merge into the stored `toolToggles` via the existing shared merge helper (the `runAs` precedent), and collapse to `undefined` when no key is set; callers already prune the emptied parent.
- One control in the shared component, serving both hosts (node agent-tuning form and workflow agent-defaults form).
- The exported section-visibility predicate switches from a truthiness test to a presence test so `webSearch: false` counts as configured.
- Out of scope: any backend/driver change (tri-state already implemented server-side), the Claude/Codex driver asymmetry, per-level inherit labelling, other fields of the shared component, the `runAs` path, and any `ToolToggles`/schema shape change.
- Brief is immutable from the round-2 readiness-gate PASS stamp.

**Full artifacts:** docs/issues/ISSUE-260809-1545-01-tooltoggles-whole-object-replacement.md · docs/adr/INDEX.md (no entries selected) · docs/issues/code-reviews/issue-260808-2022-11-code-review-20260809-141744.md

## Triage Notes

Filed 2026-08-09 from finding **L5** of the `ISSUE-260808-2022-11` dual review
(`docs/issues/code-reviews/issue-260808-2022-11-code-review-20260809-141744.md`),
which deferred it rather than fixing it in place. Not yet triaged — the account
below is the review's, carried over so the work is not lost.

**What the reviewers established:**

- The Web search checkbox writes `update("toolToggles", checked ? { webSearch: true } : undefined)`.
  That is a whole-object replacement in **both** directions, so any sibling key the
  `toolToggles` shape later grows is discarded on either transition — checking or
  unchecking Web search silently wipes it.
- **Latent, not live.** Codex independently confirmed that the frontend `ToolToggles`
  type currently defines no sibling toggle, so nothing is lost today. The defect needs a
  second key to bite.
- **Pre-existing.** `ISSUE-260808-2022-11` moved the code verbatim; it did not
  introduce the behavior. It was deferred there as squarely inside that brief's
  Out of scope ("any change to agent-defaults semantics").
- **The blast radius doubled.** That extraction promoted the control into the shared
  agent-config fields component, now rendered by both the `run_agent` node block and the
  workflow-level agent-defaults block. Whoever adds the second `toolToggles` key will
  hit this at two call sites rather than one, and the failure is silent data loss
  rather than an error.

**Fix the reviewers proposed** (not yet evaluated — triage should confirm it against
the current tree rather than adopt it): merge the field instead of replacing it —
`update("toolToggles", { ...values.toolToggles, webSearch: checked || undefined })`
— with an empty-object-to-`undefined` collapse mirroring the prune logic that
`updateAgentDefault` already applies.

**Open questions for triage:**

- Is this worth fixing pre-emptively while `toolToggles` is still single-key, or
  parked until a second key is actually proposed? Fixing it now is cheap and the
  fix is already written; deferring risks it being rediscovered as a data-loss bug.
- Does the same whole-object-replacement pattern appear on other optional config
  objects in the shared field components, or is `toolToggles` the only site? The
  review looked only at the one control it was reviewing.
- Category is provisionally `bug` — it is a real defect in written code, but it is
  unreachable today. Reclassify to `enhancement` (hardening) if triage prefers.

### Triage 2026-08-09 — claim verified, scope widened

The reported whole-object replacement is confirmed. The reviewers' **framing** is not:
they checked the key axis and stopped, so they missed that the defect bites today on
the value axis.

`webSearch` is `Option<bool>` in the backend, not `bool` — three states, each with
distinct driver behavior (`rg -n 'tool_toggles.web_search' src/driver.rs`). The
checkbox is a two-state control over a three-state field, which makes force-off
unwritable, makes unset and off render identically, and makes "on at the workflow
level, off at this node" inexpressible against the backend's whole-object fallback
merge (`rg -n 'merge!\(tool_toggles\)' src/model.rs`). None of that needs a second
key. **Not latent.**

The reviewers' proposed fix does not address any of it: `checked || undefined`
yields `undefined` on the uncheck path, so it preserves hypothetical siblings and
leaves all three live defects standing. Rejected in favour of the three-state
control, which subsumes it — the merge is still required, but as one part of a
control that can reach every state of the field.

**Resolving the three open questions:**

- **Fix now, not park.** The parked framing rested on "latent"; it isn't.
- **`toolToggles` is the only site.** Swept every object-valued write in the editor
  components (`rg -n 'update\w*\([^)]*\{' ui/src/features/editor/*.svelte`). The one
  other nested optional config edited there is `runAs`, which already merges through
  the shared helper and collapses to `undefined` correctly — the right pattern
  already exists a few lines from the broken one. `orchestrator` exists in both type
  systems but has no editor UI, so it has no exposure.
- **Category stays `bug`.** Live data loss and an inexpressible configuration, not
  hardening against a hypothetical.

**Also picked up into scope:** the agent-tuning section-visibility predicate tests
the web-search field for truthiness, so once `false` is writable a node whose only
tuning is force-off would stop surfacing its own section. Fixing the control without
this would trade one silent-loss bug for another.

**Deliberately left out and not yet filed:** the two drivers honour opposite halves
of the tri-state — Claude acts only on force-off, Codex only on force-on — while the
web-search capability is a single boolean both declare true. A three-state control
will therefore let a user select a state their agent silently ignores. Backend
semantics; owed its own issue.

`summary:` was rewritten at triage — the filed line described only the sibling-key
symptom, which is the narrowest of the three defects and the only one that is not
live.

### Readiness gate

**Readiness gate (cold-reader): FAIL** (round 1)

Classes 1–6, 8 and 9 passed. Class 7 fired on one row of a 39-row ledger: the
Current-behavior sentence asserted a bare count of the driver arms consuming the
toggle ("the two `if let Some(...)` arms") beside a discovery command that returned
a third, unrelated line and did not express the sentence's qualifier — kind (a)
rule-violation figure on brief text under edit, whose decision-table row is
unconditional.

Remedy applied per the kind-(a) rule: the figure was replaced by commands that are
authoritative for the sets described, not corrected to a different number. The
shape probe now pins the toggle field (the file separately declares a capability
`bool` of the same name), the consuming arms have their own command, and the
capability guard is named separately instead of being conflated with the arms.

Two note-only findings from the same round were taken while the record was open:
the acceptance criterion claimed the backend "tests all three states" when no Rust
test covers the unset arm (claim narrowed to implementation, which is what the
criterion actually rests on), and the same wording was dropped from the
corresponding scope boundary.

Arm-A observables were confirmed red at baseline in round 1 and are unchanged by
this remedy.

**Readiness gate (cold-reader): PASS** (round 2)

Re-gated after the round-1 remedy, with the three round-1 findings promoted as
tracked round-2 input rather than left as journal. All nine classes clear: no
decision gap above `fine`, class 6 clear on both prongs, every class-7 surface
non-blocking, class 8 inert on a never-passed record and clear on the merits
anyway, and class 9 clear on both arms — the three arm-A observables were
independently re-executed and confirmed red at baseline, and no criterion triggers
either arm-B requirement.

The substituted commands were each screened and executed, and each confirmed
authoritative for exactly the set its prose describes: the shape probe excludes the
same-named capability field, the arms command is neither missing a consuming arm nor
including a non-arm, and the capability guard's singular figure equals its command's
output. The narrowed backend claim verifies, and the round-1 overclaim survives
nowhere in force.

Three observations carry no stamp action and are recorded only so a later reader
knows they were considered: the call-site phrasings ("both hosts", "the two
callers") enumerate their members by identity, so nothing rests on the cardinality;
the field list under the "every other field" scope boundary is illustrative under a
universal quantifier; and the Triage-Notes sweep regex under-covers helper-mediated
writes, though its conclusion was independently confirmed by a different route.

The brief is immutable from this stamp.

## Code Review

Dual review (Claude + Codex, cross-verified, adjudicated inline) — `issue-260809-1545-01-code-review-20260815-182641.md`

- M1 (MEDIUM): Collapse-to-undefined path is never exercised from a populated toolToggles — FIXED (round 1)
- M2 (MEDIUM): Tautological distinct-state assertion that exercises no code — FIXED (round 1)
- L1 (LOW): Section-predicate test has a single positive assertion and no negative control — FIXED (round 1)
- L2 (LOW): Presence test now counts an explicit null as configured — dismissed (not reachable frontend state)
- L3 (LOW): Shared component gains a transitive dependency on the global workflow store — deferred
- L4 (LOW): Stringly-typed field write with a type assertion that defeats inference — deferred

Smells: 5 advisory (all appended to `docs/issues/SMELLS-LEDGER.md`; Codex reported none). No graduation rows.

## Resolution

**Commit:** `fix: three-state web search control merging into toolToggles (ISSUE-260809-1545-01)`

**Route:** `cursor` (`/cursor-developer`, composer-2.5), both for the implementation and for the single
fix round. Rationale: a localized frontend change in two Svelte/TS files, with the design decisions
already settled in the brief (three-state select mirroring the reasoning-level control; reuse the
existing `mergeConfig` helper) — no cross-module reasoning, no security surface, no ambiguity that
would justify the codex route.

**TDD:** red-green at the two seams the brief names — the shared agent-config fields component rendered
directly, and the exported `sectionHasValues` predicate as a pure function.

**Review telemetry:** dual review (Claude + Codex, cross-verified, adjudicated inline) —
`issue-260809-1545-01-code-review-20260815-182641.md`. 6 findings: 0 CRITICAL, 0 HIGH, 2 MEDIUM,
4 LOW. Outcomes: 3 FIXED, 2 deferred, 1 dismissed. 1 fix round used of a cap of 4. The two MEDIUM
findings were mutual (both reviewers independently found the same defect at the same line, then AGREEd
on each other's write-up); the four Claude-only LOW findings all drew DISAGREE from Codex and were
adjudicated by reading the code — one kept, one dismissed, two deferred. Both reviewers reproduced the
round-1 red-proofs themselves via mutation testing rather than trusting the fix agent's claim.
Advisory smells: 5, all appended to `docs/issues/SMELLS-LEDGER.md` (Codex reported none); no
in-diff duplication promotions, no graduation rows.

**Note on the deferrals:** L3 (the shared component's new transitive dependency on the workflow store
via `mergeConfig.ts`) and L4 (the stringly-typed `field: string, value: unknown` signature of the shared
merge helper) are both real, both LOW, and both fixable only by restructuring the helper the brief
mandates reusing — which is the `runAs` precedent path this issue places out of scope. They are carried
in the smells ledger.

**Suite:** `SUITE: PASS` — `just test` + `just typecheck`: Rust 466 passed / 0 failed (4 binaries),
frontend 129 passed / 0 failed (13 files), svelte-check 0 errors / 0 warnings. Acceptance criterion 9
(`cargo test` passes) is genuinely green: two reviewers had seen tmux-driven Rust tests fail in their
sandboxes, and the authoritative run confirms those were sandbox artifacts — `tmux` is available on this
host and every one of those tests passes.

**Closed:** 2026-08-15 (UTC)
