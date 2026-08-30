---
id: ISSUE-260826-0637-06
kind: issue
category: enhancement
status: done
summary: Write the edge, condition, top-level, variable, subflow-catalog, skipCondition, agent-config and template-token tables
prd: PRD-260826-0009-01
terms: [Edge Outcome, Condition, Skip Condition, Variable, Subflow, Template Token, Access Profile]
blocked_by: [ISSUE-260826-0637-05]
claimed_by: implement-issue@Mac-mini-4
claimed_at: 2026-08-30T07:03:16Z
---

## Agent Brief

**Category:** enhancement
**Summary:** Fill the table sections of the schema reference this issue owns — edges and
conditions, document-level fields, and templates and agent config. The validation catalog is the
remaining one and belongs to ISSUE-260826-0637-07.

**Current behavior:**
These sections are placeholders deferring to the source. What the old document said about them
was wrong in ways that stop a workflow from loading at all: edges were documented with field
names the struct does not have and no serde aliases to rescue them, the required edge id was
omitted entirely, and the condition mechanism — the whole of deterministic branch routing — was
absent. The template section's table has four rows: three real substitution forms out of the ten
the engine resolves, plus a fourth that does not exist at all. `contextSources` was described as injecting context into the prompt when it only
registers a substitution that does nothing unless the prompt names it.

**Desired behavior:**

*`## Edges and conditions`* documents the edge as it deserializes: its id, which is required and
whose duplication is a validation error; the endpoint fields under their real names; the outcome
channel; the label; the branch id; and the condition. It states the complete outcome set and that
there is no failure channel, so a node failure is always terminal for its cursor — with the ADR
that adds one cited as a forward reference, never as present behavior. Then the condition itself:
the flat three-field leaf shape with all three required, the dot-path field lookup, and the
complete operator set, which exists only inside the evaluator as string comparisons and has no
enum to enumerate. Note the two operator behaviours a reader cannot guess — an unrecognised
operator yields an error string rather than a match, and regex patterns above a length ceiling are
rejected at run time.

Two rules belong here because they are why a workflow the editor accepted will not run: a split
fans out over success edges and any branch, loop or reject edge leaving a split is a validation
error; a collector needs at least one inbound edge and exactly one outbound success edge, and its
expected inputs are keyed by edge label falling back to the source node id, and two inbound edges
sharing a key are a **validation error**, not a silent merge. The collapse-into-one-slot behaviour
is real but unreachable from a validated document: the barrier builder dedupes into a set behind a
log warning, and every path that starts a run rejects the workflow first. Document the rejection as
the rule and the dedupe as what the runtime would do if validation were bypassed — do not present
the collapse as the behaviour an author will meet.

Be precise about *when* those rejections happen, because the obvious phrasing is wrong: **saving a
workflow does not validate it.** The save handler normalizes and writes; validation runs on an
explicit validate request and is enforced at run start, where error-severity issues refuse the run.
A document with a branch edge leaving a split saves cleanly and fails when someone tries to run it.
Do not write "will not save" anywhere.

*`## Document-level fields`* documents the top-level keys with honest requiredness — only two are
genuinely required and the rest are defaulted — which fields are always emitted versus skipped
when empty, and that the agent-defaults map is sorted and deterministic on serialize. The limits
get their real defaults and their sentinel behaviour: a zero is rewritten back to the default, so
zero does not mean unlimited. The viewport sub-fields have no individual defaults, so a partial
viewport object fails to deserialize. Variables are documented as string-valued, with the second
role that is invisible from the struct — an empty default on a subflow's variable makes it a
required input binding, and leaving it unbound is a hard error — and with the asymmetry that root
variable names are not checked for uniqueness while subflow bindings are. The subflow catalog gets
its own table, including that nested catalogs are rejected outright and that a subflow body warns
on root-only fields.

