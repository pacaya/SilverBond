---
id: ISSUE-260826-0637-07
kind: issue
category: enhancement
status: done
summary: Write the validation catalog covering every error and warning the engine emits, with severities corrected
prd: PRD-260826-0009-01
terms: [Validation Issue, Node Kind, Subflow, Access Profile]
blocked_by: [ISSUE-260826-0637-06]
claimed_by: implement-issue@Mac-mini-4
claimed_at: 2026-08-30T08:08:27Z
---

## Agent Brief

**Category:** enhancement
**Summary:** Fill `## Validation catalog` with the issues the engine actually emits, so a reader
can predict a refused run before starting one.

**Current behavior:**
The section is a placeholder deferring to the source. The document it replaced listed four errors
and two warnings — a small fraction of what the engine can emit; derive the real inventory as
described under Key interfaces rather than taking a figure from here — and invented a third
severity: it presented graph metadata as an informational severity when metadata is a separate
field of the validation result and not an issue at all. At least one of its six entries had the
wrong severity — a task node without a prompt is a warning, not an error — and one was mislabelled,
calling the terminal-node warning a dead-end warning when dead-end node ids are, again, graph
metadata rather than an issue.

**Desired behavior:**
`## Validation catalog` enumerates what the engine emits, grouped so a reader can find the entry
matching a message they just saw. Grouping by the subject being validated — the document, the
graph, node kinds, subflows and calls, agent launch configuration, and run identity — will read
better than grouping by severity, since a reader arrives holding a message, not a severity.

Each entry gives the severity, the condition that triggers it, and a citation to the source
location that produces it. Severities come from the source, not from plausibility: `CONTEXT.md`'s
**Validation Issue** entry records that severity is an unconstrained string the engine only ever
sets to error or warning, and that there is no informational severity.

The section also states what the validation result carries besides issues — the normalized
workflow, notices, and graph metadata with its reachable, unreachable and dead-end node id lists —
and that an issue carries an optional scope naming the subflow a nested issue came from, which is
how a reader tells a root problem from a catalogued-subflow problem. It states plainly that a
version rejection never appears here at all: it is an ingest error raised before validation runs,
so a reader waiting for it on the issues channel waits forever. The document-grammar section
already says this; the catalog says it again because this is where a reader looks for it.

Several families are worth calling out rather than burying as rows, because they are where authors
get surprised: the input-bound rejections that cap subflow count, call-edge count, node count and
edge count across the root and its catalog together; the absolute working-directory enforcement
that applies to the workflow, the node, and both the pane-spawn and run-agent configs; the
extra-arguments allowlist, which rejects option-shaped arguments unless allowlisted per agent and
rejects any configuration key mentioning sandboxing or approval; the run-identity rules, including
the shell-metacharacter rejection in the user field; and the subflow call-cycle detection, which is
a warning rather than an error.

**Key interfaces:**
- The validation entry point and the per-kind validators it calls are the authority. The issues
  are assembled from string-producing branches scattered across the model module, which is why
  this catalog is hand-written and stays hand-written — there is no enum to enumerate, and turning
  these into one is an engine change the PRD excludes.
- The validation issue struct and the validation result struct define what an entry can carry;
  `CONTEXT.md`'s **Validation Issue** entry is the vocabulary.
- `docs/sources/workflow-schema-drift-260825.md` Part 1 section 1.9 is this issue's work order,
  together with items 26, 28, 30 and 33 from earlier sections, which are validation rules recorded
  under the node headings they constrain.
- Derive the issue inventory by sweeping the source for every site that **constructs** a validation
  issue — not merely those that push one onto the issues vector. Several are returned as errors or
  produced through a fallible accessor and never touch a `push`, among them the input-bound caps,
  the retry-count cap and the nested-catalog rejection that this brief separately requires you to
  document. Do not treat that list as closed — the predicate is the authority, not the examples. A
  sweep keyed on `push` silently omits all of them.
- **State the unit before you count.** One construction site can serve several distinct triggers:
  the extra-argument rejector is a single site fed by several different reason strings, and the
  absolute-cwd check is a single site fed by several different labels. The catalog's unit is the
  **distinct trigger condition a reader could be holding a message for**, not the construction
  site — that is what makes an entry findable from an error message. Report the sweep in those
  terms, and say which convention you used.
- Do not take a count from this brief or from the drift audit as the target; sweep, then report
  what the sweep found.

