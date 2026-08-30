---
id: ISSUE-260826-0004-01
kind: issue
category: bug
status: ready-for-agent
origin: docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md
summary: Delete the unreachable orchestrator branch-selection fallback; the silent first-edge default it exposed is split out to ISSUE-260830-1925-02
---

## Agent Brief

**Category:** bug
**Summary:** Delete the unreachable orchestrator branch-selection fallback and everything that exists
only to keep it compiling, changing no runtime behavior.

**Current behavior:**
In the engine's branch-routing block — the part of the next-edge selection function that runs for
non-`decide` nodes when a node has outgoing branch edges — the chosen edge is seeded from the first
branch edge before any condition is evaluated, inside a block already guarded on the branch-edge
list being non-empty. The condition scan that follows uses `.or(...)` against that seed, which
preserves it. A guard further down tests whether the chosen edge is still absent and, if the
workflow's orchestrator flag is set, calls an orchestrator helper that asks an LLM to pick a branch
id. That guard can never be true, so the helper has no reachable production call site. What
references remain, and where, is not asserted here — enumerate them yourself with
`rg -n 'run_orchestrator_branch' src/ tests/`. Treat the reference inventories in this record's
`## Triage Notes` as historical rather than current; the lists under **Out of scope** below are
specification and do bind. What matters for scoping is behavioral, and holds
however many references the command returns: nothing anywhere asserts anything about which branch
this helper selects, so deleting it removes no behavioral coverage.

The helper is also a strictly weaker duplicate of a live mechanism: `decide` nodes perform LLM
routing against a *declared* outcome set that the engine checks against the branch-edge labels — an
outcome with no matching edge, and an edge with no matching outcome, are both validation errors —
and they emit a decision event recording the call. Do not read "validated" as "validated on save":
that check runs from the explicit validate route and from run creation, which refuses an
error-severity document, while the save path normalizes and writes without invoking it. Confirm the
call sites with `rg -n 'validate_workflow\(' src/api.rs`. What matters here is only that the check exists
and is declared; when it runs is a separate question this record does not turn on. The unreachable helper prompts for a bare branch id under the
default agent, validates nothing, and — if the returned id matches no edge — falls back to the same
first branch edge. Making it reachable would therefore add an unlogged LLM call in front of an
unchanged default rather than fixing anything.

**Desired behavior:**
The unreachable helper, its call site, and the guard that gates it are gone. The prompt-refinement
orchestrator path is untouched and still runs. Routing behavior is bit-identical before and after:
a non-`decide` node with branch edges still selects the first edge whose condition matches, and
still falls back silently to the first branch edge when none match or when there is no parsed
output. The documentation that currently describes the helper as unreachable dead code no longer
names a symbol that does not exist, while still recording the silent first-edge default it also
describes.

**Key interfaces:**
- The **orchestrator branch-selection helper** — an async function taking the workflow goal, the
  node, its output, the branch-edge list, a working directory, and a tmux invocation, and returning
  a node result whose output is a branch id. Delete it entirely.
- The **workflow-level orchestrator flag** (`use_orchestrator` on the runtime workflow, serialized
  as `useOrchestrator`) — **keep it.** It still gates the live prompt-refinement path, which is a
  different function and is not in scope. Do not remove the field, its serialization, its frontend
  type, or the editor toggle that sets it.
- The **orchestrator prompt-refinement helper** — the other orchestrator function, called from the
  task-execution path and reachable whenever the flag is set and the cursor has prior output.
  Untouched.
- The existing test that asserts both orchestrator one-shots thread the `runAs` invocation prefix
  and socket — its refinement half must survive with its assertions intact. Only the branch half
  goes. Renaming that test to match its reduced scope is at your discretion.

**Acceptance criteria:**
- [ ] The orchestrator branch-selection helper no longer exists in the crate this change touches.
      Observable — returns matches before this change and none after:

      ```
      rg -n 'run_orchestrator_branch' src/ tests/
      ```

- [ ] The helper was deleted rather than renamed. Observable, anchored on the prompt text unique to
      it — returns a match before this change and none after:

      ```
      rg -n 'deciding which branch a workflow should take' src/
      ```

