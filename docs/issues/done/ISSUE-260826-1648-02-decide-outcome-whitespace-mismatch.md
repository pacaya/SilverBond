---
id: ISSUE-260826-1648-02
kind: issue
category: bug
status: done
origin: docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md
terms: [Decide Outcome]
claimed_by: implement-issue@Mac-mini-4.local
claimed_at: 2026-08-31T02:56:28Z
summary: Decide-node validation compares outcome labels to branch-edge labels untrimmed while the runtime trims, so a whitespace-padded label validates clean and then misses at runtime, reaching an arm that is otherwise unreachable
---

## Agent Brief

**Category:** bug
**Summary:** Reject whitespace-padded decide labels at validation, closing the one path by which a validated document reaches the fallthrough arm the docs call unreachable.

**Current behavior:**
Decide-node validation and decide routing disagree about surrounding whitespace.

Validation collects the labels of the node's outgoing branch edges into a set of raw strings and
tests each declared outcome for raw membership in that set. The one trim on that path is the guard
that rejects an empty outcome; the membership comparison itself sees both sides untouched. Read that
as scoped to the outcome-membership path, not to the routine — the routine trims in several other
places, including its own branch-edge label emptiness guard, and both sides of the correspondence
still need checking per **Key interfaces** below.

Routing trims the node's selected outcome before comparing it against the branch edge's raw label.

So a decide node declaring an outcome with surrounding whitespace, wired to a branch edge whose
label carries the same padding, validates clean — raw equals raw — and then fails to match when the
run reaches it, because a trimmed outcome is compared against an untrimmed label. The miss lands in
the arm that emits a workflow error and degrades to the plain success edge. Both the glossary entry
for the term and the execution-model documentation describe that arm as unreachable from a validated
document; this is the sole path found by which a validated document reaches it, so a run can route
down the success edge exactly where the documentation says routing cannot happen.

A related asymmetry sits inside outcome selection itself: the arm that reads a structured response
compares the model's label against the declared outcomes with neither side trimmed, while the arm
that falls back to free text compares against a trimmed response. Confirm this from the source
rather than from this brief, and see the note on it under **Out of scope**.

**Desired behavior:**
A decide node whose declared outcome labels, or whose outgoing branch-edge labels, carry leading or
trailing whitespace fails validation with an error-severity issue that names whitespace as the
reason. The invariant established is that a decide label is its own trimmed form; both sides of the
outcome/branch-label correspondence are held to it.

Routing is not changed. With padded labels refused at the document boundary, the existing raw
comparison at the routing site can no longer be reached with a padded label, and the fallthrough
arm becomes genuinely unreachable from a validated document rather than approximately so.

**Rejected alternative, and why.** Trimming both sides of the validation membership comparison is
the more obvious repair and is wrong here. The duplicate-outcome check inserts raw labels into its
seen-set, so two outcomes differing only in padding would pass as distinct and then collide onto a
single route once compared trimmed — converting a mis-route into an ambiguous one and requiring the
routing site to trim as well to stay coherent. Refusing padding is a single-point fix that needs no
runtime change and leaves no pair of distinct-but-equivalent labels in a valid document.

**Key interfaces:**
- The decide-node validation routine in the model module — it owns the outcome/branch-label
  correspondence, the empty-label guard, and the duplicate-outcome check. The new refusal belongs
  beside them, and must be checked against both the declared outcome list and the branch-edge labels
  of the node, not only whichever side the reported reproduction used.
- The validation issue type — error severity, node-scoped, following the message conventions of the
  sibling decide errors already emitted by that routine.
- The routing site that selects a branch edge for a decide node, and the outcome-selection helper
  it depends on — read for confirmation, not for modification.

**Binding constraint — the documentation framing does not change.** That the fallthrough arm is
unreachable from a validated document is a recorded maintainer decision, and the glossary entry for
the decide-outcome term carries an explicit `_Avoid_` clause banning any description of the success
fallthrough as behaviour an author can meet. That clause stands. This work removes the exception to
the claim; it must not soften, hedge, or restate the claim, and must not add prose anywhere
describing the padded-label route as something an author could rely on.

**Acceptance criteria:**
- [ ] A decide node declaring an outcome with leading or trailing whitespace, wired to a branch edge
      whose label carries the same padding, fails validation with an error-severity issue. This is
      the case that validates clean before this change, so the criterion goes from green to red at
      the document boundary and the new issue is what makes it red.
