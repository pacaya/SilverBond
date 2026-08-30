---
id: ISSUE-260826-0637-05
kind: issue
category: enhancement
status: done
summary: Write the common node field table and the per-kind config field tables with real types, requiredness and defaults
prd: PRD-260826-0009-01
terms: [Node Kind, Skip Condition, Condition, Access Profile]
blocked_by: [ISSUE-260826-0637-04]
claimed_by: implement-issue@Mac-mini-4
claimed_at: 2026-08-30T05:41:20Z
---

## Agent Brief

**Category:** enhancement
**Summary:** Fill `## Node fields` with the one table that describes every node, plus a subsection
per node kind — a config field table where the kind has a config struct, a stated one-liner where
it does not — each row carrying the real type, whether it is required, and its real default.

**Current behavior:**
After the generator lands, the schema reference shows every node kind as a copyable fragment and
a complete workflow, and `## Node fields` is a placeholder deferring to the source. A serialized
example cannot fill that gap: it shows *a* value and nothing marks which value is the default, so
it can never answer "what happens if I omit this?". For kinds that skip defaulted fields on
serialization it is worse — the field vanishes from the output entirely, and a default-valued
pane-spawn node serializes to little more than its tag.

Today's document compounds this by mislabelling. Its "Task Node" table is really the universal
node field table: `agent`, `prompt`, `retryCount`, `loopCondition`, `loopMaxIterations`,
`skipCondition`, `continueSessionFrom`, `contextSources`, `outputSchema` and `cwd` all sit on the
node struct, not inside `kind`. It presents `splitFailurePolicy` as split-only when it is a field
of every node with a custom deserializer mapping an explicit null onto the permissive policy. And
several of its "Required" answers are backwards — `prompt` is defaulted and an empty one is only a
warning, while `goal`, `nodes` and `edges` are all defaulted too.

**Desired behavior:**
`## Node fields` carries two subsections.

*Common node fields* — one table for the fields every node carries whatever its kind, with the
nested `kind` object itself as a row. Rows that carry a trap say so in the description rather than
leaving the reader to find it: that `agent` is optional to serde but defaults to *absent* rather
than to a name, and a task node without one is a hard validation error, so the runtime fallback
never rescues a document at run start; that
`loopCondition` is a structured condition object evaluated deterministically and never a prompt;
that `loopMaxIterations` defaults to a small value when unset and that reaching the cap without a
loop-exit edge fails the run; that `retryCount` has a hard maximum whose breach short-circuits the
whole validation pass; that `splitFailurePolicy` is universal and what an explicit null means; and
that `continueSessionFrom` carries five distinct constraints — the source's kind, a matching
resolved agent, an access profile on the source no broader than the target's, a matching resolved
working directory, and the node existing at all.

*Per-kind config fields* — one subsection per kind, in the catalog's order so a reader moving
between a generated example and its table does not have to search. Not every kind has a config
struct to tabulate, and the brief is explicit rather than leaving it to judgement: kinds that are
bare unit variants carry no config object at all, and their subsection says exactly that in one
line instead of showing an empty grid. `subflow` and `call` share one config struct under one wire
key — give each its own table anyway rather than cross-referencing, so a reader landing on either
kind is finished there. Two kinds carry agent config inside `kind` and that table is
ISSUE-260826-0637-06's: for `task` it is the only payload, so its subsection says so and points
there; for `run_agent` it sits alongside that kind's own config, so tabulate the run-agent config
here and add one row pointing the agent-config half at -06. Each row gives the
wire field name, its type, whether it is genuinely required, and its real default. Serde aliases
are part of the wire contract and belong in the table: the batch config accepts a legacy config
key, and the subflow config's workflow-name field accepts two alternative spellings. So do the
defaults that surprise — the decide model, the batch concurrency default and its runtime clamp,
the send node's enter flag defaulting to on, the run-agent kill-after flag defaulting to on, the
wait mode's default, and the subflow depth cap.

Every table cites the source location that owns it, so a reader can verify in one jump. These
tables are hand-written and stay hand-written: no check in this epic pins them, and requiredness
in particular is a serde attribute that can change without changing the construction API, the
serialized example, the harvested variant set or any validation result. Write them knowing that.

**Key interfaces:**
- The node struct is the authority for the common table; each kind's config struct is the
  authority for its own. Read the serde attributes, not the field declarations alone —
  defaulting, renaming, aliasing and skip-on-serialize all change the wire contract.
- The catalog's declared order, established by ISSUE-260826-0637-04, fixes the order of the
  per-kind subsections.
- `docs/sources/workflow-schema-drift-260825.md` Part 1 sections 1.2 and 1.3 are this issue's work
  order. Section 1.8 (`skipCondition`) and section 1.11 (agent config) both belong to
  ISSUE-260826-0637-06 — this issue's Out of scope disclaims agent config, so do not write those
  rows here.
