---
id: ISSUE-260826-0637-03
kind: issue
category: enhancement
status: done
summary: Restructure the workflow schema reference into a marker-bearing scaffold with the document grammar written and the false content deleted
prd: PRD-260826-0009-01
adrs: [ADR-260815-2009-01]
terms: [Workflow, Node Kind, Edge Outcome, Condition, Validation Issue]
claimed_by: implement-issue@Mac-mini-4
claimed_at: 2026-08-30T02:40:24Z
---

## Agent Brief

**Category:** enhancement
**Summary:** Replace `docs/workflow-schema.md`'s structure with the sectioning the rest of the epic
fills, write the document-grammar prose that opens it, and delete the content that is false.

**Current behavior:**
`docs/workflow-schema.md` describes a schema the engine does not accept. Its node examples use
the v2/v3 flat shape — a `type` string on the node with config keys flat beside it — where the
engine requires a nested, internally-tagged `kind` object. Its edge table documents `source` and
`target`, which are not fields of the edge struct and have no serde aliases, and omits `id`,
which is required. It presents `loopCondition` as an English prompt for an LLM, where the engine
requires a structured condition object it evaluates itself. Its section order leads with node
types, so a reader meets the wrong node shape before meeting the document grammar at all.

The file contains no generated-content markers. The catalog generator that follows this issue
rewrites only the span between `<!-- BEGIN GENERATED: <block-id> -->` and
`<!-- END GENERATED: <block-id> -->` markers, so until they exist it has no legal place to write.
That is why this issue comes first.

**Desired behavior:**
The document opens by teaching the grammar of a workflow document, then presents sections that
later issues fill. Concretely:

*Grammar first.* Before any node appears, the reader learns the nested `kind` shape, that
`version` and `entryNodeId` are the only genuinely required top-level keys, and that unknown keys
are accepted silently — no struct in the model denies them, so a clean save is not evidence a
field name was correct.

*The version and migration contract*, as prose rather than a table, because it is a behavioural
contract a table cannot carry: a missing `version` is a hard ingest error and not a default;
version rejection happens at ingest and is never reported as a validation issue, so it does not
reach the channel a reader is told to expect errors on; the version is force-rewritten to the
canonical value on every path that reaches validation, so validation can never reject one — find
those rewrite sites yourself rather than taking a count from here; subflows are migrated
recursively, so an ordinary superseded subflow version migrates forward with its root rather than
failing it, and only a subflow whose declared version the engine does not accept at all rejects the
document — derive which versions those are from the ingest path, not from this brief; and the
flat-to-nested `kind` migration runs on any declared version, not only the legacy ones. `CONTEXT.md`'s **Workflow**
entry states the canonical version, and its `Decisions:` clause cites the ADR that bumps it —
this document describes the present and cites that ADR as a forward reference, never as present
tense.

*Authority and regeneration.* The document states that `src/model.rs` is the authority, which of
its content is generated, and the command that regenerates it. The regeneration command does not
exist until the next issue lands; write this section so that it names the command as the contract
and the next issue supplies it.

*The false content is deleted, not annotated.* Every flat-shape node example, the `source`/
`target` edge table, and the prompt-string reading of `loopCondition` go. So do hand-written
enumerations that no later issue in this epic will regenerate, cross-reference or narrate, and
narrative claims about subsystems this document does not own. An unowned list is how the document
reached its current state, and leaving one behind reproduces the failure.

*The sections later issues fill* exist with a one-line placeholder each, and each placeholder
names `src/model.rs` as the authority for that content pending its table. An empty section that
says "see the source" is honest; one that says nothing is a hole.

**Key interfaces:**
- **The section structure is the contract with the rest of the epic.** Downstream issues anchor
  their acceptance criteria to these exact second-level headings, so this set is specification,
  not preference. Renaming one requires editing the issue that anchors to it.

  `## Document grammar` · `## Node catalog` · `## Node fields` · `## Edges and conditions` ·
  `## Document-level fields` · `## Templates and agent config` · `## Validation catalog` ·
  `## Regenerating this document`

  The version and migration contract is a subsection of `## Document grammar`.
- **Marker grammar:** `<!-- BEGIN GENERATED: <block-id> -->` … `<!-- END GENERATED: <block-id> -->`,
  HTML comments so they are invisible in rendered markdown. At least one pair sits inside
  `## Node catalog`. Choosing the block-id scheme is this issue's call, but the generator issue
  inherits it, so pick something that extends to fourteen kinds with two blocks each.
