---
id: ISSUE-260826-0520-01
kind: issue
category: bug
status: done
origin: docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md
summary: /api/capabilities hardcodes supportedNodeTypes and supportedEdgeOutcomes as string literals instead of deriving them from NodeKind and WorkflowEdgeOutcome, so the endpoint can drift from the enums silently
claimed_by: implement-issue@Mac-mini-4
claimed_at: 2026-08-31T00:58:21Z
---

## Agent Brief

**Category:** bug
**Summary:** Derive `/api/capabilities`'s node-kind and edge-outcome lists from their enums so adding a variant cannot leave the endpoint, its test, or any frontend fallback silently guessing the tag set.

**Current behavior:**
The capabilities handler publishes `supportedNodeTypes` and `supportedEdgeOutcomes` as literal
string arrays typed out by hand. Neither derives from the enum it mirrors — the node-type enum
(which carries `rename_all = "snake_case"` and a hand-written exhaustive `as_str()` projection) or
the edge-outcome enum (same derive, no string projection). Both arrays are correct as published
today; the defect is that nothing holds them correct — and the two enums are not in the same
position, which matters for how the fix is judged.

The node-type enum is matched exhaustively, with no wildcard arm, in more than one place; locate
them with `rg -n 'WorkflowNodeType::Task =>' src/model.rs`. Adding a variant therefore does not
compile clean — the build objects at each of those sites. What it does not do is point anyone at the
capabilities handler, which keeps compiling with a list that is now short. The developer is told to
edit somewhere, and the place they are told about is not the place that went stale.

The edge-outcome enum has no such projection at all — `rg -n 'WorkflowEdgeOutcome::Success =>' src/`
returns nothing — so adding a variant there compiles clean everywhere, and nothing objects at build
time, test time, or lint time.

The same tag lists are transcribed by hand in more than one other place. Find the sites with:

```
rg -n 'supportedNodeTypes|supportedEdgeOutcomes' src/ ui/src/ tests/
```

Hits fall into three groups: the handler that publishes the arrays, the frontend type declaring the
response shape (a type, not a transcription — leave it alone), and the places that write a tag list
out by hand. Three of the last group matter to this issue:

- **The endpoint's own integration test** asserts the response array against a tag list written out
  again inside the test file. Literal compared against literal: the assertion passes whenever the
  two hand-written lists agree with each other, and says nothing about whether either agrees with
  the enum. A developer adding a variant sees this test go red only if they also edited the
  handler, and the obvious repair is to paste the new tag into the test. It reads as coverage and
  provides none. The edge-outcome array is not asserted there at all.
- **The graph editor** derives its node palette from the fetched capabilities with a hand-written
  fallback list, and that fallback is *already* a strict subset of the real kind set. The
  capabilities value reaches the component as a prop that is undefined until the fetch resolves, so
  the fallback is not a failure-only path: it renders on every mount before the response lands.
  Each palette entry is gated on membership in that list, so the toolbar offers the fallback's
  kinds and silently omits the rest — presenting a short list as if it were the whole set, which is
  a worse failure than an empty palette because nothing about it looks wrong.
- **The edge inspector** carries the same construction for edge outcomes: a hand-written fallback
  list behind the same undefined-until-resolved prop, feeding the control that picks an edge's
  outcome. Unlike the palette's, this list is the complete current set, so it misrepresents nothing
  today — it is a latent instance of the identical defect, and it becomes a proper subset as soon as
  a sixth outcome exists.

**Desired behavior:**
The endpoint's two arrays are computed from their enums rather than transcribed, so that adding a
variant to either enum changes the published response with no edit to the handler. The published
payload is unchanged by this work: same tags, same order, same JSON shape — this is a derivation
change, not a wire change.

The endpoint's test asserts the response against tags obtained from the enums themselves, so that
adding a variant without extending the derivation fails the suite. An assertion comparing the
response to a tag list transcribed inside the test does not satisfy this, because both sides can
drift together. Edge outcomes are covered on the same terms as node kinds.

