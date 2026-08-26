## Plan adversary report

- scale: epic
- source-decision: author-supplied
- artifacts:
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/roadmap/RDMP-260815-2009-01-harness-workflows.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/sources/workflow-schema-drift-260825.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/CONTEXT.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/issues/ISSUE-260826-0004-01-orchestrator-branch-fallback-unreachable.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/workflow-schema.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/docs/execution-model.md`
  - `/Users/Shared/Data/work/Programming/SilverBond/src/model.rs`
  - `/Users/Shared/Data/work/Programming/SilverBond/src/runtime.rs`
  - `/Users/Shared/Data/work/Programming/SilverBond/src/storage.rs`
  - `/Users/Shared/Data/work/Programming/SilverBond/src/api.rs`

### Findings

#### Wrong problem

- **class:** wrong problem
- **impact:** The epic changes two additional documents that the author-supplied ask and parent epic did not place under gate, broadening review and ownership beyond the requested materialization.
- **evidence:** The PRD says, “**Two adjacent falsehoods are corrected in place**” and adds edits to `docs/api-reference.md` and `docs/README.md` (`PRD-260826-0009-01-docs-from-rust-truth.md:255-259`). The parent roadmap defines `docs-truth` as “Rewrite `docs/workflow-schema.md` and `docs/execution-model.md`” and names no collateral document edits (`RDMP-260815-2009-01-harness-workflows.md:65-69`), matching the author-supplied ask-summary. Are those two collateral edits part of the authorized epic or adjacent cleanup?

#### Codebase-reality collision

- **class:** codebase-reality collision
- **impact:** Optional config-field drift can leave the proposed doc test green, so strict deserialization does not establish the claimed canonical-shape contract.
- **evidence:** The PRD claims, “Strict parse proves the shape is canonical v4” and that an example “cannot lie” (`PRD-260826-0009-01-docs-from-rust-truth.md:57-63,280-289`). The model deliberately accepts unknown keys: its serde structs have no `deny_unknown_fields`, a fact the PRD itself asks the docs to state (`:115-118,223-228`). For a concrete live seam, `SendConfig` is `#[serde(default, rename_all = "camelCase")]`, defaults `enter` to `true`, and does not deny unknown fields (`src/model.rs:504-520`); validation checks only that text or a prompt exists (`:1743-1756`). A strict `WorkflowV3` probe containing `sendConfig: {"text":"hello","entter":false}` deserialized and produced zero error-severity issues, silently ignoring the misspelled documented field. What makes strict parse a field-shape pin when the production serde contract is intentionally lenient?

- **class:** codebase-reality collision
- **impact:** The plan leaves its example unit ambiguous: a copyable node snippet cannot be passed to the stated `WorkflowV3` seam, while several full-workflow examples require supporting graph structure beyond the node being illustrated.
- **evidence:** The solution promises “each node kind's section leads with a JSON example” and the user story calls it a “node example” copied into a workflow document (`PRD-260826-0009-01-docs-from-rust-truth.md:57-63,82-84`), but the test says every such example is deserialized directly as `serde_json::from_value::<WorkflowV3>()` (`:280-294`). `WorkflowV3` requires top-level `version` and `entryNodeId` (`src/model.rs:937-968`), so a node object cannot cross that seam. Validation also makes realistic full examples kind-dependent: Task requires an agent (`:1670-1677`), Split needs outbound success edges (`:1944-1970`), Collector needs inbound branches plus exactly one outbound success edge (`:1973-2003`), Decide needs one matching branch edge per outcome (`:2917-3043`), and Subflow/Call needs a valid catalog entry and exit (`:2504-2707`). A direct probe confirmed that a one-node canonical Approval workflow and a Task workflow with an agent have zero errors, terminal warnings notwithstanding, while the same Task without an agent has one error. Which exact markdown artifact—the node fragment or an entire supporting workflow—is the promised test unit?