- `WorkflowV3` and `WorkflowNode` in the model are the shapes the grammar section describes.
  Cite source locations for each claim; do not restate line numbers from this brief.
- `docs/sources/workflow-schema-drift-260825.md` is the work order. This issue owns Part 1 items
  1-2 (the nested `kind` shape and `agentConfig`'s placement), items 36 and 39 from §1.5 (the two
  genuinely required top-level keys; unknown keys accepted silently), all of §1.10 (the version and
  migration contract), and §1.12 (the narrative claims to delete). The rest of §1.1 and §1.5 are
  ISSUE-260826-0637-06's — do not write those tables here.

**Acceptance criteria:**
- [ ] All eight second-level headings above are present, in that order. Observable:
      `rg -n '^## (Document grammar|Node catalog|Node fields|Edges and conditions|Document-level fields|Templates and agent config|Validation catalog|Regenerating this document)$' docs/workflow-schema.md`
      returns eight lines; none before this change.
- [ ] Generated-block marker pairs exist inside `## Node catalog`, opener and closer matched by
      block-id, and enough of them for the catalog's shape (two blocks per kind, so either one pair
      per block or a documented scheme the generator can address). Observable:
      `rg --pcre2 --multiline -c '(?s)^## Node catalog\n(?:(?!^## ).)*?BEGIN GENERATED' docs/workflow-schema.md`
      is non-zero; no match before this change. Count `BEGIN GENERATED` and `END GENERATED`
      document-wide and confirm they are equal — but note the anchored form is the load-bearing
      one, because `## Regenerating this document` may legitimately mention the marker convention
      in prose.
- [ ] The legacy hand-written node section is gone. Observable: `rg -c '^## Node Types$' docs/workflow-schema.md`
      returns no matches; it matches before this change.
- [ ] Every node example in the file nests its tag inside a `kind` object. Observable:
      `rg -c '"kind"' docs/workflow-schema.md` returns matches; no matches before this change.
      **Do not** write an observable forbidding the string `"type": "task"`. `NodeKind` is
      internally tagged, so that string is what a *correct* v4 node carries inside `kind`, and the
      generated catalog will emit it many times over. The flat shape is distinguished by `type`
      sitting at the node root with config keys flat beside it — a structural property, checked by
      reading each example, not by grepping the tag.
- [ ] The legacy edge section, whose field table names `source`/`target`, is gone. Observable:
      `rg -c '^## Edges$' docs/workflow-schema.md` returns no matches; it matches before this
      change. Anchoring to `## Edges and conditions` would be worthless — that section does not
      exist yet and this issue leaves it a placeholder, so such a check is green at both ends.
- [ ] `## Document grammar` states the nested `kind` shape, the two required top-level keys, and
      silent acceptance of unknown keys, each citing the source location that owns it. Observable:
      `rg --pcre2 --multiline -n '(?s)^## Document grammar\n(?:(?!^## ).)*?entryNodeId' docs/workflow-schema.md`
      matches; no match before this change.
- [ ] The version and migration contract appears as prose under `## Document grammar` and states
      that a missing version is an ingest error rather than a default, and that version rejection
      never surfaces as a validation issue.
- [ ] `## Regenerating this document` names the regeneration command and states which sections are
      generated. Observable: `rg --pcre2 --multiline -n '(?s)^## Regenerating this document\n(?:(?!^## ).)*?just ' docs/workflow-schema.md`
      matches; no match before this change.
- [ ] Every remaining claim in the file is either true of the source at the time of the change or
      sits in a placeholder that defers to `src/model.rs`. Observable at the seam: each surviving
      assertion carries a source citation a reader can follow in one jump.
- [ ] `just check-v4-docs` passes and `cargo test` is green.

**Out of scope:**
- Writing any of the tables the placeholder sections defer — node fields, edges and conditions,
  document-level fields, templates and agent config, and the validation catalog each have their
  own issue.
- Writing or running the generator, and populating the marker blocks. This issue creates the
  markers and leaves the span between them empty.
