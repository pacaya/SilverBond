---
id: ISSUE-260901-0216-02
kind: issue
category: bug
status: ready-for-agent
summary: The edge inspector saves branch-edge labels with leading or trailing whitespace, producing documents that backend validation now rejects with no hint in the editor
---

## Agent Brief

**Category:** bug
**Summary:** Make the editor's branch-edge label input agree with the validation rule that now
rejects padded labels.

**Current behavior:**
Backend validation rejects a `decide` node's declared outcome label, and a `branch` edge's label,
when either carries leading or trailing whitespace. Both produce an error-severity validation
issue naming the padded label.

The editor does not agree with that rule on the edge side. Several inspector inputs normalize
their text before committing it to the workflow — trimming is the established convention across
the editor's field components. The edge inspector's label input is the exception: it commits the
raw input value, mapping only the empty string to absent. A user who types a trailing space into a
branch edge's label therefore saves a document that the backend rejects, and the editor surfaces
nothing at the point of entry.

This is a regression introduced by the validation change, not a pre-existing gap: before that
rule landed, a padded label was accepted and merely failed to match its outcome at run time.

**Desired behavior:**
A branch edge label entered through the editor never reaches the workflow document with leading or
trailing whitespace. Typing `"yes "` into the label field yields the stored label `"yes"`, matching
what the corresponding `decide` outcome input already does.

Whitespace-only input is equivalent to empty input: the label becomes absent rather than becoming
an empty-string label, preserving the existing behavior where clearing the field removes the label.

Normalization happens where the value is committed to the workflow, so a label arriving through any
path that commits through the same seam is normalized too, rather than only the keystroke handler.

**Key interfaces:**
- The edge inspector component's label field — currently commits the input's raw value with an
  empty-string-to-null mapping and no normalization.
- The workflow store mutation the edge inspector commits through — the seam where the normalized
  value must be observable.
- The backend's padded-label validation rule — the contract this change brings the editor into
  agreement with. This record does not change that rule.

**Acceptance criteria:**
- [ ] A frontend unit test asserts that committing a branch edge label of `"yes "` through the
      edge inspector's label seam stores `"yes"`. `rg -n 'yes ' ui/src/features/editor/EdgeInspector.test.ts`
      returns the padded fixture literal; the file has no such assertion before this change.
- [ ] A test asserts that committing a whitespace-only label stores an absent label, not an empty
      string, distinguishing the two by asserting on the stored value's absence rather than on
      falsiness.
- [ ] A workflow whose branch edge label is entered as padded text through the editor seam passes
      backend validation. The test asserts the resulting document produces no error-severity
      validation issue naming a padded label — identified by that issue's message text, which the
      passing path never emits.
- [ ] `npm test` passes.
- [ ] `just typecheck` passes.

**Out of scope:**
- The `decide` node's outcome label inputs, which already normalize.
- Changing, relaxing, or removing the backend padded-label validation rule.
- Any broader normalization sweep across other inspector inputs. This record covers the branch
  edge label only; the trimming convention elsewhere is context, not scope.
- Surfacing validation errors in the editor UI generally. Preventing the invalid value is the
  fix here; a general validation-feedback surface is separate.

## Triage Notes

Filed 2026-09-01 during a completeness audit of the `docs-truth` epic, from a residual recorded in
`ISSUE-260826-1648-02` and described there as "the one residual with a user-visible consequence;
worth filing separately". It was never filed.

Confirmed at audit time: the edge inspector's label handler assigns the input value with an
`|| null` fallback and no `.trim()`, while sibling field components in the same directory normalize
their input before committing. `ISSUE-260826-1648-02` is what made the resulting document invalid,
so this is fallout from that record rather than an independent defect.

**Readiness gate (cold-reader): PASS** (round 1, 2026-09-01)

Gate notes, carried for the implementer (non-binding):

- The third acceptance criterion asserts an editor-produced document passes backend validation. The
  frontend unit suite mocks validation and cannot reach the Rust validator; the only in-repo harness
  that runs editor output against a real backend is the Playwright e2e suite, which `npm test` does
  not invoke and which no CI workflow runs. Place that criterion's test where it will actually be
  executed, or say in the resolution why it lives where it does.
- The first criterion's discovery command is unanchored — it is red today because the target test
  file does not exist, not because the pattern is specific. Prefer a tighter assertion once the
  file exists.