- `CONTEXT.md`'s **Access Profile** entry distinguishes the `access` field from the `accessMode`
  enum. Only `access` belongs in these tables — it sits on the pane-spawn and run-agent config
  structs. `accessMode` is declared once, on the shared agent-defaults struct, which is reached both
  through the flattened node-level agent config and as the value type of the top-level defaults map,
  and the resolver merges the two; either way it is ISSUE-260826-0637-06's row. Keep the two apart
  and do not document `accessMode` here.

**Acceptance criteria:**
- [ ] `## Node fields` contains a common-node-fields subsection and a per-kind subsection.
      Observable: `rg --pcre2 --multiline -n '(?s)^## Node fields\n(?:(?!^## ).)*?^### ' docs/workflow-schema.md`
      matches; no match before this change. The lazy quantifier stops at the first `###`, so this
      command proves at least one subsection exists, not two — the prose above is the requirement
      and both subsections are checked by reading.
- [ ] The common table documents `splitFailurePolicy` as a field of every node. Observable:
      `rg --pcre2 --multiline -n '(?s)^## Node fields\n(?:(?!^## ).)*?splitFailurePolicy' docs/workflow-schema.md`
      matches; no match before this change. Anchor to the section rather than searching the file:
      today the field name appears in the legacy node section as a split-only field, and although
      ISSUE-260826-0637-03 deletes that section before this issue starts, an unanchored check would
      also be satisfied by any later mention elsewhere in the document.
- [ ] The common table documents `loopCondition` as a structured condition object, not a prompt
      string. Observable at the seam: the row's type column names the condition shape.
- [ ] The `agent` row states that the field is optional to serde but that a task node without one
      fails validation. Do not write "has no serde default" — the field does carry `#[serde(default)]`;
      what it lacks is a default *value*, so an omitted `agent` deserializes to absent, not to a
      name. In a table whose subject is serde attributes that distinction is the whole point.
- [ ] Every node kind has a subsection under the per-kind heading, in catalog order, carrying the
      treatment its kind calls for — the four cases named in Desired behavior, not two: a config
      field table for a kind with its own config struct; a stated one-line "carries no config
      object" for a bare unit variant; for `task`, a one-liner saying its only in-`kind` payload is
      the agent config, pointing at ISSUE-260826-0637-06; for `run_agent`, its own config table plus
      one row pointing the agent-config half at -06. Do not collapse this into a has-a-struct /
      has-none dichotomy: under that reading `task` gets either a false "carries no config object"
      line or a table this issue disclaims. Observable: derive the tag set from the source and
      confirm each has a subsection; do not assert the set's size in the document prose.
- [ ] Every field of a kind's own config struct appears as a row in that kind's table, and no row
      names a field that struct does not have — with one deliberate exception, the `run_agent`
      pointer row for `agentConfig`, which is a sibling payload of the variant rather than a field
      of the run-agent config. Serde aliases that sit on the wrapper key rather than on a struct
      field — the batch config's legacy spelling — are documented in the table's caption or note,
      not as a field row, for the same reason. Observable at the seam: compare each table against
      its struct's serde-visible fields, allowing those two exceptions.
- [ ] Every default stated in a table matches the source. Spot-checkable at the seam on the
      surprising ones — the send node's enter flag and the run-agent kill-after flag both default
      to on, and the batch concurrency default is not one.
- [ ] Both serde aliases are documented — the batch config's legacy key and the subflow config's
      two alternative workflow-name spellings.
- [ ] Every table cites the source location that owns it.
- [ ] `just check-v4-docs` passes and `cargo test` is green, including the generator's freshness
      assertion — this issue edits prose around the generated blocks and must not disturb them.

**Out of scope:**
- Editing anything between a `BEGIN GENERATED` and `END GENERATED` marker. Those spans belong to
  the generator; a hand edit there fails the freshness assertion.
- `skipCondition`'s own shape table, the edge and condition tables, top-level fields, template
  tokens, agent config and the validation catalog — ISSUE-260826-0637-06 and -07.
- Generating these tables from JSON Schema. Excluded by the PRD as a second generator.
- Any change under `src/` or `ui/`, including adding derives to make the tables generable.
- Describing v5 field shapes as present.