- [ ] The refusal is identified by a signal unique to this defect: the issue's message names
      whitespace or padding as the reason. Asserting merely that validation reports some error does
      not satisfy this — a padded branch-edge label paired with an unpadded outcome already produces
      the pre-existing "does not match an outgoing branch edge label" error, so a test keyed on
      error-presence alone is green before this change and proves nothing.
- [ ] A decide node whose outcome labels are padded is refused independently of how its branch edges
      are labelled, and a decide node whose branch-edge labels are padded is refused independently of
      how its outcomes are declared. Both sides carry the invariant; neither is refused only as a
      side effect of mismatching the other.
- [ ] A decide node whose labels carry no surrounding whitespace validates exactly as it does today,
      with the same issue set. (Preservation criterion: green before and after.)
- [ ] Routing behaviour for a valid decide document is unchanged — the same branch edge is selected
      for the same model output. (Preservation criterion: green before and after.)
- [ ] The validation catalog in `docs/workflow-schema.md` gains a row for the new error, among the
      decide rows of that catalog, matching the existing rows' `| Severity | Condition | Source |`
      column shape. Observable:

      ```
      rg -n '^\| error \| `decide`.*(whitespace|padding)' docs/workflow-schema.md
      ```

      returns the new row; it returns no matches before this change. The observable is anchored to
      the row shape rather than to the bare word so that an unrelated later use of "whitespace"
      elsewhere in the document cannot satisfy it while this record waits to be claimed.
- [ ] `just test` passes, including the docs catalog generator's committed-markdown assertion.

**Out of scope:**
- **Trimming anywhere in the runtime.** The routing site and the outcome-selection helper are read
  for confirmation and left alone. The structured-versus-free-text asymmetry noted above is expected
  to become harmless once no valid document can declare a padded outcome — confirm that this is so
  and say so in the change; if it turns out to survive the fix, file it rather than widening this
  issue.
- **The glossary entry and the execution-model prose.** Both already state the unreachability this
  work makes true, so neither needs editing. Per the binding constraint above, do not restate or
  soften them.
- **Whitespace policy for any other label, name, or identifier in the schema.** Edge labels outside
  decide nodes, node names, variable names, and outcome labels on non-decide kinds keep their current
  handling. This issue establishes the invariant only where the validator/runtime disagreement was
  demonstrated.
- **Unicode normalization, zero-width characters, or internal whitespace.** Leading and trailing
  whitespace only, by the same definition the existing empty-label guard already uses.
- **The regeneration story for the validation catalog's source citations.** That catalog is
  hand-written prose whose per-row source citations are not machine-checked, so edits to the model
  module shift line references in rows this issue did not touch. That fragility is pre-existing and
  owned by no one here; add the new row and leave the rest.

## Context Pack — generated at claim (2026-08-31T02:56:28Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01, reached via this record's `origin:`):
- User story 40: decide routing is documented in both stages, with the runtime's degrade-to-the-success-edge arm described as unreachable from a validated document — this issue removes the exception rather than the claim.
- **No engine code changes** in that epic: `docs-truth` reads `src/` and writes `docs/`; behaviour found wrong is filed separately. This issue is that separate filing, so it is *not* bound by that constraint and may edit `src/model.rs`.
- The validation catalog in `docs/workflow-schema.md` is **Tier P — hand-written, source cross-referenced**, not generated: each row cites the source location that owns it, and rows restate the emitted message. A new error means a hand-added row.
- The generated node catalog lives strictly between `<!-- BEGIN GENERATED: … -->` markers; the validation catalog sits outside every generated block, so a hand-written row cannot trip the generator's freshness assertion.
- Tier P is explicitly **not machine-pinned**: the 86 validation issues are assembled from string-producing branches scattered across `model.rs`, and per-row source citations go stale silently. Pre-existing and out of scope here.
- Terminology follows `CONTEXT.md`, including its `_Avoid_` clauses; the docs must not restate or soften the unreachability claim.

**Test seam & Testing Decisions:** observable at the document boundary — serialize a workflow, deserialize strictly with `serde_json::from_value::<WorkflowV3>()`, then `ensure_defaults` + `validate_workflow`, asserting on the resulting issue set (error severity + message text). The PRD's check 4 fixes this as the seam a reader's document actually meets; strict deserialization is used deliberately rather than `normalize_workflow_value`, because the v2→v3 migration fires unconditionally and would rescue malformed shapes. Saving never validates, so run-start validation is the only gate. Acceptance criterion 2 requires keying the assertion on the whitespace/padding wording, not on error-presence — three of the four padded shapes already error today. Criterion 6's observable is a row-shaped `rg` against the validation catalog; criterion 7 is `just test`, including the docs generator's committed-markdown assertion.

