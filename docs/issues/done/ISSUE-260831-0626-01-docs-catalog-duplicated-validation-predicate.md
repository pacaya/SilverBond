---
id: ISSUE-260831-0626-01
kind: issue
category: bug
status: done
summary: docs-catalog test carries a copy of a private validator predicate that drifts silently
claimed_by: implement-issue@macmini.home
claimed_at: 2026-08-31T20:43:00Z
---
## Agent Brief

**Category:** bug
**Summary:** Delete the docs-catalog test's copy of the validator's task-execution-field predicate; assert through the validator's warning instead

**Current behavior:**
The docs-catalog integration test — the one that generates and verifies the
node-catalog blocks embedded in the workflow schema documentation — defines its
own `has_task_execution_config`, duplicating the private predicate of that name
in the workflow validator. It uses the copy to assert that its generated example
workflows put no task-execution fields on `Split` or `Collector` nodes.

The same test function then asserts that validation produced no warning
containing the ignored-task-fields phrasing. The validator raises that warning
under the same predicate, across root and subflow bodies alike, so the copy
covers nothing the warning assertion does not already cover — and it goes stale
without failing when the validator's field list changes.

**Desired behavior:**
The test contains no copy of the predicate. Fixture cleanliness is enforced
solely by the assertion on the validator's warning, keeping the backend
authoritative for validation semantics. An example workflow that puts
task-execution fields on a `Split` or `Collector` node still fails the suite.

**What to do:**
- Delete the test-local `has_task_execution_config` and the loop over validated
  root nodes that panics on it.
- Leave the assertion that filters validation issues for the ignored-task-fields
  warning phrase unchanged. It becomes the sole enforcement of this property.
- Leave the validator's predicate private and untouched.
- Sibling helpers in the same test file derive enum variant tags out of serde
  rather than restating them. Match that style: derive from the authoritative
  source, never restate it.

**Acceptance criteria:**
- [ ] `rg -n 'fn has_task_execution_config' tests/` returns no matches; the
      definition is present under `tests/` before this change.
- [ ] `rg -n 'fn has_task_execution_config' src/model.rs` still returns the
      validator's definition — present before and after this change.
- [ ] `rg -n 'so task execution fields are ignored' tests/docs_catalog.rs` still
      returns the assertion filtering for that warning phrase — present before
      and after this change.
- [ ] `cargo test --locked` passes — green before and after this change.
- [ ] Positive control, on a scratch copy of the tree made after the change and
      discarded afterward: give the `Split` example node in the catalog example
      builder a non-empty `agent` value, changing nothing else. `cargo test
      --locked --test docs_catalog` then fails, and the failure output contains
      the ignored-task-field warning assertion's own message — a string the
      passing suite never emits. Reverting the mutation restores green.

**Out of scope:**
- Making the validator's predicate `pub` or `pub(crate)` so the test can import
  it. The surviving assertion already covers the property.
- Changing which fields count as task-execution configuration, or when the
  validator emits the ignored-task-fields warning.
- Deduplicating the repeated panic arms in the catalog length and set-equality
  assertions in the same file.
- The other smell-ledger rows against this test file; they are retired
  separately.

## Context Pack — generated at claim (2026-08-31T20:43:00Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01 — docs from Rust truth; the record carries no `prd:` frontmatter, so this is the epic the docs-catalog generator was built under):
- The node catalog is generated from Rust values rather than hand-written, so a renamed or misspelled field fails `cargo test` rather than shipping as documentation.
- Catalog contents are checked against wire-tag lists **derived from serde** for both node enums — enumerations are never restated by hand; that is the same anti-duplication rule this issue applies to the validation predicate.
- The backend stays the single authority for validation semantics; the docs suite observes it rather than re-implementing it.

**Test seam & Testing Decisions:** observable at Testing-Decisions check 4 — "structural validity, across the document boundary": every generated example workflow is serialized, read back with strict deserialization, pushed through `ensure_defaults` + `validate_workflow`, and asserted against the resulting issue list. The acceptance criteria's surviving assertion filters that issue list for the warning phrase "so task execution fields are ignored". PRD principle governing this slice: "a good test here observes what a reader would observe … neither assertion reaches into how the doc is written or how the parser is structured" — a test-local copy of a private validator predicate is exactly such a reach-in. Checks 1-3 (compile-time exhaustiveness, freshness/coverage/mapping, `NodeKind` coverage) are untouched by this change.
- Coverage note the brief relies on: the validator's warning path runs for the root graph and again per subflow body, so the warning assertion strictly dominates the deleted root-nodes-only loop.
- The validator's warning has its own unit test in its module, so deletion leaves nothing unguarded.

**ADRs:**
- No `adrs:` frontmatter on this record; the ADR index lists no ADR governing docs generation or validation-test structure. Nearest neighbours are schema-change ADRs (typed contracts, condition dialect) that this slice must not touch.

