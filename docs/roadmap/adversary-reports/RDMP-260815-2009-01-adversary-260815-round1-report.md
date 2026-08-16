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
- **impact:** The initiative can declare the harness port successful while still leaving most reasoning inside expensive LLM nodes, failing to demonstrate cheap-model routing, and failing the requested parallel-per-value use case.
- **evidence:** The roadmap states that “most nodes run cheap models, and only high-stakes steps use SoTA models,” but its only harness acceptance is “same inputs produce the same commits and record transitions as the skills” (`RDMP-260815-2009-01-harness-workflows.md:14,104`). No acceptance criterion limits node complexity, identifies high-stakes steps, checks resolved models or cost, or runs the same workflow concurrently with distinct inputs. The repo already exposes the evidence needed for such checks—`NodeExecutionLog` records the agent and flattened execution metadata (`src/runtime.rs:472-496`), including `model_used` and cost/token fields (`src/runtime.rs:115-143`)—but the roadmap does not use it. Literal record equality is also not a stable oracle: current completed issue records contain run-specific `claimed_at`, Context Pack, and review timestamps (for example `docs/issues/done/ISSUE-260809-1545-01-tooltoggles-whole-object-replacement.md`). What independent outcome oracle is intended, and where are the simplicity, model-tier, cost, and concurrent-run success criteria?

#### Codebase-reality collision

- **class:** codebase-reality collision
- **impact:** “Typed outputs” cannot be implemented or wiring-validated from the described contract because the current callable seam has no named output declaration or caller output binding.
- **evidence:** The typed-contracts epic promises “declared typed inputs/outputs” (`RDMP-260815-2009-01-harness-workflows.md:38,65`), and ADR-01 says workflows and subflows declare both (`260815-2009-typed-workflow-contracts.md:9`). In the repo, `WorkflowV3` has only `variables`, and `SubflowConfig` has only `inputs` plus a single `exit_node_id` (`src/model.rs:630-639,939-968`). Runtime completion copies the selected exit node's entire `NodeResult` into the call result (`src/runtime.rs:4672-4715`) and restores the parent variable map without exporting callee variables (`src/runtime.rs:4717-4725`). What exact output declaration, production, and caller-binding semantics are being validated?

- **class:** codebase-reality collision
- **impact:** Missing fields and parse failures have no defined routable graph outcome, so the proposed `onMissing: route` can silently choose an ordinary branch or loop exit instead of the intended error route.
- **evidence:** ADR-02 gives each leaf `onMissing: treat-as-false | error | route` and says parse failure is all-fields-missing (`260815-2009-single-condition-dialect.md:9`); the roadmap calls `parse_error` “routable” (`RDMP-260815-2009-01-harness-workflows.md:43,73`). Yet `WorkflowEdgeOutcome` contains only `success`, `reject`, `branch`, `loop_continue`, and `loop_exit` (`src/model.rs:213-221`), and neither the leaf nor AST shape names a route. Today `evaluate_condition` can return only `(bool, Option<String>)` (`src/model.rs:3182-3235`); loop routing treats false/error as exit (`src/runtime.rs:6485-6523`), while branch routing discards the error and falls back to the first branch (`src/runtime.rs:6527-6539`). What graph edge and conflict semantics does `route` denote, especially when multiple leaves in a nested AST are missing?

- **class:** codebase-reality collision
- **impact:** Treating `skipCondition` as the same AST as post-execution conditions can change when and against what data a node is skipped.
- **evidence:** The condition epic says there will be “One dialect across edge conditions, `loopCondition`, and `skipCondition`” (`RDMP-260815-2009-01-harness-workflows.md:73`). In the actual model, `StructuredCondition {field, operator, value}` and `SkipCondition {source, type, value}` are different types (`src/model.rs:128-144`), and skip evaluation occurs before the node over raw `last_output` or another node's raw output (`src/runtime.rs:2704-2731`), whereas edge and loop conditions evaluate a just-completed node's `parsed_output`. Even the UI exposes separate “structured” and “skip” modes (`ui/src/features/editor/ConditionBuilder.svelte`). What evaluation context and lifecycle preserves the existing skip semantics in the claimed single dialect?

- **class:** codebase-reality collision
- **impact:** A nonzero script exit cannot be “routable” under the current graph and runtime; it terminates/fails the cursor before ordinary edge selection.
- **evidence:** The script epic requires “exit-code-only success producing a routable failure outcome,” and ADR-03 says nonzero is “never a run abort by itself” (`RDMP-260815-2009-01-harness-workflows.md:80`; `260815-2009-script-input-indirection.md:9`). There is no failure edge variant in `WorkflowEdgeOutcome` (`src/model.rs:213-221`). More importantly, common result handling sends any unsuccessful node directly to `handle_terminal_cursor_status` and returns; `select_next_decision` is reached only for success (`src/runtime.rs:4555-4592,4621-4631`). Which model/runtime outcome is the script failure supposed to route through?

