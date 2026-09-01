---
id: ISSUE-260901-0216-01
kind: issue
category: bug
status: ready-for-agent
prd: PRD-260826-0009-01
summary: Source citations in the workflow-schema document's hand-written sections drift silently when Rust line numbers shift; correct the drifted ones and add a freshness guard that fails when a citation no longer points at what it claims
---

## Agent Brief

**Category:** bug
**Summary:** Replace the workflow schema document's positional source citations with symbolic ones,
and add a guard that fails when a citation no longer resolves.

**Current behavior:**
The schema document cites the Rust source that backs each rule it describes. Most citations are
positional — a file path with a line or line range — and some are abbreviated, carrying only a line
or range and inheriting their file from the nearest preceding full citation, which may sit earlier
in the same paragraph rather than on the same line. A third shape already exists and is not
positional: a symbol name paired with a bare file path. Enumerate the three shapes with
`rg -o -N '`[A-Za-z0-9_./-]+\.rs:[0-9]+(-[0-9]+)?`' docs/workflow-schema.md`,
`rg -o -N '`:[0-9]+(-[0-9]+)?`' docs/workflow-schema.md`, and
`rg -o -N '`src/[a-z_]+\.rs`' docs/workflow-schema.md` — the third of which matches every
citation naming a file without a line number, whatever symbol form precedes it, including
symbols carrying capitals, digits, or a `::` path, and both the `symbol, file` and `symbol in
file` separators.

Positional citations decay silently. When an edit inserts or removes lines above a cited range,
every citation below it shifts. The citation still resolves to a real range, so nothing errors and
no test notices: a reader following it lands on unrelated code, often a different rule from the
same family, which reads as plausible. This has already happened once — a commit updated only the
rows it added and left the rows below pointing at their predecessors' code.

The generated node-catalog blocks are guarded against staleness by a regeneration test. Nothing
guards the citations.

**Desired behavior:**
Citations name what they point at rather than where it sits, so line movement cannot invalidate
them, and a citation that no longer resolves fails the Rust suite.

A citation names a file, one or more symbols defined in that file, and optionally a discriminant.

- A **symbol** is either a top-level item or a member of one. Both are cited by name, a member
  qualified by its parent. The cited code includes functions, structs, struct fields, enums, enum
  variants, constants, and statics, so resolution must handle members, not only top-level items —
  and the list is a floor, not a closed set: derive the kinds actually cited from the document
  rather than assuming this enumeration is complete.
- A citation may name **several symbols** where the positional form covered a contiguous run of
  them, such as a block of related constants. Every named symbol must resolve.
- A **discriminant** is a literal substring that identifies one site among several inside the same
  symbol. It is not restricted to validation messages: where the cited code constructs a validation
  issue, a fragment of that issue's message format string is the natural choice, but a called
  function name, a distinctive expression, or any other literal occurring at the cited site serves
  equally. A discriminant is what keeps precision where a long symbol encloses many separately-cited
  sites.

A citation resolves when the file exists, every named symbol is defined in it, and the discriminant,
when present, occurs within the extent of the symbol it accompanies — not merely somewhere in the
file. Failure to resolve is a test failure, not a skip.

A citation that names a file but no symbol does not resolve. It is reported, not skipped — this is
what forces the already-position-free citations to gain symbols rather than being silently passed
over as though they were already converted.

More generally, **any citation the guard cannot parse is reported, not skipped.** Skipping and
resolving must never be indistinguishable to an observer: a guard that silently passes over a shape
it does not understand would report a clean document while checking a subset of it, which is the
failure mode this record exists to remove. Unparseable and unresolved are distinct outcomes and are
reported distinguishably.

Every citation in the document ends in this form, including the ones already carrying a symbol and
a bare file path; where such a citation names no symbol, it gains one. Most already carry one and
need no change.

A bare file path in running prose that states where something lives — rather than backing a
specific claim the sentence makes — is **not** a citation and is left alone. The document contains
both: a sentence naming a file as the authority for the schema is prose; a parenthetical pinning
the code behind a stated rule is a citation. The governing test is the one this section opens with —
a citation backs a rule the document states. The guard must not report prose mentions, and a fixture
demonstrating that is not required; getting the boundary wrong shows up as the guard reporting a
line no rule depends on. The abbreviated inheriting
form disappears, because every citation names its own symbol and inherits nothing.