**Terms:**
- No `terms:` frontmatter on this record. Working vocabulary is repo-standard: *node catalog* (generated marker-delimited blocks in the schema reference), *validation issue* (severity + message emitted by the validator), *task-execution fields* (the field set the validator's private predicate names).

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md · docs/adr/INDEX.md · CONTEXT.md

## Code Review

Review file: `issue-260831-0626-01-code-review-20260831-211010.md`

Dual review (Claude + Codex), cross-verified, adjudicated inline. Codex: zero findings at its
>=75 confidence gate. Claude: two LOW findings, both DISAGREEd by Codex at cross-verify; both
split verdicts adjudicated by reading the cited code.

- LOW-1 (low): Sole surviving guard is coupled to the validator's message wording — dismissed
  (the coupling is what the brief's "What to do" mandates, and `src/model.rs:3982`/`:3987`
  assert on strict superstrings of the filter, so a reword fails loudly there first)
- LOW-2 (low): `SMELLS-LEDGER.md:96` still `open` for the now-deleted site — dismissed as a
  finding against the diff (`docs/issues/` is excluded review scope, row unchanged from
  `9f5c7ed`), but acted on as orchestrator bookkeeping: closed as `fixed: ISSUE-260831-0626-01`

Fix rounds used: 0 of 4 — no finding survived adjudication.

Smells: 1 advisory (`tests/docs_catalog.rs:582`, Duplicated Code, borderline — the warning
phrase as a bare literal at three sites). Not promoted: the in-diff duplication rule requires
every cited site inside the diff, and this diff is a pure deletion. Merged to the ledger as
`appended`.

## Resolution

**Commit:** `fix: delete the docs-catalog test's copy of the validator task-execution predicate (ISSUE-260831-0626-01)`

**Route:** cursor (`/cursor-developer`) — route-picker classified it as localized, well-specified
test refactoring with clear acceptance criteria in a single file. The whole change is a 28-line
deletion in `tests/docs_catalog.rs`; no production code touched.

**TDD:** n/a (linear) — mechanical deletion, no behavior change. The brief's AC5 positive control
supplies the falsification evidence a red-green cycle would have: both reviewers independently
executed it (mutate the `Split` catalog example's `agent`, confirm `--test docs_catalog` fails at
`tests/docs_catalog.rs:589` with the surviving assertion's own message, revert, confirm green).

**Review telemetry:** dual review (Claude + Codex), cross-verified, adjudicated inline.
Findings by severity: 0 CRITICAL, 0 HIGH, 0 MEDIUM, 2 LOW. Codex reported zero findings at its
>=75 confidence gate; Claude reported both LOW findings (confidence 60 and 40) and Codex
DISAGREEd with both at cross-verify. Both split verdicts were adjudicated by reading the cited
code. Outcome: 0 FIXED, 0 deferred, 2 dismissed. Fix rounds used: 0 of 4. Smells: 1 advisory
(`tests/docs_catalog.rs:582`, Duplicated Code, borderline), merged to the ledger as `appended`;
not promoted, because the in-diff duplication rule requires every cited site inside the diff and
this diff is a pure deletion. Ledger row 96 — the row that promoted this issue — closed as
`fixed: ISSUE-260831-0626-01`.

**Suite:** `just test` (`cargo test --locked` + `npm test`) — PASS. 623 passed (492 Rust across 6
binaries + 131 vitest), 0 failed, 1 ignored. No pre-existing failures. E2e/Playwright not part of
this target.

**Closed:** 2026-08-31 (UTC)

## Triage Notes

Surfaced from `docs/issues/SMELLS-LEDGER.md`, which labelled the site
"Speculative Generality". The label is wrong: the predicate is an exact copy of
a private validator rule, and the defect is silent drift.

Re-derive the situation:

- `rg -A16 'fn has_task_execution_config' src/model.rs tests/docs_catalog.rs` —
  the two bodies, for comparison.
- `rg -n 'validate_graph_body\(' src/model.rs` — shows the warning path runs for
  the root graph and again per subflow, so the warning assertion reaches nodes
  the deleted root-node loop could not.
- `rg -n 'warns_when_non_task_nodes_keep_task_execution_fields' src/` — the
  validator's warning is unit-tested in its own module, so deleting the test
  copy does not leave the warning unguarded.

Considered and rejected: exposing the validator predicate to the test. It would
keep a check that duplicates the warning assertion, at the cost of widening a
private validation internal.

- Scale snapshot (non-contractual): `rg -n 'fn has_task_execution_config' -g '*.rs' src/ tests/ | wc -l` → ~2 sites, one deletion (2026-08-31)

**Readiness gate (cold-reader): FAIL** (round 1) — class 7: decaying-state counts written beside discovery commands. Remedy applied: qualitative polarity substituted, commands unchanged. Brief subsequently rewritten to instruct rather than argue, so round 2 gates the rewritten text.

**Readiness gate (cold-reader): PASS** (round 2) — rewritten brief gated clean: classes 1-9 all non-firing, class-7 ledger 41 rows with zero blocking, class-9 arm A 5 rows / arm B 2 rows. Over-stripping check clear: nothing removed in the rewrite left a decision unpinned. Two non-blocking notes carried forward, neither owing an edit: (a) "the two bodies" in the re-derive list is a number-word beside a discovery command -- ruled fine because the command on the same line names exactly two files and the phrasing cannot change what gets built; (b) the sibling smell-ledger rows read `retired:` in the working tree but still `open` at HEAD, pending commit.
