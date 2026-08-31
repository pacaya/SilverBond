---
id: ISSUE-260831-0626-01
kind: issue
category: bug
status: ready-for-agent
summary: docs-catalog test carries a copy of a private validator predicate that drifts silently
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
