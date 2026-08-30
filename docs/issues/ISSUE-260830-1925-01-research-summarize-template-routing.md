---
id: ISSUE-260830-1925-01
kind: issue
category: bug
status: ready-for-agent
origin: docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md
summary: The bundled Research & Summarize template can never reach its Quick summary branch, because its branch edges carry no conditions and depend on an unreachable orchestrator fallback
---

## Agent Brief

**Category:** bug
**Summary:** Give the bundled Research & Summarize template declared routing, so the branch it can
never currently reach becomes reachable — and add a test that stops any bundled template from
shipping unroutable branch edges again.

**Current behavior:**
The bundled Research & Summarize template branches from its first node — a plain `task` node that
performs initial research — to either a refinement loop ("Deep dive") or straight to the final
summary ("Quick summary"). Both outgoing branch edges carry a label and a branch id and **no
condition at all**, and the template sets the workflow-level orchestrator flag. Derive which
bundled documents set it with `rg -n '"useOrchestrator":\s*true' templates/` rather than taking a set
from this record. The template was evidently authored expecting the orchestrator to choose between
the two.

It cannot. In the engine's branch-routing block for non-`decide` nodes, the chosen edge is seeded
from the first branch edge before any condition is evaluated, so the orchestrator guard downstream
is unreachable — see `ISSUE-260826-0004-01`, which deletes that dead path. Worse for this template
specifically, its research node declares no `responseFormat`, so there is no parsed output and the
condition scan is skipped outright. The seeded first edge always survives. **The template always
takes "Deep dive" and can never reach "Quick summary".** It raises no validation error, runs green, and
silently exercises one of its two documented paths.

Whether any other bundled document has this shape is exactly what the guard test below derives, and
it is the authority. Do not carry a set from this record into the work.

**Desired behavior:**
The template chooses between depth and brevity through the engine's declared LLM-routing mechanism
rather than an absent one, so both documented paths are reachable and the choice is validated at
save time. Concretely: a `decide` node performs the choice, its declared outcome set corresponds
exactly to the labels on the branch edges leaving it, and the research node reaches it by an
ordinary success edge. The engine rejects a `decide` outcome with no matching branch edge, and the
converse, as validation errors — but note the limit, and do not take it on trust from here. That
validation is invoked from the explicit validate route and from run creation, which rejects an
error-severity document; the save path normalizes and writes without calling it, and nothing
validates a bundled template when it is read off disk. Confirm the call sites yourself with
`rg -n 'validate_workflow' src/`. So the engine's rejection does not by itself protect the shipped
artifact, and the guard test below is what does.

The workflow-level orchestrator flag is turned off for this template. After `ISSUE-260826-0004-01`
lands, that flag governs only per-node prompt refinement — an extra model call before each task
node that has prior output to refine against — which is not what this template set it for. The discovery command above shows which bundled
documents set the flag.

The template's substance does not change: the same three working nodes, the same prompts, the same
refinement loop with its continue/exit edges, the same variable, the same limits, and a canvas
layout that still reads left-to-right with the new node placed sensibly among the existing ones.

**Key interfaces:**
- **`decide` node config** — carries `inputs` (each a named binding with a `source`), a `prompt`,
  an optional `model`, and the declared `outcomes` list. Read the bundled Epic Dev and Dual Review
  templates for the house spelling of this node and its matching branch edges — read them as they
  stand rather than trusting any characterization of them here. Leave `model` unset so the template
  inherits the engine's default decide model.
- **Branch-edge labels** — the engine matches a `decide` outcome to a branch edge by label. The
  outcome strings and the edge labels are the same contract seen from two sides; they must agree
  exactly. **Keep this template's two existing branch-edge labels verbatim** and make the `decide`
  outcomes equal them. The exemplar templates happen to use SCREAMING_SNAKE outcome tokens; that is
  their house style, not a requirement, and renaming here would change the `chosenLabel` carried on
  the routing event for no gain.
