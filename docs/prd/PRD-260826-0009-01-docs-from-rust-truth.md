---
id: PRD-260826-0009-01
scale: epic
stakes: internal
gate: passed 2026-08-26
roadmap: RDMP-260815-2009-01
terms: [Workflow, Run, Cursor, Checkpoint, Subflow, Collector, Condition, Variable, Template Token, Validation Issue, Runtime Event, Access Profile, Workflow Contract, Boundary Normalization, Assignment, onMissing, Script Node, Profile, Failure Outcome]
adrs: [ADR-260815-2009-01]
issues: [ISSUE-260826-0637-01, ISSUE-260826-0637-02, ISSUE-260826-0637-03, ISSUE-260826-0637-04, ISSUE-260826-0637-05, ISSUE-260826-0637-06, ISSUE-260826-0637-07, ISSUE-260826-0637-08]
---

# Documentation from the Rust truth

## Problem Statement

A workflow author — human or coding agent — who opens `docs/workflow-schema.md` to learn how to
write a workflow document is told to write a schema this engine does not accept. Every node
example in its Node Types section uses the v2/v3 flat `"type": "task"` shape; the engine requires
a nested `kind` object (`src/model.rs:864-878`). Its edges use `source`/`target`; the engine
requires `from`/`to` with a mandatory `id` (`:922-937`). It documents `loopCondition` as a prompt
string for an LLM to evaluate; the engine requires a structured condition object and evaluates it
deterministically (`:898-899`). It documents 4 of 14 node kinds, and 4 errors plus 2 warnings out
of the 86 validation issues the engine can emit.

`docs/execution-model.md` is worse in a different way. It was last touched 2026-06-08 and
describes a runtime roughly two feature epochs behind: no subflows, no call frames, no parallel
batch, no decide nodes, no pane kinds. Its cursor state machine lists six states, of which
the engine has four — and omits `Runnable`, the state the entire scheduler filters on
(`src/runtime.rs:157-165`, `:2874`). It says traversal is depth-first; it is a scheduler loop that
dispatches runner kinds concurrently and resolves immediate kinds inline (`:2823-3029`). It says a run pauses for approval; `RuntimeStatus::Paused` is
never assigned outside tests.

The full inventory is `docs/sources/workflow-schema-drift-260825.md` — 92 findings against the
schema doc, 62 against the execution model, 6 cross-cutting, each cited on both sides against
commit `5f2f2c7`.

The cost is not that readers learn nothing. It is that they learn something false and stop
looking. A doc's existence suppresses the `rg` that would have found `NodeKind` at
`src/model.rs:695` and returned 14 of 14 kinds correctly. Measured against that alternative, these
two files currently have negative value.

Two facts make this urgent rather than merely embarrassing. First, RDMP-260815-2009-01 schedules
five further epics that change this schema — `typed-contracts`, `condition-ast`, `script-node`,
`cursor-state`, and `profile-catalog` — and each is specified as a *delta* against these documents;
a delta against a false baseline is not checkable. Second, the drift was invisible: the repo's
only doc guard (`scripts/check-canonical-v4-docs.sh:8`) is a blacklist of five hardcoded v3
phrases that never reads `WORKFLOW_SCHEMA_VERSION` and would pass green if every doc said v2, v5,
or nothing at all — and no CI workflow runs `cargo test` at all, so nothing else could have caught
it either.

**Success** is observable in three ways: no reader can copy a documented example the engine
rejects; every node kind the engine accepts is discoverable from the docs; and a change to the set
of node kinds, or to the shape of any documented example, fails CI rather than shipping. The third
criterion is deliberately scoped to the catalog: a change to a field's *requiredness* — adding or
removing `#[serde(default)]` — alters what the wire format accepts without touching the Rust
construction API or any generated output, so it is caught by review against the Tier P tables, not
by CI. That boundary, and the fact that a failing CI run blocks a merge only if branch protection
is configured, are both stated in ## Testing Decisions rather than left for a reader to discover.

## Solution

Rewrite both documents from the Rust source, splitting their content by **who can maintain it
correctly** — a generator, or a human with a citation — because the two documents fail for opposite
reasons and want opposite remedies.

`docs/workflow-schema.md` is roughly 90% enumeration: node kinds, config shapes, operators,
validation issues. Hand-maintained enumerations are exactly what produced 4-of-14. The node
catalog therefore stops being hand-written at all: a generator constructs each `NodeKind` as a
**Rust value**, serializes it, and emits both a readable node fragment and a complete minimal
workflow into marker-delimited blocks in the markdown. Because the examples are constructed values
rather than transcribed JSON, they are checked by the compiler — a renamed or misspelled field
fails `cargo test` rather than shipping as documentation. Because the catalog's contents are checked
against the wire-tag lists serde derives for both node enums, a new variant fails the test suite
until documented. The surrounding
prose stays hand-written and human-edited; only the blocks between the markers are generated.

`docs/execution-model.md` is the opposite: its value is in mechanism a reader cannot grep for.
That a collector's representative cursor is the first still-live waiter in arrival order, silently
discarding the other waiters' variable writes (`src/runtime.rs:6041-6098`). That failed and timed-out cursors also
arrive at a barrier, which is what lets `best_effort_continue` release one (`:6251-6284`). That a
run in which every cursor sits at a collector fails hard (`:2963-2979`). Reconstructing that
knowledge from 14,607 lines of `runtime.rs` is expensive and repeated; this file is where the
epic's effort belongs, written as conceptual prose and deliberately not as lists. Nothing in it is
generated, and the PRD does not pretend otherwise.

What remains is hand-written but source-cross-referenced: field tables, the operator set, the
validation catalog, the event vocabulary — kept because a reader needs them and no generator can
reach them, each citing the source location that owns it. What is deleted rather than rewritten is
the remainder: hand-written enumerations that will be neither generated, nor cross-referenced, nor
narrated, plus narrative claims about other subsystems. An unowned list is how this document got
here.


