---
id: ISSUE-260830-1925-02
kind: issue
category: bug
status: done
origin: docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md
summary: Decided — branch edges get a declared `default` flag and non-decide nodes must condition every unmarked branch edge; the grammar and its validation are carried into the typed-contracts epic, retiring the silent first-edge default
---

## Agent Brief

**Category:** bug
**Summary:** Decide what a non-`decide` node should do when no branch edge's condition selects it,
and make sure the `condition-ast` epic's design actually covers that case. **This needs human
judgment — see "Why this is not delegable" below. Do not implement anything from this record.**

**Current behavior:**
In the engine's branch-routing block for non-`decide` nodes, the chosen edge is seeded from the
first branch edge before any condition is evaluated. The condition scan that follows can only
replace that seed, never clear it. So two distinct situations both end at the same place:

1. **Every branch condition evaluated cleanly and every one returned false.** The seed survives.
2. **There is no parsed output at all** — the node declared no JSON response format, or its output
   did not parse — so the condition scan is skipped outright. The seed survives. A node whose branch
   edges carry no conditions at all lands here too, since there is nothing to evaluate.

In both, the run proceeds down the first branch edge. A `branch_decision` event fires carrying the
same chosen-branch and chosen-label fields it carries for a genuinely matched route, and no warning
accompanies it. There is no "no condition matched" outcome in the engine. A workflow author gets a
plausible-looking route rather than a detectable failure, and nothing in the event stream
distinguishes the two.

This is present-tense behavior and is documented as such in `docs/execution-model.md`. It is *not*
the unreachable orchestrator fallback — that is dead code, deleted by `ISSUE-260826-0004-01`, and
deleting it changes none of the above. `ISSUE-260830-1925-01` repairs the one bundled template that
depends on this default; the engine behavior itself outlives both.

**The decision this record carries:**
The `condition-ast` epic of `RDMP-260815-2009-01` is the natural owner. Its charter covers per-leaf
`onMissing` policies (`treat-as-false | error | route`), `parse_error` routing treated as
all-fields-missing, and a `failure` edge outcome that declared failures travel over.

**None of those three is either situation above.** `onMissing` is about a leaf whose *field* is
absent from otherwise-valid parsed output. `parse_error` is about output that will not parse.
Situation 1 is neither — every leaf resolved, every comparison ran, and the answer was false
everywhere. Situation 2 is only partly covered: parse failure is chartered, but "the node never
declared a structured response format" and "the branch edges declare no conditions at all" are
different, and a node in the latter shape is a document that validates clean today.

So the decision has two parts, and the second is the one at risk of falling through the gap:

- **(a) All-conditions-false.** What should the engine do? Options, with the trade-offs as they
  look from here: route over the `failure` edge when one exists and fail the run otherwise — reuses
  grammar the epic is already landing, but overloads "failure" to mean "no branch matched", which is
  arguably a routing outcome rather than a node failure. Or introduce a declared no-match outcome
  distinct from `failure` — cleaner semantically, but it is another edge outcome in the v5 grammar,
  and grammar lands in `typed-contracts`, one epic earlier, so deciding this late is expensive. Or
  keep the first-edge default and emit a distinguishable event or warning — cheapest and
  backward-compatible, but it leaves a silent misroute silent by default, which is the complaint.
  Or fail the run outright — safest, and the most likely to break existing documents.
- **(b) Branch edges with no conditions.** Rejecting this at save time is the cheap structural fix
  and needs no runtime semantics at all: a node with branch edges must carry conditions on all but
  at most one designated default. It is worth deciding separately from (a) because it can land as
  validation in `typed-contracts` without waiting for the condition evaluator, and because it is
  what actually broke the bundled template.

**Why this is not delegable:**
Both parts are grammar and semantics decisions for the v5 document shape, owned by roadmap epics
that have not started (`typed-contracts` is blocked by `docs-truth`, which is active;
`condition-ast` is blocked by `typed-contracts`). Choosing here would pre-empt a gated roadmap and
could conflict with the `failure`-outcome design those epics land. The decision also plausibly
changes the behavior of workflow documents that already exist, which is a compatibility call rather
than an implementation one.

**What a human needs to do with this record:**
- Decide (a) and (b), or explicitly assign them to `condition-ast` and `typed-contracts`
  respectively, in a form those epics will actually see — an entry in the roadmap epic body, an
  ADR, or an `## Open Questions` line on the PRD when one is written. **This has not been done.**
  This record is currently the only place the gap is written down, and a record in `docs/issues/`
  is not on the path either epic's author is required to read.
- Once decided, either close this record against the implementing epic's issues or re-triage it to
  `ready-for-agent` for the part that lands standalone.

**Out of scope:**
- Implementing any of the options above. This record is a decision, not a change.
- The unreachable orchestrator fallback (`ISSUE-260826-0004-01`) and the broken bundled template
  (`ISSUE-260830-1925-01`). Both are separately actionable and neither waits on this.
- `skipCondition`, which the `condition-ast` epic explicitly excludes and which keeps its current
  pre-execution shape.
- `decide`-node routing. A `decide` node returns from the decision function before reaching this
  block, and an outcome with no matching branch edge is already a validation error there. The defect
  described here cannot occur on that path.

## Triage Notes

Split out of `ISSUE-260826-0004-01` on 2026-08-30 at the maintainer's direction, as the half of that
record that is a design decision rather than a change. That record kept the dead-code deletion and
now cites this one as the owner of the routing default; a test it adds pins the current behavior and
names this record so a later reader knows the assertion documents a known defect rather than a
guarantee.