Neither frontend site carries a hand-written tag list. Until capabilities resolve, neither the node
palette nor the edge-outcome control may present a guessed list as though it were complete: each
renders an explicit pending or unavailable affordance instead. An empty palette that resolves to the
full set is acceptable; a short list that looks complete is the defect being removed. The edge
inspector is included even though its list is currently complete — the construction, not the current
divergence, is what this issue removes.

**Key interfaces:**
- The node-type enum and the edge-outcome enum in the model module — both serde-derived with
  snake_case tags. The node-type enum's exhaustive `as_str()` match is a compile-time obligation
  already (no wildcard arm), so it fails the build when a variant is added; the edge-outcome enum
  has no equivalent projection and needs one, or needs to reach its tags another way.
- **Reuse the in-repo harvesting technique rather than inventing a parallel one.** The docs catalog
  generator already extracts a serde enum's wire tags by driving its `Deserialize` impl with a
  custom `Deserializer` that captures the `variants` slice serde's derive generates, then aborts.
  That slice is produced by the derive macro, so it is exhaustive by construction — it needs no
  hand-maintained list and cannot fall behind a new variant. Locate it with
  `rg -n 'deserialize_enum' tests/`. Prefer this over adding an `ALL` constant, which would itself
  be a hand-maintained list with the same drift defect one level down.
- **Binding constraint — no new dependencies.** The parent roadmap fixes zero new runtime
  dependencies for this initiative; the existing serde/tokio/serde_json/rusqlite set suffices. A
  derive-macro crate for enum iteration (strum or equivalent) is therefore not available, in either
  the runtime or the dev-dependency position. The harvesting technique above is dependency-free and
  is the reason this constraint costs nothing.
- The capabilities handler in the API module; the editor component that derives the node palette from
  the fetched capabilities; and the edge inspector component that derives its outcome list the same
  way. The frontend type describing the capabilities response is a declaration, not a transcription,
  and is not the target of this work.

**Acceptance criteria:**
- [ ] The capabilities handler contains no transcribed node-kind tag list. Observable:
      `rg -n '"parallel_batch"' src/api.rs` returns no matches; it returns a match before this
      change.
- [ ] The capabilities handler contains no transcribed edge-outcome tag list. Observable:
      `rg -n '"loop_continue"' src/api.rs` returns no matches; it returns a match before this
      change.
- [ ] No test asserts the capabilities response against a hand-transcribed tag list. Observable:
      `rg -n '"parallel_batch"' tests/` returns no matches; it returns a match before this change.
- [ ] A test asserts both published arrays against tags obtained from the enums' own serde
      derive — set-equal and in the same order — so that adding a variant to either enum without
      extending the derivation fails the suite. The tags on the expected side must be harvested from
      the enum, never written out in the test.
- [ ] The published response is unchanged by this work: the two arrays carry the same tags in the
      same order as before, and no other key of the payload changes. (Preservation criterion: green
      before and after.)
- [ ] The editor carries no hand-written node-kind fallback list. Observable:
      `rg -n 'supportedNodeTypes.*\?\?' ui/src/features/editor/GraphEditor.svelte` returns no
      matches; it returns a match before this change.
- [ ] The edge inspector carries no hand-written edge-outcome fallback list. Observable:
      `rg -n 'supportedEdgeOutcomes.*\?\?' ui/src/features/editor/EdgeInspector.svelte` returns no
      matches; it returns a match before this change.