The net effect inverts the naive allocation: the schema reference gets smaller and mostly stops
being written by hand, the execution model gets substantially larger, and the node catalog absorbs most of the
v5 bump automatically when `typed-contracts` lands — changed fields regenerate on their own, while a
genuinely new kind such as the `script` node still needs its arm and catalog entry added, with the
compile stop and set-equality check forcing that rather than leaving it optional.

## User Stories

1. As a workflow author, I want to copy a node example from the schema reference into a workflow
   document and have the engine accept it, so that the documentation is usable as a starting point
   rather than a trap.
2. As a workflow author, I want to see all fourteen node kinds the engine accepts, so that I can
   discover `parallel_batch`, `subflow`, and the pane kinds without reading Rust.
3. As a workflow author, I want each node kind shown both as a copyable fragment and as a complete
   workflow that passes validation, so that I can see the node in isolation and in a document the
   engine accepts.
4. As a workflow author, I want each config field's real default stated, so that I am not
   surprised that `send.enter` defaults to `true` or `runAgent.killAfter` defaults to `true`.
5. As a workflow author, I want the nested `kind` shape shown before anything else, so that I do
   not write the flat `type` shape that only the legacy migration path accepts.
6. As a workflow author, I want edge fields documented as `from`/`to` with a required `id`, so
   that my hand-written edges deserialize.
7. As a workflow author, I want the structured condition form documented with its operator set, so
   that I can write deterministic branch routing at all.
8. As a workflow author, I want to know that `loopCondition` is a structured object evaluated
   deterministically, so that I do not write an English sentence and wait for an LLM to read it.
9. As a workflow author, I want the subflow catalog documented, so that I can compose workflows
   without reverse-engineering `subflows` from a template.
10. As a workflow author, I want to know which variables are required inputs to a subflow, so that
    I understand why an empty `default` makes a variable mandatory at the call site.
11. As a workflow author, I want the complete list of template tokens, so that I stop reaching for
    `{{node_name:field}}`, which does not exist.
12. As a workflow author, I want to know that `contextSources` only registers a `{{context:…}}`
    substitution and injects nothing on its own, so that I do not expect context I never
    referenced to appear in a prompt.
13. As a workflow author, I want the validation catalog to reflect what the engine actually
    rejects, so that I can predict a failed run before I start one.
14. As a workflow author, I want to know that splits fan out over `success` edges and that branch
    edges from a split are a validation error, so that I stop building splits the engine will refuse
    to run.
15. As a workflow author, I want collector rules stated — one outbound success edge, merge keys
    from edge labels — so that I understand why two branches sharing a label are rejected rather
    than quietly merged.
16. As a workflow author, I want limits documented with their real defaults and their `0` sentinel
    behavior, so that I do not read `maxTotalSteps: 0` as "unlimited" when it means 50.
17. As a workflow author, I want to know that unknown keys are silently accepted, so that I do not
    trust a clean save as evidence my field name was correct.
18. As a workflow author, I want `continueSessionFrom`'s five constraints listed, so that I can
    chain sessions without discovering each rule by validation error.
19. As a workflow author, I want absolute-cwd enforcement documented, so that a relative path
    failure is predictable.
20. As a workflow author, I want the `extraArgs` allowlist documented, so that I know why my
    `--sandbox` override is rejected.
21. As a workflow operator, I want the run lifecycle described as it actually executes — a
    scheduler loop that dispatches some node kinds into concurrent tasks and resolves the rest
    inline — so that I can reason about what actually runs in parallel.
22. As a workflow operator, I want the four real cursor states named, so that a `Runnable` cursor
    in a checkpoint is not a mystery.
23. As a workflow operator, I want to know that terminated cursors are removed rather than marked,
    so that I do not look for a `Failed` cursor that will never exist.
24. As a workflow operator, I want split fan-out described as copy-on-split, so that I understand
    why sibling branches never see each other's variable writes.
25. As a workflow operator, I want call frames explained, so that I understand why a subflow sees
    a replaced variable map and a separate result namespace.
26. As a workflow operator, I want variable resolution described as the two-branch selection it
    is, so that I do not expect a cascade from a cursor map to the checkpoint map that the engine
    never performs.
27. As a workflow operator, I want to know that every variable is a string at rest, so that I
    understand today's stringification behavior ahead of the typed store that replaces it.
28. As a workflow operator, I want collector barrier keying explained, so that I understand why the
    same collector in two concurrent subflow calls does not collide.
29. As a workflow operator, I want to know that failed and timed-out cursors arrive at a barrier,
    so that `best_effort_continue` releasing a barrier after a branch died makes sense.
30. As a workflow operator, I want the `{inputs, summary}` aggregate shape documented, so that I
    can address a specific branch's output instead of guessing.
31. As a workflow operator, I want the rule that picks the representative cursor at a barrier release
    described honestly, including that the other branches' variable writes are discarded, so that
    I do not rely on state that will vanish.
32. As a workflow operator, I want to know that a run whose every cursor waits at a collector
    fails hard, so that a blocked barrier is a diagnosable outcome.
33. As a workflow operator, I want the checkpoint's real contents described, so that I can read a
    checkpoint and understand a stuck run.
34. As a workflow operator, I want to know there is no checkpoint versioning today, so that I do
    not assume a migration path that does not exist.
35. As a workflow operator, I want the frozen-workflow-snapshot invariant stated, so that I
    understand why editing a workflow does not change a running run.
36. As a workflow operator, I want restart-from-node's real effects listed — cleared barriers,
    deleted descendant results, staleness marking, a new run id — so that restart is predictable.
37. As a workflow operator, I want resume's reconciliation passes described, so that a resumed run
    that drops a pending approval is explicable.
38. As a workflow operator, I want the parallel-batch lifecycle documented, including that the
    batch node succeeds only when every item succeeded.
39. As a workflow operator, I want `collectorVar`'s ordering guarantee and its JSON-encoded-string
    form documented, so that I can chain one batch into another.
40. As a workflow operator, I want decide-node routing described in both stages, so that I
    understand what happens to an outcome the model names that no branch edge matches — which
    validation rejects before the run starts, making the runtime's degrade-to-the-success-edge
    arm unreachable from a validated document.