The observation that the `condition-ast` charter may not cover either situation was made during
triage by reading the epic body against the routing code, and is the substantive reason this was not
simply folded into that epic as understood. It should be checked by whoever owns that epic rather
than taken on trust from here.

**Open, and deliberately not acted on:** carrying this into `docs/roadmap/RDMP-260815-2009-01-harness-workflows.md`
or into an ADR. That roadmap passed its gate on 2026-08-16; editing a gate-passed artifact is the
maintainer's call, not triage's. Until that happens this gap lives only in `docs/issues/`, which is
the weakest place for it — flagged rather than fixed.

No readiness-gate round was run. The gate is a precondition of `ready-for-agent`, and this record is
`ready-for-human`: its Agent Brief exists to frame a decision, not to be worked from.

## Resolution

**Decided 2026-08-30 by the maintainer.** Both parts settled together, and (a) is retired rather
than answered on its own terms.

**The rule.** A non-`decide` node with branch edges must carry a condition on every branch edge
except at most one explicitly marked `default`. Violating it is an **error-severity validation
issue**. An all-false condition scan then routes over the declared default when the node marks one,
and is an error otherwise — so no fifth `WorkflowEdgeOutcome` variant is added for "no match", and
the `failure` outcome is not overloaded to mean it.

**Why the default flag rather than requiring a condition on every edge.** Requiring all of them
would need no grammar at all and could land independently of the condition AST, but it forces an
author writing "otherwise" to hand-maintain the negation of the disjunction of every sibling
condition. Adding a third branch later silently invalidates that negation — a drift hazard that
fails in the same quiet way as the defect being fixed. One boolean in the v5 edge grammar buys a
native "else" and, decided before `typed-contracts` is specced, costs approximately nothing.

**Why this kills the defect.** The complaint was never that a fallback exists; it is that the
fallback is inferred from array position (`chosen` seeded from `branch_edges.first()` at
`src/runtime.rs:6528` before any condition is evaluated). Array order is not a semantic. Making the
fallback declared removes the inference, and distinguishes an intentional else-route from a node
that matched nothing.

**Scoping finding — `decide` nodes are exempt, and this is load-bearing.** Stated as "a node with
branch edges must carry conditions", the rule would invalidate two working bundled templates. A
repo-wide audit of every `branch` edge found five nodes with condition-less branch edges, four of
them `decide` nodes where that shape is correct:

| Document | Node | Kind | Condition-less branch edges |
|---|---|---|---|
| `templates/dual-review.json` | `decide-cross-verify`, `decide-adjudicate` | `decide` | 2 + 2 |
| `templates/epic-dev.json` | `decide-pick-story`, `decide-story-result` | `decide` | 3 + 3 |
| `templates/research-and-summarize.json` | `rs1` | `task` | 2 |

A `decide` node routes by label match against its declared outcome set
(`src/runtime.rs:6383-6387`), not by condition, and the engine already validates the outcome-set /
branch-edge-label correspondence in both directions. Conditions there would be meaningless.

**Sequencing.** `ISSUE-260830-1925-01` converts `rs1`'s routing to a `decide` node, so once it
lands **no document in the repo violates the scoped rule** — the rule breaks nothing that will
still exist when it ships. The maintainer's standing latitude to break stored documents (dev stage;
existing `workflows/*.json` are test artifacts) therefore was not needed for existing artifacts,
only to free the shape choice above.

**Correction to this record's own premise.** The Agent Brief and the discussion that produced it
both said "reject at save time". The engine does not validate on save, for anything:
`validate_workflow` has exactly two production call sites — the explicit validate route
(`src/api.rs:567`) and run creation (`src/api.rs:751`) — and `save_workflow` (`src/api.rs:144`) is
not among them. The rule is therefore an error-severity validation issue like the other 86, which
blocks run creation and surfaces in the editor through the validate route. Whether the save path
should validate at all is a posture change affecting every existing issue, left to
`typed-contracts`' save-time wiring validation to settle on its own merits; this rule does not
depend on the answer.

**Where the decision now lives.** Carried into `docs/roadmap/RDMP-260815-2009-01-harness-workflows.md`
on 2026-08-30 — a `## Coverage` row ("Declared default branch edge + condition-required branch
validation | typed-contracts"), the rule and the `decide` exemption in the `typed-contracts` epic
body, and a sentence in the `condition-ast` body stating that an all-false scan is not a `failure`.
That roadmap is gate-passed (2026-08-16); the edit was made at the maintainer's direction. This
closes the gap the record was filed to flag: the decision is now on the path
`typed-contracts`' author reads, not only in `docs/issues/`.

**Closed rather than re-triaged to `ready-for-agent`.** Nothing lands standalone ahead of
`typed-contracts`. The `default` flag is v5 edge grammar, and validation rejecting condition-less
branch edges before that grammar exists would reject documents with no way to express the
exemption. Implementation is owned by `typed-contracts` via the coverage row; when that epic is
specced, its PRD inherits the rule from the roadmap entry.

**Not done, and deliberately.** The frontend edge inspector needs an affordance for marking a
branch edge default; it is not called out separately because `condition-ast` already owns
regenerating the ConditionBuilder UI from the new condition form, and the roadmap's UX dimension
row assigns the condition builder to that epic.