**ADRs:** no `adrs:` frontmatter on this record; the parent PRD carries ADR-260815-2009-01 (v5 schema bump, delivered by `typed-contracts`), which does not govern this slice.

**Terms:**
- `Decide Outcome` [VO] — one label in a Decide Node's declared `outcomes` list, matched exactly against a branch edge's `label`; an outcome with no matching branch edge is a validation error, so the runtime's emit-`workflow_error`-and-fall-through-to-success arm is unreachable from a validated document. _Avoid_: bare "outcome" (that is Edge Outcome); describing the success fallthrough as behaviour an author can meet.
- `Decide Node` [entity] — the node that asks an LLM to pick one of its declared Decide Outcomes and routes on the result, bypassing the task pipeline entirely (no orchestrator refinement, retries, session continuation, or agent defaults).

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md · CONTEXT.md · docs/workflow-schema.md · docs/execution-model.md


## Triage Notes

Found on 2026-08-26 while grounding `CONTEXT.md`'s **Decide Outcome** entry against `src/` for the
`docs-truth` epic, and independently rediscovered by the round-5 readiness gate on
`ISSUE-260826-0637-08`. Filed rather than folded into that epic: `docs-truth` is scoped to make no
engine changes and edit nothing under `src/`, and this needs a code change.

**The defect.** Validation and the runtime disagree about whitespace.

- Validation builds the branch-label set from raw `edge.label` values and tests membership with the
  raw `outcome` string. Its only emptiness guard trims, but the comparison itself does not.
- The runtime trims the model's output before matching it against the raw `edge.label`.

So a decide node declaring an outcome `"approve "` with a branch edge labelled `"approve "`
validates clean — the raw strings match — and then fails to match at runtime, because the trimmed
`"approve"` is compared against the untrimmed `"approve "`.

**Why it matters beyond the padding itself.** The miss lands in the arm that emits a
`workflow_error` and degrades to the plain success edge. Both `CONTEXT.md` and
`ISSUE-260826-0637-08` document that arm as **unreachable from a validated document**, which is
correct for every document an author would plausibly write — validation makes an outcome with no
matching branch edge an error, and an outcome the model never names fails the node instead. This
whitespace path is the sole exception found, and it means a run can silently route down the success
edge where the documentation says routing cannot happen.

**Not urgent, and the framing should not change.** The trigger is a pathological label and the
consequence is a mis-route rather than corruption. The documentation framing is a recorded
maintainer decision, and `CONTEXT.md`'s **Decide Outcome** `_Avoid_` clause bans describing the
fallthrough as behaviour an author can meet — that stands. Fixing the engine removes the exception
rather than the framing.

**Fix shape, not yet decided.** The obvious candidates are trimming on both sides of the validation
comparison, or trimming neither and rejecting padded labels at validation. Whoever picks it up
should check whether any other validator/runtime pair in the decide path has the same asymmetry
before choosing, rather than patching the one comparison.

**Readiness gate (cold-reader): PASS** (round 1)

Independent cold reader, no planning context, 2026-08-30. All seven acceptance observables and the
one discovery command executed against the tree rather than reasoned about. Classes 1–7 and 9 all
walked; none fired. Class 8 inert (never-stamped record).

Findings worth recording:

- **Class 1 resolved both cited authorities.** The `_Avoid_` clause is `CONTEXT.md`'s **Decide
  Outcome** entry, verbatim as the brief describes; the "recorded maintainer decision" resolves to
  `ISSUE-260826-0637-08`'s instruction to describe the arm as unreachable rather than as behaviour an
  author can meet, realized in `docs/execution-model.md` and originating in PRD-260826-0009-01's
  user story 40. The Out-of-scope claim that neither document needs editing was checked directly:
  neither contains "whitespace" or "padding".
- **The rejected alternative was verified, not accepted on assertion.** The reader confirmed the
  duplicate-outcome check inserts raw labels, so trimming only the membership comparison really would
  let two padding-distinct outcomes pass as distinct and then collide onto one route.
- **Class 9 arm B req. 2 confirmed necessary and satisfied.** The brief's claim that a padded edge
  label paired with an unpadded outcome already trips the pre-existing mismatch error was verified
  against the source. Of the four padded shapes, three are green-before under an error-presence-only
  test; only the identically-padded pair is red-before. Criterion 2's whitespace-naming requirement is
  what makes the other three falsifying. `rg -n -i 'whitespace|padding' src/model.rs` confirms no
  validation message currently names either, so the signal is genuinely new.
