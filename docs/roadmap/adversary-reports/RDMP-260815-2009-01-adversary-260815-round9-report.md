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

- **class:** wrong problem
- **impact:** The initiative can pass all stated harness-port acceptance checks with two independent or duplicated workflows, without proving the requested callable-block composition in which `issue-dev` invokes `implement-issue`.
- **evidence:** The ask-summary requires “issue-dev calling implement-issue,” but Coverage says only “`issue-dev` + `implement-issue` as workflows, outcome-validated” and harness-port says “Author `implement-issue` and `issue-dev` as shipped workflow templates” before listing record, routing, concurrency, and mechanical-step criteria; none requires a call edge between them (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:50,116`). This is not blocked by the engine: `NodeKind::Subflow | NodeKind::Call` dispatches to `handle_subflow_node`, which resolves the named catalog entry, binds inputs, and pushes a call frame (`src/runtime.rs:3229-3247,3445-3543`). Is the nested call an acceptance property, or may two non-composed ports satisfy the initiative?

#### codebase-reality collision

- **class:** codebase-reality collision
- **impact:** A checkpoint can satisfy the proposed quiescence predicate yet restart at a context-dependent node with missing scope, producing incorrect inputs or a collector wait that can never release.
- **evidence:** cursor-state defines restart validity solely as “no active split families and no call frames (a quiescent root state)” and then seeds from the root cursor map (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:100`; `docs/adr/260815-2009-cursor-local-writes.md:9`). The current restart API accepts any ID in the root workflow, creates a root cursor with no incoming edge or parent, and clears split families and collector barriers (`src/runtime.rs:1480-1540`). But a `parallel_batch` body receives its `itemVar` only when `spawn_batch_item_task` inserts it into a child map (`src/runtime.rs:5081-5128,5450-5525`), while a collector entered by such a fresh cursor fabricates the first inbound arrival and waits until every required input exists (`src/runtime.rs:5873-5970`). The `bodyEntry` is explicitly present in the flat root graph (`src/model.rs:463-470`). Is checkpoint quiescence intended to legalize restart targets whose required batch/split arrival context cannot be reconstructed?

- **class:** codebase-reality collision
- **impact:** A profile-selected continuation can pass the stated identity comparison while asking a reused pane to operate in a different working directory, causing a late runtime rejection or a dishonest effective-configuration record.
- **evidence:** ADR-04 calls for the “same effective session configuration” but enumerates model, system prompt, tool toggles, budgets, allow/deny lists, extra args, and access mode; it omits cwd, then says the existing agent/access/cwd checks remain the floor specifically “for non-profile continuations” (`docs/adr/260815-2009-profile-indirection-routing.md:9`). Cwd is spawn-time state in this repo: `runAgentConfig.cwd` overrides node/workflow cwd (`src/model.rs:97-109`), and pane reuse currently requires the recorded cwd to equal the requested effective cwd (`src/tmux_exec.rs:492-543`). Does the profile bind-time identity include effective cwd, despite the exhaustive-looking list and non-profile qualification saying otherwise?

- **class:** codebase-reality collision
- **impact:** A spawn node can select and record a profile while executing an unrelated raw command, so variable-driven CLI/model routing and its acceptance oracle can report a configuration that never ran.
- **evidence:** ADR-04 makes `profile` mutually exclusive with “*every* agent selector” but names only `agent`, `runAgentConfig.agent`, and `spawnConfig.agent` (`docs/adr/260815-2009-profile-indirection-routing.md:9`). A `SpawnConfig` also has a raw `command` alternative, and `spawn_pane` chooses `command_override`/`spawnConfig.command` before it ever calls `build_agent_command`; the profile-derived agent config is bypassed on that path (`src/model.rs:60-71`; `src/tmux_exec.rs:2177-2202`). Since ADR-04 expressly includes `spawnConfig.agent` in the profile collision set, is a profile-bearing command spawn meant to be valid even though the selected CLI/model cannot take effect?

#### missed simpler alternative

none.

#### hidden coupling

none.

#### sequencing errors

none.

#### unjustified stack/dependency assumptions

none.