**What this deliberately does not detect.** Two citations that resolve to the same symbol and carry
no discriminant are textually identical, so exchanging them is undetectable. This is why the
discriminant is general rather than validation-only: a long symbol enclosing many cited sites is
exactly where identical citations would otherwise proliferate. Where a discriminant genuinely
cannot be found, the resulting loss of precision relative to a line range is accepted. Detecting an
exchange between two fully-identical citations would require a per-row machine-readable identity,
which this record does not build. In exchange the symbolic form catches something positional
citations never could: code reworded at the cited site leaves its discriminant unfound and fails
the guard.

**Key interfaces:**
- The citation grammar in the document — becoming file plus symbol or symbols plus optional
  discriminant, with no positional component anywhere.
- The docs-catalog test target — the existing home for document-freshness enforcement, and the
  precedent to follow: derive from the Rust tree, compare against the committed document, fail on
  mismatch, never write during a compare run.
- Symbol extent resolution — the guard must know where a named symbol starts and ends in order to
  scope a discriminant search to it.

**Acceptance criteria:**
- [ ] No positional citation remains.
      `rg -o -N '`[A-Za-z0-9_./-]+\.rs:[0-9]+(-[0-9]+)?`' docs/workflow-schema.md` returns no
      matches; it returns many before this change.
- [ ] No abbreviated inheriting citation remains.
      `rg -o -N '`:[0-9]+(-[0-9]+)?`' docs/workflow-schema.md` returns no matches; it returns
      several before this change.
- [ ] The Rust suite contains a test that resolves every citation in the document against the Rust
      tree. `rg -c 'citation' tests/docs_catalog.rs` returns a count; no matches before this change.
- [ ] The guard fails when a cited symbol does not exist. A fixture takes the tree under test,
      renames a cited symbol in a copy of the document to one no source file defines, runs the check
      against that copy, and asserts the check reports that citation — identified by the unresolved
      symbol's own name in the failure output, not by exit status alone.
- [ ] The guard fails when a discriminant is absent from its cited symbol. A fixture perturbs one
      discriminant in a copy of the document to a string the cited symbol does not contain, and
      asserts the check reports it, identified by the perturbed discriminant's text.
- [ ] A discriminant that occurs in the cited file but outside the cited symbol's extent is
      reported, not accepted. A fixture replaces one discriminant with a literal taken from a
      different symbol in the same file and asserts the check reports it — identified by the
      out-of-extent literal's own text paired with the name of the symbol it was expected in, a
      pairing the in-extent path never emits. Identification by exit status, or by a diagnostic the
      missing-discriminant case also produces, does not discharge this: those would leave extent
      scoping asserted rather than observed.
- [ ] Member citations and multi-symbol citations resolve, and are seen rather than skipped. A
      fixture covers one enum-variant citation and one citation naming several symbols, asserting
      that each resolves when correct and that each is reported when one of its symbols is renamed
      away — identified by the renamed symbol's own name in the failure output, which for the
      multi-symbol case must be the specific symbol perturbed and not merely the citation as a
      whole. Exit status does not discharge this, nor does any diagnostic the other fixtures also
      produce: the enum-variant half must not be able to satisfy the multi-symbol half's assertion.
- [ ] A citation whose shape the guard cannot parse is reported, and reported distinguishably from
      one that parses but fails to resolve. A fixture plants a malformed citation and asserts the
      check reports it with a diagnostic naming the unparsed text, distinct from the diagnostics the
      unresolved-symbol and missing-discriminant fixtures assert. Without this, a guard that skips
      shapes it does not understand reports a clean document while checking only part of it.
- [ ] A citation naming a file but no symbol is reported, not skipped. A fixture strips the symbol
      from one citation in a copy of the document and asserts the check reports it, identified by a
      diagnostic naming the symbol-less citation that the resolvable path never emits. Together with
      the suite-green criterion below, this is what makes conversion of the already-position-free
      citations observable: any left unconverted keeps the suite red.
- [ ] The guard reports only what is broken: every fixture above asserts that citations it left
      untouched are absent from the report. Asserting only that the perturbed citation appears is
      insufficient — a check that reported every citation would satisfy that half while verifying
      nothing.