## Context Pack — generated at claim (2026-08-30T05:41:20Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01):
- Tier P — hand-written, source-cross-referenced: the common `WorkflowNode` field table and the per-kind config field tables live here, each citing the source location that owns it.
- Generated examples cannot state defaults, so field tables stay hand-written: a serialized example shows *a* value and never marks it as the default; skip-heavy configs (`SpawnConfig`, `RunAgentConfig` skipping 10 of 11) omit the field entirely.
- Requiredness is the one drift class no check catches — a serde attribute can change without touching the construction API, catalog, or validation. Write the tables knowing they go stale silently.
- Bare unit variants (`approval`, `split`, `collector`) carry no config object and get a stated one-liner; `task`'s only in-`kind` payload is agent config, owned by the agent-config table (ISSUE-260826-0637-06).
- Today's doc mislabels: it presents universal node fields as task-node fields and `splitFailurePolicy` as split-only — a named drift-audit finding.
- No engine code changes: this epic reads `src/`, writes `docs/`. No derives added to make tables generable.
- `schemars`-derived field tables are excluded — a second generator, not an extension.
- v4 is canonical (`WORKFLOW_SCHEMA_VERSION = 4`); v5 is cited as a forward reference (ADR-260815-2009-01), never described as present.
- Generated blocks are owned by the generator; only the span between `BEGIN GENERATED`/`END GENERATED` markers is machine-written, and prose around it is hand-edited.
- Terminology follows `CONTEXT.md`: unmarked entries are true of `src/` at HEAD; `_(planned — ADR-…)_` entries are not accepted by the engine yet.
- Catalog order (established by ISSUE-260826-0637-04, now done) fixes the order of per-kind subsections.

**Test seam & Testing Decisions:** observable at `docs/workflow-schema.md` § `## Node fields` via section-anchored `rg --pcre2 --multiline` matches (subsection presence; `splitFailurePolicy` inside the section), plus reading the tables against each struct's serde-visible fields. Testing Decisions that touch it: Tier P is explicitly *not* machine-checked — the common node field table and the per-kind config tables head the "honest limits" inventory; the only machine gates this slice must keep green are the generator's freshness/coverage assertions under `cargo test` and `just check-v4-docs`, which this issue must not disturb since it edits prose around generated blocks.