- **The bundled template directory** — the guard test's subject. Templates are plain JSON workflow
  documents; some carry a subflow catalog whose entries are themselves workflow documents with
  their own nodes and edges.

**Acceptance criteria:**
- [ ] The template routes its depth choice through a `decide` node. Observable — returns no match
      before this change and a match after:

      ```
      rg -n '"type":\s*"decide"' templates/research-and-summarize.json
      ```

- [ ] The template no longer enables the workflow-level orchestrator flag. Observable — returns no
      match before this change and a match after:

      ```
      rg -n '"useOrchestrator":\s*false' templates/research-and-summarize.json
      ```

- [ ] Every branch edge in the repaired template originates from the `decide` node, and that node's
      declared outcomes equal the set of labels on those edges — no outcome without an edge, no
      branch edge without an outcome. Verify by loading the template and comparing the two sets, not
      by reading them off this brief.

- [ ] A test enumerates the bundled template files **from the filesystem** and rejects any branch
      edge that both originates from a node which is not a `decide` node and carries no condition of
      its own — checking a document's top-level edges and the edges of every entry in its subflow
      catalog. (The engine rejects nested catalogs as a validation error, so one level is the only
      shape a document reaching the engine can have — though since that validation does not run when
      templates are read off disk, treat one level as the shape to cover, not as a guarantee.) The
      template list must not be hardcoded, so a bundled template added later is covered by
      construction.

      **The rejection must be identified by a signal the passing path never emits, asserted inside
      the test run itself — not by a whole-suite failure and not by a check the implementer performs
      once by hand.** Concretely: the checker returns a diagnostic that names the offending edge and
      its source node and contains the fixed phrase `unroutable branch edge`, which nothing on the
      passing path emits. The test constructs its own violating witness at run time by taking an
      in-memory copy of a real bundled document and making exactly one change to it — re-pointing
      a branch edge at a source node that is not a `decide` node — and asserts the checker returns
      that diagnostic for it. (Stripping a condition instead would not be one change: derive
      whether any bundled branch edge carries a condition with `rg -n '"condition"' templates/`
      before assuming that route is open.) It does this once for a top-level edge and once
      for a subflow-catalog edge, so neither traversal arm can be silently missing. A whole-suite
      non-zero exit does not discharge this: the repaired corpus contains no violating document, so
      without a run-time witness a predicate that never fires stays green forever.

      The test cites `ISSUE-260830-1925-01` in a comment, so a later reader can find why it exists.
      Observables — each returns no match before this change and a match after. The first anchors on
      the diagnostic, which is the thing this criterion is really about:

      ```
      rg -n 'unroutable branch edge' src/ tests/
      rg -n 'ISSUE-260830-1925-01' src/ tests/
      ```

- [ ] The template still declares its `topic` variable, its step and visit limits, and its
      refinement loop's continue and exit edges. (Preservation criterion.)

- [ ] Every node the repaired template contains has a canvas position, including the node this
      change adds, and the layout still reads left to right. (Not preservation — the new node's
      entry does not exist at baseline. The two exemplar templates carry no canvas data at all, so
      they supply no spelling for this; follow the shape this template already uses.)

- [ ] `cargo test --locked` and `npm test` pass. (Preservation criterion.)

**Out of scope:**
- **The engine defect itself.** The unreachable orchestrator fallback is deleted by
  `ISSUE-260826-0004-01`; the silent first-edge default that outlives it is owned by
  `ISSUE-260830-1925-02`. Change no Rust routing code here. This record is independently
  actionable in either order: declared routing is the right shape for this template whether or not
  the fallback still exists.
- **The other bundled templates.** The guard test must cover them, but this record does not assert
  what it will find — that is the test's job, not this record's. If any bundled document other than
  the one named above turns out to violate the rule, stop and report it rather than editing it: that
  would be a finding this record did not anticipate, and it is the maintainer's call, not the
  implementer's.
- **Rewriting the template's prompts, its goal, or its description.** The routing is broken; the
  content is not.
