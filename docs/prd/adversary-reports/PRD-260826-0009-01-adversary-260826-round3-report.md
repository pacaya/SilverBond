## Plan adversary report
- scale: epic
- source-decision: author-supplied
- round: 3
- artifacts:
  - `docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md`
  - `docs/roadmap/RDMP-260815-2009-01-harness-workflows.md`
  - `docs/sources/workflow-schema-drift-260825.md`
  - `CONTEXT.md`
  - `docs/issues/ISSUE-260826-0004-01-orchestrator-branch-fallback-unreachable.md`
  - `Cargo.toml`
  - `justfile`
  - `.githooks/pre-commit`
  - `.github/workflows/canonical-v4-docs.yml`
  - `.github/workflows/frontend-freshness.yml`
  - `src/model.rs`
  - `src/runtime.rs`
  - `src/driver.rs`
  - `src/api.rs`
  - `src/lib.rs`
  - `src/main.rs`
  - `playwright.config.ts`
  - `docs/workflow-schema.md`

### Findings

#### Wrong problem

none.

#### Codebase-reality collision

1. **class:** codebase-reality collision  
   **impact:** medium — check 3 can pass for a document that the runtime rejects, so it does not establish the PRD's claimed “actually runnable” property. The zero-`error` requirement itself is satisfiable for all fourteen current kinds; the collision is between what that seam proves and what the PRD says it proves.  
   **evidence:** The artifact says, “Every generated workflow is pushed through `ensure_defaults` and `validate_workflow` and must produce zero `error`-severity issues, so the documented examples are … actually runnable documents” (`PRD-260826-0009-01-docs-from-rust-truth.md:377-379`). For `parallel_batch`, validation checks that `bodyEntry` is nonempty and names an existing node, but does not check its kind (`src/model.rs:3070-3087`). Execution separately requires that node to be one of the runner kinds and errors otherwise (`src/runtime.rs:2086-2097`, `:5114-5127`). Likewise, subflow validation accepts any nonempty input source while checking names and callee variables (`src/model.rs:2656-2691`), but execution can fail when that source cannot be resolved (`src/runtime.rs:3502-3513`). These are runtime constraints outside the proposed validity seam.

2. **class:** codebase-reality collision  
   **impact:** low — the Tier P decision remains defensible because serialized examples cannot label a value as *the default*, but the stated structural reason is generalized beyond the actual serde model.  
   **evidence:** The artifact says “Serde's skip attributes make a default value structurally invisible in serialized output” and later that field tables are “Not generable because serde's skip attributes erase defaults” (`PRD-260826-0009-01-docs-from-rust-truth.md:268-275`, `:401-404`). That is true for the cited `SpawnConfig` case, but not across all kinds. `BatchConfig.max_concurrent` defaults to `4` and has no `skip_serializing_if`; `NodeKind::ParallelBatch` also always serializes `batchConfig` (`src/model.rs:454-470`, `:711-714`). `SubflowConfig.max_depth` likewise has a deserialize default but no serialization skip (`:630-638`, `:715-721`). `SendConfig.enter`, `WaitConfig.mode`, and `CaptureConfig.all`/`ansi` are also serialized whenever their containing config is non-default (`:504-573`, `:727-737`). The invisibility claim therefore applies only to some defaults/configurations.

#### Missed simpler alternative

none.

#### Hidden coupling

1. **class:** hidden coupling  
   **impact:** medium — “binary/example target” leaves two materially different repository effects unresolved. Under the binary branch, existing server and e2e entry points become ambiguous; under the example branch, they do not.  
   **evidence:** The artifact settles only that the generator “lives as its own binary/example target” (`PRD-260826-0009-01-docs-from-rust-truth.md:291-297`). The package already has the implicit `silverbond` binary at `src/main.rs` and no `default-run` in `Cargo.toml:1-4`. The repository invokes it without `--bin` in `justfile:16-18` and in Playwright's web-server command (`playwright.config.ts:25-26`), with the same unqualified command documented elsewhere. A second binary target makes those calls unable to select a binary; an example target does not participate in default `cargo run` selection. Thus the target kind is not an interchangeable implementation detail.

2. **class:** hidden coupling  
   **impact:** low — the Out-of-Scope CI bound is internally contradictory, so it does not unambiguously bound the CI work.  
   **evidence:** The testing decision says “The docs freshness check rides the same job” as the full Rust suite (`PRD-260826-0009-01-docs-from-rust-truth.md:387-395`). The Out-of-Scope section then states, “The CI job is `cargo test` only” (`:428-433`). A job that regenerates documentation and diffs the generated blocks has steps beyond `cargo test`; both statements cannot be literal descriptions of the same job. The generator and freshness bullets at `:434-439` are real surface bounds, but this CI bullet is not a coherent command bound.

#### Sequencing errors

1. **class:** sequencing errors  
   **impact:** medium — the generator's first execution/freshness check has no legal output span at the point where the PRD schedules it.  
   **evidence:** The generator is constrained to rewrite “only the span” between existing `BEGIN GENERATED`/`END GENERATED` markers (`PRD-260826-0009-01-docs-from-rust-truth.md:254-258`). The sequencing note puts the generator before schema-reference assembly (`:520-524`). The current `docs/workflow-schema.md` contains no generated markers; its hand-written Node Types section starts at line 97. Therefore the marker-bearing document scaffold that the generator requires does not exist until the later schema-reference work.

#### Unjustified stack/dependency assumptions

1. **class:** unjustified stack/dependency assumptions  
   **impact:** high — the `strum` design cannot compile with the dependency placement and source boundary the PRD declares.  
   **evidence:** The artifact simultaneously says “No engine code changes. This epic reads `src/`, writes `docs/`” (`PRD-260826-0009-01-docs-from-rust-truth.md:230-232`) and that the generator “derives an iterator over `WorkflowNodeType` with `strum` as a dev-dependency” (`:279-289`). `WorkflowNodeType` is a suitable plain fieldless enum, but it is declared in `src/model.rs:157-174`; a derive must be attached to that declaration, so this entails editing `model.rs`. The crate exposes that model through its normal library target (`src/lib.rs:1-12`). Cargo dev-dependencies are not available when compiling that normal library for a separate example or binary, so an unconditional `strum::EnumIter` derive on the library enum cannot resolve from `[dev-dependencies]` (`Cargo.toml:37-39`). If the separate target is a normal binary, its regeneration command cannot use a dev-dependency directly either. Moving `strum` into the normal dependency graph would collide with the roadmap's literal “zero new runtime dependencies initiative-wide” stack decision (`RDMP-260815-2009-01-harness-workflows.md:25`). The enum's shape is not the blocker; the claimed dev-only, no-`model.rs` integration is.
