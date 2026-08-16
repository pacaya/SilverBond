## Plan adversary report

- scale: initiative
- source-decision: author-supplied
- artifacts:
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/roadmap/RDMP-260815-2009-01-harness-workflows.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/adr/260815-2009-typed-workflow-contracts.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/adr/260815-2009-single-condition-dialect.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/adr/260815-2009-script-input-indirection.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/adr/260815-2009-profile-indirection-routing.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/adr/260815-2009-cursor-local-writes.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/CONTEXT.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/sources/prior-art-workflow-engines.md`

### Findings

#### wrong problem

No findings.

#### codebase-reality collision

- **class:** codebase-reality collision
- **impact:** A restart that satisfies the amended root/quiescent checks can still run an ordinary root node with fabricated predecessor or branch values, changing its prompt, bindings, or route.
- **evidence:** ADR-05 says restart is allowed only at a target requiring no arrival context, but identifies only `parallel_batch` body nodes and collector nodes as invalid targets (`docs/adr/260815-2009-cursor-local-writes.md:9`; the roadmap summary at `docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:100` does not add other exclusions). Ordinary root nodes can also require arrival context: the runtime cursor carries `incoming_edge_id`, `incoming_node_id`, `last_output`, `last_branch_origin_id`, and `last_branch_choice` (`src/runtime.rs:203-229`), and template/binding evaluation consumes `previous_output`, `branch_origin`, and `branch_choice` (`src/runtime.rs:4135-4150,7125-7133`). The current restart constructor clears all incoming/branch fields and derives `last_output` from the last entry of the retained `BTreeMap` of results, not from the edge by which the target was reached (`src/runtime.rs:1520-1541,1555-1563`). Are root nodes whose prompts or input bindings use those arrival-scoped values also invalid restart targets, or is their original arrival context intended to be reconstructed under a rule absent from the recorded decision?

#### missed simpler alternative

No findings.

#### hidden coupling

- **class:** hidden coupling
- **impact:** The grammar-owning epic can land one v5 selector location while the later routing epic implements another, forcing a second v5 shape change or leaving `decide` with two ambiguous profile selectors.
- **evidence:** `typed-contracts` promises to land the complete v5 grammar once and explicitly names “the node-level `profile` field” (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:76`); the profile epic likewise first says nodes select through a `profile` field, but then specifies an optional `profile` on `DecideConfig` (`:108`). ADR-04 repeats both claims in the same decision (`docs/adr/260815-2009-profile-indirection-routing.md:9`). These are distinct serialized seams in the actual model: `DecideConfig` is nested inside the `NodeKind::Decide` variant (`src/model.rs:441-452,709`), while common node-level fields live on `WorkflowNode` (`src/model.rs:862-910`). Which location is the canonical `decide` selector, and if both are intentional, what prevents two values from competing when `typed-contracts` must freeze the grammar before `profile-catalog` implements resolution?

#### sequencing errors

No findings.

#### unjustified stack/dependency assumptions

No findings.