- [ ] Before capabilities resolve, neither frontend site offers a guessed tag list presented as the
      complete set. Observable at the component seam: render each component with the capabilities
      prop undefined and assert both that no tag-derived control is offered *and* that the pending or
      unavailable affordance is present — the positive half matters, because an absence-only
      assertion is also satisfied by a render that fails silently. Both fallback lists are non-empty
      before this change, so both halves of this criterion are red at baseline; the observables in the
      two criteria above locate the lists, and their contents should be read from the source rather
      than from this brief.

      **The two sites are not symmetric in test cost, and only one seam exists.** The edge inspector
      is already reachable from the existing inspector-panel test suite, which renders it and queries
      its outcome control by role; that suite passes capabilities defined everywhere today, so adding
      an undefined-capabilities case is additive. The node palette has never been render-tested: it
      mounts a graph library whose components construct a `ResizeObserver`, which jsdom does not
      implement and the repository's vitest setup file does not currently shim. Building that seam is
      in scope and is expected to need a shim alongside the existing one in that setup file. If it
      proves disproportionate, assert the palette half at whatever seam does exist and say so in the
      change rather than dropping the assertion silently.
- [ ] `just test` passes.

**Out of scope:**
- **The rest of the capabilities payload.** The `features` map, the per-agent entries, and the
  workflow-version field are all hand-maintained too, and the `features` map in particular names
  capabilities as booleans with no tie to anything. Treat that divergence set as underived rather
  than enumerated here. This issue pins the two tag arrays only.
- **Adding variants to either enum.** The `failure` edge outcome and the `script` node kind are
  owned by later epics of the parent roadmap. This issue makes those additions safe; it does not
  make them.
- **The documentation copies of these lists.** `docs/api-reference.md`'s capabilities example was
  corrected by `ISSUE-260826-0637-02`; `ARCHITECTURE.md` and `docs/backend.md` are owned by
  `ISSUE-260826-0240-01`. This issue touches no file under `docs/`.
- **The test fixture that hand-writes both tag lists.** The discovery command surfaces a fourth
  transcription site: a frontend test fixture supplying a capabilities object with both lists written
  out. Leave it. It is a fixture feeding a render, not a source of truth any user reaches, and
  pinning it to the enums would couple a frontend unit test to the Rust build for no gain. It is
  named here so that a reader running the command knows it was considered rather than missed.
- **Rejecting unknown fields anywhere in the model.** Adjacent hardening, separate decision.
- Redesigning the editor toolbar, the palette's grouping, or which kinds are offered once
  capabilities resolve. The only change to the resolved-state palette is that it stops being
  reachable from a guessed list.

## Context Pack — generated at claim (2026-08-31T00:58:21Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01, parent via `origin:`):
- Enumeration is closed by harvesting wire tags from serde's own derives, not from hand-maintained lists or source-text scans — "coverage enforced against serde's own derives, with zero new dependencies."
- The harvest technique for a plain `rename_all` enum: supply a throwaway `Deserializer` that captures the `&'static [&'static str]` serde's derive passes to `deserialize_enum`, then error out without a visitor. Exhaustive by construction.
- `strum::EnumIter` was explicitly rejected: it resolves only from `[dependencies]` (a runtime dep the roadmap forbids) and requires editing `src/model.rs`.
- Hand-written `ALL`/`CATALOG_ORDER`-style constants are only acceptable when wrapped in set-equality plus length checks against a derive harvest — a bare constant is the same drift defect one level down.
- An exhaustive `match` proves every *input* is handled but does not produce one value per variant, so it cannot by itself guarantee a set is complete (two earlier drafts were falsified on exactly this).
- PRD scope forbids engine changes and edits under `src/`; this issue is the complement — it is the code-side pin the epic deliberately could not make.

**Test seam & Testing Decisions:** observable at the endpoint's integration test under `tests/` (the capabilities response assertion) plus the frontend component render seam (existing inspector-panel suite for the edge inspector; the node palette has no render seam and needs a `ResizeObserver` shim in the repo's vitest setup, alongside the existing one). Testing Decisions that touch it: check 2 — emitted wire names must *equal* the serde-harvested variant set, catching both omission and duplication; the expected side is always harvested, never transcribed, or both sides drift together. Harvesters live in an integration-test crate, so the technique is copied/extracted, not imported. Note carried from the readiness gate: no `tests/` probe exists for edge-outcome tags, so the "harvested, not written out" criterion is the only instrument covering that half — and edge outcomes have no compile-time backstop.

