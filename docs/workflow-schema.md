# Workflow Schema Reference

SilverBond workflows are versioned node/edge graph documents. The canonical schema version is `4`
(`WORKFLOW_SCHEMA_VERSION` in `src/model.rs`). Versions `2` and `3` are accepted as migration
inputs and normalize forward at ingest. A future version `5` is decided in
[ADR-260815-2009-01](adr/260815-2009-typed-workflow-contracts.md) and delivered by the
`typed-contracts` epic; this document describes the present engine only.

## Document grammar

A workflow document deserializes to `WorkflowV3` (`src/model.rs`). It is a JSON object whose wire
keys use camelCase (`rename_all = "camelCase"` on the struct).

### Required top-level keys

Only two top-level keys are genuinely required at deserialization time:

- `version` (`WorkflowV3::version`, `src/model.rs`) — must be present as a JSON non-negative
  integer (`Value::as_u64`, `migrate_workflow_value_to_v4`, `src/model.rs`). A missing `version`,
  or a value that is not a non-negative integer (for example a string `"4"`, a float, or a
  negative number), is a hard ingest error with message `workflow version is required` — there is
  no default.
- `entryNodeId` (`WorkflowV3::entry_node_id`, `src/model.rs`) — the id of the first node to
  execute.

Every other top-level field carries a serde default or is optional, so an omitted key deserializes
to an empty value rather than failing load.

### Unknown keys

