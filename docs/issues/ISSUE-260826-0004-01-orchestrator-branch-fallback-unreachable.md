---
id: ISSUE-260826-0004-01
kind: issue
category: bug
status: needs-triage
origin: docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md
summary: The orchestrator branch fallback is unreachable dead code, and its guard makes an unmatched branch condition silently take the first branch edge
---

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
