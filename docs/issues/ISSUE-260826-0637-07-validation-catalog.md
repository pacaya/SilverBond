---
id: ISSUE-260826-0637-07
kind: issue
category: enhancement
status: ready-for-agent
summary: Write the validation catalog covering every error and warning the engine emits, with severities corrected
prd: PRD-260826-0009-01
terms: [Validation Issue, Node Kind, Subflow, Access Profile]
blocked_by: [ISSUE-260826-0637-06]
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

## Triage Notes

**Readiness gate (cold-reader): PASS** (round 3)

Round 1 found the sweep predicate under-inclusive and a decaying figure; round 2 found the
save-time framing, which had been corrected in three other artifacts and missed here. Both were
fixed and a criterion was added pinning where validation is enforced. Round 3 ran the full
nine-class rubric, re-derived the severity inventory and construction-site total from source,
and confirmed no surviving save-framing anywhere in the record.
