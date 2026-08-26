## Plan adversary report
- scale: epic
- source-decision: author-supplied
- round: 8
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
  - `src/api.rs`
  - `src/storage.rs`
  - `.github/workflows/canonical-v4-docs.yml`
  - `.github/workflows/frontend-freshness.yml`

### Findings

#### wrong problem

none.

#### codebase-reality collision

- **class:** codebase-reality collision
  / **impact:** high — the PRD still makes the pre-demotion completeness promise in its success contract and user stories, although the specified four checks explicitly do not deliver it for a `NodeKind`-only addition.
  / **evidence:** The admitted counterexample says that after `NodeKind::Script` is given the required exhaustive-match arm and mapped to an existing `WorkflowNodeType`, the harvested set, catalog length, and round-trips remain unchanged and “the suite goes green with the kind undocumented” (`PRD:343-351`); Testing Decision 3 repeats that the compile stop “does not force a catalog entry” without a matching `WorkflowNodeType` (`:476-479`). In conflict with that limit, Success still says every accepted kind is discoverable and a change to the set of node kinds fails CI rather than shipping (`:51-53`); Solution says “a new variant fails the test suite until documented” (`:72-73`); the v5 summary says the compile stop and set-equality check force a genuinely new kind's catalog entry (`:93-97`); user story 52 specifically promises that a new `NodeKind` variant fails until documented (`:208-211`); user story 61 repeats the promise for genuinely new kinds (`:237-239`); and Further Notes says the suite fails until the new-kind half is done (`:656-660`). `NodeKind` and `WorkflowNodeType` are separate enums joined by hand-written matches (`src/model.rs:157-174`, `:693-773`, `:820-860`), so the PRD's own counterexample is permitted by the current type structure.

- **class:** codebase-reality collision
  / **impact:** low — the Dimension Scan misstates the existing CI inventory.
  / **evidence:** The ops row says this epic adds “the repo's first CI job” (`PRD:637`). The repository already contains the `canonical-v4-docs` job in `.github/workflows/canonical-v4-docs.yml` and the `frontend-bundle-freshness` job in `.github/workflows/frontend-freshness.yml`. The PRD itself acknowledges both existing workflows at `:499-501` and accurately describes the addition there as the first **Rust** CI surface, not the first CI job.

#### missed simpler alternative

- **class:** missed simpler alternative
  / **impact:** high — the demotion's claim that the `NodeKind`-only gap cannot be closed without `strum`, a runtime dependency, and a `src/` edit is false; the stated reason for accepting the completeness hole does not hold.
  / **evidence:** The PRD says internally tagged `NodeKind` exposes no variant names to serde and that “no zero-dependency construction provides” enumeration, leaving `strum::EnumIter` as the only stated closure (`PRD:336-361`). A compiled read-only probe against the current `NodeKind` derive (`src/model.rs:693-695`) used only existing `serde`: a `serde::de::value::MapDeserializer` supplied `{"type":"__catalog_probe__"}`, and a custom `serde::de::Error::unknown_variant` implementation captured the derive's `expected` argument structurally. It received `Some(["task", "approval", "split", "collector", "decide", "parallel_batch", "subflow", "call", "spawn", "send", "wait", "capture", "kill", "run_agent"])`. Those are all 14 `NodeKind` wire tags, obtained without parsing source or an error string, without a new crate, and without editing `src/`. `serde` with derive is already a normal dependency (`Cargo.toml:22`). The four proposed checks do exhibit the slip exactly as described after the compile-stop edit, but the PRD overstates that slip as an unavoidable serde/dependency limit.

#### hidden coupling

- **class:** hidden coupling
  / **impact:** high — the structural-validity check does not observe the reader-facing acceptance seam it claims to protect; it relies on `Serialize` output remaining ingestible by `Deserialize` without checking that boundary.
  / **evidence:** The generator serializes constructed Rust values into the documented JSON (`PRD:68-73`, `:275-281`), but Testing Decision 4 sends the original typed `WorkflowV3` directly through `ensure_defaults` and `validate_workflow` (`:480-488`), and the next paragraph explicitly says the typed value is used instead of document ingress (`:490-494`; it calls this “Check 3”). Actual external workflow validation first calls `normalize_workflow_value` (`src/api.rs:559-568`), which migrates the JSON and then deserializes it into `WorkflowV3` before validation (`src/model.rs:1099-1113`); save and run ingress likewise start with that normalization (`src/api.rs:535-542`, `:736-751`). Compilation and validation of the pre-serialization value cannot detect an asymmetric serde change that makes the emitted document fail deserialization, so the check does not establish its claims that it observes “that a documented example is a document the engine accepts” or that examples pass “the same save-time gate” (`PRD:452-454`, `:480-482`). The named requiredness limit supplies a concrete drift mechanism: `#[serde(default)]` can change omission acceptance while leaving Rust construction, serialization, and typed validation unchanged (`PRD:532-538`).

#### sequencing errors

- **class:** sequencing errors
  / **impact:** low — the test specification's check reference no longer matches the four-check ordering, making the stated seam exception point at the wrong check.
  / **evidence:** Testing Decisions number the `NodeKind` compile stop as check 3 and structural validity as check 4 (`PRD:463-488`), but the immediately following “Deliberately not `normalize_workflow_value`” paragraph says “Check 3 uses the typed value directly” (`:490-494`). Check 3 contains no workflow value; that rationale describes check 4.

#### unjustified stack/dependency assumptions

none.