41. As a workflow operator, I want to know that decide nodes bypass retries, orchestrator
    refinement, and the agent-defaults merge, so that I do not expect node config to apply.
42. As a workflow operator, I want the pane kinds' lifecycle and target resolution
    documented, so that `active`/`current` aliasing and run-ownership enforcement are predictable.
43. As a workflow operator, I want to know that node failure is always terminal for its cursor
    today, so that I stop looking for a failure edge that does not exist.
44. As a workflow operator, I want limit violations described as aborting the run rather than
    failing a node, so that an `aborted` status is interpretable.
45. As a workflow operator, I want stagnation detection documented, so that a run aborting after
    three identical outputs is not a mystery.
46. As a workflow operator, I want the complete event vocabulary, including `seq`, so that I can
    consume the SSE stream and resume it.
47. As a workflow operator, I want to know that budgets are agent-CLI flags rather than runtime
    guards, so that I do not expect the engine to stop a run at a dollar figure.
48. As a workflow operator, I want to know that `branchChoice` is advertised by the capabilities
    endpoint but consumed by nothing, so that I do not design around a gate that has no effect.
49. As a coding agent, I want the docs to agree with the source, so that reading them does not
    make my output worse than grepping would have.
50. As a coding agent, I want the schema reference to point at `src/model.rs` as the authority, so
    that I know where to go when the doc is silent.
51. As a maintainer, I want the node catalog generated from Rust values rather than transcribed,
    so that a field rename cannot survive as documentation.
52. As a maintainer, I want a new `NodeKind` variant to fail the test suite until it is documented,
    so that node-kind coverage cannot silently regress the way it already did.
53. As a maintainer, I want the generated blocks checked for freshness in CI, so that a stale
    regeneration fails CI.
54. As a maintainer, I want `cargo test` to run in CI at all, so that the Rust suite protects the
    default branch rather than only the developer who remembers to run it.
55. As a maintainer, I want to edit documentation prose as markdown rather than as Rust string
    literals, so that generation does not make the docs harder to improve than they are today.
55a. As a maintainer, I want `cargo test` to stay side-effect-free, so that running the suite never
    rewrites tracked files under `docs/` behind my back.
55b. As a maintainer, I want one named command that regenerates the docs, cited in the docs
    themselves, so that a contributor who trips the freshness check knows what to run.
55c. As a maintainer, I want each config field's default recorded in a written table rather than
    inferred from an example, since a serialized example shows a value without ever marking it as
    the default — and for skip-heavy configs omits the field entirely.
56. As a maintainer, I want the capabilities endpoint's documented node list to match what it
    returns, so that the API reference stops making a false claim.
57. As a maintainer, I want the docs index to contain no dead links, so that the entry point to
    the documentation is trustworthy.
58. As a maintainer, I want v4 documented as canonical today with the decided v5 bump cited, so
    that the docs and `CONTEXT.md` make one claim about the present and one about the target,
    rather than contradicting each other.
59. As an epic author on this roadmap, I want a truthful baseline, so that my epic's schema delta
    is checkable against something real.
60. As an epic author on this roadmap, I want the real branch-routing default recorded — that an
    unmatched condition silently takes the first branch edge — so that `condition-ast` is designed
    against actual behavior rather than the unreachable orchestrator path the old doc describes.
61. As an epic author on this roadmap, I want changed field shapes in the v5 grammar to regenerate
    themselves, and genuinely new kinds to fail the suite until documented, so that `typed-contracts`
    inherits a documentation update rather than a documentation chore it can forget.

## Implementation Decisions

**No engine code changes.** This epic reads `src/`, writes `docs/`, and adds a documentation
generator plus its freshness check. No behavior in `model.rs`, `runtime.rs`, `storage.rs`, or
`api.rs` changes. Behavior found to be wrong is documented as it is and filed separately —
ISSUE-260826-0004-01 already carries the unreachable orchestrator branch fallback
(`src/runtime.rs:6527-6546`) and the silent first-branch-edge default it causes.

**Content is tiered by who can maintain it correctly.** Every finding in the drift audit is
assigned a tier.

- **Tier G — generated.** The node-kind catalog: for each of the 14 kinds, a readable node
  fragment and a complete minimal workflow, emitted into marker-delimited blocks. Never
  hand-edited.
- **Tier P — hand-written, source cross-referenced.** The common `WorkflowNode` field table, the
  per-node-kind config field tables, the
  template-token list, the validation catalog, the condition operator set, the event vocabulary, and
  the edge, top-level, variable, subflow-catalog, `skipCondition` and agent-config tables. Each entry cites the source location that owns it, so a reader can verify in one
  jump. **These are not machine-pinned**, and the PRD does not claim they are — see the honest
  limits below. The config field tables sit here rather than in Tier G for a structural reason
  given its own decision below.
- **Tier C — conceptual prose.** Cursor lifecycle, call frames, variable resolution, collector
  semantics, checkpoint and restart, how the scheduler decides what runs concurrently, failure and
  termination. The execution model
  is almost entirely this tier.
- **Tier D — deleted.** Hand-written enumerations that will be neither generated, nor
  source-cross-referenced, nor narrated, and
  narrative claims about other subsystems that belong in those subsystems' docs.

**The generator emits blocks, not documents.** Markdown files stay hand-authored and
human-editable; generated content lives between explicit markers
(`<!-- BEGIN GENERATED: <block-id> -->` … `<!-- END GENERATED: <block-id> -->`), and the generator
rewrites only the span between them. Prose is never in Rust string literals — that would trade one
maintenance problem for a worse one.

**Examples are constructed Rust values, not transcribed JSON.** The generator builds each
`NodeKind` as a value and serializes it with `serde_json`. This is what makes the examples
trustworthy in a way a JSON-parsing test cannot be: no struct in the model carries
`deny_unknown_fields`, so a misspelled field in transcribed JSON deserializes clean and validates
clean — a probe of `sendConfig: {"text":"hello","entter":false}` produced zero error-severity
issues. A constructed value cannot contain a field the struct does not have, because it would not
compile.

