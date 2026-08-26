## Plan adversary report
- scale: epic
- source-decision: author-supplied
- round: 7
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

### Findings

- **class:** wrong problem / **impact:** none / **evidence:** none
- **class:** codebase-reality collision / **impact:** The top-level success criterion still over-claims what CI will guard. A schema acceptance change can leave the documented schema stale while every planned check stays green. / **evidence:** The PRD says any schema change that does not update the documented node catalog fails CI (`PRD:52-53`), but later correctly concedes that adding or removing `#[serde(default)]` changes what the wire format accepts without changing the Rust construction API (`PRD:304-307`), and that all Tier P requiredness/default tables remain unpinned (`PRD:479-503`). Such a serde-requiredness change need not alter a constructed example's serialized output, the harvested enum-name set, catalog length, round-trip, validation result, or committed generated block. The high-level success claim is therefore stronger than the explicitly documented test envelope.
- **class:** missed simpler alternative / **impact:** none / **evidence:** none
- **class:** hidden coupling / **impact:** The node-kind completeness guarantee is not complete: a new engine-accepted `NodeKind` can remain absent from the catalog after the suite returns green. This falsifies the repeated claims that every accepted kind is discoverable and that a new `NodeKind` is forced into the catalog (`PRD:52`, `:68`, `:91-92`, `:205-206`, `:308-335`, `:442-452`). / **evidence:** Canonical workflow JSON deserializes `WorkflowNode.kind` as `NodeKind` (`src/model.rs:693-753`, `:862-874`); `WorkflowNodeType` is a separate 14-variant enum (`:157-174`) connected only by hand-written `node_type()` and `From<WorkflowNodeType>` matches (`:755-773`, `:820-860`). Project-wide Rust usage shows `WorkflowNodeType` outside `model.rs` only in test helpers; it is not the enum that accepts canonical node-kind wire values. Consequently, adding (for example) a `NodeKind::Script` serde variant without adding `WorkflowNodeType::Script` leaves `example_for(WorkflowNodeType)` exhaustive, leaves the harvested `WorkflowNodeType` set and `CATALOG_ORDER` length unchanged, and leaves every existing round-trip true. Existing exhaustive `NodeKind` matches initially cause ordinary compile stops, but repairing those matches can map the new variant to an existing `WorkflowNodeType`; none of the three proposed catalog checks forces a new `WorkflowNodeType` or catalog entry. A compiled read-only probe confirmed the distinction: `WorkflowNodeType::deserialize` calls `deserialize_enum` and exposes the stated 14-name static slice, while the internally tagged `NodeKind::deserialize` does not expose variant names through that hook. The same probe confirmed that omission from `CATALOG_ORDER`, duplication, an isolated mis-mapped example arm, the reverse `WorkflowNodeType`-only addition, and an isolated serde-name rename are covered as described; it also constructed one workflow per current kind and observed zero error-severity validation issues for all 14.
- **class:** sequencing errors / **impact:** none / **evidence:** none
- **class:** unjustified stack/dependency assumptions / **impact:** none / **evidence:** none. The serde harvest itself compiled using existing `serde`: a deserializer can retain the `&'static [&'static str]` passed to `deserialize_enum` and return an error directly from that method without invoking the visitor; no visitor-originated error, new dependency, or `src/` edit is required. The captured names were exactly `task, approval, split, collector, decide, parallel_batch, subflow, call, spawn, send, wait, capture, kill, run_agent`.
