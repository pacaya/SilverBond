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
- **impact:** The harness port can satisfy every stated acceptance check while retaining the harness logic in one or more large prompts, so it need not demonstrate the requested shift of logic into workflow structure and scripts or that each LLM step is simple.
- **evidence:** The problem says “the logic lives in graph structure and scripts” (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:14`), and the source ask additionally requires each step to be as simple as possible. The amended harness acceptance checks normalized records/commits, model tier, and concurrent runs, but contains no structural or behavioral check on prompt responsibility (`RDMP-260815-2009-01-harness-workflows.md:106`). The repo permits arbitrary prompt bodies on `Task`/`RunAgent` nodes (`src/model.rs:864-910`), so a monolithic prompt can produce the same records and commit while passing the model check. What evidence distinguishes the requested workflow/script port from a workflow that merely invokes the old harness discipline inside an LLM prompt?

- **class:** wrong problem
- **impact:** The cheap-model acceptance can pass despite unaccounted model calls, and “above the cheap tier” is not an executable assertion, so the initiative can claim the requested routing outcome without measuring it.
- **evidence:** The harness criterion is “measured from the execution log's `model_used`” and requires only named high-stakes nodes above “the cheap tier” (`RDMP-260815-2009-01-harness-workflows.md:106`). In the repo, `model_used` is only an opaque `Option<String>` (`src/runtime.rs:115-143`), and no model-tier taxonomy exists. More importantly, orchestrator refinement and branch calls invoke `run_tmux_oneshot` with the default agent and no config (`src/runtime.rs:7408-7474`) but are not appended to `ExecutionLog.node_executions`; only ordinary node completion appends `NodeExecutionLog` (`src/runtime.rs:4519-4540`). What defines the independent cheap/SoTA classification and accounts for non-node LLM invocations that the proposed oracle cannot see?

#### Codebase-reality collision

- **class:** codebase-reality collision
- **impact:** `enum` inputs cannot be normalized, wiring-validated, or rendered as the promised typed run-start control because the accepted contract has no source for the allowed values.
- **evidence:** ADR-01 defines contracts only as a type from `string | number | boolean | enum | json` plus `required` (`docs/adr/260815-2009-typed-workflow-contracts.md:9`), while the roadmap claims boundary parsing and a typed-input form (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:66`). The research says enum earns its place specifically as a picklist (`docs/sources/prior-art-workflow-engines.md:121,128,140`), but neither artifact declares an enum-values field; the current `WorkflowVariable` has only `name` and string `default` (`src/model.rs:113-119`). Where does an enum's independent allowed-value set live?

- **class:** codebase-reality collision
- **impact:** Script output normalization has no defined schema owner, so stdout and `SB_OUTPUT` values cannot be checked against the “node's declared output types” promised by the script slice.
- **evidence:** The script epic says outputs are normalized against “the node's declared output types” (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:82`), but ADR-01 introduces declarations only for workflows and subflows (`docs/adr/260815-2009-typed-workflow-contracts.md:9`). The current node model has only optional `output_schema: Value` (`src/model.rs:864-910`), while the same epic explicitly defers JSON-Schema validation for JSON contracts (`RDMP-260815-2009-01-harness-workflows.md:66`); no fixed-type node-output or named-`SB_OUTPUT` declaration is assigned to either epic. Which declared contract supplies the names, types, and requiredness used at the script boundary?

- **class:** codebase-reality collision
- **impact:** Existing skip rules have no mechanical meaning in the amended typed-leaf form, so migration can silently change pre-execution skipping or make saved v4 workflows unreadable.
- **evidence:** The amendment says `skipCondition` keeps its lifecycle and evaluation context but “adopts the same typed-operator leaves” (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:74`; ADR-02 at `docs/adr/260815-2009-single-condition-dialect.md:9`). Those leaves require `{field, op, value, onMissing}`, whereas current skip evaluation first selects one raw string from `previous_output` or a node's raw `output`, then applies `contains`/`not_contains`/`regex` to the whole string (`src/runtime.rs:2702-2732`); `SkipCondition` has `source`, `type`, and `value`, but no field (`src/model.rs:136-144`). What does a typed leaf's `field` address in that preserved raw-string context, and how do the existing source and regex semantics map without changing behavior?

- **class:** codebase-reality collision
- **impact:** Variable-selected profiles may alter some config fields but still fail to select the CLI, leaving a central source requirement unimplemented even though the roadmap declares the driver seam unchanged.
- **evidence:** ADR-04 says a profile bundles `agent` and is selectable by a variable, while the roadmap gives precedence “node → workflow `profiles` → app catalog → driver defaults” and says the driver seam is unchanged (`docs/adr/260815-2009-profile-indirection-routing.md:9`; `docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:26,98`). Today `agent_name_for_node` chooses the agent before `resolve_agent_config`, which receives that already-selected agent (`src/model.rs:60-94,298-310`), and validation requires an explicit agent on both task and run-agent nodes (`src/model.rs:1669-1677,1769-1782`). With node precedence, that mandatory node agent shadows the profile's `agent`; the artifacts also name no node/workflow field that associates a selector variable with a node. Which concrete selection seam lets a variable change CLI while preserving the stated precedence and save-time validation?

- **class:** codebase-reality collision
- **impact:** The cursor-state epic cannot implement its promised collector-to-scope list without inventing an unplanned collector schema, aggregation source, and destination.
- **evidence:** ADR-05 says the surviving scope is the pre-split map plus “the collector's declared aggregation,” and the roadmap promises collector aggregation into parsed JSON lists (`docs/adr/260815-2009-cursor-local-writes.md:9`; `docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:47,90`). In the actual model, `NodeKind::Collector` is a unit variant with no configuration (`src/model.rs:693-753`); its runtime output is a keyed object `{inputs, summary}`, not a declared list or variable binding (`src/runtime.rs:6009-6029`). What declares the collected binding(s), list element, ordering, and destination variable?

#### Missed simpler alternative

- **class:** missed simpler alternative
- **impact:** A new generic-collector aggregation contract adds model, editor, validation, and checkpoint surface that may duplicate data paths already sufficient once variables become JSON-valued.
- **evidence:** The cursor-state slice adds “collector aggregation of branch results into parsed JSON lists” and ADR-05 invokes a new “declared aggregation” (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:90`; `docs/adr/260815-2009-cursor-local-writes.md:9`). The repo already publishes generic collector aggregation as real `NodeResult.parsed_output` (`src/runtime.rs:6009-6029`), and `parallel_batch` already has a declared `collectorVar` that writes its ordered `items` list into the variable map (`src/model.rs:461-481`; `src/runtime.rs:5274-5311`). ADR-01's JSON-valued map removes that existing path's current stringification problem. What harness acceptance requires a second generic-collector aggregation surface rather than the already-declared parsed-output and batch-collector paths?