**Generated examples cannot state defaults, so field tables stay hand-written.** A serialized
example shows *a* value; nothing in the output marks which value is the default, so an example can
never answer "what happens if I omit this?". For skip-heavy configs it is worse — the field
disappears entirely: `SpawnConfig` carries `skip_serializing_if` on every field
(`src/model.rs:485-503`) and the `NodeKind::Spawn` arm adds `skip_serializing_if = "is_default"`
(`:723-726`), so a default-valued spawn node serializes to `{"type":"spawn"}` and documents
nothing, and `RunAgentConfig` skips 10 of its 11 fields (`:584-608`). The erasure is not universal
— `BatchConfig.max_concurrent` defaults to 4 and always serializes (`:454-470`) — but the
labelling gap is, which is what makes a written table necessary for every kind that carries a
config struct rather than only the ones whose defaults happen to survive serialization. Tier G therefore shows shape and usage; a Tier P table per kind carries field
names, types, required-ness, and defaults. This is the largest hand-maintained enumeration the
epic keeps, and it is named here rather than left implicit because an unnamed enumeration is how
the current doc reached 4-of-14.

**Coverage is enforced against serde's own derives, with zero new dependencies.** An exhaustive
`match` proves every *input* is handled; it does not produce one value per variant, so on its own
it cannot guarantee the catalog visits all 14. Two existing facts close that gap without a crate:

- **The variant skeleton comes from the library.** `impl From<WorkflowNodeType> for NodeKind`
  (`src/model.rs:820-860`) is an exhaustive match that builds one `NodeKind` per variant. It supplies
  the *shape*, not a usable example: the values it produces are defaults that mostly fail validation
  — `DecideConfig::default()` has no outcomes (`:441-452`, error at `:2958-2968`),
  `BatchConfig::default()` has empty `items_binding`/`item_var`/`body_entry` (`:473-481`, errors at
  `:3051-3087`), `SubflowConfig::default()` has an empty workflow name (error at `:2544-2556`), and
  `Task`/`RunAgent` arrive with no agent (error at `:1670-1677`). So the generator hand-authors the
  example data per kind on top of that skeleton. What constructing-in-Rust buys is not the absence
  of hand-written content — it is that the hand-written content is **compile-checked**: field names
  and types cannot drift, which transcribed JSON cannot promise. Required-ness is *not* covered — it
  is a serde attribute, so adding or removing `#[serde(default)]` changes what the wire format
  accepts without changing the Rust construction API. That is one reason the requiredness columns
  live in a Tier P table rather than being inferred from an example.
- **Enumeration is closed by harvesting both enums from serde.** `CATALOG_ORDER` is a hand-written
  `[WorkflowNodeType; N]` in `tests/docs_catalog.rs` naming the kinds the catalog documents, in
  document order; the generator iterates it and calls `example_for` per entry. Four checks sit
  around it, and together they are complete.

  1. **Compile stop.** An exhaustive `example_for(WorkflowNodeType) -> NodeKind`, carrying each
     kind's hand-authored example data. Adding a `WorkflowNodeType` variant fails to compile —
     under `cargo test`, the only profile that builds `tests/`; `cargo build` and `cargo check` do
     not touch it, so this is a suite guard and the CI job is what makes it unskippable.
  2. **The `WorkflowNodeType` list, harvested from its derive.** The type derives `Deserialize`
     (`src/model.rs:157`) with `rename_all = "snake_case"` (`:158`), and serde's derived
     implementation hands `Deserializer::deserialize_enum` a `&'static [&'static str]` of the wire
     names. The test supplies a throwaway deserializer that captures that slice and returns an
     error from `deserialize_enum` directly, without involving a visitor.
  3. **The `NodeKind` list, harvested from *its* derive.** `NodeKind` is internally tagged
     (`#[serde(tag = "type")]`, `src/model.rs:693-694`), so serde never calls `deserialize_enum`
     for it and the slice above is unreachable — but the variant list is still available. Feeding
     it a one-entry map `{"type": "<sentinel>"}` makes the derive reject the tag through
     `serde::de::Error::unknown_variant(variant, expected)`, whose `expected` argument *is* the
     full wire-tag list. A custom `Error` implementation captures it structurally, with no string
     parsing. Verified by compiled probe against this crate: exactly `task, approval, split,
     collector, decide, parallel_batch, subflow, call, spawn, send, wait, capture, kill,
     run_agent`.
  4. **Set equality across both lists, and a round-trip.** The wire names the catalog emits must
     equal the `WorkflowNodeType` harvest *and* the `NodeKind` harvest; `CATALOG_ORDER.len()` must
     equal their length. Omission fails set equality; a duplicated entry fails the length check; a
     mis-mapped arm fails the round-trip `example_for(t).node_type() == t` (`NodeKind::node_type()`,
     `src/model.rs:756-773`). And the case that defeated every earlier draft of this decision — a
     `NodeKind::Script` added without a matching `WorkflowNodeType::Script`, then mapped onto an
     existing type to satisfy the library's exhaustive `node_type()` — now fails, because the
     `NodeKind` harvest gains `script` while the catalog and the `WorkflowNodeType` harvest do not.

  Both enums are therefore checked against the derives that define the wire format, rather than
  against a hand-maintained list or a source-text scan. `serde` with `derive` is an existing
  dependency (`Cargo.toml:22`), so this costs nothing new and edits nothing under `src/`. Two
  earlier drafts of this decision reached for a source-text variant count and then for a bare
  exhaustive `match` over `NodeKind`; both were falsified by compiled probes during the gate, the
  first as formatting-dependent and the second because applying an exhaustive match to a *set* of
  values still requires a collection of them. `strum::EnumIter` would also work but only by
  attaching a derive inside `src/model.rs` (`:157-174`), resolvable by the library target only from
  `[dependencies]` — a runtime dependency the roadmap forbids, and an edit to `src/` this epic
  forbids itself.

