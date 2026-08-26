## Plan adversary report

- scale: epic
- source-decision: author-supplied
- round: 2
- artifacts:
  - `docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md`
  - `docs/roadmap/RDMP-260815-2009-01-harness-workflows.md`
  - `docs/sources/workflow-schema-drift-260825.md`
  - `CONTEXT.md`
  - `docs/issues/ISSUE-260826-0004-01-orchestrator-branch-fallback-unreachable.md`
  - `docs/workflow-schema.md`
  - `docs/execution-model.md`
  - `Cargo.toml`
  - `.githooks/pre-commit`
  - `scripts/test-pre-commit-frontend-freshness.sh`
  - `.github/workflows/frontend-freshness.yml`
  - `.github/workflows/canonical-v4-docs.yml`
  - `scripts/check-canonical-v4-docs.sh`
  - `src/model.rs`
  - `src/runtime.rs`
  - `src/storage.rs`
  - `src/api.rs`
  - `src/driver.rs`

### Findings

#### Wrong problem

No findings.

#### Codebase-reality collision

- **class:** codebase-reality collision
- **impact:** A new node kind can compile after an arm is added while still never being passed to the generator, so the promised 14-of-14/new-variant catalog coverage is not compiler-enforced.
- **evidence:** The PRD says, “Coverage is compiler-enforced, not asserted,” and “Nothing about this can be forgotten or skipped,” because the generator uses an exhaustive `match` over `NodeKind` (`PRD-260826-0009-01`, lines 258-263 and 328-332). An exhaustive match proves only that every possible *input* is handled; it does not produce one input value per variant. The repo exposes no iterator for either enum, as the PRD itself notes. `src/model.rs:695-753` declares the variants, while both `NodeKind::node_type` (`:755-773`) and `From<WorkflowNodeType>` (`:820-860`) are exhaustive mappings but enumerate nothing at runtime. The only code that actually visits every current kind is still the manually seeded `Vec<NodeKind>` at `src/model.rs:4182-4284`. What guarantees that the generator's seed/call set contains every variant, including the next one, rather than merely guaranteeing that its dispatch function has an arm for it?

#### Missed simpler alternative

No findings.

#### Hidden coupling

- **class:** hidden coupling
- **impact:** The implementation and CI slices do not yet have an executable regeneration contract; depending on the unstated target choice, `cargo test` may only compile the generator, may rewrite docs as an ordinary test side effect, or may never invoke it.
- **evidence:** The PRD says “regenerate, diff, fail on difference,” that the freshness check “rides” the new `cargo test` job, and cites reading files “inside the inline test module” as prior art (`PRD-260826-0009-01`, lines 322-348 and 361-365), but it names neither a generator path nor a command. `Cargo.toml` has a package plus dependencies and no declared generator target or workspace (`Cargo.toml:1-39`); the existing inline module at `src/model.rs:3248` contains tests, and its cited template pins only read and validate files. By contrast, the frontend pattern has a distinct producer command followed by a distinct comparison: `.github/workflows/frontend-freshness.yml:24-28` runs `npm run build` then `git diff`, while `.githooks/pre-commit:29-36` invokes the producer in a staged checkout then compares outputs. Which Rust target owns generation, what exact command executes it, and does `cargo test` invoke that command rather than merely compile its code?

#### Sequencing errors

- **class:** sequencing errors
- **impact:** Issue slicing from the cited work order can apply the superseded round-1 remediation scheme and contradict the redesigned PRD's honest limits.
- **evidence:** The PRD says every audit finding is assigned one of the new G/P/C/D tiers and calls the audit “the work order,” “grouped by tier” (`PRD-260826-0009-01`, lines 228-242 and 310-314). The referenced audit still says its tiers are the PRD's implementation decisions but defines only T1/T2/T3, with T1 “pinned by the doc-example test” (`docs/sources/workflow-schema-drift-260825.md:19-22`). Its grammar, validation, and field-table findings remain labeled T1 (for example `:28` and the Part 1.9 heading), although the round-2 PRD now limits generation to the node-kind catalog and explicitly leaves the validation catalog and field tables hand-written Tier P (`PRD`, lines 231-237 and 350-359). Which tier assignment is authoritative when `/to-issues` consumes the audit as its work order?

#### Unjustified stack/dependency assumptions

No findings.