#### Hidden coupling

- **class:** hidden coupling
- **impact:** A profile selector passed to a callable workflow can be unavailable at root run start, making the “resolve once at run start” freeze rule incompatible with legal subflow input bindings.
- **evidence:** ADR-04 requires profile selectors to be resolved and frozen once at run start (`docs/adr/260815-2009-profile-indirection-routing.md:9`), while typed contracts make subflows callable with inputs (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:66`). In the runtime, a subflow's variable map is created only when the call node executes, and input bindings are evaluated then (`src/runtime.rs:3445-3540`); a binding may legally come from an earlier node's `parsedOutput` (`src/runtime.rs:4131-4171`). Thus a subflow profile variable need not have a value when the root run starts. Is profile selection restricted to root inputs, or when is a runtime-bound callee selector resolved without violating the once-at-start and frozen-snapshot invariants?

- **class:** hidden coupling
- **impact:** Restarting from a node can discard or choose the wrong cursor-local assignments, undermining the checkpointed mutable state that harness loops are meant to rely on.
- **evidence:** ADR-05 makes assignments cursor-local and checkpointed and rejects filesystem state partly because it “desyncs on restart-from-node” (`docs/adr/260815-2009-cursor-local-writes.md:9,14`); `CONTEXT.md:10` calls the checkpoint the authority for resume and restart. Yet `restart_from` accepts only a `node_id`, destroys the prior cursor set, creates one new root cursor, and seeds it from the checkpoint's root `var_map` (`src/runtime.rs:1480-1537`). The distinct cursor maps live on `CursorState` (`src/runtime.rs:201-234`), so after a split there may be several legitimate scopes and no cursor identity in the restart request. Which cursor scope is authoritative for restart, especially when the same node was visited by multiple branches or loop iterations?

- **class:** hidden coupling
- **impact:** The profile domain model gives implementers two incompatible workflow-local persistence locations, risking a UI/backend split or accidental reuse of the shape that cannot carry an agent.
- **evidence:** The amended roadmap and ADR explicitly introduce a workflow-local `profiles` map distinct from `agentDefaults` (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:98`; `docs/adr/260815-2009-profile-indirection-routing.md:9`), but `CONTEXT.md:20` still defines a Profile as coming from “workflow-local `agentDefaults`.” In the repo, `AgentDefaults` is keyed by agent name and has no `agent` member (`src/model.rs:255-277`), the exact reason the separate map was added. Which artifact is authoritative for the ubiquitous term that later epics and UI types will implement?

#### Sequencing errors

None.

#### Unjustified stack/dependency assumptions

- **class:** unjustified stack/dependency assumptions
- **impact:** Old and new documents share the same schema version despite incompatible node and condition shapes, so compatibility, persisted-run upgrades, and forward/backward failure behavior become shape guesses rather than versioned migrations.
- **evidence:** The roadmap calls the work a “schema v4 extension” and schedules multiple later schema-changing epics, including a new node kind and replacement condition AST, without a workflow-version transition (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:25,59,66,74,82`). The brownfield convention uses `WORKFLOW_SCHEMA_VERSION = 4` and an explicit 2/3→4 normalization path; current v4 skips legacy shape migration (`src/model.rs:10,1099-1171`), and the startup persisted-run upgrader selects only `workflow_json` whose version is not 4 (`src/storage.rs:1325-1371`). Why is changing incompatible tagged/recursive shapes in place under version 4 safe, and how can migration distinguish an old-v4 flat condition from the new-v4 AST while an old binary cannot recognize a new-v4 `script` variant?
