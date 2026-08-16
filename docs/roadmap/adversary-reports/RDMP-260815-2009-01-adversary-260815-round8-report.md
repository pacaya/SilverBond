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

- **class:** wrong problem
- **impact:** The harness can pass its structural acceptance check while assigning mechanical branching to an LLM, so it need not demonstrate the requested non-LLM branching or keep mechanical logic out of model calls.
- **evidence:** The ask-summary requires “non-LLM branching,” and the roadmap says every mechanical step may be “a script node or a deterministic conditioned edge/decide route” (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:116`). In the actual engine, a `decide` route is not deterministic: `run_decide_node` emits `agent: "llm"`, constructs the default agent/model config, and invokes `run_tmux_oneshot` (`src/runtime.rs:3987-4055`); the resulting `DecisionLog` explicitly records `deterministic: false` (`src/runtime.rs:6385-6414`). How can an LLM-powered `decide` route satisfy the mechanical/non-LLM side of criterion (d)?

#### Codebase-reality collision

- **class:** codebase-reality collision
- **impact:** A profile-selected `run_agent` or `spawn` node can retain a second effective CLI selector, allowing the nested agent to shadow the profile and making variable-driven CLI routing and its execution record ambiguous.
- **evidence:** The profile epic and ADR make the new node-level `profile` field mutually exclusive only with node-level `agent` (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:108`; `docs/adr/260815-2009-profile-indirection-routing.md:9`). The repo has additional serialized agent selectors: `explicit_agent_for_node` prefers `runAgentConfig.agent` and `spawnConfig.agent` over `WorkflowNode.agent`, and `agent_name_for_node` uses that result as the actual CLI (`src/model.rs:57-94`; the nested fields are in `src/model.rs:477-499,585-630`). Does the recorded mutual-exclusion decision cover these higher-precedence nested selectors, and if not, which CLI is the profile resolution supposed to record and launch?

#### Missed simpler alternative

No findings.

#### Hidden coupling

- **class:** hidden coupling
- **impact:** A continuation can still claim a broader destination access configuration that never takes effect because the reused pane remains narrower, contradicting the new “same effective session configuration” and leaving the execution record inaccurate.
- **evidence:** ADR-04 now requires the “same effective session configuration,” but its identity list omits access and then retains the existing one-way access check as the floor (`docs/adr/260815-2009-profile-indirection-routing.md:9`). Access privilege is ordered `ReadOnly < WorkspaceWrite < FullAccess` (`src/driver.rs:269-282`), while static validation rejects only `source_privilege > current_privilege` (`src/model.rs:2072-2090`) and pane adoption likewise accepts `recorded_privilege <= expected_privilege` (`src/tmux_exec.rs:509-525`). Thus a read-only source pane can be continued by a destination resolved as workspace-write/full-access even though configuration becomes argv only at spawn (`src/tmux_exec.rs:2257-2281`) and the adopted pane cannot gain that access. In what sense are those two nodes continuation-compatible under the amended identity invariant?

- **class:** hidden coupling
- **impact:** The typed-contracts epic can complete its claimed all-at-once v5 transition while a live stored workflow and user-facing API errors still declare v4 canonical.
- **evidence:** The roadmap says “All v4-pinned surfaces move together” and now enumerates `ARCHITECTURE.md`, README/docs references, API examples, templates, frontend constructors, and the storage/CI pins (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:76`). The enumeration still omits the tracked active-store definition `workflows/Test Workflow.json`, which is stamped `"version": 4` (`workflows/Test Workflow.json:1-3`); that directory is the actual `WorkflowStore` mounted at boot (`src/app.rs:29-38,347-363`). It also omits runtime node-preview errors that say “canonical v4” in three user-visible paths (`src/api.rs:2615-2640`). Which epic owns these remaining production pins, and how is the asserted blast-radius list complete without them?

#### Sequencing errors

No findings.

#### Unjustified stack/dependency assumptions

No findings.
