## Plan adversary report
- scale: epic
- source-decision: author-supplied
- round: 5
- artifacts: `docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md`; `docs/roadmap/RDMP-260815-2009-01-harness-workflows.md`; `docs/sources/workflow-schema-drift-260825.md`; `CONTEXT.md`; `docs/issues/ISSUE-260826-0004-01-orchestrator-branch-fallback-unreachable.md`; `docs/issues/ISSUE-260826-0240-01-architecture-md-node-types-stale.md`; `Cargo.toml`; `justfile`; `playwright.config.ts`; `tests/http_api.rs`; `src/lib.rs`; `src/model.rs`; `src/runtime.rs`; `.github/workflows/canonical-v4-docs.yml`; `.github/workflows/frontend-freshness.yml`; `.githooks/pre-commit`; `scripts/check-canonical-v4-docs.sh`; `scripts/test-pre-commit-frontend-freshness.sh`

### Findings

#### Wrong problem

none.

#### Codebase-reality collision

- **class:** codebase-reality collision / **impact:** medium — wire-format requiredness can drift while the claimed compile-time guard remains green. / **evidence:** The PRD says constructed examples compile-check “field names, types, and required-ness” (`PRD:298-299`), but requiredness is controlled independently by serde attributes. For example, `DecideConfig.prompt` is a `String` with `#[serde(default)]` (`src/model.rs:443-451`), while `BatchConfig.items_binding`, `item_var`, and `body_entry` are equally ordinary `String` fields without serde defaults (`:463-470`). Adding or removing `#[serde(default)]` changes whether input may omit a field without changing the Rust construction API, so the generator still compiles. The requiredness tables are also explicitly Tier P and unpinned (`PRD:466-482`).

- **class:** codebase-reality collision / **impact:** low — one plain-build overclaim remains and contradicts the PRD’s corrected target-selection account. / **evidence:** The Solution still says a renamed or misspelled field “fails the build” (`PRD:66-67`). The same PRD later states accurately that `tests/docs_catalog.rs` is compiled by `cargo test`, while `cargo build` and `cargo check` do not touch it (`:300-305`, `:428-432`). `Cargo.toml` defines the normal library and binary targets, and Cargo metadata identifies files under `tests/` as separate test targets; therefore the earlier unqualified build claim is false even though the proposed CI `cargo test` job would catch the error.

#### Missed simpler alternative

none.

#### Hidden coupling

- **class:** hidden coupling / **impact:** high — the stated “bijection” proves that `CATALOG_ORDER` enumerates distinct `WorkflowNodeType` values, but it does not prove that the conversion used by the generator reaches each `NodeKind` variant. / **evidence:** The generator’s skeleton is explicitly `NodeKind::from(WorkflowNodeType)` (`PRD:290-299`, `:428-430`). Check 2 compares `CATALOG_ORDER.len()` with the source count of `NodeKind` and checks only the distinctness/range of `catalog_index` over `CATALOG_ORDER` (`:300-316`, `:436-439`). A future change can add one variant to each enum, include the new `WorkflowNodeType` once in `CATALOG_ORDER`, assign it a fresh index, and mistakenly map its `From` arm to an existing `NodeKind`; all three stated bijection assertions still pass while the new accepted `NodeKind` has no generated example. The current conversion is correct (`src/model.rs:820-860`), but no stated check establishes that conversion invariant.

- **class:** hidden coupling / **impact:** medium — the coverage proof depends on an unspecified source-format parser and therefore is not verified against the Rust syntax named in the task. / **evidence:** The PRD says only that the test reads `src/model.rs` and “counts the variants declared in the `enum NodeKind` block” (`PRD:308-313`); it defines no lexical rules or acceptance cases. The current block already mixes unit variants with struct variants, nested braces, and multi-line `#[serde(...)]` attributes containing commas and delimiters (`src/model.rs:693-753`). A first-closing-brace scan terminates inside `Task`; a comma-based scan must distinguish variant separators from attribute and field commas; comments and doc-comments can contain the same tokens, and attributes can conditionally remove a variant from the compiled enum. With no parser dependency and no specified lexer behavior, the count remains coupled to formatting and comment/attribute syntax, so its role as the cardinality source is not robustly established.

#### Sequencing errors

none.

#### Unjustified stack/dependency assumptions

none.
