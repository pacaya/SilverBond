---
id: ISSUE-260826-0637-08
kind: issue
category: enhancement
status: ready-for-agent
summary: Rewrite the execution model document around runtime mechanism, covering subflows, call frames, the scheduler's two dispatch classes, collector barriers, checkpoints and the pane kinds
prd: PRD-260826-0009-01
adrs: [ADR-260815-2009-02]
terms: [Run, Run Status, Cursor, Cursor Runtime State, Cursor Terminal Status, Checkpoint, Execution Epoch, Node Result, Runner Kind, Immediate Kind, Execution Log, Runtime Event, Stagnation Detection, Split Family, Copy-on-Split, Collector, Collector Barrier, Barrier Key, Merge Key, Representative Cursor, Parallel Batch, Batch Item, Call Frame, Decide Node, Decide Outcome, Approval Queue, Pane Kind, Active Pane Registry]
blocked_by: [ISSUE-260826-0637-03]
---

## Agent Brief

**Category:** enhancement
**Summary:** Rewrite `docs/execution-model.md` from the runtime source as conceptual prose about
mechanism — the document's value is knowledge a reader cannot grep for, and reconstructing it from
fourteen thousand lines of runtime code is expensive and repeated.

**Current behavior:**
The document was last touched roughly two feature epochs ago and describes a runtime that no
longer exists. It has no subflows, no call frames, no parallel batch, no decide nodes and no pane
kinds. Its cursor state machine lists six states where the engine has four, and omits the one the
entire scheduler filters on. It says traversal is depth-first; there is no depth-first traversal
anywhere — it is a scheduler loop. It says a run pauses for approval; the paused status is never
assigned outside tests, and a run blocked on approval stays running. Its checkpoint section lists
a fraction of the fields and models a single pending approval where the engine keeps a queue. It
describes the orchestrator selecting branches at decision points, and an agent capability gating
that selection, when the code path involved is unreachable.

**Desired behavior:**
A document written as prose about how the engine actually runs a workflow, deliberately not as
lists. Nothing in it is generated and nothing pretends to be. Node-kind *enumeration* does not
live here — this file explains how kinds execute and cross-references the schema reference's
generated catalog for their shapes.

Every mechanism below has a name in `CONTEXT.md`. Use those names, and honour the `_Avoid_` list on
each entry — several of them exist precisely because the old document used the banned label.

*Run and cursor lifecycle.* The four cursor runtime states and why there are no terminal ones: a
finished cursor is removed from the run's cursor list rather than marked, so a reader looking for a
failed cursor in a checkpoint will never find one. Terminal-ness is a separate concept recorded
only on a collector arrival. Every site that creates a cursor — derive the set yourself rather
than taking a number from here or from the audit, and say which counting rule you used, since
constructing a cursor value and minting a fresh cursor id do not give the same answer — most of
which the old document
never mentions — including the ephemeral batch-item cursors that never enter the run's cursor list
at all. The run statuses, including that one of them is never assigned outside tests.

*The scheduler loop.* What actually runs concurrently: runner kinds are dispatched into a
concurrent task set, immediate kinds are resolved synchronously inside the loop, and the loop then
selects across completions, approvals and a timer. Say plainly that a batch node's entire fan-out
runs inside the loop and blocks it.

**The split between the two classes has exactly one crossing, and it is worth stating.** A
runner-kind node whose skip condition fires is resolved inline in the immediate-cursor pass rather
than being dispatched; only when the skip does not fire does that arm fall through and the cursor
get dispatched into the task set. Presenting the split as clean would be a new falsehood in place
of an old one.

*Call frames and result scoping.* Entering a subflow pushes a frame that snapshots the caller's
scope and carries a separate result namespace; the push wipes the cursor's last output, counters
and branch markers and *replaces* its variable map rather than layering over it. The pop restores
**five** of the six snapshotted fields — counters, branch markers and the variable map — and
deliberately not the last output: it overwrites that with the subflow's exit-node output, and writes
the call node's own result into the caller's scope. That pair **is** the subflow return mechanism,
and it is exactly the knowledge this document exists to carry — an author expecting
`{{previous_output}}` after a call node to hold the pre-call output gets the subflow's result
instead. The snapshotted parent-output field is written into every checkpoint carrying a call frame
and read nowhere in production: a third write-only surface beside the two vestigial counter maps. Variable resolution is a two-branch selection, not a cascade: the checkpoint map is consulted
only when the cursor's own map and its call stack are both empty, and there is no fallback from a
non-empty cursor map to the checkpoint map. A zero-variable subflow therefore sees an empty scope
rather than inheriting one, and nested frames hold parent maps that no lookup consults.

