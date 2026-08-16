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

#### Wrong problem

No findings.

#### Codebase-reality collision

No findings.

#### Missed simpler alternative

No findings.

#### Hidden coupling

- **class:** hidden coupling
- **impact:** A continuation can pass the new same-profile rule while its recorded destination model/tool configuration never takes effect on the reused process, leaving both routing behavior and the execution record incorrect.
- **evidence:** The round-7 amendment requires session-linked nodes to resolve to “the same profile,” but the same paragraph also says explicit node configuration overrides profile fields (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:108`; `docs/adr/260815-2009-profile-indirection-routing.md:9`). Those overrides cover spawn-time fields including model, system prompt, tool toggles, budgets, and tool allow/deny lists (`src/model.rs:255-295,301-353`), while `runAgentConfig.extraArgs` is another per-node spawn input. On continuation, `PaneGuard::acquire` adopts the existing pane instead of spawning from the destination configuration (`src/tmux_exec.rs:415-451`); its compatibility check compares only agent, access privilege, and cwd, even though it builds the destination command (`src/tmux_exec.rs:492-543`). Since `build_agent_command` turns the full configuration and extra args into the process argv only at spawn (`src/tmux_exec.rs:2257-2300`), what prevents two nodes with the same named profile but different node overrides from claiming different resolved configurations while running the source pane unchanged?

- **class:** hidden coupling
- **impact:** The high-stakes model-routing acceptance either requires an unplanned driver-integration change or risks populating `model_used` from requested configuration rather than independent actual-model evidence.
- **evidence:** The dimension scan ratifies “driver seam unchanged” (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:26`), but harness-port now promises to “wire actual-model capture into `model_used` where the CLI exposes it” and makes the cross-check mandatory for high-stakes nodes (`:116`). The current `AgentDriver` seam has hooks only for command construction and cost/context queries/parsers, not actual-model capture (`src/driver.rs:625-659`); interactive completion queries those two hooks and defaults the rest of `AgentExecutionMetadata`, including `model_used` (`src/tmux_exec.rs:1234-1265`). `AgentOutput.model_used` is only a field declaration and `AgentOutput` has no producer anywhere in `src/` (`src/driver.rs:588-605`). Which recorded architecture decision owns the driver-specific query/parsing needed to make this evidence independent while the roadmap simultaneously declares that seam unchanged?

- **class:** hidden coupling
- **impact:** The coordinated v5 bump can still leave the repository’s architectural contract declaring v4 canonical, so the claimed all-at-once version transition finishes with contradictory source-of-truth documentation.
- **evidence:** Typed-contracts says “All v4-pinned surfaces move together” and enumerates `CLAUDE.md`, frontend constructors, the storage predicate, the canonical-doc guard, README/docs index/API examples, and templates (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:76`). The same roadmap calls `ARCHITECTURE.md` ratified architecture and limits docs-truth plus later schema deltas to `docs/workflow-schema.md` and `docs/execution-model.md` (`:23,68`), yet `ARCHITECTURE.md:263-267` independently states, “The canonical workflow format is version `4`.” No epic names that live pin. Which epic owns this omitted architectural contract, and in what sense is the asserted v4 blast-radius list complete without it?

#### Sequencing errors

No findings.

#### Unjustified stack/dependency assumptions

No findings.