- [ ] `cargo test --locked` passes with the document fully converted, so the guard is green against
      the committed document.

**Out of scope:**
- `docs/execution-model.md` and every other document. This record converts the workflow schema
  document only; extending the grammar and guard to a second document is a separate record.
- A per-row machine-readable rule identity, and therefore detection of an exchange between two
  fully-identical citations. See "What this deliberately does not detect" above.
- Widening the generated-block generator to cover more of the document. The guard reads
  hand-written citations; it does not turn hand-written sections into generated ones.
- Any change to validation behavior, to messages, or to the English condition text of catalog rows.
- Renaming Rust symbols to make them easier to cite. Citations adapt to the code, not the reverse.

## Triage Notes

Filed 2026-09-01 during a completeness audit of the `docs-truth` epic (PRD-260826-0009-01), from a
deferral recorded in `ISSUE-260826-1648-02` that was never given a record.

- Scale snapshot (non-contractual): `rg -o -N '`[A-Za-z0-9_./-]+\.rs:[0-9]+(-[0-9]+)?`' docs/workflow-schema.md | wc -l`
  and `rg -o -N '`:[0-9]+(-[0-9]+)?`' docs/workflow-schema.md | wc -l` → ~250 citations across five
  source files, concentrated in the workflow model module (2026-09-01)