**Node results are shared between split siblings at the root but private inside a subflow, and
this is a single fact split across two mechanisms.** Split children clone the parent's call stack
wholesale, and each call frame carries its own result namespace; the accessor that picks a result
map for a cursor returns one map or the other and never both, and the accessor that returns all
results is a plain clone with no merge. State the consequence directly — two branches of a split
at the root see each other's node results, the same two branches inside a subflow do not.

*Collector barriers.* Keying by scope, collector and epoch together, so the same collector reached
from two concurrent subflow calls gets two barriers rather than colliding. Expected inputs are
merge keys derived from edge labels falling back to the source node id. Two inbound edges sharing a
key are a validation error, so a runnable workflow never has them; the barrier builder does dedupe
into a set behind a log warning, but that path is reachable only if validation is bypassed — say
which is which rather than presenting the collapse as what an operator will meet. Duplicate
arrivals are dropped. Failed and timed-out cursors also
arrive at a barrier — this is what lets the permissive failure policy release one after a branch
died, and the old document gives no hint of it. The aggregate's shape: a keyed input object rather than an array — so a reader addresses a branch
by its merge key, never by position — beside a summary carrying the required count and per-status
tallies. Do not write that the total can differ from the number of inputs: a barrier releases only
when every required key has arrived, and every arrival is keyed by an inbound edge of that same
collector, so the two are equal whenever the aggregate is built. A merge-key collision collapses
both sides by the same key and cannot produce a difference either. Barriers
re-arm after release, which is what makes a loop through a collector work.

**Representative selection is first-still-live-in-arrival-order** — not "whichever branch won",
which is how it is easy to describe and is imprecise. Every other waiter is
deleted and its variable writes discarded silently. With no live survivor the engine resurrects the
first terminal arrival's snapshot, whose own source comment flags that the snapshot may be stale.

*Checkpoint, resume and restart.* What the checkpoint really carries, including mid-batch item
results, the stagnation detector's output hashes, the approval queue, and the limits frozen at
start. That two of its counter maps are vestigial and permanently empty for any run started by
current code. The content-hash dedup guard that skips a write entirely when nothing changed. That
there is no checkpoint versioning at all and forward compatibility rests on serde defaults. Resume
as its reconciliation passes rather than as "read the last checkpoint". Restart as everything it
actually does: draining the executor, reseeding a single cursor, clearing split families and
barriers, deleting descendant results, marking survivors stale, minting a new run id and marking
the old run restarted.

**The execution log is accumulated on the checkpoint throughout the run and separately persisted at
finalize.** It is written as the run proceeds. Restart resets only its header fields — run id,
start and end time, duration, terminal reason, aborted flag — and leaves the accumulated node
executions, decisions and transitions in place, so a restarted run's persisted log still carries
the prior run's entries. Do not write that restart resets the log. Describing it as written at
finalize would be wrong.