**Acceptance criteria:**
- [ ] `## Validation catalog` documents both severities the engine emits and states that there is
      no informational severity. Observable:
      `rg --pcre2 --multiline -n '(?s)^## Validation catalog\n(?:(?!^## ).)*?\bwarning\b' docs/workflow-schema.md`
      matches; no match before this change. Anchored on `warning` rather than `severity`, because
      ISSUE-260826-0637-03 leaves this section a placeholder that may well use the word `severity`
      while deferring to the source — which would make a `severity` anchor green with no catalog
      written.
- [ ] Every site in the source that constructs a validation issue is represented by an entry,
      including those returned as errors rather than pushed. Observable at the seam: sweep the model
      module for issue-construction sites under every construction form, list them, and confirm each
      maps to a catalog entry — expanding any site that serves several distinct trigger conditions
      into one entry per condition. Report the sweep's size and the unit you counted in, in the
      closing note.
- [ ] No entry claims a severity the source does not produce for that condition. Spot-checkable at
      the seam on the two the old document got wrong — a task node without a prompt is a warning,
      and the terminal-node message is a warning whose wording is about terminal nodes rather than
      dead ends.
- [ ] Graph metadata is documented as a field of the validation result rather than as an issue or
      a severity. Observable: the section names the reachable, unreachable and dead-end lists as
      metadata.
- [ ] The catalog states that a version rejection is an ingest error that never reaches the issues
      channel.
- [ ] The scope field is documented as naming the subflow a nested issue came from.
- [ ] The extra-arguments allowlist and the absolute working-directory rule each have an entry
      naming the configuration surfaces they apply to.
- [ ] The section states when validation runs and where it is enforced — advisory on an explicit
      validate request, enforced at run start, never on save — so a reader knows what a clean
      catalog does and does not promise.
- [ ] Every entry cites the source location that produces it.
- [ ] `just check-v4-docs` passes and `cargo test` is green, generator freshness included.

**Out of scope:**
- Editing anything between generated markers.
- Refactoring validation issues into an enum so the catalog could be generated. A real engine
  change, excluded by the PRD.
- Runtime failures. This catalog covers the issues validation produces; failures that occur while a
  run executes belong to `docs/execution-model.md` and ISSUE-260826-0637-08. Where the boundary is
  genuinely unclear — a rule validation checks but the runtime resolves at call time — say which
  side does what rather than picking one. **Do not frame any of this as save-time.** Saving a
  workflow does not validate it: the save handler normalizes and writes, validation runs on an
  explicit validate request, and enforcement happens at run start. `CONTEXT.md`'s **Validation
  Issue** entry bans the phrase, and sibling ISSUE-260826-0637-06 writes the same framing into an
  earlier section of this document — the two must agree.
- Any change under `src/` or `ui/`.
- Describing validation of v5 features as present.

