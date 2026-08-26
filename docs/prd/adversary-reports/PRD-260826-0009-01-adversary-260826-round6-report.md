## Plan adversary report

- scale: epic
- source-decision: author-supplied
- round: 6
- artifacts:
  - `docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md`
  - `docs/roadmap/RDMP-260815-2009-01-harness-workflows.md`
  - `docs/sources/workflow-schema-drift-260825.md`
  - `CONTEXT.md`
  - `docs/issues/ISSUE-260826-0004-01-orchestrator-branch-fallback-unreachable.md`
  - `docs/issues/ISSUE-260826-0240-01-architecture-md-node-types-stale.md`
  - `Cargo.toml`
  - `justfile`
  - `playwright.config.ts`
  - `tests/http_api.rs`
  - `src/lib.rs`
  - `src/model.rs`
  - `src/runtime.rs`
  - `.github/workflows/canonical-v4-docs.yml`
  - `.github/workflows/frontend-freshness.yml`
  - `.githooks/pre-commit`

### Findings

#### Wrong problem

none.

#### Codebase-reality collision

none.

#### Missed simpler alternative

none.

#### Hidden coupling

1. **class:** hidden coupling / **impact:** high — the proposed round-trip closes the wrong-arm case only for distinct inputs already present in `CATALOG_ORDER`; it does not detect duplicate catalog entries, and the PRD still states the stronger completeness guarantee in several normative sections. / **evidence:** `WorkflowNodeType` is `Copy + PartialEq + Eq` but not intrinsically unique inside an array (`src/model.rs:157-174`). With `CATALOG_ORDER` containing `Task` twice, both evaluations of `example_for(Task).node_type() == Task` pass; with fixed length `N`, one accepted kind can be omitted in exchange, and both the freshness comparison and every per-entry structural-validity check remain self-consistent with that incomplete generated output. `NodeKind::node_type()` is indeed public and exhaustive (`src/model.rs:755-773`), so it catches a listed `t` whose arm returns a different kind, but it supplies no distinctness property for the inputs. This directly falsifies “It catches duplicates too” and “whatever sits in `CATALOG_ORDER` is correctly and uniquely mapped” (`PRD:311-320`). The conceded omission gap is locally accurate, but the artifact still says a new variant fails “until it is documented” in Solution (`:67-69`), makes that an explicit user story twice (`:200-201`, `:227-229`), says the witness and coverage assertion force the new arm into documentation (`:83-87`), and states success as every schema change missing a catalog update failing CI (`:51-53`). Testing Decisions is internally contradictory as well: it calls the absent-entry gap check 2's job (`:436-443`) and then says check 2 does not catch an absent entry (`:444-448`).

2. **class:** hidden coupling / **impact:** low — the merge-blocking claims depend on external branch-protection state that the PRD expressly does not deliver. / **evidence:** Testing Decisions says a stale generated block “cannot be merged” (`PRD:428-432`), and Solution/Implementation describe CI as making the guard unskippable (`:303-309`). Out of Scope says whether the new job becomes a required status check is a repository-settings decision outside the PRD (`:512-515`). The two existing workflows only define CI triggers and jobs; neither repository files nor the planned workflow can require a successful check before merge. The plan therefore establishes a failing CI result, not the stated inability to merge.

#### Sequencing errors

none.

#### Unjustified stack/dependency assumptions

1. **class:** unjustified stack/dependency assumptions / **impact:** high — the accepted completeness gap is justified by a false claim that the current stack has no robust zero-new-dependency source of enum variants. / **evidence:** `WorkflowNodeType` already derives `serde::Deserialize` (`src/model.rs:157-174`), and `serde` with `derive` is an existing normal dependency (`Cargo.toml:22`). The stable `Deserializer::deserialize_enum` interface receives the derived enum's static variant-name slice. A compiled scratch probe against the repository's current `libsilverbond`, invoking `WorkflowNodeType::deserialize` with a custom deserializer that records that slice, returned all 14 canonical names: `task, approval, split, collector, decide, parallel_batch, subflow, call, spawn, send, wait, capture, kill, run_agent`. This requires neither a source-text scan, a new crate, nor an edit under `src/`, contrary to the PRD's statements that no robust zero-dependency source exists, `strum` is the only proper closure, and the residual gap is the strongest option under the epic's constraints (`PRD:321-333`).