- **Adding a `responseFormat` to the research node.** Making it emit JSON so that edge conditions
  could be evaluated is the other possible repair, and it is the wrong one — it would change the
  node's output shape for the downstream `{{previous_output}}` and `{{all_predecessors}}` consumers
  for the sake of routing.
- **Any change to how templates are installed into the workflow store.** A later epic owns the
  template-install operation.

## Triage Notes

Split out of `ISSUE-260826-0004-01` on 2026-08-30, at the maintainer's direction, as the half of
that record that is actionable now and independent of the routing-semantics question.

The defect was found while verifying that record's claims: `ISSUE-260826-0004-01` argued the dead
orchestrator fallback had no practical consequence, and the template sweep run during triage showed
it does — this template's entire branch structure depends on the unreachable path.

Verified during triage, and worth re-deriving rather than trusting: the research node declares no
`responseFormat`, so the condition scan is skipped before the seeded first edge is even challenged.
How far the same shape reaches across the bundled set is deliberately not asserted here — the guard
test this record adds derives it from the filesystem, and that is the authority.

---

**Readiness gate (cold-reader): FAIL** (round 1, 2026-08-30)

Independent cold reader, no planning context. Classes 1, 2, 4, 5, 6 and 8 do not fire; class 9 arm A
does not fire, with all three acceptance observables executed and confirmed red at baseline. Every
central factual premise was independently re-derived and all held — including the claim that this is
the only bundled document with the defect, which the reader confirmed by walking all six bundled
files and the one subflow catalog. This is a failure of authoring form and falsifiability, not of
substance.

Two independent blockers, plus one delegable gap.

**Class 7, five kind (a) rule-violation figures.** Every claim quantifying over the bundled template
set — "the only bundled one that sets the orchestrator flag", "no other bundled template has this
shape", "every other bundled template leaves it off", "none has this defect", and the same claim
restated in the notes — was asserted without a discovery command. All five were true when written.
That does not save them: adding one bundled template, or one subflow-catalog entry, silently
falsifies all five, and the record's own out-of-scope section leaned on one of them to tell the
implementer not to edit sibling templates.

**Remedy:** the two claims a single read-only command can derive now carry it
(`rg -n '"useOrchestrator": true' templates/`). The three cross-set uniqueness claims were
**withdrawn rather than supported** — the brief no longer asserts how far the shape reaches, and
names the guard test as the authority that derives it. Withdrawing the figure is the cleaner remedy
where the brief did not actually need the fact, and it removes the surface instead of maintaining
it.

