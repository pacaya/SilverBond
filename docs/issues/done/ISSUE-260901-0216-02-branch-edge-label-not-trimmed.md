---
id: ISSUE-260901-0216-02
kind: issue
category: bug
status: done
claimed_by: implement-issue@macmini
claimed_at: 2026-09-01T08:17:20Z
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

## Context Pack — generated at claim (2026-09-01T08:17:20Z)

**PRD decisions relevant to this slice:** no PRD linked on this record (no `prd:` frontmatter); the only in-repo PRD, `PRD-260826-0009-01` (docs-from-Rust-truth), is the epic this bug was filed out of during audit, not a decision source for the fix.

**Test seam & Testing Decisions:** observable at the workflow-store mutation the edge inspector commits a branch edge label through — the acceptance criteria assert on the *stored* label (`"yes "` → `"yes"`; whitespace-only → absent, asserted by absence rather than falsiness), not on the keystroke handler. Testing decisions that touch it: the frontend unit suite (`npm test` → vitest) mocks validation and cannot reach the Rust validator, so the third criterion (editor-produced document passes backend validation) has no home in the unit suite; the only harness running editor output against a real backend is the Playwright e2e suite, which `npm test` does not invoke and no CI workflow runs. Place that test where it will actually execute, or record in the resolution why it lives where it does. Backend remains authoritative for validation (project convention); this record does not change the padded-label rule.

**ADRs:** no `adrs:` frontmatter on this record. Nearest neighbours in the index, context only:
- ADR-260815-2009-01 — Typed workflow contracts over a JSON-valued variable store · accepted
- ADR-260809-1601-01 — Adopt the shared record conventions for issues, decisions, and reviews · accepted

**Terms:**
- `Decide Outcome` — one label in a Decide Node's declared `outcomes` list, matched **exactly** against a branch edge's `label`; an outcome with no matching branch edge is a validation error. (Exact matching is why a padded edge label breaks the pairing.) Avoid the bare word "outcome" — that is Edge Outcome.
- `Decide Node` — the node that asks an LLM to pick one of its declared Decide Outcomes and routes on the result, bypassing the task pipeline.

**Full artifacts:** docs/adr/INDEX.md · CONTEXT.md (§ "Agents, decisions, and panes") · docs/issues/ISSUE-260826-1648-02 (the validation change this is fallout from) · docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md

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

## Code Review

Review file: `issue-260901-0216-02-code-review-20260901-090151.md` (Claude + Codex, cross-verified, adjudicated inline)

**ACCEPTED** after 4 fix rounds of 4. 18 findings: 12 FIXED, 2 dismissed, 4 deferred. Both reviewers independently recommended closing.