- **`docs/getting-started.md`.** It carries the only anchored deep link into
  `docs/workflow-schema.md` anywhere in the repo, and it points at a subsection this issue's
  deletion rule removes, so the anchor dies when this lands. Repointing it is deliberately not
  granted here — file it separately. This issue is already the largest scaffold pass in the epic
  and the fix is a one-line link edit in a file the epic otherwise does not touch.
- `docs/execution-model.md`, `docs/api-reference.md` and `docs/README.md`.
- Any change under `src/` or `ui/`. This epic makes no engine changes.
- Describing the v5 grammar as present. v5 is decided in ADR-260815-2009-01 and delivered by the
  `typed-contracts` epic; cite it as a forward reference only.

## Context Pack — generated at claim (2026-08-30T02:40:24Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01):
- `docs/workflow-schema.md` is restructured, not patched: grammar first (nested `kind`, the two genuinely required top-level keys, silent acceptance of unknown keys), then the generated catalog, then Tier P tables, then authority/regeneration.
- The generator emits blocks, not documents: generated content lives only between `<!-- BEGIN GENERATED: <block-id> -->` … `<!-- END GENERATED: <block-id> -->`, and the generator rewrites only that span — so the markers must exist before it can run (Further Notes: sequencing puts this scaffold first).
- Content is tiered by who can maintain it correctly: Tier G generated catalog, Tier P hand-written but source-cross-referenced, Tier C conceptual prose, Tier D deleted. Tier D is exactly the unowned hand-written enumerations and the narrative claims about other subsystems.
- The version/migration contract is **Tier C, not Tier P** — prose, because a table cannot carry it: missing `version` is a hard ingest error not a default; version rejection is an ingest `bail!` that never reaches the validation-issue channel; the version is force-rewritten on every path reaching validation; subflows migrate recursively; the flat→nested `kind` migration fires on any declared version.
- Version framing: v4 is canonical, v5 is decided (ADR-260815-2009-01) and unlanded — cite it as a forward reference, never present tense.
- The regeneration command is cited by the docs themselves (a `just` recipe wrapping `SB_REGEN_DOCS=1`); this issue names the contract, the generator issue supplies the command.
- `src/model.rs` is stated as the authority, with which parts are generated and how to regenerate them.
- No engine changes: this epic reads `src/`, writes `docs/`.
- Terminology follows `CONTEXT.md`, whose unmarked entries are true of `src/` at HEAD; `_(planned — ADR-…)_` entries are not accepted by the engine yet.
- `docs/sources/workflow-schema-drift-260825.md` is the work order; this slice owns Part 1 items 1–2, §1.5 items 36 and 39, all of §1.10, and §1.12.

**Test seam & Testing Decisions:** observable at the committed markdown of `docs/workflow-schema.md` — headings, marker pairs and example shape are checked by reading and by the brief's `rg` observables, plus `just check-v4-docs` and `cargo test`. Testing Decisions that touch it: the primary guarantee is generation plus a freshness diff (regenerate, diff, fail on difference), so marker pairs must be addressable and balanced; all of Tier P and Tier C is explicitly **not** machine-checked and rests on per-claim source citations and review — which is why every surviving assertion must carry a citation a reader can follow in one jump; a field's requiredness is the one drift class no check catches.