**ADRs:**
- ADR-260815-2009-02 — One condition dialect: owned nested AST, typed operators, onMissing · accepted (adds the sixth edge outcome `failure`)
- ADR-260815-2009-03 — Script input indirection: no templating into source, exit-code-only success · accepted (adds the fifteenth node kind `script`)
- Neighbor: ADR-260815-2009-01 — Typed workflow contracts over a JSON-valued variable store · accepted (`typed-contracts` lands the `script` variant in the v5 grammar earlier still)

**Terms:**
- `Node Kind` — the fourteen-variant tagged union in a node's required `kind` object; the bare `type` tag alone is what `/api/capabilities` publishes as `supportedNodeTypes`.
- `Edge Outcome` — the channel an edge is traversed on: `success | reject | branch | loop_continue | loop_exit`, the complete set; there is no failure channel today. Decisions: ADR-260815-2009-02 adds a sixth.
- `Runner Kind` / `Immediate Kind` — dispatch partitions over Node Kind; both are separate hand-maintained lists, out of scope here.

**Constraint (binding, from RDMP-260815-2009-01):** zero new dependencies, runtime *or* dev — the existing serde/tokio/serde_json/rusqlite set suffices; the derive-harvest technique is dependency-free and is why this constraint costs nothing.

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md (§ Implementation Decisions, § Testing Decisions) · docs/roadmap/RDMP-260815-2009-01-harness-workflows.md · docs/adr/INDEX.md · CONTEXT.md

## Code Review

Review file: `issue-260826-0520-01-code-review-20260831-021621.md`

Dual review (Claude + Codex), cross-verified and adjudicated inline. 15 raw findings from the two
reviewers reduced to 8 after merging two cross-reviewer duplicates and dropping four on adjudication;
one further finding (L5) was raised on fix-round-1 re-verification. **ACCEPTED** — all 9 terminal.

- H1 (HIGH): Capabilities drift test uses the production generator as its own expected value — FIXED
- H2 (HIGH): The shipped `public/` bundle still contains the deleted fallback — FIXED
- M1 (MEDIUM): A failed capabilities fetch renders "Loading…" forever and leaves the editor unusable — FIXED
- M2 (MEDIUM): New GraphEditor test exceeds Vitest's default timeout and carries no explicit budget — FIXED
- L1 (LOW): Toolbar block indentation not updated for the new guard — FIXED
- L2 (LOW): Per-request allocation for a value that is already `&'static` — FIXED
- L3 (LOW): `<label class="field">` wraps no form control in the pending branch — FIXED
- L4 (LOW): `aria-live="polite"` is inert on both edges of the transition — FIXED
- L5 (LOW): Fix round 1 dropped the edge seam's pending-state coverage — FIXED (same-site escalation: redesign round)

Dropped on adjudication (reasoning in the review file): CLAUDE-5, CLAUDE-7, CLAUDE-10, CLAUDE-11.