No struct in `src/` uses `deny_unknown_fields` (verify with `rg 'deny_unknown_fields' src/`). An
additional unknown key, or a misspelled name on an optional or defaulted field, is silently
ignored on ingest. Misspelling a required field (`version`, `entryNodeId`, a node's `id`, `name`,
or `kind`, an edge's `id`, `from`, `to`, or `outcome`, or a `NodeKind` tag key) still fails
deserialization. A document that saves cleanly is therefore not evidence that every field name was
correct.

### Node shape

Each entry in `nodes` deserializes to `WorkflowNode` (`src/model.rs`). A node always carries
`id`, `name`, and a required nested `kind` object. Execution fields common across kinds (`agent`,
`prompt`, `contextSources`, `responseFormat`, `outputSchema`, retry and loop fields,
`splitFailurePolicy`, `cwd`, `continueSessionFrom`, and others) sit on the node object beside
`kind`
(`WorkflowNode`, `src/model.rs`).

`kind` is an internally-tagged union (`NodeKind`, `src/model.rs`): the tag key is `type`, and its
value selects both what the node does and which config object, if any, is carried inside `kind`.
Variant-specific config (`agentConfig` on `task` and `run_agent`, `decideConfig`, `batchConfig`,
`subflowConfig`, `spawnConfig`, and the other `*Config` keys) lives inside `kind`, not on the node
root. For example, a task node nests agent overrides under `kind.agentConfig`
(`NodeKind::Task`, `src/model.rs`); the canonical serialized v4 shape keeps variant config inside
`kind`. On a canonical v4 document that already has `kind`, a root-level `agentConfig` is
silently ignored and the override is lost (see **Unknown keys** above).

```json
{
  "id": "research",
  "name": "Research Phase",
  "kind": {
    "type": "task",
    "agentConfig": {
      "model": "claude-opus-4"
    }
  },
  "agent": "claude",
  "prompt": "Research {{var:topic}} and provide a summary."
}
```

The fourteen `type` values and their config shapes will be listed in the generated node catalog
below once the catalog is populated; until then `NodeKind` (`src/model.rs`) is the authority.
`/api/capabilities` publishes the same wire tags as `supportedNodeTypes` (`src/api.rs`).

### Version and migration contract

Ingest runs `normalize_workflow_value` (`src/model.rs`), which calls `migrate_workflow_value_to_v4`
before deserialization. This path owns version acceptance and structural migration; it is separate
from `validate_workflow`, which emits `ValidationIssue` values.

**Missing or unsupported `version`.** If `version` is absent, or present but not a JSON
non-negative integer (`Value::as_u64`, `migrate_workflow_value_to_v4`, `src/model.rs`), ingest
fails with `workflow version is required` — there is no default version. If `version` is a
non-negative integer outside the accepted set, ingest fails with `Only workflow versions 2, 3, and
4 are supported. Received version N.` (`anyhow::bail!` in `migrate_workflow_value_to_v4`,
`src/model.rs`). Both paths are ingest errors; neither becomes a `ValidationIssue` and neither
reaches the validation-issue channel exposed by run creation or the validation endpoint. Saving
a workflow does not validate (`save_workflow`, `src/api.rs`).

**Canonical rewrite.** On every path that reaches validation, `version` is force-rewritten to the
canonical value: `migrate_workflow_value_to_v4` sets `version` to `WORKFLOW_SCHEMA_VERSION` after
migration (`src/model.rs`), and `ensure_defaults` assigns `workflow.version = WORKFLOW_SCHEMA_VERSION`
again at the start of `validate_workflow` (`src/model.rs`). Validation therefore cannot reject a
document for its version number.

**Subflow catalog.** Entries in the root `subflows` map (`WorkflowV3::subflows`, `src/model.rs`)
are migrated recursively — each nested body is passed through `migrate_workflow_value_to_v4`
(`src/model.rs`). A subflow whose declared version is an accepted legacy or current version migrates
forward with its root; only a subflow whose `version` the ingest path does not accept at all rejects
the whole document.

**Flat-to-nested `kind` migration.** In `migrate_workflow_value_to_v4` (`src/model.rs`), the
declared `version` is read and matched before any node migration runs; only after that match
accepts the document does `migrate_v2_nodes_to_v3_kind` (`src/model.rs`) run. That call is not
gated on legacy versions — it runs for every accepted declared version, including a document that
already declares version `4`, not only versions `2` or `3`. `migrate_v2_nodes_to_v3_kind` walks the
`nodes` array and calls `migrate_v2_node_to_v3_kind` (`src/model.rs`) only when a node object has
a top-level `type` key and no `kind` key; nodes that already carry `kind` are skipped entirely.

`migrate_v2_node_to_v3_kind` (`src/model.rs`) removes the top-level `type` string and builds a new
`kind` object whose `type` field carries that tag. A `match` on the tag moves recognized config
fields from the node root into `kind` — for example the `"task"` arm calls `move_v2_config_field`
for `agentConfig`, the `"decide"` arm for `decideConfig`, and so on (`migrate_v2_node_to_v3_kind`,
`src/model.rs`). After that `match`, a loop over a fixed array of eleven literal root key names —
`agentConfig`, `decideConfig`, `batchConfig`, `parallelBatchConfig`, `subflowConfig`, `spawnConfig`,
`sendConfig`, `waitConfig`, `captureConfig`, `killConfig`, `runAgentConfig` — calls `node.remove`
on each (`migrate_v2_node_to_v3_kind`, `src/model.rs`); any config object left at the node root
because its key was not moved for the node's `type` is discarded without error. An unrecognized
`type` string hits the `_ => {}` catch-all (`migrate_v2_node_to_v3_kind`, `src/model.rs`), leaving
`kind` with only the tag, which then fails `NodeKind` deserialization.

Legacy `outputSchema` shorthand is rewritten only when the declared document version is below `4` —
`if version < WORKFLOW_SCHEMA_VERSION as u64` gates the call to
`migrate_legacy_output_schemas_in_workflow` (`migrate_workflow_value_to_v4`, `src/model.rs`),
unlike the flat-to-nested `kind` migration above.

On database init, `upgrade_run_workflow_json` (`src/storage.rs`) attempts to re-normalize stored
run snapshots whose `workflow_json` is not version `4`. Valid supported legacy snapshots are
rewritten; rows with invalid JSON, or normalization failures (unsupported versions or other ingest
errors), are skipped with a `tracing::warn!` and left byte-unchanged.

## Node catalog

The node-catalog generator (ISSUE-260826-0637-04) will emit one readable **fragment** block and
one complete **workflow** block per node kind. Block IDs will follow
`node-catalog:<wire-tag>:fragment` and `node-catalog:<wire-tag>:workflow`, where `<wire-tag>` is
the node's `kind.type` value (`WorkflowNodeType::as_str`, `src/model.rs`).

### task

<!-- BEGIN GENERATED: node-catalog:task:fragment -->
<!-- END GENERATED: node-catalog:task:fragment -->

<!-- BEGIN GENERATED: node-catalog:task:workflow -->
<!-- END GENERATED: node-catalog:task:workflow -->

### approval

<!-- BEGIN GENERATED: node-catalog:approval:fragment -->
<!-- END GENERATED: node-catalog:approval:fragment -->

<!-- BEGIN GENERATED: node-catalog:approval:workflow -->
<!-- END GENERATED: node-catalog:approval:workflow -->

### split

<!-- BEGIN GENERATED: node-catalog:split:fragment -->
<!-- END GENERATED: node-catalog:split:fragment -->

<!-- BEGIN GENERATED: node-catalog:split:workflow -->
<!-- END GENERATED: node-catalog:split:workflow -->

### collector

<!-- BEGIN GENERATED: node-catalog:collector:fragment -->
<!-- END GENERATED: node-catalog:collector:fragment -->

<!-- BEGIN GENERATED: node-catalog:collector:workflow -->
<!-- END GENERATED: node-catalog:collector:workflow -->

### decide

<!-- BEGIN GENERATED: node-catalog:decide:fragment -->
<!-- END GENERATED: node-catalog:decide:fragment -->

<!-- BEGIN GENERATED: node-catalog:decide:workflow -->
<!-- END GENERATED: node-catalog:decide:workflow -->

### parallel_batch

<!-- BEGIN GENERATED: node-catalog:parallel_batch:fragment -->
<!-- END GENERATED: node-catalog:parallel_batch:fragment -->

<!-- BEGIN GENERATED: node-catalog:parallel_batch:workflow -->
<!-- END GENERATED: node-catalog:parallel_batch:workflow -->

### subflow

<!-- BEGIN GENERATED: node-catalog:subflow:fragment -->
<!-- END GENERATED: node-catalog:subflow:fragment -->

<!-- BEGIN GENERATED: node-catalog:subflow:workflow -->
<!-- END GENERATED: node-catalog:subflow:workflow -->

### call

<!-- BEGIN GENERATED: node-catalog:call:fragment -->
<!-- END GENERATED: node-catalog:call:fragment -->

<!-- BEGIN GENERATED: node-catalog:call:workflow -->
<!-- END GENERATED: node-catalog:call:workflow -->

### spawn

<!-- BEGIN GENERATED: node-catalog:spawn:fragment -->
<!-- END GENERATED: node-catalog:spawn:fragment -->

<!-- BEGIN GENERATED: node-catalog:spawn:workflow -->
<!-- END GENERATED: node-catalog:spawn:workflow -->

### send

<!-- BEGIN GENERATED: node-catalog:send:fragment -->
<!-- END GENERATED: node-catalog:send:fragment -->

<!-- BEGIN GENERATED: node-catalog:send:workflow -->
<!-- END GENERATED: node-catalog:send:workflow -->

### wait

<!-- BEGIN GENERATED: node-catalog:wait:fragment -->
<!-- END GENERATED: node-catalog:wait:fragment -->

<!-- BEGIN GENERATED: node-catalog:wait:workflow -->
<!-- END GENERATED: node-catalog:wait:workflow -->

### capture

<!-- BEGIN GENERATED: node-catalog:capture:fragment -->
<!-- END GENERATED: node-catalog:capture:fragment -->

<!-- BEGIN GENERATED: node-catalog:capture:workflow -->
<!-- END GENERATED: node-catalog:capture:workflow -->

### kill

<!-- BEGIN GENERATED: node-catalog:kill:fragment -->
<!-- END GENERATED: node-catalog:kill:fragment -->

<!-- BEGIN GENERATED: node-catalog:kill:workflow -->
<!-- END GENERATED: node-catalog:kill:workflow -->

### run_agent

<!-- BEGIN GENERATED: node-catalog:run_agent:fragment -->
<!-- END GENERATED: node-catalog:run_agent:fragment -->

<!-- BEGIN GENERATED: node-catalog:run_agent:workflow -->
<!-- END GENERATED: node-catalog:run_agent:workflow -->

## Node fields

Per-field tables for `WorkflowNode` and each `NodeKind` config shape are deferred to a later
pass; `src/model.rs` is the authority until then.

## Edges and conditions

Edge, outcome, and condition field tables are deferred to a later pass; `src/model.rs` (`WorkflowEdge`,
`WorkflowEdgeOutcome`, `StructuredCondition`) is the authority until then.

## Document-level fields

Top-level `WorkflowV3` field tables are deferred to a later pass; `src/model.rs` is the authority
until then.

## Templates and agent config

Template-token and agent-configuration tables are deferred to a later pass; `src/model.rs` and
`src/runtime.rs` (`resolve_template_vars`) are the authority until then.

## Validation catalog

The validation-issue catalog is deferred to a later pass; `validate_workflow` and `ValidationIssue`
in `src/model.rs` are the authority until then.

## Regenerating this document

`src/model.rs` is the schema authority. The **Node catalog** section is generated: each block
between a matched HTML comment pair (block id in the comment) will be rewritten by the catalog
generator; all other sections are hand-written prose cross-referenced to the source. The per-kind
`### <wire-tag>` headings in **Node catalog** are hand-maintained and must be updated when the
`NodeKind` variant set changes.

ISSUE-260826-0637-04 will supply the catalog generator and the refresh command:

```bash
just regen-docs
```

That recipe will wrap `SB_REGEN_DOCS=1` around the catalog generator test. Once the generator
lands, `cargo test` will regenerate in memory and assert the committed markdown matches — it will
not rewrite tracked files.