**ADRs:**
- ADR-260815-2009-01 — Typed workflow contracts over a JSON-valued variable store · accepted (the decided v5 bump; forward reference only)
- ADR-260815-2009-02 — One condition dialect: owned nested AST, typed operators, `onMissing` · accepted (neighbor: cited by the `Condition` and `Edge Outcome` glossary entries this slice's grammar prose touches; also forward-reference only)

**Terms:**
- `Workflow` — versioned node/edge graph definition (schema `version: 4`; v2–v3 documents normalize forward at ingest), including its subflow catalog; `Decisions:` cites ADR-260815-2009-01 (bumps to v5). Avoid: pipeline, flow.
- `Node Kind` — the fourteen-variant tagged union in a node's required `kind` object, selecting both what the node does and which config shape it carries; the bare `type` tag is what `/api/capabilities` publishes. Avoid: step type.
- `Edge Outcome` — the channel an edge is traversed on: `success | reject | branch | loop_continue | loop_exit`, the complete set — no failure channel. Avoid: edge type, transition.
- `Condition` — the engine's single post-execution deterministic branching form (edge and loop conditions): one flat `{field, operator, value}` leaf, evaluated by the engine and never by an LLM; `skipCondition` is a separate pre-execution form. Avoid: expression, rule.
- `Validation Issue` — one entry in a validation result (`{severity, nodeId?, scope?, message}`, severity only ever `error` or `warning`); validation is enforced only at run start, saving never validates, and a version rejection never reaches this channel. Avoid: save-time validation, lint.

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md · docs/adr/INDEX.md · CONTEXT.md · docs/sources/workflow-schema-drift-260825.md

## Code Review

Review file: `issue-260826-0637-03-code-review-20260830-030625.md`

- M1 (MEDIUM): Root `agentConfig` documented as "not accepted" when it is silently ignored — FIXED
- M2 (MEDIUM): Document asserts a save-time validation-issue channel that does not exist — FIXED
- M3 (MEDIUM): Regeneration contract written as an operational command that does not exist — FIXED
- M4 (MEDIUM): Repo-wide `deny_unknown_fields` negative carries no followable citation — FIXED
- M5 (MEDIUM): Version failure modes and their messages are conflated — FIXED
- M6 (MEDIUM): Startup snapshot re-normalization stated as unconditional — FIXED
- L1 (LOW): Unknown-key leniency overstated — FIXED
- L2 (LOW): Catalog described as already listing config shapes — FIXED
- L3 (LOW): Flat→nested migration paragraph omits two silent-data-loss behaviours — FIXED (round 2, same-site redesign)
- L4 (LOW): "Execution and UI fields" — `WorkflowNode` carries no UI fields — FIXED
- L5 (LOW): Fourteen `### <wire-tag>` headings hand-maintained outside every generated span — FIXED
- L6 (LOW): Accepted-version qualifier buried the v4 case (admitted round 1, drift §1.10 item 78) — FIXED

Dismissed on adjudication: CODEX-5, CLAUDE-7, CLAUDE-11, CLAUDE-12 (rationale in the review file).
Fix rounds used: 2 of 4. Round 2 ran under the same-site escalation rule as a paragraph redesign against ten checkable properties; both reviewers returned MECHANISM: GONE, all properties PASS, all round-1 fixes HELD, no new findings.
Smells: 0 (both reviewers returned empty blocks; the one candidate was folded into L5 rather than double-counted).
Two advisory nits recorded in the review file, deliberately not spent as a third round.

## Triage Notes

**Readiness gate (cold-reader): PASS** (round 2)

Round 1 returned findings on two acceptance criteria and the drift-audit work order; all were
fixed before round 2. Round 2 ran the full nine-class rubric, executed every observable, built
both a positive fixture and a hostile fixture to test whether the criteria can be gamed, and
confirmed the eight pinned section headings match what every sibling record anchors to.

**Readiness gate (cold-reader): REOPENED** (round 3, 2026-08-26, breakdown-adversary finding)

The round-9 breakdown adversary found that the version-and-migration prose would have had the
implementer write a new falsehood: it said a canonical root with a stale subflow is rejected, when
the engine migrates catalogued subflows forward recursively and rejects only versions it does not
accept. Corrected to a discovery instruction rather than a restated version set.

**Readiness gate (cold-reader): FAIL** (round 4, full-enumeration) — class 7, unverified factual
premise, kind (a) rule-violation figure.

The promoted round-9 finding was **discharged**: the rewritten subflow-recursion sentence is true
of the ingest path, its discovery instruction is executable, and it introduces no class-7 surface.
All four previously unexamined claims in the same paragraph verified true.

The blocking surface is adjacent and pre-existing: the version-and-migration paragraph asserts the
version is force-rewritten "in two places" — a bare occurrence count of code sites with no
discovery command, which is the banned form. The count is currently true; this is an authoring-rule
violation, not a falsehood. The remedy is to substitute a command or drop the count, never to
refresh it. `PRD-260826-0009-01` carries the identical figure and is owed the same fix; no sibling
issue record carries it, so no sibling re-gate is owed.

Non-blocking (`delegable`): `docs/getting-started.md` deep-links the `runAs` subsection that this
issue's deletion rule removes, and that file is absent from `## Out of scope`, so a cold reader
cannot tell whether repointing it is permitted. It is the only anchored deep link into
`docs/workflow-schema.md` repo-wide, and no sibling issue catches it.

The eight-heading contract was re-derived from scratch and verified compatible with every
downstream anchor — no orphan on either side.

**Readiness gate (cold-reader): PASS** (round 5, full-enumeration)

Both round-4 findings are discharged. The replacement for the occurrence count was attacked as an
overclaim and survived: validation's own entry point performs the version rewrite as its first
statement, so "every path that reaches validation" is structurally guaranteed rather than a path
enumeration a reader could get wrong, and a production-region sweep confirms no validation branch
reads the version at all. The discovery instruction lands both write sites in one command, and no
numeral survives anywhere in that paragraph. The PRD now carries identical corrected wording, and
no sibling issue record carries the figure, so no sibling re-gate was owed. The `docs/getting-started.md`
permission fork is closed by the new exclusion. The eight-heading contract was re-derived from
scratch — including a sweep shaped to catch heading strings embedded inside longer backticked
commands, which a backtick-only sweep misses — and is compatible with every dependent anchor in
both directions. Classes 1-5 swept 15 surfaces, class 7 swept 36, class 9 arm A executed all six
observables (red) with arm B untriggered on all ten. No class fired; one `delegable` gap remains,
recorded below.

The residue: the exclusion says to file the repoint separately but names no record, where the PRD's
analogous `ARCHITECTURE.md` exclusion points at a real one. Deliberately not fixed here — another
full-enumeration round costs more than the pointer is worth, and the follow-up record is being filed
alongside this batch.

## Resolution

**Commit:** `feat: restructure workflow schema reference into a marker-bearing scaffold (ISSUE-260826-0637-03)`

**Route:** `cursor` (both the implementation pass and both fix rounds). Rationale: a single-file documentation restructure with explicit acceptance criteria and no DevOps, security, architecture or large-context signal — the route-picker returned `cursor` on all three dispatches independently.

**TDD:** `n/a (linear)` — documentation work, no behaviour-changing seam. The acceptance criteria's `rg` observables plus `just check-v4-docs` are the verification surface; no test was added, and none of the epic's engine code was touched.

**Review telemetry:** dual review (Claude + Codex, cross-verified, adjudicated inline). Claude raised 14 findings, Codex 9; after merging five independent confirmations and dismissing four on adjudication, **11 findings** entered the file — 6 MEDIUM, 5 LOW, no CRITICAL or HIGH. One further finding (L6) was admitted mid-loop from a round-1 re-verification. Final: **12 FIXED, 0 deferred, 4 dismissed**, 0 smells. **2 of 4 fix rounds used.**

Round 2 ran under the **same-site escalation rule**: L3 came back `STILL_BROKEN` and a new finding landed in the same paragraph the round-1 fix had just edited, so the two were treated as one defect and the **Flat-to-nested `kind` migration** paragraph was rewritten end-to-end from `migrate_workflow_value_to_v4` and `migrate_v2_node_to_v3_kind` in call order, against ten checkable properties, rather than patched again. Both reviewers returned `MECHANISM: GONE` with all ten properties passing and all round-1 fixes held.

The four dismissals, with their grounds: the `/api/capabilities` sentence is mandated by `CONTEXT.md`'s **Node Kind** entry and §1.12 sanctions folding rather than deleting; the subflow-omitting-`version` case is already covered by the ingest contract stated in the same section; and the `docs/getting-started.md` anchor and `docs/README.md` blurb are both explicitly out of scope, the former already filed as ISSUE-260826-1648-01.

Two advisory nits are recorded in the review file and deliberately not spent as a third round — a relative clause that a careless reader could misparse, and drift §1.10 item 80 (the `capture`/`kill` arms injecting an empty config), which was never part of any finding as adjudicated and changes no reader's behaviour.

**Suite:** `just test` (→ `cargo test --locked` then `npm test`) — **595 passed, 0 failed, 0 skipped** (Rust 449 + 17 across 4 binaries; vitest 129 in 13 files). `just check-v4-docs` exits 0 ("No stale v3 canonical-format references found") — the one check that reads the changed file; it is not part of the `just test` aggregate and was run separately. No generator freshness test exists in this tree; that is expected-absent and arrives with ISSUE-260826-0637-04.

**Known consequence, accepted by the brief:** `docs/getting-started.md`'s deep link to the removed `runAs` subsection now dangles. Repointing it was deliberately withheld from this issue and is filed as ISSUE-260826-1648-01; the repo is link-broken in the window between the two.

**Date:** 2026-08-30 (UTC)
