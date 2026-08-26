---
id: ISSUE-260826-0637-06
kind: issue
category: enhancement
status: ready-for-agent
summary: Write the edge, condition, top-level, variable, subflow-catalog, skipCondition, agent-config and template-token tables
prd: PRD-260826-0009-01
terms: [Edge Outcome, Condition, Skip Condition, Variable, Subflow, Template Token, Access Profile]
blocked_by: [ISSUE-260826-0637-05]
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

## Triage Notes

**Readiness gate (cold-reader): PASS** (round 3)

Round 1 found three blocking issues including a false save-time framing; round 2 found the
collector merge-key rule inverted and the banned phrase surviving in Out of scope. All were
fixed, upstream included. Round 3 ran the full nine-class rubric, built post-blocker fixtures to
test each observable against its real baseline rather than today's tree, and swept both repaired
claims across this record and the upstream artifacts — no survivor.