- H1 (HIGH): `just typecheck` fails, so acceptance criterion 5 is not met — FIXED
- M1 (MEDIUM): e2e test bypasses the editor seam that acceptance criterion 3 names — FIXED
- M2 (MEDIUM): `setEdgeLabel` normalizes every edge outcome, rewriting collector merge keys — FIXED
- M3 (MEDIUM): the ~55-line workflow fixture is duplicated across both new test files — FIXED
- M4 (MEDIUM): blur-only commit lets Cmd+S save the pre-edit label — FIXED (redesign; same-site cluster with L6)
- M5 (MEDIUM): the loosened e2e predicate can capture the pre-edit validation request and flake — FIXED
- M6 (MEDIUM): an edit that normalizes to the stored value leaves a stale draft visible after undo — deferred (display-only; stored document correct throughout; better than the pre-change baseline)
- L4 (LOW): JS `.trim()` and Rust `str::trim()` disagree on U+0085 — FIXED (closed in both directions)
- L5 (LOW): acceptance criterion 3's test lives in a harness nothing runs — FIXED (discharged in `## Resolution` below)
- L6 (LOW): trim-on-`oninput` desyncs the visible field from the document — FIXED
- L7 (LOW): new e2e spec couples untypechecked test code to an app source path — FIXED
- L8 (LOW): unit test passes the pre-store edge object as the prop — FIXED
- L9 (LOW): no positive control on the e2e filter — dismissed (pre-existing Rust control at `src/model.rs:6224-6239`)
- L10 (LOW): test-local `document` shadows the jsdom global — dismissed (verified inert; advisory smell)
- L11 (LOW): changing an edge's outcome to `branch` does not renormalize its existing label — FIXED
- L12 (LOW): a normalization regression surfaces as an opaque 30s timeout — FIXED
- L13 (LOW): forward outcome transition can normalize into a sibling branch-label collision — FIXED (guard deleted; the finding's own premise was falsified and its author conceded)
- L14 (LOW): committing on every keystroke restores a full-document clone and undo entry per character — deferred (neutral against the pre-change baseline)
- L15 (LOW): the reconciliation invariant has a second copy in the display gate — deferred (reviewers disagree whether it is redundancy or defence-in-depth; not a defect either way)
- L16 (LOW): the component reads the same edge through two references — deferred (structural; no behavioral consequence)

Smells: 9 advisory in `docs/issues/SMELLS-LEDGER.md` (all count 1); a tenth — the duplicated fixture — was promoted to finding M3 because every cited site lay inside the issue diff. No graduation rows.

## Resolution

**Commit:** `fix: trim branch edge labels at the workflow-store commit seam (ISSUE-260901-0216-02)`

**Route:** cursor (`/cursor-developer`) for the implementation and all four fix rounds. Chosen by `route-picker` each time on the same rationale: well-scoped frontend work — a Svelte component, a store mutation, a trim utility, and two test suites — with no DevOps, cross-layer security, or ambiguous-spec signal that would justify codex.

**TDD:** `red-green at the workflow-store mutation the edge inspector commits a branch edge label through` — the acceptance criteria name the stored label rather than the keystroke handler, so the seam is the store mutation.

**What shipped.** Branch edge labels are normalized where they are committed to the workflow, not in the keystroke handler: `workflowStore.setEdgeLabel` trims on the Unicode `White_Space` property — matching Rust's `char::is_whitespace` rather than ECMAScript's narrower set — and maps a whitespace-only label to absent. Normalization is scoped to `outcome === "branch"`; every other outcome keeps the prior `rawLabel || null` mapping, because a collector inbound edge's label *is* its merge key (`src/runtime.rs`) and trimming it would silently rewrite runtime behavior. `setEdgeOutcome` renormalizes an existing label on a forward transition to `branch`, so a padded label cannot arrive at a branch edge by changing the outcome instead of the label. The inspector keeps a focus-scoped draft so the visible text stays byte-for-byte what the user typed while the store holds the normalized value, with the draft discarded whenever focus is lost, the selected edge changes, or the store diverges.

**Where acceptance criterion 3's test lives, and why** (discharging the readiness gate's second branch). The criterion asks that an editor-produced document pass real backend validation. Nothing `npm test` reaches can satisfy that: the vitest suite mocks validation and cannot invoke the Rust validator. The assertion therefore lives in `ui/e2e/branch-edge-label-trim.spec.ts`, the only in-repo harness that runs editor output against a real backend — it loads the fixture through the app, clicks the edge, types the padded label into the rendered Label field, blurs, captures the document the store actually POSTs, and re-submits that document to `/api/validate-workflow` asserting zero error-severity issues. The gate's caveat still stands and is recorded rather than papered over: that suite runs only under `just test-e2e` and no CI workflow invokes it, so this criterion is not enforced by `just test`. Both reviewers mutation-proved the spec is load-bearing — removing the normalization from the store makes it fail with a real value diff.

**Review telemetry.** 18 findings across a round-0 dual review and 4 fix rounds: 1 HIGH, 6 MEDIUM, 11 LOW. Outcomes: 12 FIXED, 2 dismissed, 4 deferred. Fix rounds used: 4 of 4. Both reviewers independently recommended closing.

Three things worth carrying forward. The same-site escalation rule fired on the label field and shaped rounds 2-4: L6's fix caused M4, M4's redesign caused CODEX-5, and each round's finding was narrower than the last. Round 2 was therefore run as a redesign against five stated properties rather than another patch, and the residue (M6) was deferred rather than given an unverified fifth edit after the cap. Second, cross-verification twice overturned a reviewer's own position on evidence: Codex's CODEX-6 premise — that the backend rejects duplicate sibling branch labels — was falsified against `src/model.rs`, where branch labels collapse into a `BTreeSet` and no such rule exists, so the guard it motivated was deleted in round 4 and its author conceded. Third, a fix agent once reported a finding FIXED that was not (L7's import removal), which both reviewers caught; the later rounds' briefs demanded a failing check behind every FIXED claim.

**Deferred, with reasons in the review file:** M6 (a stale draft stays visible after an undo that was a no-op for that field — display-only; the stored document is correct throughout and this is strictly better than the pre-change baseline, which stored the padded value), L14 (per-keystroke undo clone — neutral against the pre-change baseline), L15 (the invariant has a second copy in the display gate — the reviewers disagree whether that is redundancy or defence-in-depth, and it is not a defect either way), L16 (two references to the same edge — structural, no behavioral consequence).

**Suite.** `just test` — the project's documented full suite and the criterion the brief names — passes: `cargo test --locked` 461 + 8 + 31 + 17 passed, 1 ignored; vitest 15 files / 142 tests passed. `just typecheck` passes: 618 files, 0 errors, 0 warnings (618 rather than the original 614 because this change pulled `ui/e2e/**` into the typecheck graph). `just test-e2e` has **6 pre-existing failures outside this slice**, in `app.spec.ts` and `run-agent-pane-stream.spec.ts`: five share one root cause, `POST /api/runs` returning 403 from `privileged_unlock_required()` because the e2e environment sets neither `SILVERBOND_UNLOCK_PASSWORD_HASH` nor `SILVERBOND_AGENT_USER`, and the sixth is an unrelated WebSocket frame assertion. Neither spec imports any module this diff touches, and that suite is not run by `just test` or by any CI workflow, so the failures predate this work and are untouched by it. This slice's own e2e spec passes.

**Date:** 2026-09-01 (UTC)
