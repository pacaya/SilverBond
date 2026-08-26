---
id: ISSUE-260826-0637-05
kind: issue
category: enhancement
status: ready-for-agent
summary: Write the common node field table and the per-kind config field tables with real types, requiredness and defaults
prd: PRD-260826-0009-01
terms: [Node Kind, Skip Condition, Condition, Access Profile]
blocked_by: [ISSUE-260826-0637-04]
---

## Agent Brief

**Category:** enhancement
**Summary:** Fill `## Node fields` with the one table that describes every node, plus a subsection
per node kind — a config field table where the kind has a config struct, a stated one-liner where
it does not — each row carrying the real type, whether it is required, and its real default.

**Current behavior:**
After the generator lands, the schema reference shows every node kind as a copyable fragment and
a complete workflow, and `## Node fields` is a placeholder deferring to the source. A serialized
example cannot fill that gap: it shows *a* value and nothing marks which value is the default, so
it can never answer "what happens if I omit this?". For kinds that skip defaulted fields on
serialization it is worse — the field vanishes from the output entirely, and a default-valued
pane-spawn node serializes to little more than its tag.

Today's document compounds this by mislabelling. Its "Task Node" table is really the universal
node field table: `agent`, `prompt`, `retryCount`, `loopCondition`, `loopMaxIterations`,
`skipCondition`, `continueSessionFrom`, `contextSources`, `outputSchema` and `cwd` all sit on the
node struct, not inside `kind`. It presents `splitFailurePolicy` as split-only when it is a field
of every node with a custom deserializer mapping an explicit null onto the permissive policy. And
several of its "Required" answers are backwards — `prompt` is defaulted and an empty one is only a
warning, while `goal`, `nodes` and `edges` are all defaulted too.

**Desired behavior:**
`## Node fields` carries two subsections.

*Common node fields* — one table for the fields every node carries whatever its kind, with the
nested `kind` object itself as a row. Rows that carry a trap say so in the description rather than
leaving the reader to find it: that `agent` is optional to serde but defaults to *absent* rather
than to a name, and a task node without one is a hard validation error, so the runtime fallback
never rescues a document at run start; that
`loopCondition` is a structured condition object evaluated deterministically and never a prompt;
that `loopMaxIterations` defaults to a small value when unset and that reaching the cap without a
loop-exit edge fails the run; that `retryCount` has a hard maximum whose breach short-circuits the
whole validation pass; that `splitFailurePolicy` is universal and what an explicit null means; and
that `continueSessionFrom` carries five distinct constraints — the source's kind, a matching
resolved agent, an access profile on the source no broader than the target's, a matching resolved
working directory, and the node existing at all.

*Per-kind config fields* — one subsection per kind, in the catalog's order so a reader moving
between a generated example and its table does not have to search. Not every kind has a config
struct to tabulate, and the brief is explicit rather than leaving it to judgement: kinds that are
bare unit variants carry no config object at all, and their subsection says exactly that in one
line instead of showing an empty grid. `subflow` and `call` share one config struct under one wire
key — give each its own table anyway rather than cross-referencing, so a reader landing on either
kind is finished there. Two kinds carry agent config inside `kind` and that table is
ISSUE-260826-0637-06's: for `task` it is the only payload, so its subsection says so and points
there; for `run_agent` it sits alongside that kind's own config, so tabulate the run-agent config
here and add one row pointing the agent-config half at -06. Each row gives the
wire field name, its type, whether it is genuinely required, and its real default. Serde aliases
are part of the wire contract and belong in the table: the batch config accepts a legacy config
key, and the subflow config's workflow-name field accepts two alternative spellings. So do the
defaults that surprise — the decide model, the batch concurrency default and its runtime clamp,
the send node's enter flag defaulting to on, the run-agent kill-after flag defaulting to on, the
wait mode's default, and the subflow depth cap.

Every table cites the source location that owns it, so a reader can verify in one jump. These
tables are hand-written and stay hand-written: no check in this epic pins them, and requiredness
in particular is a serde attribute that can change without changing the construction API, the
serialized example, the harvested variant set or any validation result. Write them knowing that.

**Key interfaces:**
- The node struct is the authority for the common table; each kind's config struct is the
  authority for its own. Read the serde attributes, not the field declarations alone —
  defaulting, renaming, aliasing and skip-on-serialize all change the wire contract.
- The catalog's declared order, established by ISSUE-260826-0637-04, fixes the order of the
  per-kind subsections.
- `docs/sources/workflow-schema-drift-260825.md` Part 1 sections 1.2 and 1.3 are this issue's work
  order. Section 1.8 (`skipCondition`) and section 1.11 (agent config) both belong to
  ISSUE-260826-0637-06 — this issue's Out of scope disclaims agent config, so do not write those
  rows here.