This keeps the roadmap's dependency constraint satisfied literally rather than by reading "runtime"
narrowly, and it keeps "no engine code changes" literally true — nothing under `src/` is edited,
because the conversion the generator needs is already there. `node_kind_round_trip_all_variants`
(`src/model.rs:4180-4321`) remains the weaker prior pattern: its `Vec<NodeKind>` is hand-seeded with
no counterpart check, so an omission both compiles and passes.

**The generator lives in a new integration test, `tests/docs_catalog.rs`, and writes only when
asked.** Naming the file matters, because "a test module" and "nothing under `src/` is edited" are
only jointly satisfiable there: an inline `#[cfg(test)]` module inside any existing `src/*.rs` edits
`src/`, and a new `src/docgen.rs` still needs a `mod` line in `src/lib.rs`. A file under `tests/`
compiles as its own test crate against the public API — `WorkflowNodeType`,
`From<WorkflowNodeType> for NodeKind`, `ensure_defaults` and `validate_workflow` are all public
(`src/lib.rs` exposes `pub mod model`) — and dev-dependencies are available to it, though this
design needs none. It joins `tests/http_api.rs` as the crate's second integration test. By default it regenerates the blocks *in memory* and asserts equality against the committed
markdown, so `cargo test` stays side-effect-free and never rewrites tracked files. Setting `SB_REGEN_DOCS=1`
makes the same code write instead, and a `just` recipe wraps that — giving the docs a concrete
regeneration command to cite, which a contributor tripping the freshness check needs. This is the
`expect-test` / `insta` / `trybuild` pattern rather than an invention.

A separate binary target was considered and rejected on a concrete collision: `Cargo.toml` declares
no `default-run` and `src/main.rs` is the implicit sole binary, so a second one makes the
unqualified `cargo run` in `justfile:16-18` and `playwright.config.ts:25-26` ambiguous and breaks
both `just server` and the e2e harness. The `tests/` home has neither problem and reaches the library types
directly through the public API.

**Each generated example is a complete, valid workflow.** The seam that validates an example takes
a whole document — `WorkflowV3` requires `version` and `entryNodeId` (`src/model.rs:940`, `:951`)
— and per-kind validation imposes real structure: a Task needs an agent (`:1668-1677`), a Split
needs outbound success edges (`:1943-1970`), a Collector needs inbound edges and exactly one
outbound success edge (`:1972-2003`), a Decide needs a branch edge per outcome (`:2917-3043`), a
Subflow needs a catalog entry and a resolvable exit (`:2537-2707`). The generator emits the
supporting graph each kind requires, and the fragment shown for reading is extracted from that
same document, so the two can never disagree.

**`docs/workflow-schema.md` is restructured, not patched.** It opens with the document grammar —
the nested `kind` shape, the two genuinely required top-level keys, and the serde leniency that
accepts unknown keys silently. Then the generated node catalog. Then the Tier P tables — the common `WorkflowNode` field
table, per-kind config fields, edges, conditions and their operator set, variables, the subflow catalog, top-level fields,
`skipCondition`, agent config, template tokens, and the validation catalog. That is every Tier P
member except the event vocabulary, which belongs to the execution model. It states plainly that `src/model.rs` is
the authority, which parts are generated, and how to regenerate them.

**`docs/execution-model.md` is rewritten around mechanism.** Run and cursor lifecycle; the
scheduler loop and the split between kinds dispatched into concurrent tasks and kinds resolved
inline; call frames and the two-branch variable resolution;
collector barriers including keying, merge keys, failure arrivals, and the lossy rule that picks
the representative cursor; checkpoint contents, persistence dedup, resume reconciliation, and restart; the pane
kinds; failure classification and the run-killers; and the event vocabulary, the one Tier P
table this file owns. Where `CONTEXT.md` names one of these mechanisms the rewrite uses that name and honours its
`_Avoid_` list; some — the checkpoint's persistence dedup and the limit-violation abort among them
— are sub-behaviours of entried concepts and need no noun of their own. Node-kind *enumeration* does not live
here — this file explains how kinds execute and cross-references the generated catalog for shapes.

**Terminology follows the approved glossary, which now states present state.** Both documents must
use one name per concept across both files, and `CONTEXT.md` is that authority. The glossary
session settled both halves it owed. It added 30 engine terms the rewrite needs, and it adopted a
rule that makes the file safe to quote: **an unmarked entry's definition is true of `src/` at HEAD;
an entry marked `_(planned — ADR-…)_` is defined by that ADR instead, and the marker is the
standing notice that the engine does not accept it yet.** Three entries defined in their v5 shape
were rewritten to v4 with the change recorded in their `Decisions:` clause (Workflow, Variable,
Condition); seven terms with no referent in `src/` — Workflow Contract, Boundary Normalization,
Assignment, onMissing, Script Node, Profile, Failure Outcome — kept their ADR-derived definitions
and gained the marker. Collector was found true of the v4 engine and was sharpened rather than
rewritten. An agent told to "use `CONTEXT.md`'s vocabulary" can no longer write a v5 definition
into a v4 reference without stepping over an explicit marker.

**Version framing: v4 is canonical, v5 is decided and unlanded.** `WORKFLOW_SCHEMA_VERSION = 4`
(`src/model.rs:10`) governs what the docs describe. The bump to v5 is recorded in
ADR-260815-2009-01 and delivered by the `typed-contracts` epic; the docs cite it as a forward
reference rather than describing it as present. `CONTEXT.md` states v4 to match, carrying the bump
as `Decisions: ADR-260815-2009-01 (bumps to v5)` on the Workflow entry — the glossary and the docs
now make the same claim, and `typed-contracts` flips both.

**The version/migration contract is Tier C, not Tier P.** It is a behavioral contract with
consequences a table cannot carry: a missing `version` key is a hard ingest error rather than a
default; version rejection is an `anyhow::bail!` at ingest and never reaches the validation-issue
channel; the version is force-rewritten to the canonical value on every path that reaches validation, so
validation can never reject one;
subflows are migrated recursively; and the v2→v3 `kind` migration fires on *any* declared version,
not only 2 and 3 (`src/model.rs:1152`).