*`## Templates and agent config`* lists all ten substitution forms and no others. It states the
resolution behaviour `CONTEXT.md`'s **Template Token** entry records: nothing errors on a miss,
the two dot-path forms substitute the empty string, and the four name-keyed forms are left in the
prompt verbatim. It corrects `contextSources` to registering a substitution rather than injecting
anything, and documents that the all-predecessors form covers direct inbound predecessors only,
drops empty outputs, joins with a separator, and prefixes results carried over from a prior run.
`skipCondition` gets its shape table here — its source field's default, that its kind field is
renamed on the wire, its three permitted values, and that anything else evaluates false silently
with only the regex form getting a compile check. Agent config documents the node-level shape,
which flattens the shared defaults so that only the tool allow and deny lists are node-exclusive;
the access mode enum with its real default, which is not the most restrictive one; and that the
`access` field on the pane-spawn and run-agent configs bypasses the merge chain entirely.
`CONTEXT.md`'s **Access Profile** entry is the authority for keeping that field and the access
mode apart.

**Key interfaces:**
- The edge struct, the condition struct and its evaluator, the workflow struct's top-level fields,
  the variable struct, the skip-condition struct, and the node-level agent config struct are the
  authorities. Read serde attributes, not field declarations alone.
- The substitution forms are implemented as inline string and regex matching in the template
  resolver with no enum to enumerate, which is why this list is hand-written and stays so.
- `docs/sources/workflow-schema-drift-260825.md` Part 1 items 3–8, §1.4 items 30 and 33 (the split
  and collector graph rules), §1.5 except items 36 and 39 (which ISSUE-260826-0637-03 wrote into the
  document grammar), §1.6, §1.7, §1.8 and §1.11 are this issue's work order. Item 33 also appears in
  ISSUE-260826-0637-07's order, as does item 30 — deliberately in both cases: this issue states the
  split and collector rules as prose a reader needs, that one catalogues the validation issues they
  produce.

**Acceptance criteria:**
- [ ] `## Edges and conditions` documents the required edge id and the complete outcome set.
      Observable: `rg --pcre2 --multiline -n '(?s)^## Edges and conditions\n(?:(?!^## ).)*?loop_exit' docs/workflow-schema.md`
      matches; no match before this change.
- [ ] The same section documents the condition leaf shape and its full operator set, and states
      that an unrecognised operator does not match. Observable at the seam: the documented operator
      strings are compared against the evaluator's. Compare *strings*, not match arms — the
      evaluator groups the four ordering comparisons into one arm, so an arm count understates the
      set.
- [ ] `## Document-level fields` states that exactly two top-level keys are required. Observable:
      `rg --pcre2 --multiline -n '(?s)^## Document-level fields\n(?:(?!^## ).)*?entryNodeId' docs/workflow-schema.md`
      matches; no match before this change.
- [ ] The limits table documents the zero sentinel as meaning the default rather than unlimited.
- [ ] The variable table documents that an empty default on a subflow variable makes it a required
      input binding.
- [ ] `## Templates and agent config` lists exactly ten substitution forms. Observable:
      `rg --pcre2 --multiline -n '(?s)^## Templates and agent config\n(?:(?!^## ).)*?all_predecessors' docs/workflow-schema.md`
      matches; no match before this change. Anchor it to the section: a document-wide check for the
      invented form is **already green** by the time this issue starts, because
      ISSUE-260826-0637-03 deletes the legacy template section outright. Every observable in this
      brief must be red against the tree as it stands *after* 03, 04 and 05 have landed — not
      against today's tree.
- [ ] The invented form stays absent. Observable: `rg -c 'node_name:' docs/workflow-schema.md`
      returns no matches. **Preservation criterion** — 03 already removed it; this issue must not
      reintroduce it while writing the real list.
- [ ] The template section states the miss behaviour for both groups of forms, matching
      `CONTEXT.md`'s **Template Token** entry.
- [ ] The `skipCondition` table documents the wire rename of its kind field and the silent
      false on an unrecognised value. Observable:
      `rg --pcre2 --multiline -n '(?s)^## Templates and agent config\n(?:(?!^## ).)*?not_contains' docs/workflow-schema.md`
      matches; no match before this change.
- [ ] The agent-config section documents the access mode's real default and that the pane-spawn
      and run-agent `access` field bypasses the merge chain.
- [ ] Every table cites the source location that owns it.
- [ ] `just check-v4-docs` passes and `cargo test` is green, generator freshness included.