- `CONTEXT.md`'s **Access Profile** entry distinguishes the `access` field from the `accessMode`
  enum. Only `access` belongs in these tables — it sits on the pane-spawn and run-agent config
  structs. `accessMode` is declared once, on the shared agent-defaults struct, which is reached both
  through the flattened node-level agent config and as the value type of the top-level defaults map,
  and the resolver merges the two; either way it is ISSUE-260826-0637-06's row. Keep the two apart
  and do not document `accessMode` here.

**Acceptance criteria:**
- [ ] `## Node fields` contains a common-node-fields subsection and a per-kind subsection.
      Observable: `rg --pcre2 --multiline -n '(?s)^## Node fields\n(?:(?!^## ).)*?^### ' docs/workflow-schema.md`
      matches; no match before this change. The lazy quantifier stops at the first `###`, so this
      command proves at least one subsection exists, not two — the prose above is the requirement
      and both subsections are checked by reading.
- [ ] The common table documents `splitFailurePolicy` as a field of every node. Observable:
      `rg --pcre2 --multiline -n '(?s)^## Node fields\n(?:(?!^## ).)*?splitFailurePolicy' docs/workflow-schema.md`
      matches; no match before this change. Anchor to the section rather than searching the file:
      today the field name appears in the legacy node section as a split-only field, and although
      ISSUE-260826-0637-03 deletes that section before this issue starts, an unanchored check would
      also be satisfied by any later mention elsewhere in the document.
- [ ] The common table documents `loopCondition` as a structured condition object, not a prompt
      string. Observable at the seam: the row's type column names the condition shape.
- [ ] The `agent` row states that the field is optional to serde but that a task node without one
      fails validation. Do not write "has no serde default" — the field does carry `#[serde(default)]`;
      what it lacks is a default *value*, so an omitted `agent` deserializes to absent, not to a
      name. In a table whose subject is serde attributes that distinction is the whole point.
- [ ] Every node kind has a subsection under the per-kind heading, in catalog order, carrying the
      treatment its kind calls for — the four cases named in Desired behavior, not two: a config
      field table for a kind with its own config struct; a stated one-line "carries no config
      object" for a bare unit variant; for `task`, a one-liner saying its only in-`kind` payload is
      the agent config, pointing at ISSUE-260826-0637-06; for `run_agent`, its own config table plus
      one row pointing the agent-config half at -06. Do not collapse this into a has-a-struct /
      has-none dichotomy: under that reading `task` gets either a false "carries no config object"
      line or a table this issue disclaims. Observable: derive the tag set from the source and
      confirm each has a subsection; do not assert the set's size in the document prose.
- [ ] Every field of a kind's own config struct appears as a row in that kind's table, and no row
      names a field that struct does not have — with one deliberate exception, the `run_agent`
      pointer row for `agentConfig`, which is a sibling payload of the variant rather than a field
      of the run-agent config. Serde aliases that sit on the wrapper key rather than on a struct
      field — the batch config's legacy spelling — are documented in the table's caption or note,
      not as a field row, for the same reason. Observable at the seam: compare each table against
      its struct's serde-visible fields, allowing those two exceptions.
- [ ] Every default stated in a table matches the source. Spot-checkable at the seam on the
      surprising ones — the send node's enter flag and the run-agent kill-after flag both default
      to on, and the batch concurrency default is not one.
- [ ] Both serde aliases are documented — the batch config's legacy key and the subflow config's
      two alternative workflow-name spellings.
- [ ] Every table cites the source location that owns it.
- [ ] `just check-v4-docs` passes and `cargo test` is green, including the generator's freshness
      assertion — this issue edits prose around the generated blocks and must not disturb them.

**Out of scope:**
- Editing anything between a `BEGIN GENERATED` and `END GENERATED` marker. Those spans belong to
  the generator; a hand edit there fails the freshness assertion.
- `skipCondition`'s own shape table, the edge and condition tables, top-level fields, template
  tokens, agent config and the validation catalog — ISSUE-260826-0637-06 and -07.
- Generating these tables from JSON Schema. Excluded by the PRD as a second generator.
- Any change under `src/` or `ui/`, including adding derives to make the tables generable.
- Describing v5 field shapes as present.

## Triage Notes

**Readiness gate (cold-reader): PASS** (round 5, full-enumeration)

Rounds 1-4 ran without terminal stamps; round 4 raised findings against the per-kind treatment
criterion and the config-field-row criterion, both of which were rewritten. Round 5 ran as a
full-enumeration round per the round-3 escalation rule with those two criteria promoted as tracked
findings and judged from scratch: the four treatment cases were verified exhaustive over the
harvested variant set, and both row-rule exceptions verified as real properties of the source and
no wider than the facts. Classes 1-5 swept 30 decision surfaces, class 7 swept 48 extracted
surfaces, class 9 arm A executed every embedded observable against the tree (all red), arm B found
no triggered requirement, class 6 was re-examined cold on both prongs, and class 8 was inert on a
never-stamped record. No class fired.