- [ ] `docs/execution-model.md` no longer names the deleted symbol, and still records the silent
      first-edge default in the same passage. Observable — returns a match before this change and
      none after:

      ```
      rg -n 'run_orchestrator_branch' docs/execution-model.md
      ```

      The absence half alone would also be satisfied by deleting the passage outright, so it is
      paired with a preservation observable that must stay green before and after — the passage's
      description of the default has to survive the edit:

      ```
      rg -n --pcre2 'first branch edge is (?:taken silently|silently taken)' docs/execution-model.md
      ```

      The pattern tolerates either word order because the implementer is editing that very line and
      an honest reword must not turn a preservation observable red.

- [ ] A test pins the routing behavior this change must not alter: a non-`decide` node whose branch
      edges all carry conditions that evaluate false routes to the first branch edge, and the same
      node routes to the first branch edge when the node produces no parsed output. The test names
      `ISSUE-260830-1925-02` in a comment as the record that owns changing this behavior, so a later
      reader knows the assertion documents a known defect rather than a desired guarantee. The
      routing function this exercises is private to the engine crate's runtime module, so the test
      belongs beside the existing routing tests in that module's `#[cfg(test)]` block — which is why
      both observables scope `src/` and not `tests/`.
      Observables — each returns no match before this change and a match after. The first
      anchors on the test itself so the criterion measures the work rather than a comment; you
      choose the rest of the name:

      ```
      rg -n 'fn .*unmatched_branch' src/
      rg -n 'ISSUE-260830-1925-02' src/
      ```

- [ ] The live prompt-refinement path still runs when the workflow orchestrator flag is set.
      (Preservation criterion — green before and after.) The surviving half of the invocation-
      threading test still asserts the refinement one-shot receives the `runAs` prefix and socket.

- [ ] `cargo test --locked` and `npm test` pass. (Preservation criterion.)

**Out of scope:**
- **Changing what happens when no branch condition matches.** The silent first-edge default stays
  exactly as it is. It is a real defect, and it is owned by `ISSUE-260830-1925-02` — which does not
  itself decide the fix: that record is `ready-for-human` and puts the choice to the maintainer,
  naming `condition-ast` as the likely owner while arguing that epic's charter may not cover this
  case. Deleting dead code and changing routing semantics are different changes; doing both here
  would make the diff impossible to review as a no-op.
- **The `branchChoice` driver capability.** It is advertised by the Claude driver and read by
  nothing on the routing path. That remains true after this change and stays true. It is a driver-
  seam field, and ADR-260815-2009-04 records that this initiative makes exactly one driver-seam
  change, which is not this one — so removing it is chartered to no epic and needs a maintainer
  decision first. Leave the field, its registry copy, its advertised value, the `/api/capabilities`
  serialization, and the `docs/agent-drivers.md` row alone.
- **The workflow orchestrator flag and its UI.** See Key interfaces — it still has a live consumer.
- **The `decide` node's agent-configuration handling.** The `decide` path bypasses the agent-
  defaults merge and uses the default agent; that is a separate finding owned by the
  `profile-catalog` epic. Do not touch it.
- **`docs/sources/workflow-schema-drift-260825.md` and anything under `docs/issues/code-reviews/`.**
  Those are preserved historical audit records. They describe the tree as it stood when they were
  written and are not maintained against later changes. Do not edit them, even though they name the
  deleted symbol.

## Triage Notes

Found on 2026-08-25 while auditing `docs/execution-model.md` against `src/runtime.rs` for the
`docs-truth` epic of RDMP-260815-2009-01. Filed rather than fixed: the `docs-truth` epic
excludes engine code changes, and this finding has consequences for two later epics that
should be weighed before anyone touches the code.

**The defect.** In the branch-routing block at `src/runtime.rs:6527-6546`:

```rust
if !branch_edges.is_empty() {
    let mut chosen = branch_edges.first().copied().map(|edge| edge.clone());   // :6528
    if let Some(parsed) = &result.parsed_output {
        chosen = branch_edges.iter().find(/* condition matched */)
            .map(|edge| (*edge).clone())
            .or(chosen);                                                        // :6538
    }
    if chosen.is_none() && workflow.use_orchestrator {                          // :6541
        … run_orchestrator_branch(…) …                                          // :6546
    }
```

`chosen` is seeded from `branch_edges.first()` inside a block already guarded on
`!branch_edges.is_empty()`, and `.or(chosen)` preserves that `Some`. So `chosen.is_none()`
at `:6541` can never be true. `run_orchestrator_branch` (`src/runtime.rs:7444`) has exactly
one production call site — the unreachable one at `:6546` — plus one test call site at
`:10148` that keeps it compiling.