## Context Pack — generated at claim (2026-08-30T08:08:27Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01):
- The validation catalog is **Tier P** — hand-written, source cross-referenced; each entry cites the source location that owns it, so a reader verifies in one jump.
- Refactoring validation issues into an enum so the catalog could be generated is explicitly Out of Scope — a real engine change with its own blast radius.
- No engine code changes: this epic reads `src/`, writes `docs/`. No edits under `src/` or `ui/` at all.
- Honest-limits list names the validation catalog as not machine-checked: the issues are assembled from string-producing branches scattered across `model.rs` (the PRD's own figure of 86 is a point-in-time count, not a target — sweep and report).
- The version/migration contract is Tier C, not Tier P: version rejection is an `anyhow::bail!` at ingest and never reaches the validation-issue channel; the version is force-rewritten on every path reaching validation, so validation can never reject one.
- `docs/workflow-schema.md` is restructured, not patched: the validation catalog sits with the other Tier P tables after the generated node catalog.
- Terminology follows `CONTEXT.md`, honouring its `_Avoid_` lists; unmarked entries are true of `src/` at HEAD, `_(planned — ADR-…)_` entries are not accepted by the engine yet.
- Generated blocks live only between `<!-- BEGIN GENERATED: … -->` markers and are never hand-edited — the catalog is prose outside them.
- User story 13 is this slice: the catalog must reflect what the engine actually rejects so an author can predict a failed run before starting one. Adjacent stories 19 (absolute-cwd) and 20 (`extraArgs` allowlist) name two families this slice must surface.

**Test seam & Testing Decisions:** observable at the `docs/workflow-schema.md` `## Validation catalog` section, verified by `rg --pcre2 --multiline` anchored on `warning`, plus a source sweep of the model module for every issue-**construction** site (not merely `push` sites), counted in **distinct trigger conditions a reader could hold a message for** rather than construction sites; `just check-v4-docs` and `cargo test` (generator freshness included) must be green. Testing Decisions that touch it: the primary guarantee for generated artifacts is generation + a freshness diff, not an assertion, and it gates only the generated blocks — this hand-written section is protected by review against citations, not by CI; check 4 pushes every generated example through `ensure_defaults` + `validate_workflow` requiring zero `error`-severity issues, and states there is no save-time gate — validation is enforced at run start; validation checks some things the runtime resolves later (`parallel_batch.bodyEntry` existence vs. dispatchability; subflow input sources name-checked vs. resolved at call time), the boundary this slice must describe rather than pick a side on.

**ADRs:** none in this record's frontmatter. Nearest relevant from `docs/adr/INDEX.md` (forward references only — do not document as present):
- ADR-260815-2009-01 — Typed workflow contracts over a JSON-valued variable store · accepted (the decided, unlanded v5 bump).
- ADR-260815-2009-02 — One condition dialect: owned nested AST, typed operators, onMissing · accepted.

**Terms:**
- `Validation Issue` — one entry in a validation result `{severity, nodeId?, scope?, message}`; `severity` is an unconstrained string the engine only ever sets to `error` or `warning` — there is no `info` severity; `scope` names the subflow a nested issue came from; graph metadata rides the result alongside the issues rather than as one; validation is advisory where requested and enforced only at run start — saving never runs it — and a version rejection is an ingest error that never reaches this channel. _Avoid_: save-time validation, "validation error" for the whole set, lint. ↔ `src/model.rs` (`ValidationIssue`, `ValidationResult`), `src/api.rs` (run-start enforcement).
- `Node Kind` — the fourteen-variant tagged union in a node's required `kind` object, selecting both what the node does and which config shape it carries. _Avoid_: step type.
- `Subflow` — a workflow invoked as a callable block from another workflow via a call frame. _Avoid_: sub-workflow, child workflow.
- `Access Profile` — the named argv bundle a `spawn` or `run_agent` node selects with `access`, validated against the names the Agents Registry declares for that agent, by the same validation pass as everything else (advisory on request, enforced at run start, never on save), and applied outside the agent-defaults merge chain; distinct from `accessMode`, the four-variant `read_only | edit | execute | unrestricted` driver-level enum defaulting to `execute`. _Avoid_: access mode, bare "profile".

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md · docs/adr/INDEX.md · CONTEXT.md · docs/sources/workflow-schema-drift-260825.md (Part 1 §1.9, items 26/28/30/33)

## Code Review

Review file: `issue-260826-0637-07-code-review-20260830-083809.md` (Claude + Codex, cross-verified, adjudicated inline; 3 fix rounds, ACCEPTED)

- H1 (HIGH): Dead-end metadata and the terminal-node warning falsely described as different predicates — FIXED
- H2 (HIGH): Wait-marker regex validation is not limited to `until` mode — FIXED
- H3 (HIGH): Command-driven `spawn` nodes bypass `access` validation entirely — FIXED
- M1 (MEDIUM): `scope` presented as a universal root-vs-subflow discriminator — FIXED
- M2 (MEDIUM): Coverage gap — `src/model.rs:1385-1390` construction site has no entry — FIXED
- M3 (MEDIUM): Call-edge cap row overstates what is counted — FIXED
- M4 (MEDIUM): The seven `extraArgs` rows have no path to their construction site — FIXED
- L1 (LOW): Two rows use Rust/message field names where the document uses wire names — FIXED
- L2 (LOW): Unreachable-node warning row does not say it is root-graph only — FIXED
- L3 (LOW): "warnings do not fail the response" implicates that errors do — FIXED (rounds 2 and 3; two fix-round regressions folded in)
- L4 (LOW): Two kind-agnostic node-field rules filed under "Graph topology and edges" — FIXED
- L5 (LOW): Nested-catalog row cites outside the preamble's function range — dismissed
- L6 (LOW): `save_workflow` citation carries no line numbers — dismissed
- L7 (LOW): Inconsistent numeric formatting across the bounds rows — dismissed
- L8 (LOW): Sweep size not reported in the schema document — dismissed
- L9 (LOW): H1's rewrite cites a blank line for the root-only-metadata claim — FIXED (round-1 regression)
- L10 (LOW): "propagates its own error" understates a hydration failure, which is always 500 — FIXED (round-2 regression)

17 distinct findings: 13 FIXED, 4 dismissed, 0 deferred, 0 unfixed. By severity: 3 HIGH, 4 MEDIUM, 10 LOW. Fix rounds used: 3 of 4.

Smells: 1 advisory (borderline Duplicated Code, `docs/workflow-schema.md:1603`); not promoted — cited second sites lie outside the issue diff. Ledger: appended. No graduation rows.

## Resolution

**Commit:** `feat: write the validation catalog covering every error and warning the engine emits (ISSUE-260826-0637-07)`

**Route:** `cursor` (`/cursor-developer`) for the implementation and all three fix rounds. Rationale:
single-file documentation work in `docs/workflow-schema.md` with bounded source verification — no
cross-module editing, no security logic, no architectural ambiguity. Re-routed independently before
each fix round; `route-picker` returned `cursor` every time.

**TDD:** `n/a (linear)` — documentation work. The acceptance criteria are observable at the
`## Validation catalog` section itself, checked by an `rg --pcre2 --multiline` anchor plus
`just check-v4-docs`, not by a red-green test seam.

**Sweep report (the closing note the acceptance criteria require):**

- **Unit counted:** the **distinct trigger condition a reader could be holding a message for**, not
  the Rust construction site. This is the unit the brief specifies, because it is what makes an entry
  findable from an error message.
- **Construction sites swept:** **86** in `src/model.rs`, under every construction form —
  `issues.push(ValidationIssue`, `return Err(ValidationIssue`, `ok_or_else(|| ValidationIssue`, and
  match-arm pushes — not merely sites that `push`. 70 error, 16 warning. The count was derived
  independently three times: by the implementer, and by each of the two reviewers separately; all
  three agreed on 86.
- **Catalog entries written:** **98 rows** (the initial pass wrote 97; review finding M2 added one).
  The expansion from 86 sites to 98 rows comes from sites serving several distinct triggers —
  `validate_absolute_cwd` → 5 surface rows, `reject_agent_extra_arg` → 7 reason rows, the
  split/collector task-fields site → 2, the wait-timing site → 2.
- **Coverage:** all 86 sites map to an entry, verified programmatically by the Claude reviewer on
  re-verification. The one gap the initial pass left — the defensive `ok_or_else` at
  `src/model.rs:1385-1390`, unreachable from its only caller — was caught as M2 by both reviewers and
  is now documented as an internal invariant rather than silently omitted.

**Review telemetry:** dual review (Claude + Codex), cross-verified adversarially, adjudicated inline.
**17 distinct findings — 3 HIGH, 4 MEDIUM, 10 LOW.** Outcomes: **13 FIXED, 4 dismissed, 0 deferred,
0 unfixed.** **3 of 4 fix rounds used.** 15 findings came from the initial review; 2 (L9, L10) were
regressions the fix rounds themselves introduced, and 2 further regressions were folded back into L3
rather than given IDs.

Worth recording for the TDD'd-vs-linear comparison: **every regression in this issue came from newly
composed prose asserting source behaviour** — first the fix agent's, then the orchestrator's own
replacement text. Rounds 2 and 3 removed that by supplying exact, source-verified text and by
instructing both reviewers to audit *the orchestrator's* text rather than the fix agent's compliance.
That instruction is what caught L10 and the round-2 regression. Codex conceded its one outstanding
disagreement (L10) on the source after re-reading, not on authority. Codex also contributed two
findings Claude's pass missed entirely (H2, M4) — the dual-reviewer arrangement earned its cost here.

**Suite:** `SUITE: PASS`. `just check-v4-docs` exit 0 (generator freshness included — no marker block
disturbed); Rust `cargo test --locked` 472 passed / 0 failed / 1 ignored; UI vitest 129 passed / 0
failed across 13 files. `just test-e2e` was not run: it is not part of `just test` (which this
project's `CLAUDE.md` defines as the Rust suite plus `npm test`) and the acceptance criteria name
only `just check-v4-docs` and `cargo test`. One pre-existing Rust compiler warning in `src/` is
unrelated to this diff and untouched by it.

**Smells:** 1 advisory (borderline Duplicated Code at `docs/workflow-schema.md:1603`), appended to
`docs/issues/SMELLS-LEDGER.md`. Not promoted to a finding — the in-diff promotion rule requires every
cited site to lie inside the issue diff, and the second sites are pre-existing text from sibling
issues. No graduation rows emitted.

**Not fixed, recorded deliberately:** the final reflow left one 24-character orphan line and four
lines one character over the file's ~100-column wrap. Markdown joins them, so rendering is unaffected.
Both reviewers saw it and judged it not worth a fix round; noted here so a later reader knows it was
seen rather than missed.

**Date:** 2026-08-30 (UTC)

## Triage Notes

**Readiness gate (cold-reader): PASS** (round 3)

Round 1 found the sweep predicate under-inclusive and a decaying figure; round 2 found the
save-time framing, which had been corrected in three other artifacts and missed here. Both were
fixed and a criterion was added pinning where validation is enforced. Round 3 ran the full
nine-class rubric, re-derived the severity inventory and construction-site total from source,
and confirmed no surviving save-framing anywhere in the record.