**Out of scope:**
- Editing anything between generated markers.
- The validation catalog — ISSUE-260826-0637-07. This issue states the handful of graph rules a
  reader needs in place, in the language fixed above — refused at run start, not at save; the
  enumeration of all validation issues is separate.
- Node field tables — ISSUE-260826-0637-05, already landed when this starts.
- Describing the nested condition AST, `onMissing`, the failure outcome or typed variables as
  present. `onMissing` and Failure Outcome carry the `_(planned — ADR-…)_` marker in `CONTEXT.md`;
  the condition AST and the JSON-valued variable store do not — Condition and Variable are unmarked
  entries defining the present v4 forms, carrying the future shape in a `Decisions:` clause instead.
  Either way, cite the ADRs as forward references and describe v4.
- Any change under `src/` or `ui/`.

## Context Pack — generated at claim (2026-08-30T07:03:16Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01):
- Tier P — hand-written, source cross-referenced: the edge, top-level, variable, subflow-catalog, `skipCondition`, agent-config and template-token tables are explicitly Tier P; each entry cites the source location that owns it.
- Not generable, deliberately: template tokens are inline string/regex matching in `resolve_template_vars`; the condition operator set exists only inside `evaluate_condition` as string comparisons — no enum to enumerate, so these lists stay hand-written.
- Generated examples cannot state defaults, so field tables carry names, types, required-ness and defaults in prose tables rather than being read off an example.
- Editing inside `<!-- BEGIN/END GENERATED -->` markers is forbidden — the generator owns only the node catalog blocks in `docs/workflow-schema.md`.
- Document order for the restructured reference: grammar → generated catalog → Tier P tables (common node fields, per-kind config, edges/conditions/operators, variables, subflow catalog, top-level, `skipCondition`, agent config, template tokens, validation catalog).
- Saving never validates: validation is advisory on an explicit request and enforced at run start; the PRD's Testing Decisions state "there is no save-time gate to pass".
- v4 is canonical (`WORKFLOW_SCHEMA_VERSION = 4`); v5 features (condition AST, `onMissing`, typed variables, failure outcome) are cited as forward references only, never as present behavior.
- No engine changes: this epic reads `src/`, writes `docs/`. Known-wrong behavior is filed separately (ISSUE-260826-0004-01), not fixed here.
- The drift audit `docs/sources/workflow-schema-drift-260825.md` is the work order; this issue owns Part 1 items 3–8, §1.4 items 30 and 33, §1.5 (except 36, 39), §1.6, §1.7, §1.8, §1.11.
- Requiredness is the one drift class no check catches (a serde attribute change leaves every check green) — hence the requiredness columns must be reviewed against source, not inferred.

**Test seam & Testing Decisions:** observable at `docs/workflow-schema.md` section content — `rg --pcre2 --multiline` anchored per section (`## Edges and conditions` → `loop_exit`, `not_contains`; `## Document-level fields` → `entryNodeId`; `## Templates and agent config` → `all_predecessors`), plus the preservation check `rg -c 'node_name:'` returning none, and `just check-v4-docs` + `cargo test` (freshness of generated blocks included). Testing Decisions that touch this seam: the freshness check gates generated blocks only and is not a general doc linter, so these Tier P tables rest on review against cited source lines; all observables must be red against the tree *after* 03/04/05 land, not today's tree; compare documented operator *strings* against the evaluator's, not match arms (four ordering comparisons share one arm).