**Evidence the drift is real.** Commit `3dbc9ce` ("reject whitespace-padded decide labels at
validation") inserted a helper and two call sites into the workflow model module and updated only
the two catalog rows it added. The rows below the insertion point shifted and were not updated.
Spot-checked at audit time: the row for the empty-`decide`-prompt warning cited a range that had
become the interior of the padded-label helper; the row for duplicate outcome labels cited the
range that constructs the *at-least-one-outcome* error; the `parallel_batch` `itemsBinding` row
cited a range that had become `decide` branch-edge code. The two rows that commit authored cite
correctly. Prose citations drifted the same way — the `deadEndNodeIds`, `get_nested_field`, and
`parse_structured_output` references each resolve to unrelated code.

Re-derive the current drifted set with the guard this record asks for; the specific rows above are
a record of what one audit found, not a checklist to work from.

**Why the guard and the correction belong in one record.** Correcting the citations by hand
restores truth for one commit. The drift documented above entered the tree one commit *after* the
validation catalog shipped and passed CI, which is direct evidence that hand correction without a
guard re-earns the same defect. The two halves share a seam — the check must run against corrected
citations to be green — so they are one slice, not two.

**Readiness gate (cold-reader): FAIL** (round 1, 2026-09-01) — acceptance criterion conflated
lines-containing-citations with citations, and would have let the guard skip the abbreviated
continuation form. Criteria rewritten.

**Readiness gate (cold-reader): FAIL** (round 2, 2026-09-01) — classes 3, 4 and 7. The brief's
matching mechanism rests on a premise the document falsifies. Held at `needs-triage` pending a
maintainer decision; see below.

**Readiness gate (cold-reader): FAIL** (round 3, 2026-09-01) — classes 3 and 7. Symbol-kind list
falsified by the corpus (enum variants, sub-symbol citations with no available discriminant,
multi-symbol ranges); the abbreviated-form inventory asserted same-line inheritance, which is false.

**Readiness gate (cold-reader): FAIL** (round 4, 2026-09-01) — classes 7 and 9B. The third-shape
enumerator under-derived its set roughly sevenfold and no criterion tested that shape; the
extent-scoping criterion named no failure signal.

**Readiness gate (cold-reader): FAIL** (round 5, 2026-09-01) — class 9B arm B requirement 2, sole
gap: the member/multi-symbol criterion named no failure signal. Same defect class as round 4's,
surviving one criterion over — the previous remedy fixed the instance and did not sweep the genus.
Remedied by naming the signal, adding a criterion requiring unparseable citations to be reported
distinguishably, and re-sweeping every criterion for the same shape.

**Readiness gate (cold-reader): PASS** (round 6, 2026-09-01) — all classes fine. Class 9B ledger of
14 rows over 7 fixture criteria, none firing; class 7 ledger of 24 rows, none blocking; class 6
fires on neither prong. The brief is immutable from this stamp.

Gate notes carried for the implementer (non-binding):

- The prose-versus-citation boundary is a bounded delegation, not an open decision. The corpus makes
  it concrete: of the bare-path occurrences the third enumerator returns, all but one are
  parentheticals backing a stated rule, and the single prose instance matches the brief's first
  exemplar verbatim. Both error directions have teeth — over-reporting turns the suite red, and
  under-reporting is caught by the symbol-less and unparseable fixtures.
- The suite-green criterion is the only thing binding conversion to the guard. Do not weaken it to a
  subset run; it is what makes an unconverted citation anywhere in the document fail the build.

### Decision — citations become symbolic (resolved 2026-09-01)

Round 2 failed this record because its brief keyed the guard on validation messages while the
document's catalog rows carry only English paraphrases. Reproduced at gate time: of the 100 rows
matching `^\| (error|warning) \| ` in `docs/workflow-schema.md`, five have condition text appearing
verbatim in `src/model.rs`, and those five are coincidences of short phrasing. A worked pair — the
row reads ``​`decide` node has duplicate outcome labels`` while the source emits
`"\"{}\" decide node has duplicate outcome \"{}\"."` — plural against singular, paraphrase against
interpolated format string. There is no mechanical row-to-rule key in the document as it stands.

Three options were put to the maintainer: (1) add a per-row machine-readable rule identity;
(2) replace positional citations with symbolic ones; (3) weaken the guard to assert only that a
cited range contains some validation-issue construction. **Option 2 was chosen** (maintainer
decision, 2026-09-01). Rationale: the freshness guard is only needed because citations are
positional, so making them symbolic dissolves the defect rather than reporting it. Option 3 stays
green under the drift class that actually occurred once ranges shift within a family; option 1 buys
swap detection at the cost of a second identity to keep honest across a hundred rows.

The brief above is rewritten accordingly. Two consequences were established before rewriting and
are carried into it explicitly:

- **Same-symbol swap detection is given up.** Catalog rows share enclosing symbols — the decide
  validation function backs roughly ten of them — so two rows in one symbol that exchange citations
  both still resolve. Round 1's swap criterion was dropped rather than kept as an unmeetable
  requirement; keeping it would have forced option 1's machinery in through the back door.
- **Message rewording becomes detectable**, which the positional scheme never caught: a discriminant
  that no longer occurs in its cited symbol fails the guard.

The previous Out of scope forbade changing the citation grammar. That exclusion is reversed by this
decision and the brief no longer carries it.

### Round 3 outcome and the generalized grammar (2026-09-01)

Round 3 failed on classes 3 and 7. Three findings, each reproduced before being accepted:

- **The symbol-kind list was a floor presented as a ceiling.** Citations target enum *variants*
  (several point at individual `NodeKind` variants), and citations into a long function point at
  distinct sub-locations — verified with the template-resolution function, which encloses several
  separately-cited regexes. Under the previous grammar all of these collapsed to one identical
  citation string with the guard green, because the discriminant was restricted to validation
  messages and so was unavailable to separate them. The grammar now admits members and multi-symbol
  citations, and the discriminant is any distinguishing literal within the cited symbol's extent.
  This is the same weakness identified independently before the gate reported.
- **A citation may span several symbols.** One citation covers a contiguous run of constants;
  "a symbol", singular, had no rule for it. Now stated.
- **The continuation form does not inherit from the same line.** The brief asserted it did. It is
  false — several abbreviated citations inherit from a full citation earlier in the paragraph, on a
  previous line. Reproduce with
  `rg -n '`:[0-9]+(-[0-9]+)?`' docs/workflow-schema.md | rg -v '\.rs:[0-9]'`. The claim was carried
  over from an earlier round's phrasing without being checked; the brief no longer makes it, and
  states the shapes with discovery commands rather than as an inventory.

Also corrected: a third citation shape already in the document — a symbol paired with a bare file
path, carrying no line number — was named by neither the inventory nor any acceptance criterion,
leaving it undetermined whether the guard covered it. It is now explicitly in scope, since it is
already position-free and needs only a symbol where it lacks one.

The scale snapshot was sitting under this section rather than under `## Triage Notes`, where the
authoring rules place it; moved.