Fix rounds used: 2 of 4. Round 1 routed to codex (8 findings, all fixed); round 2 routed to cursor as a
redesign brief under the same-site escalation rule (L5 landed in the exact region M1's fix had rewritten).

Both reviewers verified every fix independently. Notable: the L5 assertions were mutation-tested, and the
round-1 re-verification **corrected the record on M2** — the ~8.5s timings behind CLAUDE-3/CODEX-3 were
almost entirely machine load (load average ~620 on 8 cores at review time); the test measures 329-364 ms
on an unloaded machine. The explicit timeout was kept as a guard regardless.

Recorded but not filed as findings: no test exercises the `AppShell` capabilities-error wiring itself —
both seam tests pass `capabilitiesError` in directly, so a wiring-level regression would be caught only by
the typechecker.

Smells: 4 advisory, none promoted (duplication-promotion rule applied; no smell had every cited site
inside the issue diff). Graduation advisory: no rows emitted.

## Triage Notes

Found on 2026-08-26 during the glossary session for PRD-260826-0009-01 (`docs-truth` epic of
RDMP-260815-2009-01), while grounding the **Node Kind** and **Edge Outcome** terms against `src/`.
Filed rather than folded into that PRD: `docs-truth` is scoped to make no engine changes and edit
nothing under `src/` (PRD § Dimension Scan, architecture shape), and this needs a code change.

**The defect.** `src/api.rs:216-217` publishes both lists as literal string arrays:

```rust
"supportedNodeTypes": ["task", "approval", "split", "collector", "decide", "parallel_batch", "subflow", "call", "spawn", "send", "wait", "capture", "kill", "run_agent"],
"supportedEdgeOutcomes": ["success", "reject", "branch", "loop_continue", "loop_exit"],
```

Neither is derived from its enum. `WorkflowNodeType` (`src/model.rs:159-174`) already carries
`#[serde(rename_all = "snake_case")]` and an `as_str()` (`:176`), and `WorkflowEdgeOutcome`
(`src/model.rs:215-221`) carries the same derive. Adding a variant to either enum compiles clean
and leaves the endpoint stale — the same failure class as the documentation drift the parent epic
exists to delete, in the one surface that is supposed to be the machine-readable truth.

Both lists happen to be **correct today**. This is a missing pin, not a live falsehood.

**Why it will bite on a known schedule.** `condition-ast` (RDMP-260815-2009-01) adds a sixth edge
outcome, `failure` (ADR-260815-2009-02), and `script-node` adds a fifteenth node kind
(ADR-260815-2009-03). Both epics have to remember to edit a string literal in `src/api.rs` that
nothing points them at. `typed-contracts` lands the `script` variant in the v5 grammar earlier
still, widening the window in which the enum and the endpoint disagree.

**Also in scope for whoever takes this.** `ui/src/features/editor/GraphEditor.svelte:240-241`:

```js
const supportedNodeTypes = $derived(
  capabilities?.supportedNodeTypes ?? ["task", "approval", "split", "collector"],
);
```

A third hardcoded copy, and this one is already the stale four-kind list — so if the capabilities
fetch fails or has not resolved, the editor's palette silently offers 4 of 14 node kinds rather
than failing visibly. That is a worse outcome than an empty palette, because it looks like a
complete list.

**Relation to the parent epic.** PRD-260826-0009-01 fixes `docs/api-reference.md:24`, which
advertises a four-kind list where the endpoint returns fourteen (audit item X2). That corrects the
*document* against the endpoint. This issue is the other half: nothing pins the *endpoint* against
the enums. The epic's success criterion 3 — a change to the set of node kinds fails CI rather than
shipping — is served by the generated doc catalog and its set-equality check, and by nothing at all
for `/api/capabilities`.

**Suggested shape, for triage to accept or replace.** Derive both arrays from the enums (a
`WorkflowNodeType::ALL` / `WorkflowEdgeOutcome::ALL` slice plus the existing `as_str()`), or, if the
literals are wanted for wire stability, add a test asserting set-equality between each literal and
its enum's serde tags — the same technique PRD-260826-0009-01 adopts for the node catalog in
`tests/docs_catalog.rs`. The frontend fallback wants deleting or narrowing to an explicit
loading/error state rather than a silent short list.

**Evidence base.** `docs/sources/workflow-schema-drift-260825.md` item X2 covers the doc-side half;
the endpoint-side gap is not in that audit and was found separately.

**Readiness gate (cold-reader): PASS** (round 1)

Independent cold reader, no planning context, 2026-08-30. Both discovery commands and all four
executable acceptance observables run against the tree; classes 1–7 and 9 walked, none fired. Class
8 inert (never-stamped record).

Findings worth recording:

- **The derivation is achievable and order-preserving.** The reader compared both enums' declaration
  order against the published arrays mechanically and found them identical, and confirmed serde emits
  variants in declaration order — so deriving cannot reorder the payload. It also confirmed
  `as_str()` has no wildcard arm, making it a compile-time obligation as the brief claims.
- **The harvester locator lands unambiguously.** `rg -n 'deserialize_enum' tests/` returns exactly
  two lines, both inside the node-type harvest function, and does *not* surface the second unrelated
  harvester — so a cold reader cannot pick the wrong one. Both target enums derive `Deserialize`, so
  the technique transfers to the edge-outcome enum unchanged, and the harvester imports only from
  `serde::de`, confirming it is dependency-free.
- **Class 6(b) was the closest call in the gate** and was argued rather than waved through: the
  backend and frontend criteria are each technically demoable alone, but neither is sufficient —
  backend-only leaves the pre-fetch palette still misrepresenting completeness, frontend-only leaves
  nothing correct to propagate. The unifying demo is that a locally-added variant reaches the palette
  with no hand edit.
- **Ignore-blindness was cross-checked rather than assumed**: the ignore-respecting and `-uu --hidden`
  forms of the discovery command return identical results over the searched paths.
- All four absence-shaped observables return hits on the current tree, so each can go red→green.

Three non-blocking notes, all verdict `fine`. Two are acted on in round 2 below; the third — that the
harvester lives in a separate integration-test crate and so must be copied or extracted rather than
imported — needs no edit, since the brief already specifies reusing the *technique*.

**Readiness gate (cold-reader): REOPENED** (round 2, 2026-08-30, acting on round-1 notes)

1. **The discovery command did not surface the site the prose attributed to it.** The brief said the
   editor's fallback was among the command's hits; it is not — that fallback contains neither search
   term. The prose located the site behaviorally and the acceptance criterion carried the exact
   regex, so nothing was misdirected, but a command that does not derive the set the text claims for
   it is exactly the class-7(a) shape and is being corrected rather than left standing. Replacing it
   with a command that surfaces every transcription site.

2. **A third fallback was not named.** The command surfaces a second frontend fallback — the edge
   inspector's outcome list — carrying the identical defect one enum over. It is not a live subset
   today (it is the complete current set), but it becomes a proper subset the moment the `failure`
   outcome lands, which is precisely the schedule this record cites as the reason to act now.
   **Judgment: in scope, not excluded.** Leaving it would have this issue reproduce its own defect on
   the timetable it was filed to pre-empt, and the fix is the same deletion one file over. Adding an
   acceptance criterion for it and widening the desired-behavior statement to both fallbacks.

Also tightening the pre-resolution palette criterion, which was stated as a pure absence and so was
vacuously satisfiable by a render that failed silently; it now requires the positive affordance the
desired-behavior paragraph already called for.

**Readiness gate (cold-reader): FAIL** (round 3, full-enumeration)

Independent cold reader, no planning context, 2026-08-30. Class 7 fired with two blocking surfaces
in brief text under edit; classes 1, 2, 3, 5, 6, 9 did not fire; class 4 carries one bounded
delegation. All three round-2 edits were assessed individually and all three achieved their stated
purpose — edit 3 also introduced one of the two blocking surfaces.

**Blocking — class 7(a), decision-needed.** Current behavior claimed that adding a variant to
*either* enum "compiles clean … with no test, lint, or build step objecting." That is false for the
node-type enum, which is matched exhaustively with no wildcard arm in more than one place, so the
build does object. Worse, the brief contradicted itself: its own Key interfaces bullet stated the
opposite. This was the record's motivating premise and it was half wrong. The accurate framing was
already sitting in the Triage Notes — nothing *points* a developer at the handler — and the brief
had flattened it into something stronger and untrue.

**Blocking — class 7(a), decision-needed.** The pre-resolution criterion asserted bare counts of the
controls each component renders today, with no deriving command, while every other criterion in the
record establishes its red state with an executable observable. The counts were correct but rested
on an unexpressed structural fact (the palette's overflow group is gated on a list disjoint from the
fallback), which no command in the record derives. The reader noted that round 2 had applied exactly
this standard to a strictly weaker case, and that consistency required firing here. That is right.

**Delegable — class 4.** The criterion said "render *each* component", but only one of the two
frontend sites has an existing render seam; the other has never been render-tested and mounts a
library needing a jsdom shim the repo's test setup does not yet provide. Bounded by an existing
precedent in that setup file, but the brief has to say so rather than let the implementer discover
the asymmetry.

**Delegable — internal inconsistency.** The Summary promised that a new variant "cannot leave … the
editor palette silently stale." The scope does not deliver that and cannot: the palette's kinds come
from hardcoded per-kind guards plus a hand-written primitive list which `supportedNodeTypes` only
*gates*, and Out of scope explicitly forecloses touching them. Round 1's class-6(b) reasoning had
leaned on this same overclaim; round 3 re-judged 6(b) from scratch, found round 1's unifying demo
unachievable, and replaced it with one that holds — add a variant and exactly one thing goes red,
the capabilities test, with no site left guessing. Under that framing the round-2 widening tightens
the slice rather than tipping it.

**Note — class 7, fine.** The discovery command surfaces a fourth transcription site, a test fixture
that hand-writes both full tag lists, which the brief adjudicates neither way.

Remedies applied below; re-gated as round 4. No `REOPENED` stamp is owed — the authoritative verdict
at the time of edit is `FAIL`, so the brief was already ungated.

**Readiness gate (cold-reader): PASS** (round 4, full-enumeration)

Independent cold reader, no planning context, 2026-08-30. Full gate, not a diff review. All five
round-3 remedies assessed for relocation rather than repair; all five fixed. Classes 1, 2, 3, 5, 6,
7 and 9 do not fire. Class 4 carries one bounded delegation. Fourteen class-7 surfaces swept, all
`fine`.

- **The edge-outcome half of remedy 1 was verified independently of the brief's own command**, which
  is the check that matters: repo-wide ignore-blind sweeps for every spelling an exhaustive match
  could take — grouped arms, `Self::`-spelled arms, glob imports — all return nothing. No projection
  over that enum exists anywhere in `src/` or `tests/`. Both halves of the split claim are true.
- **Remedy 2 was checked for being a count in prose clothing** and is not: "non-empty" is a
  cardinality predicate stable under variant addition, where the deleted counts decayed on exactly
  that event. The reader also confirmed the half that non-emptiness does *not* establish — that no
  pending affordance exists in either component today — so both halves of that criterion are red at
  baseline.
- **No decaying figure was refreshed to a corrected number**, which is the remedy self-check's
  standing trap. The round-3 counts were deleted, not restated.
- **Class 6(b) was argued from scratch a third time**, inheriting neither round 1's nor round 3's
  framing, and the reader falsified round 3's stamped "exactly one thing goes red" — two further
  exhaustive matches over the node-type enum live in the docs-catalog test. That is a defect in
  round 3's stamp reasoning, not in the brief, and is recorded here rather than silently dropped.

Eight non-blocking notes, all `fine`. Two were considered for a further round and **declined with
reason**, recorded here so the finding survives for whoever claims this:

- The reader observed that the `tests/` probe covers node kinds only, so a post-change test that
  harvests node-kind tags but hand-writes the edge-outcome tags would satisfy every mechanical
  observable, and suggested a matching probe for an outcome tag. **That probe is not available:** the
  outcome tags do not appear anywhere under `tests/` today, so an absence criterion naming one would
  already be satisfied at baseline and would fire class 9 arm A as non-falsifying. The mechanical net
  is therefore asymmetric by construction, and the acceptance criterion requiring the expected side
  to be harvested rather than written out is the only instrument covering the outcome half. Worth
  knowing when reviewing the change, since edge outcomes are also the enum with no compile-time
  backstop.
- The frontend type module hand-transcribes both tag sets as TypeScript string unions. Left
  unadjudicated in the brief. They cannot be derived from the Rust enums without codegen, which is
  well outside this issue, and the brief's existing exclusion of the response-shape type covers the
  module. Naming them would cost a reopen and re-gate cycle for an exclusion that changes no
  instruction.

Brief is immutable from this stamp. Promoting to `ready-for-agent`.

## Resolution

**Commit:** `fix: derive /api/capabilities tag lists from their enums (ISSUE-260826-0520-01)`

**Route:** `cursor` for the implementation — a well-scoped multi-file change with the derivation
technique already present in-repo to copy, no architectural ambiguity. Fix round 1 routed to `codex`
(two HIGH-severity findings; severity is itself a routing risk signal), fix round 2 back to `cursor`
(one LOW finding in a single frontend test file).

**TDD:** red-green at the component render seam (`ui/src/features/editor/` — the edge inspector via the
existing inspector-panel suite; the node palette needed a new render seam, which required `ResizeObserver`
and `matchMedia` shims alongside the existing `<dialog>` shim in the vitest setup file) and at the
capabilities integration seam under `tests/`.

**What landed.** `src/serde_wire_tags.rs` harvests a serde enum's wire tags by driving its `Deserialize`
impl with a throwaway `Deserializer` that captures the `&'static [&'static str]` the derive passes to
`deserialize_enum` — the technique the docs-catalog generator already used, dependency-free and exhaustive
by construction. The capabilities handler publishes both arrays from that harvest instead of literals. The
endpoint's integration test harvests its expected side independently, via its own test-local probe rather
than the production helpers. Both frontend fallback lists are gone, replaced by explicit pending and
unavailable affordances. The embedded `public/` bundle was rebuilt so the served application matches.

**Review telemetry.** Dual review (Claude + Codex), cross-verified and adjudicated inline. 15 raw findings
→ 8 kept after merging two cross-reviewer duplicates and dropping four on adjudication; one more (L5)
surfaced during fix-round-1 re-verification, for 9 total. By severity: 2 HIGH, 2 MEDIUM, 5 LOW. Outcomes:
**9 FIXED, 0 deferred, 0 dismissed.** Fix rounds used: **2 of 4**. Round 2 was dispatched as a redesign
brief under the same-site escalation rule, since L5 landed in the exact region round 1's M1 fix had
rewritten; both reviewers judged the named mechanism removed rather than patched around.

**Suite:** `just test` — **PASS**, 604 passed / 0 failed / 1 skipped (~38s). Rust 473 across 5 binaries
including 1 ignored; vitest 131 across 14 files. No pre-existing failures.

**Three things worth carrying forward.**

1. The `public/` bundle was **already stale at HEAD** before this work — commit `72c6e24` landed four
   `ui/src/` files with no rebuild, and `public/` was last built at `a831cfa`. Rebuilding therefore also
   swept unrelated `AgentConfigFields`/`sectionUtils` deltas into this commit. That is the repository's
   build-artifact convention being honoured, not scope creep, but the sweep is real and is recorded here
   rather than left to be discovered in the diff. Separately: this clone's `core.hooksPath` is unset, so
   `.githooks/pre-commit` never fired — the freshness hook that would have caught this is not active until
   someone runs `just setup`.
2. The review **corrected its own record on M2.** Two reviewers independently measured the new GraphEditor
   test at ~8.2-8.5s against a 5s default budget and filed it as a real defect. On re-verification at
   normal load the same test measured 329-364 ms: the original timings were almost entirely machine load
   (average ~620 on 8 cores at review time). The explicit timeout was kept as a guard, but the "SvelteFlow
   is intrinsically expensive to mount" reasoning behind the finding was wrong.
3. **Uncovered seam, noted not filed:** no test exercises the `AppShell` capabilities-error wiring itself —
   both component seam tests pass `capabilitiesError` in directly, so a wiring-level regression there would
   be caught only by the typechecker.

**Closed:** 2026-08-31 (UTC).
