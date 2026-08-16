---
id: ADR-260815-2009-01
status: accepted
terms: [Workflow Contract, Variable, Boundary Normalization]
---

# Typed workflow contracts over a JSON-valued variable store

Workflows and subflows declare their inputs and outputs with types from a fixed small set — `string | number | boolean | enum | json` — plus `required` flags (an `enum` declaration carries its allowed-value set inline; script nodes declare their named outputs from the same type set in node config); subflow wiring is validated at save time and run-start overrides are parsed and normalized against the declared types exactly once, at the boundary. Declared outputs are bound by the callee: `outputs: {name: {type, from: <binding>}}` maps each name to an internal binding (node output or variable), and at call completion the engine materializes exactly those named outputs as the call node's parsed output, which callers consume like any node output — the callee's variable scope is never exposed. The workflow document version bumps to 5, with 4→5 normalization following the existing v2/v3→4 precedent (`normalize_workflow_value`, `src/model.rs:1099`) — the flat condition triple normalizes to a single-leaf AST mechanically. The complete v5 grammar (full condition AST, `script` node variant, `profiles` map, node-level `profile` field, `assignVars`, contracts) is defined, parsed, and validated in one step; later epics deliver execution semantics behind validation gates that reject not-yet-supported features with clear errors, so the version always discriminates document shape. Internally the per-execution variable store (`var_map`, today `BTreeMap<String, String>` in `src/runtime.rs`) becomes JSON-valued; template substitution sites receive strings through one explicit, deterministic stringification function, and write-once node outputs (`{{node:ID.parsedOutput…}}`) remain the default data path, with mutable variables as the escape hatch. Boundary normalization covers every externally-supplied surface, including script-node outputs (stdout/`SB_OUTPUT` parsed against the node's declared output types). Persisted checkpoints are versioned and migrated on read (per the `upgrade_run_workflow_json` precedent, `src/storage.rs:1328`) so pre-migration runs resume rather than break. Subflow catalog entries carry explicit provenance in the v5 grammar: `ref` entries (name → stored workflow) are re-hydrated from the workflow store and re-validated at save/validate/run, so a caller never runs against a stale callee contract; `inline` entries (authored in-document, e.g. the editor's save-selection-as-compound) are authoritative as written, and a name collision between an inline entry and a stored workflow is a save-time validation error.

## Considered Options

- **Strings-only store + typed comparison operators** — rejected: loop/batch aggregation (`parallel_batch`, collector results, accumulating lists) degenerates into JSON-in-a-string with re-parse obligations at every consumer, the debt class Argo users report continuously (escaped-string aggregation, argo-workflows #12812/#13510). The survey's famous stringly footguns (GH Actions `"false"`-truthy, the Norway problem, Terraform `==`) indict implicit *coercion*, not typed storage — typed operators are adopted regardless (ADR-260815-2009-02), so they are not an alternative to a typed store.
- **JSON-Schema-validated I/O** — deferred, additive: a `schema` field can extend the type set later without migration.

## Consequences

Parse failures surface once, at the boundary, with a precise location — never mid-run at a comparison site, and never differently depending on how a value entered (GH Actions' worst contract bug is the same input arriving typed or stringly by invocation path). The checkpoint serde, binding resolution, and condition evaluation migrate to `Value`; the external override API keeps accepting strings, parsed against declared types at run start.

## Evidence

- Loop aggregation returning escaped JSON strings instead of parsed structure is a recurring open complaint class in Argo.
  Provenance: [docs/sources/prior-art-workflow-engines.md](../sources/prior-art-workflow-engines.md) §4 (argo-workflows #12812, #13510); 2026-08-15.
