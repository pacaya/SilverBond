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
- **impact:** The harness can pass its cheap/SoTA routing acceptance check while the CLI actually ran a fallback or different model.
- **evidence:** The roadmap calls success “measured cheap-model routing” (`RDMP-260815-2009-01-harness-workflows.md:14`), but harness-port makes the oracle the requested “resolved model ids” and checks the driver only “where present” (`:116`). In the repo, actual-model evidence is only the optional `AgentExecutionMetadata.model_used` (`src/runtime.rs:117-143`), and the interactive execution success path fills cost/token/session fields then defaults everything else (`src/tmux_exec.rs:1244-1265`). There is no assignment to `model_used` anywhere in `src/`; `AgentOutput.model_used` is merely a field declaration (`src/driver.rs:590-604`) and `AgentOutput` has no producer. A resolved profile therefore proves configuration resolution, not the model the CLI used. Does this acceptance criterion establish the stated routing outcome on either current driver?

#### Codebase-reality collision

- **class:** codebase-reality collision
- **impact:** Save/validate/run re-hydration can either overwrite an authoritative inline compound or retain a stale store-derived callee, depending on an authority distinction the model cannot represent.
- **evidence:** Typed-contracts says “embedded subflow copies are re-hydrated from the workflow store” at save/validate/run so callers are never stale (`RDMP-260815-2009-01-harness-workflows.md:76`; `docs/adr/260815-2009-typed-workflow-contracts.md:9`). The actual editor also creates authoritative inline definitions: `saveSelectionAsCompound` deep-clones the selected graph into `subflowDoc` and inserts it directly into the root `subflows` map (`ui/src/lib/stores/workflowStore.svelte.ts:812-926`) without saving a separate workflow. `WorkflowV3.subflows` is only `BTreeMap<String, Box<WorkflowV3>>` and carries no store-origin/provenance marker (`src/model.rs:939-968`). Correspondingly, current hydration deliberately treats every existing map key as loaded and consults `WorkflowStore` only for missing names (`src/api.rs:584-604`). When an inline catalog key also exists in `WorkflowStore`, which definition is authoritative, and how can the stated never-stale rule distinguish that case with the planned grammar?

#### Missed simpler alternative

none.

#### Hidden coupling

- **class:** hidden coupling
- **impact:** A continued node can be recorded as using its resolved destination profile while it actually inherits the source pane’s model, reasoning, tools, system prompt, and possibly narrower access.
- **evidence:** ADR-04 defines a profile as a coherent bundle of `agent`, `model`, `reasoningLevel`, `toolToggles`, and related config, but calls only “same resolved agent, source access privilege not broader, same effective working directory” the “full continuation invariant” (`docs/adr/260815-2009-profile-indirection-routing.md:9`; roadmap `:108`). In the repo, profile/session-shaping configuration is turned into CLI arguments only when a pane is spawned (`src/tmux_exec.rs:2257-2300`). On `continueSessionFrom`, `PaneGuard::acquire` adopts the already-running pane instead (`:415-458`); compatibility checks only agent, the one-way access inequality, and cwd (`:492-543`), and do not reconfigure the process. Thus two profiles can satisfy the recorded invariant while differing in every other session-shaping field. Why is that considered continuation-compatible when the destination profile cannot take effect on the reused session?

#### Sequencing errors

none.

#### Unjustified stack/dependency assumptions

none.