**The adjacent falsehoods are corrected in place.** `docs/api-reference.md:24` advertises four
supported node types where `src/api.rs:216` returns fourteen; the same document's node-testing
example uses the legacy flat shape that endpoint refuses by name; `docs/README.md:20` links to a
file that does not exist. None is a schema change. Leaving a known-false claim about a live
endpoint standing through five schema epics is not defensible, and each is a one-line fix.

**The drift audit is the work order.** `docs/sources/workflow-schema-drift-260825.md` carries all
160 findings with citations against commit `5f2f2c7`, grouped by tier. It is point-in-time
evidence in the same class as the roadmap's ingested prior-art research, not documentation — once
the rewrite lands, the generator and the freshness check are the durable guarantee. Four of its
entries were corrected during this PRD's gate pass (D29, D37, X2, X4) and carry inline correction
notes, as does its tier legend.

## Testing Decisions

A good test here observes what a reader would observe: that a documented example is a document the
engine accepts, and that a kind the engine accepts is a kind the reader can find. Neither
assertion reaches into how the doc is written or how the parser is structured.

**The primary guarantee is generation plus a freshness check, not an assertion.** This is the
repo's existing pattern for derived artifacts, pointed at a new one: `.githooks/pre-commit`
rebuilds the frontend from the staged tree and diffs it against staged `public/`, and
`.github/workflows/frontend-freshness.yml` does the same in CI. The docs generator gets the same
treatment — regenerate, diff, fail on difference. A stale generated block fails CI; whether that
blocks the merge depends on branch protection, a repository setting this PRD does not change.

**Four checks, in decreasing strength:**

1. **Compile-time (within `cargo test`).** The generator constructs every example as a Rust value
   on the skeleton from `From<WorkflowNodeType>` (`src/model.rs:820-860`). A renamed field or a
   changed type fails to compile; so does a new variant, because `example_for` is exhaustive. The
   generator lives in `tests/`, so this fires under `cargo test` rather than `cargo build` — which
   is precisely why the CI job matters. It covers only what the generator emits: shape and usage,
   not defaults, and not whether a new variant reached the catalog. That is check 2's job.
2. **Freshness, coverage and mapping.** The committed markdown matches what the generator produces,
   catching a regenerated-but-uncommitted or hand-edited block. The catalog's emitted wire names
   must equal the variant set harvested from serde, catching both an omitted kind and a duplicated
   entry. And every entry must round-trip (`example_for(t).node_type() == t`), catching an example
   arm that returns the wrong kind.
