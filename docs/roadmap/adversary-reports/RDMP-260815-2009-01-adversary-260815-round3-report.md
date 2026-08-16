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
- **impact:** The harness can pass the routing acceptance while actually running expensive or fallback models at nodes declared `cheap`, so the test proves profile labeling rather than the requested cheap-LLM execution.
- **evidence:** The roadmap calls success “measured cheap-model routing” (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:14`) but makes the oracle read resolved-profile records “against the profiles' declared tiers” and explicitly not the driver-reported `model_used` (`RDMP-260815-2009-01-harness-workflows.md:109`); ADR-04 makes `cheap | standard | high` a freely declared field beside the requested model (`docs/adr/260815-2009-profile-indirection-routing.md:9`). The actual execution result has an independent `model_used` field (`src/runtime.rs:115-143`), because the model a driver ultimately used can differ from requested configuration. What prevents a SoTA model from being labeled `cheap`, or a driver fallback from running one, while every stated acceptance assertion remains green?

#### Codebase-reality collision

- **class:** codebase-reality collision
- **impact:** The amended restart rule cannot reliably identify split-local executions, so restart can still discard the cursor state it claims to protect or reject legitimate root executions of a shared node.
- **evidence:** The roadmap and ADR-05 say restart is root-scope-only and that restarting “into an active split or subflow body is rejected by validation” (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:93`; `docs/adr/260815-2009-cursor-local-writes.md:9`). Subflows are structurally separate, but split bodies are not: `NodeKind::Split` is a unit variant and all branch nodes remain in the same flat `WorkflowV3.nodes` graph (`src/model.rs:693-721,939-968`). The restart request carries only `node_id`; today it accepts every ID in that root vector, clears split/call identity, and creates a new root cursor from `checkpoint.var_map` (`src/runtime.rs:1480-1539`). Since a node can also be reached in more than one cursor context, what repo-level fact can validation use to decide that a requested root node is “inside an active split” without a cursor/visit identity?

- **class:** codebase-reality collision
- **impact:** Model-running `decide` nodes can bypass variable-selected profiles and their audit records, leaving profile routing incomplete despite the artifact's node-wide wording.
- **evidence:** ADR-04 says “Nodes select a profile via a node-level `profile` field” and describes the work as extending `resolve_agent_config` (`docs/adr/260815-2009-profile-indirection-routing.md:9`); the roadmap likewise says nodes select profiles and the harness oracle reads resolved-profile execution records (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:101,109`). In the repo, `run_decide_node` is a separate LLM path that hard-codes `DEFAULT_AGENT`, takes a raw `DecideConfig.model`, constructs `AgentConfig` directly, and never calls `agent_name_for_node` or `resolve_agent_config` (`src/runtime.rs:3987-4060`; `src/model.rs:444-456`). Is `decide` intentionally excluded from profiles, and if not, which artifact assigns the separate routing, validation, and recording work its current execution seam requires?

- **class:** codebase-reality collision
- **impact:** The cursor-state slice has two incompatible output promises, so implementers must either change the established collector contract or fail the promised list aggregation.
- **evidence:** The coverage table promises “collector aggregation into parsed lists,” and the epic says both `parallel_batch.collectorVar` and collector-node `parsed_output` “become real JSON lists” while introducing no new aggregation surface (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:47,93`; ADR-05 at `docs/adr/260815-2009-cursor-local-writes.md:9`). Only `parallel_batch.collectorVar` is an ordered items list (`src/model.rs:461-481`; `src/runtime.rs:5295-5311`). A generic collector already has `Value`-typed `parsed_output`, but its stable shape is an object `{inputs: <merge-keyed map>, summary: ...}`, not a list (`src/runtime.rs:6009-6029`). Does “parsed lists” apply only to `parallel_batch`, or is changing the generic collector's existing object contract still hidden inside the no-new-surface claim?

#### Missed simpler alternative