- **class:** codebase-reality collision
- **impact:** Adding a `NodeKind` variant can still leave the proposed documentation coverage test green, contrary to the PRD's central future-drift assertion.
- **evidence:** The PRD says every variant's wire name via `as_str` will be checked and “Adding a variant without documenting it fails the build” (`PRD-260826-0009-01-docs-from-rust-truth.md:291-297`). `as_str` does exist and returns all fourteen assumed wire names: `WorkflowNodeType::as_str` maps them at `src/model.rs:176-194`, and `NodeKind::as_str` delegates to it at `:755-777`. But neither enum exposes an exhaustive iterator or variant list. The existing test named `node_kind_round_trip_all_variants` manually constructs a `Vec<NodeKind>` with fourteen entries (`:4177-4286`); once ordinary exhaustive production matches are updated for a new variant, omission from that manual vector does not itself fail compilation. What existing public seam makes the prospective docs-name collection exhaustive rather than another hand-maintained list?

- **class:** codebase-reality collision
- **impact:** Treating the drift audit as the work order can replace a stale statement with a new false statement about the live capabilities contract.
- **evidence:** The preserved audit says, “**No `branchChoice` capability exists anywhere in `src/`**” (`docs/sources/workflow-schema-drift-260825.md:420`), and the PRD makes that audit the 154-finding work order (`PRD-260826-0009-01-docs-from-rust-truth.md:261-265`). In fact `AgentCapabilities` has `branch_choice` under `#[serde(rename_all = "camelCase")]` (`src/driver.rs:90-109`), registry capabilities copy it (`:111-130`), and the Claude fallback advertises it as `true` (`:722-745`); `/api/capabilities` serializes each driver's capabilities (`src/api.rs:199-216`). The separate claim that the orchestrator branch fallback is unreachable is correct (`src/runtime.rs:6527-6565`), but absence of runtime effect is not absence of the API capability. Which present-state truth is the rewrite meant to record for this advertised-but-unused flag?

- **class:** codebase-reality collision
- **impact:** The execution-model rewrite may invent a three-level lookup precedence that the runtime does not perform.
- **evidence:** The PRD twice requires “the three-level variable scope” (`PRD-260826-0009-01-docs-from-rust-truth.md:133-138,230-235`), following audit D29. The actual lookup helper has only two branches: use `checkpoint.var_map` when the cursor map and call stack are both empty, otherwise use `cursor.var_map` (`src/runtime.rs:2628-2637`). Subflow entry saves the parent map in a call frame and replaces `cursor.var_map` wholesale (`:3487-3542`); subflow exit restores that saved map (`:4691-4724`). It does not walk three maps, and nested call frames can store more than one latent parent map. What three simultaneously resolvable levels does the PRD intend the docs to name?

#### Missed simpler alternative

No findings.

#### Hidden coupling

- **class:** hidden coupling
- **impact:** Most content designated “machine-pinned” can change in the Rust authority without any planned assertion failing.
- **evidence:** Tier 1 includes “Node kinds and their config shapes, edge fields, the template-token list, the validation catalog, the event vocabulary” (`PRD-260826-0009-01-docs-from-rust-truth.md:210-216`), but the Testing Decisions specify only example fidelity and node-kind section coverage (`:291-304`). The omitted surfaces are not all model enums: `RuntimeEvent.kind` is an unconstrained `String` and event names are literals spread through `runtime.rs` (`src/runtime.rs:52-69`, including emission sites such as `:3558`, `:3841`, `:6452`, `:6640`); template forms are implemented in `resolve_template_vars` (`:7063-7153`); validation vocabulary is assembled from many string-producing branches in `model.rs`. How are those runtime- and validator-owned enumerations pinned by the one `model.rs` doc-example test the PRD specifies?

- **class:** hidden coupling
- **impact:** The promised “fails `cargo test` rather than shipping” guarantee is opt-in locally and is not enforced by the repository's shipping automation.
- **evidence:** The success statement requires a future schema change without docs to fail `cargo test` “rather than shipping” (`PRD-260826-0009-01-docs-from-rust-truth.md:47-49`), while the test decision explicitly adds “No new CI workflow” (`:273-278`). `just test-rust` is only a local recipe for `cargo test` (`justfile:47-49`), and the repository's only GitHub workflows are `.github/workflows/canonical-v4-docs.yml` (the acknowledged five-phrase blacklist) and `.github/workflows/frontend-freshness.yml`; neither runs Rust tests. What existing enforced path prevents the documented drift from shipping when `cargo test` is not run?

#### Sequencing errors

No findings.

#### Unjustified stack/dependency assumptions

No findings.