3. **`NodeKind` coverage.** The catalog's emitted wire names must also equal the `NodeKind`
   wire-tag list harvested from its own derive (via `Error::unknown_variant`'s `expected`
   argument — see ## Implementation Decisions). This is what catches a `NodeKind` variant added
   without a matching `WorkflowNodeType`, which checks 1-2 cannot see.
4. **Structural validity, across the document boundary.** Every generated workflow is serialized,
   read back with a strict `serde_json::from_value::<WorkflowV3>()`, then pushed through
   `ensure_defaults` and `validate_workflow`, and must produce zero `error`-severity issues. The
   round-trip matters: validating the constructed value in place would test the Rust struct, not
   the document a reader copies, and would miss an asymmetric serde change that makes the emitted
   JSON fail to load. So the documented examples are not merely well-shaped but pass the same
   gate a user's document meets at run start — saving never validates, so there is no save-time
   gate to pass. This is
   deliberately *not* a runnability claim: some constraints live only in the runtime, outside this
   seam — validation checks that `parallel_batch.bodyEntry` names an existing node but not that its
   kind is dispatchable (`src/model.rs:3070-3087` versus `src/runtime.rs:2086-2097`, `:5114-5127`),
   and subflow input sources are name-checked at save time but resolved at call time
   (`src/model.rs:2656-2691` versus `src/runtime.rs:3502-3513`). Examples are chosen to satisfy
   both, but only the first is machine-enforced.

**The example crosses the document boundary, and deliberately not via `normalize_workflow_value`.**
Check 4 does not validate the constructed value in place — that would test the Rust struct, not
the document a reader copies. It serializes the example, deserializes it back with a strict
`serde_json::from_value::<WorkflowV3>()`, and validates *that*, so an asymmetric serde change
that makes an emitted document fail to load is caught. Strict deserialization rather than the
production `normalize_workflow_value` (the path real ingress takes, `src/api.rs:563`), because
`migrate_v2_nodes_to_v3_kind` is called unconditionally
(`src/model.rs:1152`) and fires on any node with `type` and no `kind` whatever version the document
declares. A normalize-based check would rescue exactly the malformed shapes this epic exists to
eliminate.

**A CI job runs the full `cargo test` suite, deliberately and beyond what criterion 3 requires.**
Criterion 3 alone would be satisfied by a narrow freshness diff mirroring
`frontend-freshness.yml`'s `git diff --exit-code`. The broader job is chosen on its own merits:
this repo has 466 Rust tests and no CI has ever run them — `.github/workflows/` contains only
`canonical-v4-docs.yml` and `frontend-freshness.yml`, neither touching Rust — and this epic is the
first to add a Rust CI surface at all, making it the cheapest moment to close that gap. The
justification is repo hygiene, not the documentation criterion, and it is recorded as such rather
than smuggled in under criterion 3. The docs freshness check rides the same job. Bounds are in
`## Out of Scope`.

**Honest limits — what is not machine-checked.** All of Tier P is hand-written and stays
hand-written this epic. Its full inventory, stated plainly because understating it is how the
current doc failed:

- **The common `WorkflowNode` field table** — the fields every node carries whatever its kind,
  including `loopCondition`, `loopMaxIterations`, `retryCount`, `skipCondition`, `continueSessionFrom`
  and `splitFailurePolicy`. Today's document has no such table: it presents these as task-node fields
  and `splitFailurePolicy` as split-only, which is one of the drift audit's findings.
- **Per-node-kind config field tables** — one per kind that carries a config struct, each carrying
  field names, types, required-ness, and defaults; the bare unit variants (`approval`, `split`,
  `collector`) carry no config object and get a stated one-liner instead, and `task`'s only
  in-`kind` payload is its agent config, which the agent-config table owns. Together with the common table, the largest hand-maintained enumeration in the epic, and
  not generable: a serialized example shows *a* value and can never label it as *the* default (see the
  corresponding implementation decision).
- **The template-token list** — implemented in `resolve_template_vars` (`src/runtime.rs:7063-7153`)
  as inline string matching, with no enum to enumerate.
- **The validation catalog** — the 86 issues are assembled from string-producing branches scattered
  across `model.rs`.
- **The event vocabulary** — `RuntimeEvent.kind` is an unconstrained `String` (`src/runtime.rs:56`)
  with names as literals at emission sites including `:3558`, `:3841`, `:6452`, `:6640`.
- **The condition operator set** — defined only inside `evaluate_condition`
  (`src/model.rs:3182-3234`) as string comparisons; no enum exists to enumerate.
- **Edge, top-level, variable, subflow-catalog, `skipCondition` and agent-config tables.**

That is the complete inventory; the Tier P definition above and this list name the same set
deliberately, because understating it is how the current doc failed. Turning the event kinds or the
validation issues into enums would make two of these generable, and both are engine changes with
their own blast radius — out of scope. Tier C prose is not machine-checkable at all, and no attempt is made to fake it — a
mechanism paragraph cannot be asserted against `runtime.rs`. Both tiers rest on the drift audit's
citations and on review. This is the real cost of the split, and it is why enumerations are pushed
into Tier G wherever the source permits.

**The one drift class no check catches.** A field's requiredness is a serde attribute: adding or
removing `#[serde(default)]` changes what a document may omit without changing the Rust
construction API, the serialized example, the harvested variant set, the catalog, or any validation
result. Every check in this epic stays green through it, and the Tier P table recording it goes
stale silently. This is the sharpest limit of the design, and it is why success criterion 3 is
scoped to the node catalog rather than to the schema as a whole. Closing it would need the field
tables generated from the serde attributes themselves — the `schemars` path in ## Out of Scope.

**Prior art in the repo**, in preference order: `.githooks/pre-commit` plus
`scripts/test-pre-commit-frontend-freshness.sh` for a generated-artifact freshness check that has
its own regression test; the bundled-template pins at `src/model.rs:6074` and `:6194` for reading a
repo file from `CARGO_MANIFEST_DIR` inside the inline test module;
`scripts/check-canonical-v4-docs.sh` as the negative example — a blacklist that cannot verify, kept
running but never relied on.

## Out of Scope

**Bounds on the three additions this epic makes beyond the parent entry's wording.** The generator,
the freshness check, and the CI job are in scope; these limits are not:

- **The CI job is `cargo test` only.** No clippy, no `rustfmt` gate, no build matrix, no
  cross-platform runners, and no migration of the frontend suites (`npm test`, Playwright) into it.
  Whether it becomes a required status check is a repository-settings decision outside this PRD.
- **The generator emits the node-kind catalog and nothing else.** It does not generate prose, the
  execution-model document, field tables, or any block in a file other than
  `docs/workflow-schema.md`.
- **The freshness check gates the generated blocks only.** It is not a general documentation
  linter, and it does not run in a git hook this epic — the existing `.githooks/pre-commit` is not
  extended, so the developer-side obligation is "run the regeneration recipe", enforced in CI.

- **All engine behavior changes.** Including the dead orchestrator fallback and the silent
  first-branch-edge default, filed as ISSUE-260826-0004-01 for separate triage.
- **Refactoring event kinds or validation issues into enums** so they could be generated. Both
  would make Tier P generable and both are real engine changes with their own blast radius.
- **The v5 schema bump and every feature behind it** — typed contracts, the condition AST, the
  script node, profiles, `assignVars`. Owned by `typed-contracts` and its successors. This epic
  documents v4 as it stands; the generated catalog will absorb the v5 shapes when they land.
- **`schemars`-derived field tables.** Generating the Tier P config field tables from JSON Schema
  would remove the largest hand-maintained enumeration the epic keeps, and it is the natural
  successor to this work. It is excluded on scope rather than on dependency cost — the epic itself
  takes no new dependency at all — because it requires a JSON-Schema-to-markdown
  rendering layer, per-struct derive annotations across ~20 config types, and a decision about how
  faithfully schemars reproduces serde defaults and aliases. That is a second generator, not an
  extension of this one.
- **Renaming the guard script and its CI workflow.** `scripts/check-canonical-v4-docs.sh` and
  `.github/workflows/canonical-v4-docs.yml` carry "v4" in their names and patterns; re-pointing
  them belongs to the epic that performs the bump, which owns the repo-wide version sweep.
- **Every other document that under-reports node kinds.** `ARCHITECTURE.md` (9),
  `docs/backend.md` (10), `README.md` (4), `docs/architecture-overview.md` (1), `CLAUDE.md` (0).
  Excluded because they are not the roadmap's named deliverable and several will need editing again
  at the v5 bump — **not** because they are merely incomplete. `ARCHITECTURE.md:289-296` reads
  "Current first-class node types:" followed by four, which is as false as the two claims this
  epic does fix. It is left standing deliberately and its disposal is owned by ISSUE-260826-0240-01,
  filed so this exclusion points at a real record rather than at nothing.
- **`workflows/Test Workflow.json`**, a stray user artifact first committed on 2026-03-14 because the
  workflow store defaults to the process working directory (`src/app.rs:24`, `:58-60`). Real, and
  not a documentation problem.
- **A separate research artifact.** The parent entry's second coverage row, "Research artifact
  preserved in-repo", is satisfied by `docs/sources/workflow-schema-drift-260825.md` — this epic
  produces and commits it. No further artifact is owed. (`docs/sources/prior-art-workflow-engines.md`
  predates this epic and belongs to the roadmap itself.)
- **Extending the generator to the four unpinned bundled templates.** `dual-review.json`,
  `code-review-pipeline.json`, `research-and-summarize.json`, and
  `build-deploy-with-approval.json` have no schema pin. Worth doing; not this epic's deliverable.

## Open Questions

None.

## Dimension Scan

| Dimension | Verdict | Citation |
|---|---|---|
| problem/success | decided | ## Problem Statement |
| scope boundary | decided | ## Out of Scope |
| domain terms | decided | CONTEXT.md — 46 terms, 30 minted for this epic; the present-state-vs-target rule and its `_(planned — ADR-…)_` marker are stated in the file header and in ## Implementation Decisions |
| architecture shape | decided | brownfield — no engine changes and nothing under `src/` edited; the tier model, the marker-block grammar, and the generator's home (a new integration test, `tests/docs_catalog.rs`, in-memory by default, writing only under `SB_REGEN_DOCS=1`) in ## Implementation Decisions |
| stack | decided | brownfield ratified — **zero new dependencies of any kind**; enumeration reuses the library's existing `From<WorkflowNodeType> for NodeKind` (`src/model.rs:820-860`) plus the variant list harvested from the `Deserialize` derive `WorkflowNodeType` already carries (`serde`, `Cargo.toml:22`); serialization uses the existing `serde_json` (`Cargo.toml:23`) |
| data/schema | decided | ## Implementation Decisions — `src/model.rs` as source of truth, the required top-level keys and serde leniency, and the version/migration contract; the v5 bump is ADR-260815-2009-01, delivered by `typed-contracts` |
| contracts/integrations | decided | one doc-side correction, `docs/api-reference.md:24` aligned to `src/api.rs:216`; no contract changes |
| UX (if UI) | n/a | no UI surface — the deliverable is documentation, a generator, and a CI job |
| testing/seams | decided | ## Testing Decisions |
| NFRs (stakes-triggered) | n/a | `stakes: internal`; no perf, security, compliance, or a11y bar is owed by a documentation epic |
| ops envelope | n/a | local desktop/localhost app; no deploy, rollout, or monitoring surface (inherited from RDMP-260815-2009-01). The epic does add the repo's first Rust CI surface (two non-Rust workflows already exist), but CI is developer tooling decided under testing/seams, and the one repository-settings question it raises (required status check) is declined in ## Out of Scope |

## Further Notes

**Sequencing within the epic.** The glossary session gated drafting and has already run — both
documents are written in the vocabulary it settled, so no slice re-opens it. The schema reference's
skeleton comes first: the
generator rewrites only the span between `BEGIN GENERATED`/`END GENERATED` markers, and today's
`docs/workflow-schema.md` contains none, so the generator has no legal output span until the
marker-bearing scaffold exists. The generator follows the scaffold, since it fixes the shape of
every catalog block and drafting prose around a moving target wastes a pass. The
schema reference is then mostly assembly; the execution model is the large hand-written piece and
wants uninterrupted authorship. Slicing is `/to-issues`' call, but consistency of voice argues for
one author per document rather than parallel section writers.

**Why the CI job, given that the roadmap did not ask for one.** It is justified on repo hygiene,
not on the documentation criterion: 466 Rust tests exist and no CI has ever run them. A narrow
freshness diff would satisfy success criterion 3 by itself; the full suite is a deliberate,
separately-argued addition, bounded in `## Out of Scope`.

**Why generation, given that the roadmap only asked for a rewrite.** A hand-written v4 reference
has a shelf life of one epic: `typed-contracts` is next and it replaces `StructuredCondition`
wholesale, bumps the version, and adds four grammar features. A generated catalog absorbs the changed-field half of that
as a regeneration and fails the suite until the new-kind half is done; a hand-written one is rewritten by
hand twice in a month, with nothing forcing either. The generator
and the freshness check are additions the parent entry does not name, justified by success
criterion 3 rather than by the entry's wording; the CI job's broader scope is argued separately
above. Recorded here so the additions are deliberate rather than drift.

**Why `CONTEXT.md` no longer disagrees with the docs, recorded so the old contradiction is not
re-discovered.** The glossary was seeded as the initiative's v5 target state — a JSON-valued
variable store, a nested condition AST with `onMissing`, `assignVars`, profiles, the script node,
the failure outcome — none of which exist in `src/` today. The code sat between two documents that
were both wrong in opposite directions: the docs lagged it by two epochs, the glossary led it by
one initiative. The glossary session closed its half by making every unmarked definition true of
HEAD and flagging the rest, so this epic inherits one authority rather than two. Each later
schema-changing epic owns the terms its ADR names, per RDMP-260815-2009-01's `docs-truth` entry.

## Post-Epic Corrections

Added 2026-09-01 at epic close. The body above is preserved as written — it records the state of
the repo when this PRD was authored, and its Problem Statement is deliberately a "before" framing.
These are the passages a later reader would otherwise take as present tense and be misled by.

- **`## Out of Scope`, on other documents under-reporting node kinds.** The claim that
  `ARCHITECTURE.md` and `docs/backend.md` are "left standing deliberately" at nine and ten node
  kinds no longer holds. `ISSUE-260826-0240-01` — named in that same paragraph — brought both to
  all fourteen. The exclusion was overturned during the epic, not carried.
- **`## Implementation Decisions` and `## Out of Scope`, on the orchestrator branch fallback.** The
  statement that `ISSUE-260826-0004-01` "already carries" the unreachable fallback and the silent
  first-branch-edge default it causes is doubly superseded: that record deleted the fallback, and
  the silent default was separated into `ISSUE-260830-1925-02` and reassigned to the
  `typed-contracts` epic of RDMP-260815-2009-01. The line range cited alongside the claim now
  addresses different code.
- **`## Problem Statement` and `## Testing Decisions`, on the absence of CI.** Both state in the
  present tense that no CI workflow runs the Rust suite. `ISSUE-260826-0637-01` added one. The
  test count quoted beside that claim has also moved; derive the current figure by running the
  suite rather than reading it here.

Residual work this epic did not close is filed as `ISSUE-260901-0216-01` through `-06` and is
tracked on the roadmap's `docs-truth` entry, not here.