- **Criterion 6 observable confirmed red-before**: `rg -n -i 'whitespace' docs/workflow-schema.md`
  returns nothing (exit 1) on the current tree.
- **Blast radius checked beyond the brief's claims**: no shipped template carries a padded decide
  label, and the frontend has no mirrored decide validation to keep in sync.

Two non-blocking nits recorded, both verdict `fine`, addressed in round 2 below.

**Readiness gate (cold-reader): REOPENED** (round 2, 2026-08-30, acting on round-1 nits)

Reopening to act on both nits rather than leaving them. Neither changes what gets built; both make
the brief harder to misread and harder to rot.

1. **"The only place it trims"** is count-shaped prose that is false under a routine-wide reading —
   the branch-edge label guard trims too. The round-1 reader scoped it charitably to the
   outcome-membership loop, where it is exactly true, and found no build impact because Key
   interfaces separately requires checking both sides. Tightening it to name the path it means.
2. **Criterion 6's observable is a document-wide word search.** It is red-before today, but any
   unrelated future use of "whitespace" anywhere in that document would silently make it
   green-before, defeating the criterion while the record sits in `ready-for-agent`. Replacing it
   with a row-shaped observable anchored to the validation-catalog table.

**Readiness gate (cold-reader): PASS** (round 3, full-enumeration)

Independent cold reader, no planning context, 2026-08-30. Full gate on the whole brief, not a diff
review. Classes 1–7 and 9 walked; none fired. Both round-2 edits assessed and found to have achieved
their stated purpose without introducing a new defect.

- **Edit 1 verified against source both ways**: exactly one `.trim()` inside the outcome-membership
  loop, and four more elsewhere in the routine including the named branch-edge guard. The "several
  other places" hedge was argued as a possible class-7(a) figure and ruled acceptable qualitative
  hedging — it is a retraction of precision whose only checkable component is a named, locatable
  site, and its build instruction is structural rather than numeric. Magnitude derived anyway and
  found true.
- **Edit 2 was checked for the opposite failure** — overshooting into an unachievable criterion. The
  reader constructed eleven candidate rows from the table's real format and tested the pattern
  against them rather than trusting the brief. Four match, and they are the four a builder reading
  the neighbouring rows would actually write. Judged achievable rather than over-tight for two
  independent reasons: criterion 2 already forces the word whitespace or padding into the issue
  message, and the catalog's own stated convention is that a row restates the message. Also confirmed
  the round-1 observable was *also* red-before, so the edit hardened a working criterion against
  future drift rather than repairing a broken one.
- **A conflict between criteria 6 and 7 was checked and does not exist**: the validation catalog sits
  outside every generated block, so a hand-written row cannot trip the generator's idempotency
  assertion.
- **Round-1's own blast-radius claim was re-derived rather than inherited** — six template files
  swept ignore-blind, zero padded decide labels — and the no-frontend-mirror claim re-confirmed.

Five non-blocking nits recorded, all verdict `fine`, none requiring action. Two are noted here rather
than acted on:

- The reader suggested criterion 7 be marked a preservation criterion like 4 and 5. Fair, and left
  alone deliberately: it is a regression gate on every brief in this repo, marking it changes nothing
  a builder does, and a further reopen/re-gate cycle to relabel it would cost more than it returns.
- The reader flagged `ready-for-agent` in the round-2 stamp as outside the repo's frontmatter
  vocabulary, having inventoried existing records and found only `needs-triage`/`done`/`wontfix` in
  use. **Dismissed with reason**: `ready-for-agent` is a defined status in the triage state machine;
  the inventory reflects that no record currently sits in it, not that it is invalid. This is the
  expected cost of a cold reader with no planning context, and the gate is worth that cost.

Brief is immutable from this stamp. Promoting to `ready-for-agent`.

## Code Review

Review file: `issue-260826-1648-02-code-review-20260831-032547.md`

Dual review (Claude + Codex, cross-verified, adjudicated inline) over the issue diff vs `c03d72d`, then four
fix rounds with re-verification by both reviewers each round. 13 raised at first review (Claude 10, Codex 3),
one merged, one duplication smell promoted, and six more surfaced by fix-round re-verification — 19 findings,
all terminal.