- **class:** codebase-reality collision
- **impact:** The proposed non-pane script can execute as the SilverBond app user even when the security configuration intends all process workloads to run as a downgraded agent user.
- **evidence:** `CONTEXT.md` defines Script Node as a “synchronous, non-pane node,” while ADR-03 says it runs “behind the existing process-launch privileged gate” (`CONTEXT.md:19`; `260815-2009-script-input-indirection.md:9`). The existing gate authorizes a run and may populate `workflow.run_as` (`src/api.rs:778-814`), but the actual identity prefix is applied by `build_tmux_invocation` at the tmux boundary (`src/tmux_exec.rs:80-95`); the current runner executes only tmux node variants (`src/tmux_exec.rs:292-370`). A direct `sh` child has no existing run-as-aware executor, and `node_launches_process` does not yet include such a node (`src/api.rs:866-881`). How does a non-pane script inherit both the authorization decision and the enforced execution identity rather than merely passing the UI gate?

- **class:** codebase-reality collision
- **impact:** Workflow-local named profiles cannot select a CLI through the existing `agentDefaults` shape or merge chain as claimed.
- **evidence:** ADR-04 says a profile bundles `agent + model + reasoningLevel + toolToggles`, lives both in the app catalog and workflow-local `agentDefaults`, and extends `resolve_agent_config` (`260815-2009-profile-indirection-routing.md:9`). In the repo, `AgentDefaults` explicitly is “keyed by agent name” and has no `agent` member (`src/model.rs:255-277`); `agent_name_for_node` chooses the CLI from the node before resolution (`src/model.rs:60-94`); and `resolve_agent_config` then indexes `workflow_defaults` by that already-selected agent (`src/model.rs:298-319`). A profile name and an agent name are therefore different keys with no represented relationship. What is the workflow-local profile schema and how is precedence defined when the selected profile's agent conflicts with a node's agent?

- **class:** codebase-reality collision
- **impact:** Cursor-local assignments can leak nondeterministically past a collector from whichever branch becomes the representative, contradicting the claim that branch writes die and cross only via aggregate data.
- **evidence:** ADR-05 says “a parallel branch's writes live and die with its cursor” and only collector aggregation crosses branches (`260815-2009-cursor-local-writes.md:9`). The runtime's collector state explicitly retains the first terminal arrival's cursor snapshot and warns that its variable snapshot may be stale (`src/runtime.rs:445-449`). On release it removes the other waiting cursors and continues with `representative_state.var_map` (`src/runtime.rs:6091-6098,6172-6206`). Once `assignVars` exists, one branch's local writes survive solely because that branch supplied the representative. How does the proposed invariant hold against this existing survivor-selection behavior?

#### Missed simpler alternative

- **class:** missed simpler alternative
- **impact:** Migrating every checkpoint, cursor, call frame, binding path, preview context, and frontend variable type to JSON may consume most of the initiative before proving either harness workflow, despite the repo already having a typed data path.
- **evidence:** ADR-01 rejects the strings-only store because collector/batch aggregation would otherwise become escaped JSON strings (`260815-2009-typed-workflow-contracts.md:13`). In the current repo, `NodeResult.parsed_output` is already `Option<Value>`, batch checkpoint results are `Value`, and collectors publish `parsed_output` as real JSON (`src/runtime.rs:98-104,560-572,6009-6079`); template consumers can address `{{node:ID.parsedOutput.path}}` without reparsing (`src/runtime.rs:7096-7116`). The string map already supplies per-run overrides and subflow inputs (`src/api.rs:724-734`; `src/runtime.rs:3502-3515`). Why is a global `var_map -> Value` migration required for the ask, rather than using the existing parsed-output path for structured aggregates while applying explicit typing at the declared input/condition seams?

#### Hidden coupling

- **class:** hidden coupling
- **impact:** Save-time caller/callee type validation can become stale immediately after a callee changes, allowing a parent to validate and run against an old embedded contract indefinitely.
- **evidence:** The typed-contracts epic promises save-time subflow wiring validation (`RDMP-260815-2009-01-harness-workflows.md:39,65`), and the supplied research explicitly warns to invalidate a caller's cached schema on callee change (`docs/sources/prior-art-workflow-engines.md`, §3 lesson 1). Current workflow documents store subflows by value (`src/model.rs:962-966`), `WorkflowStore::save` writes that whole normalized document (`src/storage.rs:1045-1064`), and `hydrate_saved_subflows` treats every already-embedded name as loaded and never refreshes it (`src/api.rs:584-603`). The save route currently neither hydrates nor validates (`src/api.rs:535-542`). What dependency/invalidation rule keeps parent contracts aligned with subsequently edited callees?