- **class:** missed simpler alternative
- **impact:** The initiative adds a second durable profile authority, APIs/UI for it, precedence rules, snapshot injection, and dangling-reference behavior without any source success criterion that requires profiles outside a workflow.
- **evidence:** The source ask requires variable-selected CLI/model configuration, while the roadmap introduces both a workflow-local `profiles` map and an app-global `profiles.json`, then accepts that shared workflows can dangle (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:48,54,101`; `docs/adr/260815-2009-profile-indirection-routing.md:9,18`). The repo already persists complete workflow documents through `WorkflowStore`, including workflow-local configuration maps and embedded subflows (`src/model.rs:939-968`; `src/storage.rs:1045-1064`), so a workflow-local profile map alone can carry an allow-listed variable selector and make shipped templates self-contained. Which required harness outcome cannot be met by that existing persistence boundary and therefore justifies the app-global catalog surface?

#### Hidden coupling

- **class:** hidden coupling
- **impact:** A variable-selected profile can make a saved `continueSessionFrom` edge invalid only for particular runs, causing late pane-adoption failure or a privilege/agent mismatch that static validation currently prevents.
- **evidence:** ADR-04 permits profiles to change the agent and related agent configuration at bind time (`docs/adr/260815-2009-profile-indirection-routing.md:9`). Existing continuation validation assumes both are statically known: it requires source and destination to use the same agent, resolves both configurations, and compares access-profile privilege (`src/model.rs:2022-2099`); tmux reuse independently refuses a recorded agent mismatch or broader recorded access at execution (`src/tmux_exec.rs:492-525`). The profile epic names mutual exclusion with `agent` but no invariant across the allowed profile sets of two session-linked nodes. Are all selector combinations required to be continuation-compatible, or when does a selector-dependent incompatibility cease to be a save-valid workflow?

- **class:** hidden coupling
- **impact:** JSON accumulation can multiply across cursors and call frames and rewrite an unbounded SQLite checkpoint after every transition, turning the new mutable-state feature into a storage and resume failure mode.
- **evidence:** The typed-contract and cursor-state epics migrate `var_map` to `Value` and explicitly support accumulating lists (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:69,93`; ADR-01 at `docs/adr/260815-2009-typed-workflow-contracts.md:9`). In the repo the map is duplicated at the checkpoint root, on every `CursorState`, and in every `CallFrameState.parent_var_map` (`src/runtime.rs:185-223,560-607`), and the whole checkpoint is serialized into `runs.state_json` on updates (`src/storage.rs:311-348`). No epic assigns per-variable or per-run byte limits even though the initiative's own research says “Bound accumulation and say so” and calls for explicit caps that fail at write time (`docs/sources/prior-art-workflow-engines.md:176-180`). What bounds the multiplied checkpoint state before serialization, database growth, or resume deserialization becomes the first enforcement mechanism?

#### Sequencing errors

- **class:** sequencing errors
- **impact:** `typed-contracts` cannot finish its stated 4→5 normalization while remaining independent and green, because it is scheduled to emit a condition representation owned by its downstream blocker.
- **evidence:** `typed-contracts` says its 4→5 normalizer mechanically converts every flat condition to a single-leaf AST (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:69`; ADR-01 at `docs/adr/260815-2009-typed-workflow-contracts.md:9`), but `condition-ast` is explicitly blocked by `typed-contracts` and owns the AST model, evaluator, failure routing, and UI (`RDMP-260815-2009-01-harness-workflows.md:71-77`). Current v4 deserialization expects the flat `StructuredCondition {field, operator, value}` at both edge and loop sites (`src/model.rs:130-134,899,934`). Is the first epic expected to implement and execute the AST before the epic that owns it, or to write v5 documents that its own runtime cannot deserialize/evaluate?

#### Unjustified stack/dependency assumptions

- **class:** unjustified stack/dependency assumptions
- **impact:** Multiple incompatible document shapes will share `version: 5`, so intermediate v5 readers can reject or silently discard later-epic data and later readers cannot rely on the version to select a migration.
- **evidence:** The typed-contract epic creates v5 and says later epics extend it “additively,” while those later epics replace the flat condition object, add a new tagged `script` node variant, and add workflow profile fields (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:69,77,85,101`). The brownfield convention treats the current version as already canonical: normalization shape-migrates older versions, then directly deserializes the current version (`src/model.rs:1099-1171`). Unknown struct fields are ignorable, but an unknown internally tagged `NodeKind` variant is not, and the old and new condition objects are incompatible. Why is one version safe for the typed-contract-only shape and every later shape when the existing convention gives a reader no feature/version discriminator?

- **class:** unjustified stack/dependency assumptions
- **impact:** A valid JSON contract value can exceed process environment/argv limits and make a script fail before it starts, despite satisfying every declared SilverBond type and output cap.
- **evidence:** ADR-03 requires all dynamic script inputs to travel “exclusively through environment variables ... and argv” (`docs/adr/260815-2009-script-input-indirection.md:9`), while ADR-01 adds unrestricted `json` values to the variable store and caps only script outputs (`docs/adr/260815-2009-typed-workflow-contracts.md:9`; roadmap script epic at `docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:85`). The selected research explicitly records that env vars have per-value and aggregate `ARG_MAX` limits and that large parsed values need a file fallback (`docs/sources/prior-art-workflow-engines.md:226-232`). What declared input bound or transport rule prevents an otherwise valid JSON binding from crossing the host process-launch ceiling?
