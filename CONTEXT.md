# Context

Ubiquitous language for SilverBond — the workflow engine that runs LLM CLIs and scripted steps as node/edge graphs. Single context; seeded by RDMP-260815-2009-01, grows with the work.

## Language

**Workflow** [aggregate] — a versioned node/edge graph definition (schema `version: 5` per ADR-260815-2009-01; v2–v4 normalize forward), including its subflow catalog. _Avoid_: pipeline, flow. ↔ src/model.rs (`WorkflowV3`)
**Run** [aggregate] — one execution of a workflow, with its own frozen workflow snapshot, variables, and checkpoint; many runs of one workflow may exist concurrently. _Avoid_: job, instance. ↔ src/runtime.rs (`PersistedRun`)
**Cursor** [entity] — an independent execution pointer inside a run, carrying its own variable scope and call stack; splits create cursors, collectors join them. ↔ src/runtime.rs (`CursorState`)
**Checkpoint** [VO] — the serialized full state of a run persisted after every significant transition; the sole authority for resume and restart. ↔ src/runtime.rs (`RuntimeCheckpoint`)
**Subflow** [entity] — a workflow invoked as a callable block from another workflow via a call frame. _Avoid_: sub-workflow, child workflow. ↔ src/runtime.rs (`handle_subflow_node`)
**Workflow Contract** [VO] — a workflow's declared, typed inputs and outputs (`string | number | boolean | enum | json`, `required` flags); the callee owns it, callers are validated against it. _Avoid_: signature, interface. Decisions: ADR-260815-2009-01
**Variable** [VO] — a named, JSON-valued piece of per-execution state scoped to a cursor; distinct from write-once node outputs, which are the default data path. ↔ src/model.rs (`WorkflowVariable`), src/runtime.rs (`var_map`). Decisions: ADR-260815-2009-01
**Boundary Normalization** [service] — parsing and validating externally-supplied values (run-start overrides, subflow inputs, script outputs) against declared types exactly once, at entry. Decisions: ADR-260815-2009-01
**Assignment** [VO] — a binding-evaluated, cursor-local mid-run variable write (`assignVars`); never an arbitrary expression. Decisions: ADR-260815-2009-05
**Collector** [service] — the barrier node where parallel cursors join and branch results aggregate into parsed JSON values in the surviving scope. ↔ src/runtime.rs (`handle_collector_entry`). Decisions: ADR-260815-2009-05
**Condition** [VO] — the engine's single post-execution deterministic branching form (edge and loop conditions): a nested `all`/`any`/`not` AST over typed-operator leaves; the pre-execution `skipCondition` is a separate, untouched form. _Avoid_: expression, rule. ↔ src/model.rs (`StructuredCondition`). Decisions: ADR-260815-2009-02
**onMissing** [VO] — a condition leaf's declared policy (`treat-as-false | error | route`) for a field the upstream output never emitted; parse failure evaluates as all-fields-missing. Decisions: ADR-260815-2009-02
**Script Node** [entity] — a synchronous, non-pane node running static interpreter source (`sh` first) with env-var inputs, stdout+parse output, and exit-code-only success. Decisions: ADR-260815-2009-03
**Profile** [entity] — a named, allow-listed bundle of agent + model + reasoning + tool config with a declared routing tier, from the app catalog (`profiles.json`) or the workflow-local `profiles` map (distinct from `agentDefaults`), selected per node via a `profile` field and per run by variable. _Avoid_: model string, preset. Decisions: ADR-260815-2009-04
**Failure Outcome** [VO] — the edge outcome taken when a node fails or a condition routes on missing data; taken only where a failure edge is declared, terminal otherwise. Decisions: ADR-260815-2009-02
**Agents Registry** [service] — the tmux-tools machine-level record of which CLIs are installed; profiles reference it, never replace it. ↔ src/driver.rs (registry loading)