**Class 9 arm B, requirement 2 (specific failure signal), on the guard-test criterion.** The
criterion asserted a refusal as the behavior under test but identified it only as "`cargo test
--locked` fails and the failure names that template", delivered by a falsification the implementer
runs once by hand. Two independent grounds for the failure, either sufficient: a whole-suite
non-zero exit is not a signal unique to the defect — that template is also read by existing template
loading and seeding tests, so an unrelated regression satisfies the criterion as written — and
nothing was asserted at run time, so once the repair lands the shipped corpus contains no violating
document and a predicate that never fires stays green forever.

**Remedy:** the criterion now requires the checker to return a diagnostic carrying the fixed phrase
`unroutable branch edge`, which the passing path never emits, and requires the test to build its own
violating witness at run time from an in-memory copy of a real bundled document differing in exactly
one dimension — once for a top-level edge and once for a subflow-catalog edge, so neither traversal
arm can be silently missing. The leading observable now anchors on that diagnostic rather than on a
bare search for this record's ID, which would have gone green on a comment alone.

**Class 3, delegable — the branch-label vocabulary was unpinned.** The brief said the template's
substance does not change while pointing at exemplars whose outcomes are SCREAMING_SNAKE tokens,
against this template's prose labels. Either choice would have validated and satisfied the criteria,
while renaming would change the label carried on the routing event. Now pinned: keep the existing
labels verbatim.

Also fixed from the round's non-blocking notes:

- **A rationale sentence over-claimed a guarantee.** The brief said the `decide` outcome-to-label
  validation means the two "can no longer drift apart silently". That validation runs on the save
  and validate routes — not when bundled templates are read off disk — so it does not protect the
  shipped artifact at rest. The brief now states the limit and points at the guard test as what
  actually protects it.
- The two JSON observables hard-coded a single space after the colon; both now tolerate any
  whitespace, so a reformat cannot defeat them.
- The guard-test predicate was grammatically ambiguous — "a node that is neither a `decide` node nor
  carries a condition" attached the condition to the node rather than to the edge. Rewritten with
  the edge as the subject.
- One criterion was labelled a preservation criterion while partly change-introducing: it required a
  canvas position for every node, including the node this change adds, which does not exist at
  baseline. Split into a genuine preservation criterion and a separate change-introducing one.

---

**Readiness gate (cold-reader): FAIL** (round 2, 2026-08-30)

Independent cold reader, no planning context, full round re-judged from scratch. **Both round-1
blockers discharged in substance.** Class 9 arm B is now satisfied: the reader confirmed requirement
2 is triggered and met by a run-time assertion on a signal unique to the defect, checked that the
fixed diagnostic phrase is absent repo-wide so it cannot pre-match, and verified the witness is
genuinely constructible — real documents exist in both traversal arms, and re-pointing one branch
edge is exactly one change. It also confirmed the anti-borrowing rule holds: the witness is a body
this criterion carries for its own sake. The class-3 label pin holds. Classes 1, 2, 4, 5, 6 and 8 do
not fire; class 9 arm A does not fire, with all four observables re-executed and red.

Failed on a **new, narrower** class-7 finding: the withdrawal of the cross-set uniqueness claims was
**incomplete**. One of the five figures round 1 named by name was still standing verbatim in the
Agent Brief's out-of-scope section — "They were checked during triage and none has this defect" — in
the very section round 1 said was leaning on it. The brief was simultaneously asserting and
disclaiming the same fact. The reader also flagged, as the weaker of the two and explicitly
overrulable, the Key-interfaces sentence asserting that both exemplar templates are in the bundled
set and both are correct.

This is exactly the failure the remedy self-check's third item exists to catch: a rule derived from
measurement ("the cross-set claims were withdrawn") whose measurement did not sweep the space it
quantifies over. One `rg` over the phrases round 1 had itself quoted would have found the survivor
before submission. Recorded plainly because the same self-check will be owed on any future round.

**Remedy applied, 2026-08-30.** The surviving out-of-scope claim is withdrawn — the section now
states only the conditional instruction, which never needed the premise. The exemplar sentence no
longer characterizes those files at all; it directs the implementer to read them as they stand.

**A factual error introduced by the round-1 remedy is corrected here.** Last round's rewrite said the
decide-outcome validation "runs on the save and validate routes". The save half is wrong: the save
path normalizes and writes without invoking validation, so an invalid workflow saves successfully.
The routes that actually reject are the explicit validate route and run creation, which refuses an
error-severity document. The load-bearing half — that nothing validates a bundled template when it
is read off disk — was correct, so the brief's conclusion stands. The sentence now names the real
call sites and tells the implementer to confirm them rather than trust the record.

**The same error exists in sibling `ISSUE-260826-0004-01`,** which says `decide` outcomes are
"validated at save time against branch-edge labels". That record is ungated, so correcting it needs
no `REOPENED`; it is being corrected in the same pass.

Also fixed from the round's non-blocking notes:

- **One of the two mutation options offered for building the witness was vacuous.** "Removing a
  branch edge's condition" presupposes a bundled branch edge that has one, and none does — an
  implementer taking that route would have had to add a condition first, silently breaking the
  exactly-one-change requirement. The option is dropped, and the criterion now carries the command
  that shows why.
- The claim that one level of subflow nesting is "exhaustive" leaned on a validation rule that, by
  this record's own argument, does not run when templates are read off disk. Reworded to say one
  level is the shape to cover, not a guarantee.
- The discovery command hard-coded a single space after the colon — the very brittleness the
  acceptance observables were loosened to avoid last round. Now whitespace-tolerant.
- "An extra model call before every task node" overstated: the refinement call is also gated on the
  cursor having prior output, so the entry node does not get one.
- "It validates clean" was true only of error-severity issues; the template does raise a terminal-
  node warning. Reworded to "raises no validation error".

**Tracked observation, not actioned — the guard predicate is narrower than the disease.** It catches
a branch edge whose source is not a `decide` node and which carries no condition. But a non-`decide`
node whose branch edges *do* carry conditions, on a node that declares no JSON response format, has
the same defect: the conditions are never evaluated, the seeded first edge wins, and a documented
path is unreachable. The engine only warns on that shape, and by this record's own argument warnings
do not run at template read. No bundled document has that shape today, so it is latent rather than
live. It belongs with `ISSUE-260830-1925-02`, which owns the routing semantics, and is recorded here
so it is not lost.

**Also noted:** the subflow arm of the witness has exactly one source document in the bundled set. It
is sufficient today and has no redundancy — if that document ever loses its catalog the criterion
becomes unsatisfiable as written, and nothing in this record would surface that.

---

**Readiness gate (cold-reader): PASS** (round 3, 2026-08-30)

Independent cold reader, no planning context. Full round, re-derived from scratch rather than as a
diff against round 2 — every observable re-executed, every premise re-verified. All classes `fine`;
class 6 does not fire in either prong; class 8 inert. Twenty-four class-7 surfaces swept against a
paired unrestricted discovery pass, none mapping to a blocking row.

- **The round-2 blocker is discharged.** The survivor phrase now appears only inside the round-1 and
  round-2 journal quotations and nowhere in the Agent Brief.
- **Class 9 arm B was re-derived independently rather than inherited**, and both requirements are
  satisfied. The reader confirmed the diagnostic phrase is absent repo-wide so it cannot pre-match,
  and that it is unique to the defect rather than shared with the success path.
- **The dropped mutation option was checked and its removal strictly improved the criterion.** With
  no bundled branch edge carrying a condition anywhere, the predicate's second conjunct is already
  satisfied, so re-pointing an edge's source is genuinely one change — while the dropped option
  would have required adding a condition and then removing it. Both traversal arms were confirmed
  constructible against real documents.
- **The rewritten validation-call-site sentence was re-derived clause by clause and is correct.** The
  reader did not assume the correction: it traced both production call sites, confirmed run creation
  rejects error-severity documents, confirmed the save path neither validates nor is validated
  downstream, and confirmed the template read path does not validate either.

Eight non-blocking notes. Three carried forward for whoever claims this:

- **The subflow-catalog premise is the sharpest remaining authoring weakness.** The witness's
  subflow arm rests on at least one bundled document having a catalog, stated only existentially,
  while the condition premise on the adjacent line carries a full discovery command. The asymmetry
  is visible and `rg -n '"subflows"' templates/` would close it — it would also hand the implementer
  the one document that arm needs. The reader declined to score it class 7 and flagged that a
  maintainer could reasonably overrule that call.
- One criterion pins the literal `"useOrchestrator": false` rather than accepting deletion of the
  key. Since that field carries a serde default, deleting it would be semantically equivalent but
  leaves the observable red. The pin is deliberate; an implementer who "removes the flag" fails it.
- The refinement-call description is narrower than the code — that gate is not keyed to task nodes.
  Harmless here, since this template contains only task nodes, but it describes a sibling record's
  post-state and should not be lifted verbatim.

**One correction owed, recorded rather than claimed.** The round-2 stamp above says the same
save-time error "is being corrected in the same pass" in sibling `ISSUE-260826-0004-01`. At the time
of this round it had **not** landed — that record still describes `decide` outcomes as validated at
save time. It is owed, and stating it in the future tense was the same over-claiming this record
already failed a round for. The sibling is ungated, so no stamp is freezing the defective copy and
its own gate will catch it; that is a reason it is safe, not a reason it is done.

Brief is immutable from this stamp. Promoting to `ready-for-agent`.

