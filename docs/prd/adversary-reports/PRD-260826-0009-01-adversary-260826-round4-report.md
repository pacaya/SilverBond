## Plan adversary report

- scale: epic
- source-decision: author-supplied
- round: 4
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
  - `.githooks/pre-commit`
  - `.github/workflows/canonical-v4-docs.yml`
  - `.github/workflows/frontend-freshness.yml`
  - `src/model.rs`
  - `src/runtime.rs`
  - `src/lib.rs`
  - `src/main.rs`
  - `tests/http_api.rs`
  - `docs/workflow-schema.md`
  - `docs/execution-model.md`

### Findings

1. **class:** codebase-reality collision  
   **impact:** high — the proposed coverage check can pass while a node kind is absent from the generated catalog, so the epic's central “every accepted kind is discoverable” CI guarantee is not established.  
   **evidence:** The PRD says the source-text variant count equaling `CATALOG_ORDER.len()` “catches a variant given a witness arm but never added to the catalog” (`PRD:290-298`, `:408-411`). It proves only equal cardinality. Rust arrays permit duplicate `WorkflowNodeType` values, and the proposed exhaustive `catalog_index(WorkflowNodeType)` proves only that every enum input has a match arm; neither stated check proves that each value occurs once in `CATALOG_ORDER`. An order with the right length that repeats an existing value and omits another therefore compiles and passes the count assertion. The actual source has separate 14-variant `WorkflowNodeType` and `NodeKind` declarations (`src/model.rs:159-174`, `:695-753`) and an exhaustive conversion between them (`:820-859`), but none of those library facts imposes uniqueness or set equality on a test-side array.

2. **class:** codebase-reality collision  
   **impact:** medium — the PRD still promises a normal build-time documentation stop that the selected test-only home cannot provide; a developer can make the library exhaustive and run `cargo build` or `cargo check` successfully while the catalog remains stale.  
   **evidence:** The generator and witness are explicitly placed in a `#[cfg(test)]` module (`PRD:314-325`, `:554`), so they are compiled by a test target, not by the normal library/binary build. The PRD nevertheless says the generator walks an exhaustive match over `NodeKind` and that a new variant “breaks the build until it is documented” (`:62-67`), then says the compile-time check “cannot be skipped by forgetting to run something” (`:402-405`). The detailed scheme actually proposes `catalog_index(WorkflowNodeType)`, not a match over `NodeKind` (`:290-293`). `src/lib.rs:1-14` exposes the library and its test-only support separately, while `Cargo.toml` has ordinary library/binary targets and no mechanism that compiles integration/test modules during `cargo build`. The planned CI `cargo test` does compile the witness, but that is a suite/CI guard rather than the stated build guard.

3. **class:** hidden coupling  
   **impact:** medium — `From<WorkflowNodeType>` does not by itself produce the valid, useful examples the plan attributes to it; the generator needs hand-maintained, variant-specific config initialization in addition to the acknowledged supporting graph, and the v5 catalog cannot be absorbed automatically.  
   **evidence:** The PRD calls value construction “already solved” by `From<WorkflowNodeType>` and says the generator calls it “rather than hand-seeding values” (`PRD:282-289`), then describes only the supporting graph required by per-kind validation (`:327-334`) and says the catalog absorbs the v5 bump automatically (`:82-84`, `:490-492`). The actual conversion produces `DecideConfig::default()`, `BatchConfig::default()`, empty `SubflowConfig` for both `subflow` and `call`, default pane configs, `Task { agent_config: None }`, and `RunAgent` with no agent (`src/model.rs:820-859`). Those values do not validate cleanly as examples without per-kind data: empty decide outcomes are an error (`:2958-2968`); default batch has empty `items_binding`, `item_var`, and `body_entry` (`:473-481`) and all three are errors (`:3051-3087`); default subflow/call has an empty workflow name (`:641-648`) and that is an error (`:2544-2556`). Task, spawn, send, and run-agent defaults also need common-node/config data to avoid hard errors (`:1670-1677`, `:1704-1722`, `:1743-1755`, `:1769-1782`). Separately, `typed-contracts` is expressly scheduled to land the new `script` node grammar (`RDMP:71-77`), while the proposed fixed `CATALOG_ORDER` must be manually extended before that new kind can appear. The scheme safely detects some drift, but its meaningful fixtures and future kind wiring remain hand-maintained.

4. **class:** unjustified stack/dependency assumptions  
   **impact:** low — the final artifact gives contradictory dependency instructions, leaving the supposedly settled stack decision internally inconsistent.  
   **evidence:** The implementation decision says coverage uses zero new dependencies (`PRD:282-310`), and the dimension scan repeats “zero new dependencies of any kind” (`:555`); current `Cargo.toml:6-39` contains `serde_json` already and no enumeration crate, so the revised generator requires no addition. The `schemars` exclusion still says “the epic already takes one dev-dependency for enumeration” (`PRD:493-498`). That is a stale statement from the rejected enumeration design and directly contradicts both the actual manifest and the final stack decision.

- wrong problem: none.
- missed simpler alternative: none.
- sequencing errors: none.