**ADRs:**
- ADR-260815-2009-01 — Typed workflow contracts over a JSON-valued variable store · accepted (the decided-but-unlanded v5 bump; cite as forward reference only)
- ADR-260815-2009-02 — One condition dialect: owned nested AST, typed operators, onMissing · accepted (neighbor: replaces today's flat `Condition`, so `loopCondition` rows describe the v4 flat leaf)

**Terms:**
- `Node Kind` — the fourteen-variant tagged union in a node's required `kind` object, selecting both what the node does and which config shape it carries; the bare `type` tag is what `/api/capabilities` publishes. _Avoid_: step type.
- `Condition` — the engine's single post-execution deterministic branching form (edge and loop conditions): one flat `{field, operator, value}` leaf over a dot-path field, evaluated by the engine and never by an LLM. _Avoid_: expression, rule.
- `Skip Condition` — a node's pre-execution guard (`{source, type, value}`, `type` one of `contains | not_contains | regex`); unknown type evaluates false silently, invalid regex fails the run before any node executes. _Avoid_: bare "condition".
- `Access Profile` — the named argv bundle a `spawn` or `run_agent` node selects with `access`, validated against the Agents Registry names for that agent and applied outside the agent-defaults merge chain; distinct from `accessMode`, the four-variant driver-level enum defaulting to `execute` (that one is -06's row). _Avoid_: access mode, bare "profile".

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md · docs/adr/INDEX.md · CONTEXT.md · docs/sources/workflow-schema-drift-260825.md (Part 1 §§1.2–1.3) · docs/issues/done/ISSUE-260826-0637-04-node-catalog-generator.md

## Code Review

Review file: `issue-260826-0637-05-code-review-20260830-060732.md`

Dual review (Claude + Codex) over the issue diff vs `02fb8f43`, cross-verified and adjudicated
inline, then four fix rounds (the full cap). Codex's two findings were both confirmed by Claude and
merged (H1, M5). Codex agreed with eight of Claude's fourteen; the six it disputed were split
verdicts adjudicated against the source — three upheld, three dismissed. Two further findings were
opened during fix rounds (M7, L8), each verified against the source before being recorded.

- H1 (HIGH): `contextSources` documents a wire shape that cannot deserialize — FIXED
- M1 (MEDIUM): Common `agent` row understates the validation rule — `run_agent` also hard-errors — FIXED
- M2 (MEDIUM): Per-kind tables omit the node-level fallback chain, making "Default: absent" misleading — FIXED (rounds 1-3)
- M3 (MEDIUM): `retryDelay`'s runtime default (2 seconds) is omitted — FIXED
- M4 (MEDIUM): `idleSeconds` / `readyStableSeconds` runtime defaults omitted in the wait and run_agent tables — FIXED (rounds 1-2)
- M5 (MEDIUM): Per-kind preamble overclaims field-level skip-on-serialize coverage — FIXED
- M6 (MEDIUM): `skipCondition` forward reference points at a section that does not document it — dismissed
- M7 (MEDIUM): Common `cwd` row promises a per-node override the engine applies only to `spawn` — FIXED (rounds 3-4)
- L1 (LOW): Common and run_agent `cwd` rows omit the absolute-path requirement — FIXED
- L2 (LOW): `outputSchema` Notes says "JSON Schema object" but the field accepts any JSON value — FIXED
- L3 (LOW): Common-section preamble contradicts its own table on `kind` — FIXED
- L4 (LOW): Per-kind preamble drops the "requiredness is serde, not validation" caveat — FIXED
- L5 (LOW): `task`, `approval`, `split`, `collector` subsections cite source without line ranges — FIXED
- L6 (LOW): `prompt` note reads as exclusive to task nodes — dismissed
- L7 (LOW): Inline Notes citations lack line numbers — dismissed
- L8 (LOW): "read only by `spawn` nodes" is literally inaccurate — the read happens, it just has no
  effect — **deferred**. Surfaced on the round-4 re-verify with the fix cap exhausted. The cell's
  behavioral conclusion is correct and both reviewers agree on it; only the word "read" overstates,
  because `resolve_agent_config` (`src/runtime.rs:3912`) reads `node.cwd` (`src/model.rs:332`) on
  every dispatch before the value is discarded at `src/driver.rs:457`. Remedy is one phrase, fully
  specified in the review file; `ISSUE-260826-0637-06` re-enters this document and is the natural
  place to land it.

Smells: 0 (both reviewers returned empty blocks; the brief-mandated subflow/call table duplication
was suppressed at source as the documented standard overriding the baseline).

## Triage Notes

**Readiness gate (cold-reader): PASS** (round 5, full-enumeration)

Rounds 1-4 ran without terminal stamps; round 4 raised findings against the per-kind treatment
criterion and the config-field-row criterion, both of which were rewritten. Round 5 ran as a
full-enumeration round per the round-3 escalation rule with those two criteria promoted as tracked
findings and judged from scratch: the four treatment cases were verified exhaustive over the
harvested variant set, and both row-rule exceptions verified as real properties of the source and
no wider than the facts. Classes 1-5 swept 30 decision surfaces, class 7 swept 48 extracted
surfaces, class 9 arm A executed every embedded observable against the tree (all red), arm B found
no triggered requirement, class 6 was re-examined cold on both prongs, and class 8 was inert on a
never-stamped record. No class fired.

## Resolution

**Commit:** `feat: write the common node field table and per-kind config field tables (ISSUE-260826-0637-05)`

**Route:** cursor (`/cursor-developer`) for the implementation and all four fix rounds. Chosen by
`route-picker` each time: a single documentation file, well-scoped, with the accuracy burden lying in
reading serde attributes rather than in cross-module reasoning.

**TDD:** n/a (linear) — documentation prose, no behavior-changing seam.

**Review telemetry:** 16 findings recorded — 1 HIGH, 8 MEDIUM, 7 LOW. 12 FIXED, 3 dismissed, 1
deferred (L8, LOW). Fix rounds used: 4 of 4. Two reviewers (Claude + Codex), cross-verified; 6 of the
14 first-round findings drew a split verdict and were adjudicated by the orchestrator reading the
source, as were both fix-round disputes (M2 in Codex's favor, M7 in Claude's, L8 in Codex's).

**Note for the epic.** Three of the four fix rounds shipped a wrong or mislabelled source citation:
round 1 cited the run-agent code path for wait-node defaults, round 2 an off-by-one, round 3 a
validation-time reader filed under an execution-time label. The last two originated in the
orchestrator's own fix briefs, not in fixer error, and none was catchable by `just check-v4-docs`,
the generator freshness assertion, or any test — `tests/docs_catalog.rs` is the only test that reads
this file and it checks generated spans, not hand-written prose. This is the Tier P silent-staleness
risk PRD-260826-0009-01 names, demonstrated three times inside a single issue. Round 3 onward
required each citation to be re-derived with `rg -n` and the command output pasted into the fix
report, which is what stopped the pattern.

**Suite:** `SUITE: PASS` — `just check-v4-docs` green; `cargo test --locked` 449 + 6 + 17 passed, 0
failed, 1 ignored; `npm test` 129 passed across 13 files. `tests/docs_catalog.rs` 6 passed / 1
ignored, including `node_catalog_generator` and `freshness_check_is_compare_only_by_construction`, so
the generated blocks are undisturbed. Playwright e2e was not run — it is not part of `just test` and
a prose-only change cannot reach it. The 7 socket/tmux tests that failed with `Operation not
permitted` inside a reviewer's sandbox passed in this unsandboxed session.

**Date:** 2026-08-30 (UTC)
