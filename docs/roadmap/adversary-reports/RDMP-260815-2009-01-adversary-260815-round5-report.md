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
- **impact:** Valid v4 workflows using relational conditions can fail migration or change behavior, contradicting the promised behavior-identical 4→5 normalization.
- **evidence:** ADR-02 limits migrated conditions to a legacy family of “`eq_legacy`, `ne_legacy`, plus the string `contains`/`matches`” and says this preserves v4 exactly (`docs/adr/260815-2009-single-condition-dialect.md:9`); the typed-contracts epic likewise promises a “legacy operator family preserving v4 condition semantics” (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:57,74`). The actual v4 evaluator also supports `>`, `<`, `>=`, and `<=`, after stringifying the JSON field and parsing both sides as `f64` (`src/model.rs:3182-3235`); the frontend offers all four (`ui/src/features/editor/ConditionBuilder.svelte:32`), and the Rust suite pins `>=` as supported behavior (`src/model.rs:3825-3837`). Mapping such a leaf to a strict typed-number operator is not equivalent for a JSON string such as `"12"`, which v4 accepts numerically, while the stated legacy family has no relational member. Which v5 leaf preserves these four supported v4 operators and their coercion/error behavior?

- **class:** codebase-reality collision
- **impact:** The harness acceptance criterion is not executable against the planned grammar unless it silently means conditioned edges or introduces an unplanned sixteenth node kind.
- **evidence:** Harness acceptance requires every mechanical step to be “a script or condition node” (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:114`). The complete-v5 owner names a new `script` variant but no condition variant (`ibid.:74`), and the condition epic implements conditions only on edges and `loopCondition` (`ibid.:82`). That matches the repo: the fourteen `WorkflowNodeType` variants contain no condition node (`src/model.rs:153-187`; `ui/src/lib/types/workflow.ts:3-17`), while conditions live at `WorkflowEdge.condition` and `WorkflowNode.loop_condition` (`src/model.rs:897-934`; `ui/src/lib/types/workflow.ts:245-265`). Does “condition node” mean a conditioned edge/loop, and if so, how is the structural acceptance criterion evaluated without counting a nonexistent node type?

- **class:** codebase-reality collision
- **impact:** Profile-selected nodes can lose existing workflow defaults or resolve them in an unintended order, making runtime configuration and the routing oracle depend on an unstated merge rule.
- **evidence:** ADR-04 deliberately retains `agentDefaults` but declares the complete precedence as “node config → workflow-local `profiles` → app catalog → driver defaults” (`docs/adr/260815-2009-profile-indirection-routing.md:9`); the profile epic repeats that chain while saying `agentDefaults` “stays keyed by agent name” (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:106`). In the actual merge seam, workflow `agent_defaults[agent]` is the middle layer between node overrides and built-in defaults (`src/model.rs:255-301,301-351`), and it can supply model, reasoning, access, tools, budgets, and orchestrator config. Where does this retained brownfield layer sit for a node that selects a profile, or is it intentionally ignored only on the profile path?

#### Missed simpler alternative

No findings.

#### Hidden coupling

- **class:** hidden coupling
- **impact:** The grammar-owning epic can define or validate `assignVars` inconsistently with the state epic, forcing a coordinated schema correction after v5 has already been declared complete.
- **evidence:** `cursor-state` says `assignVars` grammar is landed by `typed-contracts` and that it only implements the write semantics later (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:98`), but `typed-contracts` lists only ADR-01 through ADR-04 as invariants (`ibid.:69-74`), omitting ADR-05—the only decision that defines assignment as binding-only and cursor-local (`docs/adr/260815-2009-cursor-local-writes.md:7-9`). This is not an existing field that can be carried through mechanically: `WorkflowNode` currently has no assignment member (`src/model.rs:862-910`). Which invariant constrains the v5 serde shape and validation gate for `assignVars` before the ADR-05-owned implementation epic begins?

- **class:** hidden coupling
- **impact:** The shipped `issue-dev` template still cannot reliably call the shipped `implement-issue` template without a new installation/storage capability that the harness epic's own gap clause excludes.
- **evidence:** The amended harness epic now asserts that “The templates instantiate into the workflow store (subflow resolution consults `WorkflowStore` only)” and bounds gap-fixing to prior-epic defects/small extensions (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:114`). The repo has no template-instantiation operation: `TemplateStore` only lists individual templates (`src/storage.rs:1075-1148`), `/api/templates` is GET-only (`src/api.rs:150,642-644`), and clicking a template merely opens one remapped document in the editor (`ui/src/features/workflows/Sidebar.svelte:99-112`) for a separate manual save (`ui/src/app/AppShell.svelte:56-67,392-399`). Boot seeding writes bundled files only to `templates_dir`, not `workflows_dir` (`src/app.rs:347-364`; `src/storage.rs:136-151`), while hydration resolves callees exclusively through `state.workflows.get` (`src/api.rs:584-603`). Which scheduled epic owns installing both related templates into `WorkflowStore` with the names needed by the call, rather than merely asserting that instantiation happens?

#### Sequencing errors

- **class:** sequencing errors
- **impact:** The v4 documentation baseline and the first v5 schema slice can be taken concurrently or in reverse order, causing the same truth files to be overwritten or eliminating the baseline that later deltas are meant to extend.
- **evidence:** `docs-truth` says it “establishes the baseline” by rewriting `docs/workflow-schema.md` and `docs/execution-model.md` from the current fourteen-kind v4 source, and later schema epics own deltas to those same files (`docs/roadmap/RDMP-260815-2009-01-harness-workflows.md:63-67`). `typed-contracts` has no `Blocked by: docs-truth` despite immediately moving the repo to v5 and owning its documentation delta (`ibid.:69-74`); other real dependencies in the same roadmap are expressed explicitly with `Blocked by` (`ibid.:76-111`). The repo currently is the v4/14-kind baseline (`src/model.rs:10,153-187`) and the two target docs are live shared files. What dependency guarantees that the baseline epic completes before `typed-contracts` edits the same documents?

#### Unjustified stack/dependency assumptions

No findings.