*The pane kinds.* Their lifecycle, and how a pane node finds its pane — which is **not** through
the per-run pane registry, however plausible that sounds. `send`, `wait`, `capture` and `kill`
parse their configured target (or a pane alias carried in the previous node's output) through the
tmux-tools target parser, then gate the result on the run's owned-target set: a run may only
address panes it owns. The pane registry is a separate map, consulted when a session-continuing
node reuses a pane and by the HTTP pane-context endpoints — that is where the registered keys, the
`active`/`current` aliases that resolve only when exactly one pane is registered, and node-id
prefix matching by highest sequence actually apply. The separation is about *resolution*, not
about writes: `send`, `wait` and `capture` each register their resolved target in the pane registry
under their own node key after resolving it by other means, and `kill` clears it — so the pane
kinds do write the registry even though none of them reads it to find its target. Document both mechanisms and keep them
distinct; `CONTEXT.md`'s **Active Pane Registry** entry states the boundary.

**Before writing the resume section, trace this.** Neither the pane registry nor the owned-target
set appears in the checkpoint's field list — both live on the in-memory run object — so a resumed
run starts with both empty. For pane nodes the consequence runs through the **ownership gate**, not
through a registry lookup: with an empty owned-target set, work out whether a `send` or `capture`
against a still-live pane is refused as un-owned, or whether the gate is skipped when no run
context is attached. Trace it and document what you find. Session reuse and the HTTP pane-context
path depend on the registry instead, and degrade differently — cover them separately. If any of it
turns out to be a defect rather than a documented limitation, file it separately rather than fixing
it here.

*Failure and termination.* That there is no failure edge outcome, so a node failure always ends
its cursor — and that outside any split family it does considerably more than that: it fails the
whole run and cancels every other cursor. Split-family membership is sticky, so a cursor that has
ever been enrolled keeps the in-split treatment for the rest of the run rather than only between a
split and its collector. Cite ADR-260815-2009-02 as the forward reference that adds a failure
outcome, never as present behavior. The three-way failure classification. The split failure policies and
that a policy applies across every family a cursor belongs to, so nested splits stack. The
run-killers: limit violations, which abort the run whole rather than failing a node and produce an
aborted status rather than a failed one; stagnation detection; and the backstop. That a run where
every cursor sits at a collector fails hard.

*Decide nodes.* That they bypass the task pipeline entirely on entry — no orchestrator refinement,
no retries, no session continuation, no agent-defaults merge. Two-stage routing: outcome selection,
where a structured label outside the declared set fails without falling back to prose; then edge
selection, whose emit-a-workflow-error-and-degrade-to-the-plain-success-edge arm is unreachable
from a validated document, because an outcome with no matching branch edge label is itself a
validation error. Describe that arm as unreachable rather than as behaviour an author can meet.

**Branch routing's real default.** An unmatched condition silently takes the first branch edge;
there is no "no condition matched" outcome, and nothing marks the route as defaulted: a
`branch_decision` event fires either way, carrying the same chosen-branch and chosen-label fields
as a matched route, and no warning accompanies it. Do not write that no event is emitted — one is.
The orchestrator fallback the old document describes is unreachable dead code.
`ISSUE-260826-0004-01` records the analysis behind it; cite it rather than re-deriving it, but
verify any claim you lift from it against the source, because it was itself corrected on this
point.

*The event vocabulary* — the one hand-written table this document owns. Every event kind the
runtime emits, including the several the old table omits, plus the monotonic sequence number stamped
when an event is appended, which is what makes the stream resumable and which the old document
never mentions. Event kinds are unconstrained strings emitted as literals at scattered sites, so
derive the list by sweeping those sites; there is no enum to enumerate.

*Delete rather than rewrite* the narrative claims about other subsystems that belong in those
subsystems' documents, and any hand-written enumeration this issue neither narrates nor tabulates.

**Key interfaces:**
- The runtime module is the authority throughout; storage owns the checkpoint's persistence and
  the workflow-snapshot migration. Name types and functions, not line numbers — this document
  outlives them.
- `CONTEXT.md` is the terminology authority. This issue's `terms:` list is the vocabulary it
  covers; every one of those entries is unmarked and therefore describes the engine as it is.
- `docs/sources/workflow-schema-drift-260825.md` Part 2 is the work order — sections 2.1 through
  2.7, each finding cited on both sides.
- The schema reference's `## Node catalog` is the cross-reference target for node shapes.

**Acceptance criteria:**
- [ ] The document names the four real cursor runtime states, including the one the scheduler
      filters on. Observable: `rg -c 'Runnable' docs/execution-model.md` returns matches; no
      matches before this change.
- [ ] Subflows and call frames are documented. Observable: `rg -ci 'call frame' docs/execution-model.md`
      returns matches; no matches before this change.
- [ ] Parallel batch is documented, including that the batch node succeeds only when every item
      succeeded. Observable: `rg -c 'parallel_batch' docs/execution-model.md` returns matches; no
      matches before this change.
- [ ] Representative selection is documented as first-still-live-in-arrival-order and states that
      the other waiters' variable writes are discarded. Observable:
      `rg -ci 'representative' docs/execution-model.md` returns matches; no matches before this
      change.
- [ ] The event table documents the monotonic sequence number as what makes the stream resumable.
      Observable: `rg -ci 'seq' docs/execution-model.md` returns matches; no matches before this
      change. (`loop_max_reached` belongs to the coverage criterion below — it would go green with
      nothing written about `seq` at all.)
- [ ] Every event kind the runtime emits appears in the table. Observable at the seam: sweep the
      runtime module for event-construction sites, list the kinds, and confirm each has a row.
      **Scope the sweep to production code** — the crate's test modules construct events with
      throwaway kind strings — not in the runtime module, but in others — and a whole-`src/` sweep
      silently adds them to the
      vocabulary. Report the sweep's size in the closing note; do not take a count from this brief.
- [ ] The document states that there is no failure edge outcome, that a node failure ends its
      cursor, and that outside any split family it fails the whole run and cancels every other
      cursor — citing ADR-260815-2009-02 as a forward reference rather than present behavior. Do
      not write that a node failure is terminal for its cursor alone: `CONTEXT.md`'s **Edge
      Outcome** entry bans that phrasing precisely because it understates the blast radius.
- [ ] The single crossing between the two dispatch classes — a runner-kind node resolved inline
      when its skip condition fires — is documented.
- [ ] The document states that node results are shared between split siblings at the root and
      private inside a subflow.
- [ ] The execution log is described as accumulated on the checkpoint during the run and separately
      persisted at finalize.
- [ ] The real branch-routing default is documented, and the unreachable orchestrator fallback is
      not described as live behavior. Observable at the seam: no surviving claim that the
      orchestrator selects branches, and no claim that an agent capability gates that selection.
- [ ] The resume section describes what actually happens to a pane node after a resume: the
      ownership set is in-memory and comes back empty, the ownership registration is always present
      on the production pane path, and the owned-target check is a membership test — so the node is
      refused with a not-owned-by-this-run error rather than silently retargeting. Confirm this
      against source before writing it, and cover the registry-dependent paths — session reuse and
      the HTTP pane-context endpoints — separately, since they degrade differently.
- [ ] The document uses `CONTEXT.md`'s names for the concepts this issue's `terms:` list covers,
      and uses no `_Avoid_` label **as the name of the concept it is banned for**. Observable at the
      seam: extract the `_Avoid_` clauses for those entries and read the document against them.
      This is a naming check, not a token ban, and it must not be run as one: `transition` is on an
      `_Avoid_` list *and* is a production event kind the coverage criterion above requires in the
      event table, and `condition`, `outcome` and `join` are banned only in their bare or nominal
      senses, which a document about the execution model uses constantly.
- [ ] Claims are traceable: each mechanism paragraph cites the source location that owns it. This
      document is not machine-checkable and the epic does not pretend otherwise — it rests on
      citations and review.
- [ ] `just check-v4-docs` passes and `cargo test` is green.

**Out of scope:**
- Any change under `src/` or `ui/`. Behavior found to be wrong is documented as it is and filed
  separately — the unreachable orchestrator fallback and the silent first-branch default are
  already filed as `ISSUE-260826-0004-01`.
- **Editing ADR-260815-2009-01, whose body still describes node outputs as write-once.** That is
  false — the result insertion is a plain map insert with no occupancy guard, so a loop revisit
  replaces the node's own prior result. `CONTEXT.md`'s **Node Result** entry already records the
  replacement behavior correctly, and this document should too. The ADR's wording fix belongs to
  the `typed-contracts` epic, which owns that ADR; do not edit it here.
- `docs/workflow-schema.md`, which has its own issues, and the node-kind catalog, which is
  generated into it.
- Generating any part of this document. Nothing in it is generated.
- Describing typed variables, the condition AST, `assignVars`, profiles, the script node or the
  failure outcome as present. Most carry the `_(planned — ADR-…)_` marker in `CONTEXT.md`;
  the condition AST and the JSON-valued variable store do not, because Condition and Variable are
  unmarked entries defining the present v4 forms and carry the future shape in a `Decisions:`
  clause instead. Describe v4 either way.
- Documenting the PTY interaction tiers beyond correcting the claim that interaction pauses the
  run — it blocks only the calling task's receiver while sibling cursors keep executing.

## Triage Notes

**Readiness gate (cold-reader): PASS** (round 5, full-enumeration)

Rounds 1-4 ran without terminal stamps; round 4 raised a finding against the call-frame paragraph,
which was rewritten. Before round 5 the brief was edited in four places to track corrections landed
in `CONTEXT.md` — the failure-scope claim and its acceptance criterion, the decide fall-through
arm's reachability, and the pane kinds' registry writes. Round 5 ran as a full-enumeration round
with all four promoted as tracked findings and judged from source: the call-frame arity, the
write-only parent-output field, the two-branch variable resolution, sticky split-family membership,
and the send/wait/capture registrations were each verified against `src/`. Classes 1-5 swept 42
decision surfaces, class 7 swept 57 extracted surfaces, class 9 arm A executed all five in-scope
observables against the tree (all red at baseline) and arm B found no triggered requirement among
15 criteria, class 6 prong (b) was argued at length and declined on the PRD's recorded
one-author-per-document decision, and class 8 was inert on a never-stamped record. No class fired.