**Two consequences, one root cause:**

1. **Dead code.** The orchestrator-LLM branch fallback has never run in this form. Whether the
   fix is to delete it or to make it reachable is the actual decision this issue carries.

2. **Silent misroute (the sharper half).** Because `chosen` defaults to the first branch edge,
   a node whose branch conditions *all fail to match* — or whose `parsed_output` is `None`
   entirely — silently routes down `branch_edges.first()`. There is no "no condition matched"
   outcome in the engine, and nothing marks the route as defaulted: a `branch_decision` event is
   emitted either way (`src/runtime.rs:6570`), carrying the same `chosenBranch` and `chosenLabel`
   fields as a matched route, and no warning accompanies it. A workflow author gets a
   plausible-looking route instead of a detectable failure.
   *(Corrected 2026-08-26: an earlier revision of this issue said no event is emitted on this path.)*

**Why it is worth deciding before the engine work starts** (not a request to reopen the
roadmap's passed gate — RDMP-260815-2009-01 gate passed 2026-08-16):

- `condition-ast` exists to add `onMissing` policies (`treat-as-false | error | route`) and
  `parse_error` routing — i.e. exactly the outcomes this code path silently swallows today.
  The epic's design should be checked against this real default rather than against
  `docs/execution-model.md:118`, which describes the unreachable orchestrator path as live
  behavior.
- `harness-port` acceptance criterion (d) requires the ported workflows to run "with the
  orchestrator-LLM fallback disabled, so every model invocation is node-scoped and logged."
  That criterion is vacuous as written — the fallback is already unreachable. The criterion
  should be re-pointed at whatever genuinely produces unlogged model invocations; the audit
  flagged `decide` nodes as the real candidate, since `run_cursor_task` short-circuits to
  `run_decide_node` at `src/runtime.rs:3785-3795`, bypassing the `agent_defaults` merge and
  using a hardcoded `DEFAULT_AGENT` (`src/runtime.rs:4027`) with only `decideConfig.model`.

**Related documentation drift** (handled by the `docs-truth` epic, not here):
`docs/execution-model.md:118` and `:229` describe the orchestrator as selecting branches at
decision points; `:233` claims a `branchChoice` agent capability *gates* that selection. The
capability itself is real — `AgentCapabilities.branch_choice` (`src/driver.rs:96`), copied into
registry capabilities (`:118`), advertised `true` by the Claude driver (`:733`) and serialized by
`/api/capabilities` (`src/api.rs:199-216`) — but no code reads it on the routing path, and the path
it would gate is unreachable per the defect above. So it is advertised and unconsumed, not absent.
Those lines get corrected to the real behavior as part of the docs rewrite; this issue owns the
code.

---

**Triage, 2026-08-30.** Verified both halves of the filed defect against the tree and confirmed
them. Two corrections to this record's own analysis, one new finding, and a three-way split.

**Correction 1 — the `harness-port` argument above is wrong; disregard it.** This record claims
criterion (d) of the `harness-port` epic ("the ported workflows run with the orchestrator-LLM
fallback disabled, so every model invocation is node-scoped and logged") is "vacuous as written"
because the fallback is unreachable. It is not vacuous. There are two orchestrator paths, not one.
`run_orchestrator_branch` is dead, but `run_orchestrator_refinement` is live: it is called from the
task-execution path whenever the workflow orchestrator flag is set and the cursor has prior output,
and it issues a real one-shot model call under the default agent to rewrite the node's prompt.
Disabling the flag disables that call, which is exactly the un-node-scoped invocation the criterion
is about. No change to `harness-port` is warranted on this record's account.

The `decide`-node observation this record offers as the "real candidate" is separately true — the
`decide` path short-circuits before the agent-defaults merge and runs under the default agent with
only `decideConfig.model` — but it already has an owner: the `profile-catalog` epic routes the
`decide` path through profile resolution and forbids a second selector on `DecideConfig`. Nothing
to do here.

**Correction 2 — the documentation half is already discharged.** This record's closing section
routes the `docs/execution-model.md` drift to the `docs-truth` epic. That rewrite has landed, and
the document now states the real behavior and cites this record by ID. The only documentation work
left is the consequence of deleting the symbol, which the Agent Brief owns.

**New finding — a shipped template is silently broken by this defect today.**
A bundled template sets the orchestrator flag — derive which, and how many, with
`rg -n '"useOrchestrator":\s*true' templates/` — and its first node is a plain `task` whose outgoing
branch edges carry no conditions at all. It was
authored expecting the orchestrator to choose between them. That node declares no `responseFormat`,
so there is no parsed output, the condition scan is skipped entirely, and the seeded first edge
survives. The template always takes its "Deep dive" branch and can never reach "Quick summary".
Split out as `ISSUE-260830-1925-01`.

Blast radius is bounded to that one template. Every other bundled document carrying branch edges
hangs them off `decide` nodes — at top level and inside subflow catalogs — and the decision function
returns on the `decide` arm before reaching the branch-routing block, so they are unaffected. Do not
take that from this note: `ISSUE-260830-1925-01` adds a guard test that derives the set from the
filesystem, and it is the authority.

**Maintainer decision, 2026-08-30 — delete, do not make reachable.** The argument that settled it:
making the guard reachable would not fix the silent misroute, because the helper's own failure path
falls back to the same first branch edge when the returned id matches no edge. It would add an
unlogged model call in front of an unchanged default. The helper is also a strictly weaker duplicate
of `decide`, which does LLM routing against a declared outcome set the engine checks against the
branch-edge labels — and the initiative's direction is declared, deterministic routing. (That check
runs from the validate route and from run creation, not on save; see the Agent Brief, which states
this correctly and notes the record does not turn on it.) Deletion costs no behavioral coverage:
nothing that references it asserts anything about branch selection — only that the call errors and
that the tmux invocation was threaded.

**Three-way split of this record:**

- **This record** keeps the dead-code deletion, now scoped to it. Behavior must be bit-identical.
- **`ISSUE-260830-1925-01`** — repair the broken bundled template. Independently actionable: giving
  that node declared routing is correct whether or not the fallback survives.
- **`ISSUE-260830-1925-02`** — the silent first-edge default itself, written up as a decision for
  the maintainer rather than fixed now. That record argues `condition-ast` is the likely owner but
  deliberately stops short of assigning it, because the epic's charter may not cover this case.

`summary:` was rewritten when the record's scope narrowed at this split, rather than left describing
work two other records now own.

**Deliberately not folded in — the `branchChoice` driver capability.** It is advertised `true` by the
Claude driver, copied into registry capabilities, and serialized by `/api/capabilities`, while
nothing reads it on the routing path. Deleting the fallback does not change that; it only makes it
permanent. Removing the field is a driver-seam change, and ADR-260815-2009-04 states that the
`AgentDriver` seam gains exactly one extension in this initiative — an actual-model capture hook,
whose per-driver wiring `harness-port` owns — and that this is the initiative's only driver-seam
change. So removing `branch_choice` is not chartered to `harness-port` or to any other epic here; it
sits outside the initiative and needs a maintainer decision before anyone touches it. Recorded so
the finding is not lost.

---

**Readiness gate (cold-reader): FAIL** (round 1, 2026-08-30)

Independent cold reader, no planning context. Classes 1-6, 8 and 9 do not fire. All four acceptance
observables were executed and confirmed red at baseline, and the reader specifically tested whether
an absence criterion could be satisfied by renaming rather than deleting: it can be for the symbol
search alone, but the paired observable anchored on the helper's unique prompt literal closes it,
and the reader confirmed that literal is unique to the dead helper and distinct from the surviving
refinement helper's. All five central factual premises reproduced correctly.

Blocked on three class-7 kind (a) rule-violation figures on brief text under edit:

1. **A wrong count.** The note claimed "the two other bundled templates with branch edges hang them
   off `decide` nodes". There are three, not two — `multi-agent-plan-implementation.json` carries
   branch edges inside its `subflows["dual-review"]` catalog, which the sibling record's own stated
   sweep scope ("at top level and inside subflow catalogs") covers and this note missed. The
   conclusion survives — all three route off `decide` nodes, so blast radius really is one template
   — but the figure was wrong, and a later reader re-deriving from it gets a set one short.
2. **A count where the rule mandates qualitative polarity.** One acceptance observable read "returns
   one match before this change"; the other three used the permitted qualitative form.
3. **Bare occurrence counts of call sites**, in three places. The symbol-search command is not
   authoritative for them: it derives all occurrences, while the figures rest on a test-versus-
   production qualifier the command does not express. Load-bearing, because the brief asks the
   implementer to delete "everything that exists only to keep it compiling" and the count is what
   told them when they were done.

**Remedy applied, 2026-08-30** (no `REOPENED` stamp: the record had never been stamped `PASS` or
`WAIVED`, so the brief was not immutable). The wrong count was corrected **in place** rather than by
appending, against this section's usual append-only discipline — it was written in this same triage
session, was never gated, and leaving a false figure standing to honor append-only would have been
the worse outcome. It was replaced with a qualitative statement plus a pointer to the guard test in
`ISSUE-260830-1925-01`, which derives the set from the filesystem and is now named as the authority.
The polarity count and all three call-site counts were replaced with qualitative forms or with the
enumeration command; no numeral was refreshed. Also fixed from the round's non-blocking notes:

- The first criterion's prose said "anywhere in the Rust sources" while its observable scoped only
  `src/`. Both now read `src/ tests/`, matching the sibling record's scoping.
- The documentation criterion was one-sided — it checked only that the deleted symbol is gone, which
  deleting the whole passage would also satisfy. It now carries a paired preservation observable on
  the sentence describing the first-edge default, which must stay green.
- The regression-test criterion's only observable was a bare search for this record's sibling ID,
  which goes green the moment the ID appears in any comment anywhere. It now leads with an
  observable anchored on the test function itself, with the ID search kept as a secondary check.
- **A citation was overstated and is now corrected.** The note claimed ADR-260815-2009-04 "puts the
  driver seam under `harness-port`". The ADR says something narrower and materially different: the
  `AgentDriver` seam gains exactly one extension in this initiative — an actual-model capture hook,
  whose per-driver wiring `harness-port` owns — and that is the initiative's *only* driver-seam
  change. Removing `branch_choice` is therefore not chartered to `harness-port` or to any epic; it
  sits outside the initiative and needs a maintainer decision. The out-of-scope boundary is
  unaffected, but the record was handing work to an epic the ADR has closed to it.

**Correction to the filing narrative above (appended, not edited — that text predates this session).**
Two decayed references, neither load-bearing. The code excerpt annotates `.or(chosen)` as `:6538`;
it is at `:6539`, and every other line annotation in that block is exact. And the three
`docs/execution-model.md` line citations in the closing section point into a document that has since
been rewritten to fewer lines than two of them name — Correction 2 above already discharges that
section, and the citations are superseded rather than repaired.

---

**Readiness gate (cold-reader): FAIL** (round 2, 2026-08-30, full-enumeration)

Independent cold reader, no planning context. Classes 1-6, 8 and 9 all `fine`; class 9 arm A
re-executed with all six observables red or correctly exempt. Seventy-seven class-7 surfaces swept.
All five central premises independently reproduced, and the reader confirmed the round-1 correction
of the bundled-template claim by sweeping the whole space itself — the corrected statement holds.

**The round-1 remedy block over-claimed, and that is the finding worth recording.** It asserted that
"the polarity count and all three call-site counts were replaced". The polarity count was. The
call-site counts were not: one sat verbatim in the filing narrative, unchanged from the last commit,
and another survived in the maintainer-decision paragraph in the words "its only test call site".
The reader established this by diffing the record against `HEAD` — the surviving figures are
invisible to anyone reading only the current text, precisely because the remedy block says they were
cleared. A self-report broader than the edits it describes is worse than no self-report, because it
suppresses the check.

Two further kind (a) figures blocked, both in the Agent Brief:

- The Current-behavior passage stated the helper's remaining references as an inventory and then put
  a discovery command beside it. That is the shape the authoring rule names explicitly as
  "decoration readers treat as an assertion". The command was not authoritative for the figure
  either: it derives every occurrence, while the figure rested on a test-versus-production qualifier
  the command cannot express.
- **A figure I introduced in the round-1 remedy, which was also wrong.** Aligning one criterion's
  scope, I wrote that "the Rust sources" means `src/` and `tests/` "for this repository". It does
  not: `src-tauri/src/main.rs` and `src-tauri/build.rs` are tracked Rust sources of a second crate
  that path-depends on this one. A remedy that introduces a fresh false inventory is a net loss.

**Remedy applied, 2026-08-30, and this time swept before being claimed.** Both brief-side figures
are **withdrawn** rather than restated: the Current-behavior passage now asserts no inventory and
carries only the command plus the behavioral point that actually scopes the work — that nothing
anywhere asserts what this helper selects, which holds however many references the command returns.
The criterion no longer enumerates directories; it names the crate the change touches and lets its
observable define its own scope. The maintainer-decision paragraph's count is gone.

The filing narrative's call-site figures are **left standing and disclaimed** rather than edited.
That text predates this session and this record's append-only discipline; the brief now instructs
the implementer to treat no inventory anywhere in the record — its `## Triage Notes` named
explicitly — as current, and carries no inventory of its own for a reader to fall back on. This is
the disclaimer pattern that cleared the gate on `ISSUE-260826-0240-01`, and it is used here
deliberately rather than by default.

**Sweep, reported rather than asserted.** After editing, a scan for counts and inventories across
everything above the gate-stamp journal returns three surfaces: the verbatim ADR quotation
("exactly one driver-seam change" — a spec-figure whose provenance the reader confirmed against the
ADR text), and two in the untouched filing narrative, both covered by the disclaimer above. Command:
`rg -n --pcre2 '\b(exactly one|one production|one test call site|two later epics)\b'` over the
record.

Also fixed, both from the round's class-1 notes, and both cases of a correction landing on the wrong
surface:

- **The ADR overstatement was corrected in the notes last round but left standing in the brief**,
  which is the only surface a downstream implementer reads. The brief's out-of-scope rationale still
  said "the driver seam is owned by a later epic". It now records what the ADR actually says — this
  initiative makes exactly one driver-seam change and this is not it, so removing the capability is
  chartered to no epic and needs a maintainer decision.
- **`ISSUE-260830-1925-02` was described as routing the default to `condition-ast`.** It does not. It
  is `ready-for-human`, names `condition-ast` as the likely owner, and then argues that epic's
  charter may not cover either situation, leaving the assignment to the maintainer. Both the brief
  and the notes now say that. This is the same genus of error as the ADR overstatement — a
  neighbouring artifact's position restated more decisively than the artifact takes it.

**Correction to the filing narrative (appended, not edited).** Its statement that the finding "has
consequences for two later epics" is now stale in this record's own terms: Correction 1 above
withdrew the `harness-port` consequence, leaving the `condition-ast` one.

One non-blocking note actioned: the documentation criterion's preservation half anchored on an exact
eight-word phrase on the very line the implementer must edit, so an honest reword would have turned
a preservation observable red. It now tolerates either word order.

---

**Readiness gate (cold-reader): PASS** (round 3, 2026-08-30) — **superseded by a post-round edit; see the REOPENED stamp below.**

Independent cold reader, no planning context, full round re-derived from scratch. Every gap `fine`;
class 6 does not fire in either prong; class 8 inert; class 9 fires on neither arm. Seventy-six
class-7 surfaces swept. Both round-2 blockers confirmed discharged, and the reader verified that the
two **withdrawals** stranded nothing: completeness is now carried by the total-absence sweep in the
first criterion, which cannot pass while any reference survives at any cardinality, plus the
cardinality-independent safety claim. It also confirmed the round-2 remedy's `src-tauri` finding
independently — two crates exist, and `src/` plus `tests/` is exactly the engine crate.

The "left standing and disclaimed" argument was judged on its own merits and upheld, with the limit
named: the pattern works here **only** because an acceptance criterion is a total sweep of the very
set the disclaimed figure counts. Absent that, the reader said it would have blocked on a blanket
disclaimer as pure immunisation. Worth carrying — it is not a reusable device.

**The round-2 stamp's sweep was inadequate, and the reader was right to say so.** It reported the
command it ran and called the result "three surfaces". Scoped to this record that alternation
returns nine lines, four above the gate journal; as written it carried no path operand at all, so
executed verbatim it searches the whole tree. Worse, a four-literal alternation cannot measure a
space defined as "counts and inventories". A genuine sweep of that range — digits plus number-words
— returns 67 lines, of which 6 assert a count or inventory as fact. That is the second consecutive
round in which the self-report was broader than what it established, and the second in the same
genus. The edits were sound both times; the measurement backing them was not.

**Readiness gate (cold-reader): REOPENED** (round 4 pending, 2026-08-30)

The brief was edited after round 3 returned, so that verdict does not describe the current text and
must not stand as its stamp. No `REOPENED` was owed at the moment of the edit — the authoritative
verdict was then `FAIL` round 2 — but writing a `PASS` that judged superseded text would be worse
than the over-claiming this record has already failed twice for.

**The edit was made because round 3 passed a claim that is false.** The brief said `decide` outcomes
are "validated at save time against branch-edge labels". They are not. The reader verified the
validation *rule* exists and treated that as confirming the premise, without checking when it runs.
It runs from the explicit validate route and from run creation; the save path normalizes and writes
without invoking it — `save_workflow` is three statements and none of them validates. The sibling
record `ISSUE-260830-1925-01` failed a round on this exact error and its round-3 reader traced the
call sites; this record's reader did not, and passed it. **A gate round is evidence, not proof.**

The correction was owed and publicly recorded: the round-2 stamp on `ISSUE-260830-1925-01` said it
"is being corrected in the same pass", and at that record's round 3 it had not been. It is corrected
now. The rewritten passage states what the engine actually checks, warns against reading "validated"
as "validated on save", carries the call-site command, and notes that when the check runs is a
question this record does not turn on — which is true, and is why the error was rationale rather
than instruction.

Three non-blocking notes from round 3 were actioned in the same edit:

- **The blanket disclaimer was self-undermining at its edges.** "Treat no inventory anywhere in this
  record as current" literally told the implementer to distrust the brief's own out-of-scope lists,
  which are specification. Now scoped: the Triage Notes' reference inventories are historical, the
  out-of-scope lists bind.
- The regression criterion's observables scope `src/` while the absence criterion scopes
  `src/ tests/`, and the brief never said why — an implementer who put the test under `tests/` would
  have done the work and left the criterion red. The brief now says the routing function is private
  to the engine crate's runtime module, so the test belongs beside the existing routing tests there.
- A template filename sat beside its own discovery command in the notes — the same decoration shape
  round 2 blocked on in the brief, surviving where the round-3 remedy had not applied its own
  standard uniformly. The filename is gone; the command derives it.

Two round-3 notes deliberately **not** actioned, recorded so they are visible: the `:6538` line
annotation in the fenced excerpt is still wrong (it is `:6539`) and three `docs/execution-model.md`
citations still point past the end of that file. Both sit in the filing narrative, both are already
disclaimed as superseded by appended corrections, and editing another author's narrative to fix
citations that carry no instruction is churn.

---

**Readiness gate (cold-reader): PASS** (round 4, 2026-08-30, full-enumeration) — **superseded by a post-round edit; see the REOPENED stamp below.**

Full-enumeration round, opened with the D4 conditional. Every gap `fine`; class 6 does not fire in
either prong; class 9 fires on neither arm. Fifty-seven class-7 surfaces swept against an extraction
list of the same size, plus a fifteen-item classes-1-to-5 ledger. All ten central premises reproduced.

**The reader corrected two things this record's own journal had asserted, and both corrections stand:**

- **Class 8 is not inert on this record.** Round 3's PASS at line 420 is a live stamp, so the record
  *has* been stamped `PASS` at some round, which makes class 8 applicable across the later
  `REOPENED`. Earlier stamps here said class 8 was inert; that was wrong from round 3 onward. The
  reader judged it substantively anyway and it does not fire — every candidate constraint in the
  notes is already expressed in the brief.
- **There is no note-only escape on this record.** With no current authoritative `PASS`, nothing is
  "already-stamped text", so every kind (a) surface falls under the strict row.

**On the round count as a signal** — the D4 re-check asked whether four rounds means the record is
the wrong shape. It does not: every failure has been prose hygiene inside the record, never scope,
and the work has monotonically shrunk since the three-way split. What the round count actually
measures is that this record's self-reports were broader than its edits three rounds running.

**Remedy self-check item 3 failed a third time**, and the reader demonstrated it rather than
asserting it: the round-3 stamp's "67 lines, of which 6" carries no command, and the reader could
not reproduce 67 under any of twenty-six configurations of range and word-set — observed values span
6 to 120. The figure in the same paragraph that *does* carry its command ("nine lines, four above
the gate journal") reproduced exactly. That contrast is the rule's whole point, and it is recorded
here as the standing lesson rather than explained away.

**Readiness gate (cold-reader): REOPENED** (round 5 pending, 2026-08-30)

Round 4 passed, and its first non-blocking note was that **the false claim which voided round 3 was
still in this record**, verbatim, in the maintainer-decision paragraph — while the REOPENED stamp
above asserts "It is corrected now." The round-4 edit fixed the Agent Brief's copy and missed the
notes' copy. That is the same genus as round 2's finding about a correction landing on the wrong
surface, mirrored, and it is the fourth consecutive round in which a self-report here outran its
edit.

Corrected now — and this time verified by command rather than asserted. `rg -n -i
'save.time.validated|validated at save'` over the record returns exactly one line, inside the
REOPENED stamp above, where the claim is quoted in order to be corrected. No assertion of it
survives anywhere in the record.

The record is reopened rather than stamped because round 4 judged text that no longer exists. The
edits are narrowing and were all recommended by round 4 itself, which is a reason they are low-risk,
not a reason the stamp may cover text it never saw — that principle is why round 3's PASS was
withdrawn, and applying it selectively when it is inconvenient would be worse than the extra round.

Two further round-4 notes actioned in the same edit:

- A cardinality sat beside its own discovery command in the notes ("One bundled template sets the
  orchestrator flag"), which is the decoration shape this record removed a filename for in the
  previous edit. The command now derives both which and how many.
- The call-site command in the brief was scoped to `src/`, where sixty of its sixty-two lines are
  unit-test call sites in the model module. It now scopes the API module, isolating the two
  production call sites the sentence is actually about.

Deliberately not actioned, and visible rather than silent: the `// :6538` annotation is still wrong
(it is `:6539`) and two `docs/execution-model.md` citations still point past that file's end. Both
sit in the filing narrative, both are disclaimed twice as superseded, and neither carries an
instruction.

---

**Readiness gate (cold-reader): PASS** (round 5, 2026-08-30, full-enumeration)

Full-enumeration round. Every class `fine`; class 6 does not fire in either prong; class 8 is
applicable and does not fire; class 9 fires on neither arm. Eighty-seven class-7 surfaces swept
against an extraction list of the same size, with a fifteen-item classes-1-to-5 ledger. All central
premises reproduced, tracing invocation rather than existence where the claim was about when
something runs.

**The claim that voided round 3 is gone.** The reader did not accept the record's own check: it ran
four wider pattern families of its own, including a paraphrase net for "on write", "at persist",
"refuses to save" and similar forms that the record's pattern would have missed, and got zero hits.
Every surviving mention either negates the claim or quotes it in order to correct it.

**Four figures in this record's own gate journal are wrong, and are corrected here rather than left
standing.** Each is journal, none is load-bearing, and the conclusion each supports was
independently re-derived as correct — but a record that has failed four rounds on measurement
discipline should not leave its own numbers wrong.

- The round-5 stamp says the verification command "returns exactly one line". **It returns two.** The
  second is that stamp's own sentence, which self-matches because it quotes the pattern. The cause
  is structural and the lesson is the rule this record keeps relearning: that figure should have
  been qualitative — "returns only the quoted-to-correct instance and this command's own text" —
  because a count written beside a command that the writing itself perturbs cannot be stable.
- "Sixty of its sixty-two lines are unit-test call sites" is **fifty-nine**. The sixtieth is
  `validate_workflow`'s own definition, which sits above the test boundary and is neither a test nor
  a call site. The exact defect the rule names: a figure resting on a qualifier its command does not
  express.
- The round-4 stamp says **three** `docs/execution-model.md` citations point past that file's end.
  **Two** do. The round-5 stamp used the right number without noting it was correcting one.
- "Round 3's PASS at line 420" is at **line 422** — a line citation into this record, already
  decayed. The finding it supports, that class 8 is applicable, is correct.

**On five rounds.** The D4 maintainer-attention check was re-run and does not indicate the record is
the wrong shape: scope has shrunk monotonically since the three-way split and has not grown, and
every blocker across five rounds was prose and figure hygiene rather than scope. What the round
count measures is that this record's self-reports outran its edits four rounds running. That is a
process signal about how the record was authored, not about the work it specifies.

One boundary worth carrying, established at round 3 and re-affirmed since: the "left standing and
disclaimed" treatment of the filing narrative's inventories holds **only** because an acceptance
criterion totally sweeps the very set those figures count. It is not a reusable device for
immunising figures generally, and a reader would have blocked on it as pure immunisation had that
condition not held.

Five non-blocking notes, all explicitly cleared to travel with the record. The one worth acting on
outside this record: both sibling records this brief depends on are untracked in git — they exist
and say what this record claims, but they are one `git clean` from disappearing.

Brief is immutable from this stamp. Promoting to `ready-for-agent`.