- M1 (MEDIUM): Brief's mandated confirmation of the structured-vs-free-text asymmetry is absent from the change — FIXED
- M2 (MEDIUM): Empty `outcomes` list returns before the new branch-edge whitespace check — FIXED
- M3 (MEDIUM): The two new whitespace guards are verbatim duplicates (promoted in-diff duplication smell) — FIXED
- L1 (LOW): Preservation test asserts only that error-severity issues are empty — FIXED
- L2 (LOW): Single-sided padding is not exercised at the seam — FIXED
- L3 (LOW): Validation-catalog citation refresh is half-done — deferred
- L4 (LOW): New branch-edge whitespace error identifies the edge by label, not `edge.id` — dismissed
- L5 (LOW): Criterion-1 test cannot distinguish which of the two new checks fired — dismissed
- L6 (LOW): Whitespace-only branch-edge label is refused without naming whitespace — dismissed
- L7 (LOW): Editor trims decide outcomes but not branch-edge labels — deferred
- L8 (LOW): Resumed runs are not re-validated — dismissed
- L9 (LOW): New catalog row's source range includes the block's closing brace — dismissed
- L10 (LOW): Field-reference tables do not mention the new whitespace invariant — dismissed
- L11 (LOW): Fix round 1 left the validation catalog half-updated, two rows citing the same range — FIXED
- L12 (LOW): The empty-outcomes regression does not pin the suppression invariant M2 put at risk — FIXED
- L13 (LOW): Whitespace-only branch-edge labels are now reported twice — FIXED (same-site escalation: cluster {M2, L13} redesigned in round 2)
- L14 (LOW): The empty-outcomes suppression policy is now expressed in two places — FIXED
- L15 (LOW): The new projection helper was introduced without updating the older test that inlines it — FIXED
- L16 (LOW): L14's binding shifted the source and re-staled both whitespace catalog rows — FIXED

Smells: 3 advisory (all appended to `docs/issues/SMELLS-LEDGER.md`), 1 promoted into findings as M3.

## Resolution

**Commit:** `fix: reject whitespace-padded decide labels at validation (ISSUE-260826-1648-02)`

**What landed.** `validate_decide_node_config` now refuses a decide node whose declared outcome labels or whose
outgoing branch-edge labels carry leading or trailing whitespace, with an error-severity issue naming
whitespace as the reason. Both sides of the outcome/branch-label correspondence carry the invariant
independently. Routing is unchanged, so with padded labels refused at the document boundary the runtime's
degrade-to-the-success-edge arm becomes genuinely unreachable from a validated document rather than
approximately so. The validation catalog in `docs/workflow-schema.md` gains one row per new error.

The brief's mandated confirmation is recorded in the source beside the outcome check: the structured-vs-free-text
asymmetry in `select_decide_outcome` survives in code — the brief bans trimming anywhere in the runtime, so it
must — but it is harmless, because a structured mismatch yields a failed selection whose caller returns before
`select_next_decision` is reached. No follow-up record was filed on that account; the brief's "if it turns out
to survive the fix, file it" branch is about the asymmetry remaining *harmful*, which it does not.

**Route:** `cursor` on all four fix rounds and the initial implementation, chosen by `route-picker` each time —
the work is localized to one Rust file (one validation function plus its unit tests) with the fix shape decided
in the brief, no cross-module reasoning and no security-across-layers concern.

**TDD:** red-green at the document boundary — serialize, deserialize strictly with
`serde_json::from_value::<WorkflowV3>()`, then `ensure_defaults` + `validate_workflow`, asserting on the
resulting issue set.

**Review telemetry.** 19 findings — 3 MEDIUM, 16 LOW; 11 FIXED, 2 deferred, 6 dismissed. Fix rounds used: 4 of
4. Round 2 was a redesign round under the same-site escalation rule (cluster {M2, L13}); both reviewers returned
PASS on all six checkable properties it had to satisfy. Two findings were deferred rather than fixed: L3
(citation staleness in catalog rows this issue did not touch, explicitly disclaimed by the brief) and L7 (the
editor trims decide outcomes but not branch-edge labels — outside this brief's frontend boundary, and the one
residual with a user-visible consequence; worth filing separately).

**Suite:** `just test` green — Rust 461 + 6 + 17 passed, 0 failed, 1 ignored; frontend 131 passed across 14
files. No pre-existing failures. Two tests in the Rust suite (`run_control_routes_return_typed_client_errors` in
`tests/http_api.rs` and `runtime::tests::decide_abort_returns_promptly_and_kills_pane`) were observed failing
under concurrent cargo invocations during review and passing in isolation; both passed in the authoritative run.
Neither has a decide node in its fixture and this change touches no runtime code — recorded as latent
load-sensitive flakes, not caused here.

**Closed:** 2026-08-31 (UTC).