- **class:** hidden coupling
- **impact:** The two “shipped workflow templates” may not compose at runtime: an `issue-dev` template that names the separately shipped `implement-issue` template will not be hydrated as a callable subflow.
- **evidence:** The harness epic says to author `implement-issue` and `issue-dev` “as shipped workflow templates,” with `issue-dev` calling `implement-issue` implied by the source ask (`RDMP-260815-2009-01-harness-workflows.md:104`). The repo separates `TemplateStore` from `WorkflowStore` (`src/app.rs:173-177`); `/api/templates` only lists templates (`src/api.rs:642-644`), while subflow hydration consults only `state.workflows.get` (`src/api.rs:584-603`). `TemplateStore` itself only exposes listing, not name lookup (`src/storage.rs:1075-1145`). Does “calling implement-issue” mean an embedded snapshot or a separately addressable shipped template, and which epic owns that currently absent resolution contract?

- **class:** hidden coupling
- **impact:** Resumed or restarted runs can resolve a different profile after the app-global catalog changes, violating per-execution reproducibility and potentially changing CLI privileges/model cost mid-run.
- **evidence:** ADR-04 says profiles are app-global, resolved at run start, and the resolved profile/model is recorded in the execution log (`260815-2009-profile-indirection-routing.md:9`). Existing persistence freezes only the workflow and `RuntimeCheckpoint`; the checkpoint has no resolved-config catalog (`src/runtime.rs:560-605`), and `RuntimeContext::start_run` accepts only workflow plus string overrides (`src/runtime.rs:1394-1440`). `NodeExecutionLog` records agent/output metadata but not a resolved profile bundle (`src/runtime.rs:472-496`). The runtime is constructed with a `Database`, not `AppState`, so resume has no catalog snapshot unless a new coupling is introduced. Where is the run-start resolution frozen for later nodes, crash resume, and restart-from-node?

#### Sequencing errors

- **class:** sequencing errors
- **impact:** The documentation epic can finish green and become false as soon as the next schema epic lands, leaving the initiative's final docs without the new node and contract forms.
- **evidence:** `docs-truth` is first and explicitly rewrites docs for “all 14 node kinds, v4 shapes” with “No engine code changes” (`RDMP-260815-2009-01-harness-workflows.md:54-58`). Later epics change the v4 workflow/variable/condition shapes and add a fifteenth `script` kind (`ibid.:60-96`), but no later epic owns updating those two truth documents. Why is the truth rewrite sequenced before the changes it must ultimately describe?

- **class:** sequencing errors
- **impact:** Implementing the script epic before typed-contracts creates a temporary string IO contract that must be redesigned when variable bindings and checkpoints become `Value`-typed.
- **evidence:** `condition-ast`, `run-state`, and `profile-catalog` explicitly declare `Blocked by: typed-contracts`, but `script-node` does not (`RDMP-260815-2009-01-harness-workflows.md:67-96`). Script inputs are variables/bindings sent through env and argv and script JSON feeds `parsedOutput` (`260815-2009-script-input-indirection.md:9`), while ADR-01 changes binding resolution and condition/checkpoint values to `Value` (`260815-2009-typed-workflow-contracts.md:18`). Since roadmap order alone is not a dependency and initiative epics may be taken independently, why is script-node not sequenced against the value/stringification contract it consumes?

- **class:** sequencing errors
- **impact:** The initiative cannot demonstrate the explicit “same workflow in parallel with different values” behavior through its only UI until after harness acceptance, and the earlier run-start form is built against state known to be replaced.
- **evidence:** `multi-run-ui` is “Deliberately last” and called “not needed until overlapping harness runs are real” (`RDMP-260815-2009-01-harness-workflows.md:106-110`), even though parallel executions with distinct per-run values are part of the source ask and typed-contracts earlier builds the run-start input UI (`ibid.:41,65`). Today `AppShell` derives overrides from stored defaults, owns one run stream, and stores one `runId` (`ui/src/app/AppShell.svelte:30-33,195-245`; `ui/src/lib/stores/workflowStore.svelte.ts:1011-1049`); `RunPanel` disables Run whenever that singleton is running (`ui/src/features/runtime/RunPanel.svelte:63-66`). Why does the user-visible concurrency criterion come after the port that is supposed to prove the initiative, and what prevents the typed run-start UI from being redone during the singleton-to-multi-run change?

#### Unjustified stack/dependency assumptions

- **class:** unjustified stack/dependency assumptions
- **impact:** The profile epic silently introduces a new durable configuration service without a decided persistence convention, lifecycle, migration path, or authority boundary.
- **evidence:** The dimension scan declares “stores unchanged (workflows as JSON files, runs in SQLite)” (`RDMP-260815-2009-01-harness-workflows.md:25`), while the profile epic simultaneously requires an “App-global profile store (backend-persisted, UI-edited)” (`ibid.:47,96`). The brownfield app has paths only for workflow files, template files, and the SQLite database (`src/app.rs:22-35`); `AppState` has only `WorkflowStore`, `TemplateStore`, runtime/database access, pane streams, and security (`src/app.rs:172-180`); SQLite initialization creates run/event/session/log tables but no general settings store (`src/storage.rs:173-220`). Which existing store is actually unchanged, and what repo convention justifies the new app-global service's persistence and API semantics?
