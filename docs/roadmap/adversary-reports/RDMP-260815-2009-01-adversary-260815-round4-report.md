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

- **class:** codebase-reality collision
- **impact:** Existing v4 conditions can change truth value or become invalid during the promised mechanical 4→5 migration.
- **evidence:** ADR-01 says “the flat condition triple normalizes to a single-leaf AST mechanically,” while ADR-02 requires explicitly typed operators such as `eq_string` and `eq_number` that “never” infer (`docs/adr/260815-2009-typed-workflow-contracts.md:9`; `docs/adr/260815-2009-single-condition-dialect.md:9`). A v4 `StructuredCondition` contains only string fields `{field, operator, value}` (`src/model.rs:128-134`), and its evaluator deliberately stringifies every JSON field before `==`, `!=`, `contains`, and `matches`; only relational operators attempt numeric parsing (`src/model.rs:3182-3235`). Thus, for example, both a JSON number `1` and string `"1"` match the same v4 `== "1"` condition, but the document carries no fact from which a non-inferring normalizer can choose `eq_number` versus `eq_string`. What typed v5 leaf preserves each existing v4 condition's behavior without either inference or a legacy coercing operator?

- **class:** codebase-reality collision
- **impact:** The declared workflow outputs cannot be produced, normalized, or wired by callers from the contract described, so callable blocks may expose only the old untyped exit-node result.
- **evidence:** The typed-contracts epic and ADR-01 promise declared typed “inputs/outputs” and save-time subflow wiring validation, but define only the type set and `required` flag (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:73`; `docs/adr/260815-2009-typed-workflow-contracts.md:9`). In the repo, `SubflowConfig` has input bindings and an `exitNodeId`, but no output bindings (`src/model.rs:630-638`); completion copies the selected exit node's whole `NodeResult` into the call result and then restores `parent_var_map`, discarding the callee variable scope (`src/runtime.rs:4672-4725`). What document field maps each declared workflow output to a callee value, and what caller-facing binding consumes those named outputs?

- **class:** codebase-reality collision
- **impact:** A profile-selected continuation can pass the stated same-agent check yet still be rejected at pane adoption, or attempt to continue a session under incompatible privilege or working-directory assumptions.
- **evidence:** The profile epic and ADR-04 define continuation compatibility only as “same resolved agent” at bind time (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:105`; `docs/adr/260815-2009-profile-indirection-routing.md:9`). Current continuation validation also resolves both agent configs and rejects a source pane whose access privilege is broader than the destination, and independently rejects different effective working directories (`src/model.rs:2054-2120`); `tmux_exec.rs:492-540` repeats those access-profile and cwd checks at pane reuse. Since a profile carries “related agent config” and node fields override it, what full bind-time compatibility invariant covers access privilege and cwd as well as agent identity?

#### Missed simpler alternative

No findings.

#### Hidden coupling

- **class:** hidden coupling
- **impact:** Restarting a quiescent sequential run can silently lose every cursor-local assignment, including after a normally completed run.
- **evidence:** ADR-05 and the cursor-state epic treat “no active split families and no call frames” as a sufficient quiescent root state for restart (`docs/adr/260815-2009-cursor-local-writes.md:9`; `docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:97`). The checkpoint actually has two distinct variable locations: `RuntimeCheckpoint.var_map` and each `CursorState.var_map` (`src/runtime.rs:201-225,558-607`). Current restart always seeds the new cursor from the checkpoint-level map (`src/runtime.rs:1480-1537`), while terminal completion removes the cursor outright (`src/runtime.rs:4900-4915`). Under the new invariant that `assignVars` writes only to the cursor, neither absence of split families nor an empty call stack makes the checkpoint-level map current, and a completed checkpoint has no cursor left to consult. What state invariant preserves the latest root cursor assignments for restart at both live-quiescent and terminal checkpoints?

- **class:** hidden coupling
- **impact:** The v5 bump can leave user-facing documentation and shipped artifacts claiming v4 is canonical despite the epic's assertion that the entire version blast radius moves together.
- **evidence:** The typed-contracts epic says “All v4-pinned surfaces move together” but enumerates only `CLAUDE.md`, two frontend sites, the storage predicate, and the canonical-doc guard; docs-truth assigns later schema deltas only for `docs/workflow-schema.md` and `docs/execution-model.md` (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:66,73`). Additional live pins include the root constraint “workflows must be canonical `version: 4`” (`README.md:105-108`), the docs index's “Complete v4 workflow format reference” (`docs/README.md:7-12`), v4 request examples in the HTTP contract (`docs/api-reference.md:80-87,111-116`), and bundled templates stamped v4 (for example `templates/epic-dev.json:1-3`). Which epic owns these omitted production/documentation pins, and what makes the claimed blast-radius enumeration complete?

#### Sequencing errors

- **class:** sequencing errors
- **impact:** Later epics still mutate the serialized v5 shape, recreating multiple incompatible “version 5” grammars and defeating the amendment's version-discriminator guarantee.
- **evidence:** `typed-contracts` claims to land “the complete v5 document grammar once” and says later epics add only semantics behind gates (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:73`; ADR-01 at `docs/adr/260815-2009-typed-workflow-contracts.md:9`). Yet `condition-ast` later “Adds the `failure` edge outcome to the graph model,” `cursor-state` later adds serialized `assignVars` on nodes, and `profile-catalog` later adds node-level `profile` plus `DecideConfig.profile` (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:81,97,105`). These are grammar, not merely execution semantics: today outcomes are the serialized `WorkflowEdgeOutcome` enum, common node fields live on serialized `WorkflowNode`, and decide fields live on serialized `DecideConfig` (`src/model.rs:213-221,441-452,862-934`). Are all of those fields actually owned by `typed-contracts`; if so, why do their downstream epics still say they add them, and if not, how does version 5 continue to discriminate document shape?

- **class:** sequencing errors
- **impact:** The first v5 epic must either reject formerly valid v4 workflows with conditions or implement condition execution before the epic that owns it.
- **evidence:** `typed-contracts` mechanically rewrites every flat condition into the v5 AST, while explicitly leaving AST execution semantics to the blocked downstream `condition-ast` epic and relying on validation gates for not-yet-supported features (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:73,78-81`). Current edge and loop execution accepts only the flat `StructuredCondition` and calls `evaluate_condition` directly (`src/model.rs:130-134,3182-3235`; `src/runtime.rs:6486,6534`). A gate that rejects the AST also rejects the normalized form of an existing v4 condition, whereas accepting it requires at least leaf-AST runtime semantics in `typed-contracts`. What executable condition representation remains available between these two epics so that 4→5 normalization is backward-compatible and `typed-contracts` can finish green?

#### Unjustified stack/dependency assumptions

No findings.