**ADRs** (forward references only — the record has no `adrs:` field; named via the brief and glossary):
- ADR-260815-2009-02 — One condition dialect: owned nested AST, typed operators, `onMissing` · accepted (also adds the sixth `failure` edge outcome).
- ADR-260815-2009-01 — Typed workflow contracts over a JSON-valued variable store · accepted (the PRD's `adrs:` entry; makes Variable JSON-valued, bumps to v5).
- ADR-260815-2009-04 — Model/CLI routing via allow-listed named profiles · accepted (relevant only to keep the planned **Profile** apart from the real **Access Profile**).

**Terms:**
- `Edge Outcome` — the channel an edge is traversed on: `success | reject | branch | loop_continue | loop_exit`, the complete set; no failure channel, so a node failure ends its cursor and outside a split family fails the whole run. _Avoid_ calling failure terminal for its cursor alone.
- `Condition` — the single post-execution deterministic branching form (edge and loop conditions): one flat `{field, operator, value}` leaf over a dot-path field, engine-evaluated, never by an LLM; `skipCondition` is a separate form. _Avoid_: expression, rule.
- `Skip Condition` — a node's pre-execution guard `{source, type, value}` with `type` one of `contains | not_contains | regex`; an unknown type evaluates false silently, an invalid regex fails the run before any node executes. _Avoid_ bare "condition".
- `Variable` — a named, string-valued piece of per-execution state scoped to a cursor; distinct from node results, the default data path.
- `Subflow` — a workflow invoked as a callable block from another workflow via a call frame. _Avoid_: sub-workflow, child workflow.
- `Template Token` — one of ten `{{…}}` substitution forms; binding sources are a separate brace-free grammar over the same names; nothing errors on a miss — the two dot-path forms substitute empty string, the four name-keyed forms stay verbatim.
- `Access Profile` — the named argv bundle a `spawn` or `run_agent` node selects with `access`, validated against the Agents Registry names, applied outside the agent-defaults merge chain; distinct from `accessMode`, the `read_only | edit | execute | unrestricted` enum defaulting to `execute`.
- `Failure Outcome` / `onMissing` — both `_(planned — ADR-260815-2009-02)_`; describe as absent from v4.

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md · docs/adr/INDEX.md · CONTEXT.md · docs/sources/workflow-schema-drift-260825.md · docs/issues/done/ISSUE-260826-0637-05-node-field-tables.md

## Code Review

Review file: `issue-260826-0637-06-code-review-20260830-073344.md`

Claude + Codex, cross-verified, adjudicated inline. **ACCEPTED** after 2 fix rounds of a cap of 4.
16 findings after merge and promotion (4 MEDIUM, 12 LOW); no CRITICAL/HIGH (docs-only diff).
13 FIXED, 2 deferred, 3 dismissed.

- M1 (MEDIUM): Edge `condition` applicability and evaluation gate are both misstated — FIXED
- M2 (MEDIUM): Token-precedence prose contradicts itself (same-site escalation; redesigned) — FIXED
- M3 (MEDIUM): Serialize-emission behaviour documented only on `model` — FIXED
- M4 (MEDIUM): The five-value outcome set is enumerated twice within the diff — FIXED
- L1 (LOW): The regex ceiling is 256 UTF-8 bytes, not 256 characters — FIXED
- L2 (LOW): Zero-limit normalization applies only to the root workflow — FIXED
- L3 (LOW): `accessMode` puts a resolve-time default in the wire Default column — FIXED
- L4 (LOW): Split cardinality — zero success edges errors, exactly one warns — FIXED
- L5 (LOW): Cited symbol `should_skip_cursor` does not exist — FIXED
- L6 (LOW): A 245-character line breaks the document's wrap convention — FIXED
- L7 (LOW): `branchId` described as inert when it determines the reported `chosenBranch` — FIXED
- L13 (LOW): The M3 fix left two emission conventions in one section — FIXED
- L8 (LOW): `ui.canvas` and `ui.canvas.nodes` are undocumented — deferred
- L9 (LOW): Stale tracker parentheticals at `:1096` and `:1262` — deferred
- L10 (LOW): Operators table has no trailing `*Source:*` line — dismissed: cites its sources inline
- L11 (LOW): Ten-form claim lacks the binding-source caveat — dismissed: the claim is already scoped
- L12 (LOW): Tier-P subsection order diverges from the PRD list — dismissed: follows the brief and 03's landed scaffold

M1 merges CODEX-1 with CLAUDE-1/2/3 (one defect split three ways); L4 and L5 each merge a
Claude/Codex duplicate pair. M4 is a promoted in-diff duplication smell — both cited sites inside the
diff. M2 hit the same-site escalation rule: round 1's point patch closed nothing and produced two
findings inside the region it had just written (Codex NEW-1, Claude CLAUDE-14), so both were folded
into M2 and round 2 redesigned the region against four checkable properties rather than patching
again. L13 entered in round 2 from Claude's CLAUDE-15 after orchestrator verification.

Smells: 4 advisory (3 Duplicated Code, 1 Primitive Obsession), all `appended` to
`docs/issues/SMELLS-LEDGER.md`; no graduation rows.

## Triage Notes

**Readiness gate (cold-reader): PASS** (round 3)

Round 1 found three blocking issues including a false save-time framing; round 2 found the
collector merge-key rule inverted and the banned phrase surviving in Out of scope. All were
fixed, upstream included. Round 3 ran the full nine-class rubric, built post-blocker fixtures to
test each observable against its real baseline rather than today's tree, and swept both repaired
claims across this record and the upstream artifacts — no survivor.

## Resolution

**Commit:** `feat: write the edge, condition, top-level, variable, subflow-catalog, skipCondition, agent-config and template-token tables (ISSUE-260826-0637-06)`

**Route:** cursor (all three dispatches — initial implementation and both fix rounds). Chosen by
`route-picker` each time: single-file documentation work over `docs/workflow-schema.md` requiring
close reading of Rust source but no cross-module edits, no security-across-layers reasoning and no
architecture decisions. Round 2 was re-routed after the redesign escalation and still resolved to
cursor — raw difficulty does not force codex.

**TDD:** `n/a (linear)` — docs-only slice; `src/` and `ui/` are out of scope, so no behaviour changes
and nothing to drive red-green. Verification is the four section-anchored `rg` observables plus the
preservation check, all confirmed red against the post-03/04/05 baseline before dispatch.

**Review telemetry:** 2 fix rounds used of a cap of 4. 16 findings after merge and promotion —
4 MEDIUM, 12 LOW, no CRITICAL/HIGH (docs-only diff). **13 FIXED, 2 deferred, 3 dismissed.**
20 raw findings were filed across the two reviewers; merging cut them to 16: M1 absorbed CODEX-1 plus
CLAUDE-1/2/3 (one defect split three ways), and L4 and L5 each merged a Claude/Codex duplicate pair.
M4 was promoted out of the advisory smell track by the in-diff duplication rule. 4 advisory smells
merged to `docs/issues/SMELLS-LEDGER.md`, all `appended`; no graduation rows.

M2 triggered the **same-site escalation** rule: round 1's point patch of the template-token
precedence region closed nothing and produced two findings inside the region it had just written
(Codex NEW-1 MEDIUM, Claude CLAUDE-14 LOW). Both were folded into M2 as one defect and round 2
redesigned the region around a named mechanism — two orthogonal axes interleaved so each qualifier
landed on the wrong claim — judged against four checkable properties rather than a prose claim. That
round closed M2 and L13 with zero new findings on either reviewer's sweep.

The M2 fix brief carried an error of the orchestrator's own: its Axis-A summary said the outcome "is
always verbatim", which is wrong for the shadowing case. The implementer declined that instruction
and shipped the correct distinction; both reviewers confirmed it. Recorded in the review file's
outcome paragraph, attributed to the brief rather than the fix.

Deferred (both real, both outside this slice): L8 `ui.canvas`/`canvas.nodes` coverage — drift item 41
names the viewport only; L9 stale tracker parentheticals at `:1096`/`:1262` — outside `ISSUE_DIFF`,
written by ISSUE-260826-0637-05. Dismissed after the orchestrator verified each against the file:
L10 (the Operators table cites its sources inline), L11 (the ten-form claim is already scoped to
`resolve_template_vars`), L12 (the `##` scaffold was fixed by the landed ISSUE-260826-0637-03 and is
pinned by this issue's acceptance criteria; the `###` order matches the Agent Brief).

**Review file:** `issue-260826-0637-06-code-review-20260830-073344.md`

**Suite:** `SUITE: PASS` — `just test` green: 601 passed (472 Rust across 5 targets + 129 vitest),
0 failed, 1 ignored (the `SB_REGEN_DOCS`-gated regen test). `docs_catalog` green, so generated-block
freshness holds. `just check-v4-docs` run separately by the orchestrator: clean. All four
section-anchored observables match, `node_name:` absent, and the template table holds exactly ten
forms.

**Date:** 2026-08-30 (UTC)
