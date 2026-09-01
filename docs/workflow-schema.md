# Workflow Schema Reference

SilverBond workflows are versioned node/edge graph documents. The canonical schema version is `4`
(`WORKFLOW_SCHEMA_VERSION`, `src/model.rs`). Versions `2` and `3` are accepted as migration
inputs and normalize forward at ingest. A future version `5` is decided in
[ADR-260815-2009-01](adr/260815-2009-typed-workflow-contracts.md) and delivered by the
`typed-contracts` epic; this document describes the present engine only.

## Document grammar

A workflow document deserializes to `WorkflowV3` (`src/model.rs`). It is a JSON object whose wire
keys use camelCase (`rename_all = "camelCase"` on the struct).

### Required top-level keys

Only two top-level keys are genuinely required at deserialization time:

- `version` (`WorkflowV3::version`, `src/model.rs`) — must be present as a JSON non-negative
  integer (`migrate_workflow_value_to_v4`, `src/model.rs`, `Value::as_u64`). A missing `version`,
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

The fourteen `type` values and their config shapes are listed in the generated node catalog
below; `NodeKind` (`src/model.rs`) remains the schema authority for fields not shown in an
example. `/api/capabilities` publishes those tags under the `supportedNodeTypes` wire key, using
`workflow_node_type_tags` (`src/serde_wire_tags.rs`).

### Version and migration contract

Ingest runs `normalize_workflow_value` (`src/model.rs`), which calls `migrate_workflow_value_to_v4`
before deserialization. This path owns version acceptance and structural migration; it is separate
from `validate_workflow`, which emits `ValidationIssue` values.

**Missing or unsupported `version`.** If `version` is absent, or present but not a JSON
non-negative integer (`migrate_workflow_value_to_v4`, `src/model.rs`, `workflow version is required`), ingest
fails with `workflow version is required` — there is no default version. If `version` is a
non-negative integer outside the accepted set, ingest fails with `Only workflow versions 2, 3, and
4 are supported. Received version N.` (`migrate_workflow_value_to_v4`, `src/model.rs`, `Only workflow versions`). Both paths are ingest errors; neither becomes a `ValidationIssue` and neither
reaches the validation-issue channel exposed by run creation or the validation endpoint. Saving
a workflow does not validate (`save_workflow`, `src/api.rs`).

**Canonical rewrite.** On every path that reaches validation, `version` is force-rewritten to the
canonical value: `migrate_workflow_value_to_v4` sets `version` to `WORKFLOW_SCHEMA_VERSION` after
migration (`migrate_workflow_value_to_v4`, `src/model.rs`, `workflow.insert`), and `ensure_defaults` assigns `workflow.version = WORKFLOW_SCHEMA_VERSION`
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

The node-catalog generator emits one readable **fragment** block and one complete **workflow**
block per node kind. Block IDs follow `node-catalog:<wire-tag>:fragment` and
`node-catalog:<wire-tag>:workflow`, where `<wire-tag>` is the node's `kind.type` value
(`WorkflowNodeType::as_str`, `src/model.rs`).

### task

<!-- BEGIN GENERATED: node-catalog:task:fragment -->
```json
{
  "agent": "claude",
  "id": "example",
  "kind": {
    "type": "task"
  },
  "name": "Task",
  "prompt": "Complete the step."
}
```
<!-- END GENERATED: node-catalog:task:fragment -->

<!-- BEGIN GENERATED: node-catalog:task:workflow -->
```json
{
  "cwd": "",
  "edges": [],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "agent": "claude",
      "id": "example",
      "kind": {
        "type": "task"
      },
      "name": "Task",
      "prompt": "Complete the step."
    }
  ],
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:task:workflow -->

### approval

<!-- BEGIN GENERATED: node-catalog:approval:fragment -->
```json
{
  "id": "example",
  "kind": {
    "type": "approval"
  },
  "name": "Approval",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:approval:fragment -->

<!-- BEGIN GENERATED: node-catalog:approval:workflow -->
```json
{
  "cwd": "",
  "edges": [],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "example",
      "kind": {
        "type": "approval"
      },
      "name": "Approval",
      "prompt": ""
    }
  ],
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:approval:workflow -->

### split

<!-- BEGIN GENERATED: node-catalog:split:fragment -->
```json
{
  "id": "example",
  "kind": {
    "type": "split"
  },
  "name": "Split",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:split:fragment -->

<!-- BEGIN GENERATED: node-catalog:split:workflow -->
```json
{
  "cwd": "",
  "edges": [
    {
      "from": "example",
      "id": "split_a",
      "outcome": "success",
      "to": "branch_a"
    },
    {
      "from": "example",
      "id": "split_b",
      "outcome": "success",
      "to": "branch_b"
    }
  ],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "example",
      "kind": {
        "type": "split"
      },
      "name": "Split",
      "prompt": ""
    },
    {
      "agent": "claude",
      "id": "branch_a",
      "kind": {
        "type": "task"
      },
      "name": "Branch A",
      "prompt": "Complete the step."
    },
    {
      "agent": "claude",
      "id": "branch_b",
      "kind": {
        "type": "task"
      },
      "name": "Branch B",
      "prompt": "Complete the step."
    }
  ],
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:split:workflow -->

### collector

<!-- BEGIN GENERATED: node-catalog:collector:fragment -->
```json
{
  "id": "example",
  "kind": {
    "type": "collector"
  },
  "name": "Collector",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:collector:fragment -->

<!-- BEGIN GENERATED: node-catalog:collector:workflow -->
```json
{
  "cwd": "",
  "edges": [
    {
      "from": "split",
      "id": "split_a",
      "outcome": "success",
      "to": "branch_a"
    },
    {
      "from": "split",
      "id": "split_b",
      "outcome": "success",
      "to": "branch_b"
    },
    {
      "from": "branch_a",
      "id": "a_collect",
      "label": "a",
      "outcome": "success",
      "to": "example"
    },
    {
      "from": "branch_b",
      "id": "b_collect",
      "label": "b",
      "outcome": "success",
      "to": "example"
    },
    {
      "from": "example",
      "id": "collect_after",
      "outcome": "success",
      "to": "after"
    }
  ],
  "entryNodeId": "split",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "split",
      "kind": {
        "type": "split"
      },
      "name": "Split",
      "prompt": ""
    },
    {
      "agent": "claude",
      "id": "branch_a",
      "kind": {
        "type": "task"
      },
      "name": "Branch A",
      "prompt": "Complete the step."
    },
    {
      "agent": "claude",
      "id": "branch_b",
      "kind": {
        "type": "task"
      },
      "name": "Branch B",
      "prompt": "Complete the step."
    },
    {
      "id": "example",
      "kind": {
        "type": "collector"
      },
      "name": "Collector",
      "prompt": ""
    },
    {
      "agent": "claude",
      "id": "after",
      "kind": {
        "type": "task"
      },
      "name": "After",
      "prompt": "Complete the step."
    }
  ],
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:collector:workflow -->

### decide

<!-- BEGIN GENERATED: node-catalog:decide:fragment -->
```json
{
  "id": "example",
  "kind": {
    "decideConfig": {
      "outcomes": [
        "yes",
        "no"
      ],
      "prompt": "Choose a path"
    },
    "type": "decide"
  },
  "name": "Decide",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:decide:fragment -->

<!-- BEGIN GENERATED: node-catalog:decide:workflow -->
```json
{
  "cwd": "",
  "edges": [
    {
      "from": "example",
      "id": "decide_yes",
      "label": "yes",
      "outcome": "branch",
      "to": "yes_node"
    },
    {
      "from": "example",
      "id": "decide_no",
      "label": "no",
      "outcome": "branch",
      "to": "no_node"
    }
  ],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "example",
      "kind": {
        "decideConfig": {
          "outcomes": [
            "yes",
            "no"
          ],
          "prompt": "Choose a path"
        },
        "type": "decide"
      },
      "name": "Decide",
      "prompt": ""
    },
    {
      "agent": "claude",
      "id": "yes_node",
      "kind": {
        "type": "task"
      },
      "name": "Yes",
      "prompt": "Complete the step."
    },
    {
      "agent": "claude",
      "id": "no_node",
      "kind": {
        "type": "task"
      },
      "name": "No",
      "prompt": "Complete the step."
    }
  ],
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:decide:workflow -->

### parallel_batch

<!-- BEGIN GENERATED: node-catalog:parallel_batch:fragment -->
```json
{
  "id": "example",
  "kind": {
    "batchConfig": {
      "bodyEntry": "body",
      "itemVar": "item",
      "itemsBinding": "items",
      "maxConcurrent": 4
    },
    "type": "parallel_batch"
  },
  "name": "Batch",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:parallel_batch:fragment -->

<!-- BEGIN GENERATED: node-catalog:parallel_batch:workflow -->
```json
{
  "cwd": "",
  "edges": [],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "example",
      "kind": {
        "batchConfig": {
          "bodyEntry": "body",
          "itemVar": "item",
          "itemsBinding": "items",
          "maxConcurrent": 4
        },
        "type": "parallel_batch"
      },
      "name": "Batch",
      "prompt": ""
    },
    {
      "agent": "claude",
      "id": "body",
      "kind": {
        "type": "task"
      },
      "name": "Body",
      "prompt": "Complete the step."
    }
  ],
  "useOrchestrator": false,
  "variables": [
    {
      "default": "[\"a\",\"b\"]",
      "name": "items"
    }
  ],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:parallel_batch:workflow -->

### subflow

<!-- BEGIN GENERATED: node-catalog:subflow:fragment -->
```json
{
  "id": "example",
  "kind": {
    "subflowConfig": {
      "maxDepth": 10,
      "workflowName": "child"
    },
    "type": "subflow"
  },
  "name": "Subflow",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:subflow:fragment -->

<!-- BEGIN GENERATED: node-catalog:subflow:workflow -->
```json
{
  "cwd": "",
  "edges": [],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "example",
      "kind": {
        "subflowConfig": {
          "maxDepth": 10,
          "workflowName": "child"
        },
        "type": "subflow"
      },
      "name": "Subflow",
      "prompt": ""
    }
  ],
  "subflows": {
    "child": {
      "cwd": "",
      "edges": [],
      "entryNodeId": "exit",
      "goal": "Illustrate a node kind",
      "limits": {
        "maxTotalSteps": 0,
        "maxVisitsPerNode": 0
      },
      "name": "child",
      "nodes": [
        {
          "agent": "claude",
          "id": "exit",
          "kind": {
            "type": "task"
          },
          "name": "Exit",
          "prompt": "Complete the step."
        }
      ],
      "useOrchestrator": false,
      "variables": [],
      "version": 4
    }
  },
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:subflow:workflow -->

### call

<!-- BEGIN GENERATED: node-catalog:call:fragment -->
```json
{
  "id": "example",
  "kind": {
    "subflowConfig": {
      "maxDepth": 10,
      "workflowName": "child"
    },
    "type": "call"
  },
  "name": "Call",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:call:fragment -->

<!-- BEGIN GENERATED: node-catalog:call:workflow -->
```json
{
  "cwd": "",
  "edges": [],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "example",
      "kind": {
        "subflowConfig": {
          "maxDepth": 10,
          "workflowName": "child"
        },
        "type": "call"
      },
      "name": "Call",
      "prompt": ""
    }
  ],
  "subflows": {
    "child": {
      "cwd": "",
      "edges": [],
      "entryNodeId": "exit",
      "goal": "Illustrate a node kind",
      "limits": {
        "maxTotalSteps": 0,
        "maxVisitsPerNode": 0
      },
      "name": "child",
      "nodes": [
        {
          "agent": "claude",
          "id": "exit",
          "kind": {
            "type": "task"
          },
          "name": "Exit",
          "prompt": "Complete the step."
        }
      ],
      "useOrchestrator": false,
      "variables": [],
      "version": 4
    }
  },
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:call:workflow -->

### spawn

<!-- BEGIN GENERATED: node-catalog:spawn:fragment -->
```json
{
  "id": "example",
  "kind": {
    "spawnConfig": {
      "agent": "claude",
      "sessionName": "catalog-spawn"
    },
    "type": "spawn"
  },
  "name": "spawn",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:spawn:fragment -->

<!-- BEGIN GENERATED: node-catalog:spawn:workflow -->
```json
{
  "cwd": "",
  "edges": [],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "example",
      "kind": {
        "spawnConfig": {
          "agent": "claude",
          "sessionName": "catalog-spawn"
        },
        "type": "spawn"
      },
      "name": "spawn",
      "prompt": ""
    }
  ],
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:spawn:workflow -->

### send

<!-- BEGIN GENERATED: node-catalog:send:fragment -->
```json
{
  "id": "example",
  "kind": {
    "sendConfig": {
      "enter": true,
      "target": "catalog-session",
      "text": "hello"
    },
    "type": "send"
  },
  "name": "send",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:send:fragment -->

<!-- BEGIN GENERATED: node-catalog:send:workflow -->
```json
{
  "cwd": "",
  "edges": [],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "example",
      "kind": {
        "sendConfig": {
          "enter": true,
          "target": "catalog-session",
          "text": "hello"
        },
        "type": "send"
      },
      "name": "send",
      "prompt": ""
    }
  ],
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:send:workflow -->

### wait

<!-- BEGIN GENERATED: node-catalog:wait:fragment -->
```json
{
  "id": "example",
  "kind": {
    "type": "wait",
    "waitConfig": {
      "mode": "idle",
      "target": "catalog-session"
    }
  },
  "name": "wait",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:wait:fragment -->

<!-- BEGIN GENERATED: node-catalog:wait:workflow -->
```json
{
  "cwd": "",
  "edges": [],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "example",
      "kind": {
        "type": "wait",
        "waitConfig": {
          "mode": "idle",
          "target": "catalog-session"
        }
      },
      "name": "wait",
      "prompt": ""
    }
  ],
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:wait:workflow -->

### capture

<!-- BEGIN GENERATED: node-catalog:capture:fragment -->
```json
{
  "id": "example",
  "kind": {
    "captureConfig": {
      "all": false,
      "ansi": true,
      "lines": 50,
      "target": "catalog-session"
    },
    "type": "capture"
  },
  "name": "capture",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:capture:fragment -->

<!-- BEGIN GENERATED: node-catalog:capture:workflow -->
```json
{
  "cwd": "",
  "edges": [],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "example",
      "kind": {
        "captureConfig": {
          "all": false,
          "ansi": true,
          "lines": 50,
          "target": "catalog-session"
        },
        "type": "capture"
      },
      "name": "capture",
      "prompt": ""
    }
  ],
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:capture:workflow -->

### kill

<!-- BEGIN GENERATED: node-catalog:kill:fragment -->
```json
{
  "id": "example",
  "kind": {
    "killConfig": {
      "target": "catalog-session"
    },
    "type": "kill"
  },
  "name": "kill",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:kill:fragment -->

<!-- BEGIN GENERATED: node-catalog:kill:workflow -->
```json
{
  "cwd": "",
  "edges": [],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "example",
      "kind": {
        "killConfig": {
          "target": "catalog-session"
        },
        "type": "kill"
      },
      "name": "kill",
      "prompt": ""
    }
  ],
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:kill:workflow -->

### run_agent

<!-- BEGIN GENERATED: node-catalog:run_agent:fragment -->
```json
{
  "id": "example",
  "kind": {
    "runAgentConfig": {
      "agent": "claude",
      "killAfter": true,
      "prompt": "Run a short command"
    },
    "type": "run_agent"
  },
  "name": "run_agent",
  "prompt": ""
}
```
<!-- END GENERATED: node-catalog:run_agent:fragment -->

<!-- BEGIN GENERATED: node-catalog:run_agent:workflow -->
```json
{
  "cwd": "",
  "edges": [],
  "entryNodeId": "example",
  "goal": "Illustrate a node kind",
  "limits": {
    "maxTotalSteps": 0,
    "maxVisitsPerNode": 0
  },
  "name": "node-catalog-example",
  "nodes": [
    {
      "id": "example",
      "kind": {
        "runAgentConfig": {
          "agent": "claude",
          "killAfter": true,
          "prompt": "Run a short command"
        },
        "type": "run_agent"
      },
      "name": "run_agent",
      "prompt": ""
    }
  ],
  "useOrchestrator": false,
  "variables": [],
  "version": 4
}
```
<!-- END GENERATED: node-catalog:run_agent:workflow -->

## Node fields

### Common node fields

Every node deserializes to `WorkflowNode` (`src/model.rs`). The table below lists every field on
that struct, including the required nested `kind` object. Requiredness reflects serde attributes, not
runtime validation — a field marked optional may still trigger a validation error for a specific
kind.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `id` | string | Yes | — | Node identifier; must be unique within the workflow. |
| `name` | string | Yes | — | Display name. |
| `kind` | object | Yes | — | Internally-tagged `NodeKind` union (`type` tag plus variant config). See **Per-kind config fields** and the generated **Node catalog**. |
| `agent` | string or absent | No | absent | Carries `#[serde(default)]` but no default *value* — an omitted key deserializes to absent, not to an agent name. A `task` or `run_agent` node with no agent is a hard validation error at run start; the runtime `DEFAULT_AGENT` fallback never rescues a document. |
| `prompt` | string | No | `""` | An empty prompt is only a warning for `task` nodes. |
| `contextSources` | array of `{name, nodeId}` | No | `[]` | `ContextSource` entries (`ContextSource`, `src/model.rs`); `nodeId` is the referenced node identifier. Registers `{{context:<name>}}` substitutions; does not inject text into the prompt. |
| `responseFormat` | `"text"` \| `"json"` or absent | No | absent | |
| `outputSchema` | JSON value or absent | No | absent | Raw, unvalidated JSON value (`deserialize_output_schema`, `src/model.rs`, `Option::<Value>::deserialize`). Legacy `{field: type}` shorthand is converted only during v2/v3→v4 migration (`migrate_workflow_value_to_v4`, `src/model.rs`, `migrate_legacy_output_schemas_in_workflow`). Setting it while `responseFormat != "json"` produces a warning (`validate_graph_body`, `src/model.rs`, `output schema but responseFormat is not json`). |
| `retryCount` | non-negative integer or absent | No | absent | Treated as `0` when absent. Values above 10 are rejected (`MAX_NODE_RETRY_COUNT`, `src/model.rs`) with a hard validation error that short-circuits the whole validation pass. |
| `retryDelay` | non-negative integer or absent | No | absent | Delay in seconds between retries. Runtime uses `2` seconds when unset (`run_cursor_task`, `src/runtime.rs`, `from_secs`). |
| `timeout` | non-negative integer or absent | No | absent | Per-node execution timeout in seconds. |
| `skipCondition` | object or absent | No | absent | Pre-execution guard; shape is documented in **Templates and agent config**. |
| `loopMaxIterations` | non-negative integer or absent | No | absent | Runtime default is `5` when unset (`select_next_decision`, `src/runtime.rs`, `unwrap_or(5)`). Reaching the cap without a `loop_exit` edge fails the run. |
| `loopCondition` | `{field, operator, value}` or absent | No | absent | `StructuredCondition` (`src/model.rs`) — a flat deterministic leaf evaluated by the engine, never by an LLM. Not a prompt string. |
| `splitFailurePolicy` | `"best_effort_continue"` \| `"fail_fast_cancel"` \| `"drain_then_fail"` | No | `best_effort_continue` | Universal node field (`WorkflowNode::split_failure_policy`, `src/model.rs`). Explicit JSON `null` deserializes to `best_effort_continue` via a custom deserializer. |
| `cwd` | string or absent | No | absent | Validated as absolute when set (`validate_graph_body`, `src/model.rs`). At execution time it is read only by `spawn` nodes (`execute_spawn`, `src/tmux_exec.rs`, `.or(node.cwd.as_deref())`); at validation time it also feeds `continueSessionFrom`'s matching-working-directory check (`validate_graph_body`, `src/model.rs`, `working_directory_for_node`). `task` and `run_agent` execution ignores it: a `task` node runs in the scoped workflow cwd (`execute_workflow`, `src/runtime.rs`, `scoped_workflow_cwd`), and a `run_agent` node uses `runAgentConfig.cwd` when set, falling back to that same scoped workflow cwd (`run_agent_interactive`, `src/tmux_exec.rs`). |
| `continueSessionFrom` | string (node id) or absent | No | absent | Adopts an agent session from another node. Validation enforces five constraints: the source node must exist; its kind must be `task` or `run_agent`; resolved agents must match; the source's access profile must be no broader than the target's; resolved working directories must match. |

*Source: `WorkflowNode` (`WorkflowNode`, `src/model.rs`).*

### Per-kind config fields

Subsections follow the **Node catalog** order. Wire keys use camelCase (`rename_all = "camelCase"`
on each config struct). Config *objects* omitted from serialized output when default-valued are
noted; individual fields are additionally skipped when default-valued — see the struct's serde
attributes. Requiredness reflects serde attributes, not runtime validation — a field marked
optional may still trigger a validation error for a specific kind. An absent key and an explicit
default deserialize the same.

#### task

The only in-`kind` payload is `agentConfig` (agent overrides). That object is documented in
**Templates and agent config**.

*Source: `NodeKind::Task` (`NodeKind::Task`, `src/model.rs`).*

#### approval

Carries no config object — `approval` is a bare unit variant (`NodeKind::Approval`,
`src/model.rs`).

#### split

Carries no config object — `split` is a bare unit variant (`NodeKind::Split`, `src/model.rs`).

#### collector

Carries no config object — `collector` is a bare unit variant (`NodeKind::Collector`, `src/model.rs`).

#### decide

Config key: `decideConfig` (inside `kind`).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `inputs` | array of `{name, source}` | No | `[]` | Input bindings for the decide prompt. |
| `prompt` | string | No | `""` | |
| `model` | string or absent | No | absent | When absent, the runtime resolves `"claude-haiku-4-5"` (`default_decide_model`, `src/model.rs`). |
| `outcomes` | array of strings | No | `[]` | Branch labels; each needs a matching `branch` edge. |

*Source: `DecideConfig` (`DecideConfig`, `src/model.rs`).*

#### parallel_batch

Config key: `batchConfig` inside `kind`. The wrapper also accepts the legacy alias
`parallelBatchConfig` (`NodeKind::ParallelBatch`, `src/model.rs`).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `itemsBinding` | string | Yes* | `""` if `batchConfig` omitted | Variable name holding the array to iterate. |
| `maxConcurrent` | non-negative integer | No | `4` | Clamped to `1`…`MAX_PARALLEL_BATCH_CONCURRENT` (32) at runtime (`handle_parallel_batch_node`, `src/runtime.rs`, `MAX_PARALLEL_BATCH_CONCURRENT`). |
| `itemVar` | string | Yes* | `""` if `batchConfig` omitted | Loop variable name for each item. |
| `bodyEntry` | string | Yes* | `""` if `batchConfig` omitted | Node id of the batch body entry point. |
| `collectorVar` | string or absent | No | absent | Optional variable to collect body outputs. |

\*When `batchConfig` is present, `itemsBinding`, `itemVar`, and `bodyEntry` have no per-field
serde default and must be supplied. When the whole `batchConfig` object is omitted, the struct's
`Default` supplies empty strings.

*Source: `BatchConfig` (`BatchConfig`, `src/model.rs`), wrapper alias (`NodeKind::ParallelBatch`, `src/model.rs`).*

#### subflow

Config key: `subflowConfig` (inside `kind`).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `workflowName` | string | No | `""` | Also accepted as `workflow` or `subflowName` (`SubflowConfig`, `src/model.rs`, `subflowName`). |
| `exitNodeId` | string or absent | No | absent | Optional early-exit node inside the subflow body. |
| `inputs` | array of `{name, source}` | No | `[]` | Bindings passed into the subflow body. |
| `maxDepth` | non-negative integer | No | `10` | Nesting depth cap (`default_max_call_depth`, `src/model.rs`). |

*Source: `SubflowConfig` (`SubflowConfig`, `src/model.rs`), `NodeKind::Subflow` (`NodeKind::Subflow`, `src/model.rs`).*

#### call

Config key: `subflowConfig` (inside `kind`) — same struct as `subflow`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `workflowName` | string | No | `""` | Also accepted as `workflow` or `subflowName` (`SubflowConfig`, `src/model.rs`, `subflowName`). |
| `exitNodeId` | string or absent | No | absent | Optional early-exit node inside the called workflow body. |
| `inputs` | array of `{name, source}` | No | `[]` | Bindings passed into the called workflow. |
| `maxDepth` | non-negative integer | No | `10` | Nesting depth cap (`default_max_call_depth`, `src/model.rs`). |

*Source: `SubflowConfig` (`SubflowConfig`, `src/model.rs`), `NodeKind::Call` (`NodeKind::Call`, `src/model.rs`).*

#### spawn

Config key: `spawnConfig` (inside `kind`). The whole object is skipped on serialize when equal to
`SpawnConfig::default()`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `agent` | string or absent | No | absent | Agent driver to spawn. Falls back to node-level `agent` when absent (`explicit_agent_for_node`, `src/model.rs`). |
| `command` | string or absent | No | absent | Shell command alternative to an agent launch. |
| `access` | string or absent | No | absent | Named access profile from the Agents Registry; distinct from `accessMode` on agent defaults. |
| `extraArgs` | array of strings | No | `[]` | |
| `cwd` | string or absent | No | absent | Must be absolute when set. Falls back to node-level `cwd`, then workflow cwd, when absent (`execute_spawn`, `src/tmux_exec.rs`, `let cwd = cfg`). |
| `name` | string or absent | No | absent | Pane display name. |
| `sessionName` | string or absent | No | absent | Tmux session name. |

*Source: `SpawnConfig` (`SpawnConfig`, `src/model.rs`).*

#### send

Config key: `sendConfig` (inside `kind`). The whole object is skipped on serialize when equal to
`SendConfig::default()`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `target` | string or absent | No | absent | Session or pane target. |
| `text` | string | No | `""` | Text to send. |
| `enter` | boolean | No | `true` | Press Enter after sending. |

*Source: `SendConfig` (`SendConfig`, `src/model.rs`).*

#### wait

Config key: `waitConfig` (inside `kind`). The whole object is skipped on serialize when equal to
`WaitConfig::default()`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `target` | string or absent | No | absent | Session or pane target. |
| `mode` | `"idle"` \| `"ready"` \| `"until"` | No | `idle` | `WaitMode` (`src/model.rs`). |
| `marker` | string or absent | No | absent | Marker string for `until` mode. |
| `timeout` | non-negative integer or absent | No | absent | Timeout in seconds. Falls back to node-level `timeout` when absent (`timeout_for_node`, `src/runtime.rs`). |
| `idleSeconds` | number or absent | No | absent | Idle duration for `idle` mode. Runtime uses `2.0` seconds when unset (`wait_for_mode`, `src/tmux_exec.rs`). |
| `readyStableSeconds` | number or absent | No | absent | Stability window for `ready` mode. Runtime uses `DEFAULT_READY_STABLE_SECONDS` (`2.0`) when unset (`wait_for_mode`, `src/tmux_exec.rs`). |

*Source: `WaitConfig` (`WaitConfig`, `src/model.rs`).*

#### capture

Config key: `captureConfig` (inside `kind`). The whole object is skipped on serialize when equal to
`CaptureConfig::default()`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `target` | string or absent | No | absent | Session or pane target. |
| `lines` | non-negative integer or absent | No | absent | Line count when `all` is false. |
| `all` | boolean | No | `false` | Capture entire scrollback. |
| `ansi` | boolean | No | `false` | Include ANSI escape sequences. |

*Source: `CaptureConfig` (`CaptureConfig`, `src/model.rs`).*

#### kill

Config key: `killConfig` (inside `kind`). The whole object is skipped on serialize when equal to
`KillConfig::default()`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `target` | string or absent | No | absent | Session or pane target. |
| `sessionName` | string or absent | No | absent | Alternative session identifier. |

*Source: `KillConfig` (`KillConfig`, `src/model.rs`).*

#### run_agent

Config keys: `runAgentConfig` and `agentConfig` (both inside `kind`). The `runAgentConfig` object
is skipped on serialize when equal to `RunAgentConfig::default()`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `agent` | string or absent | No | absent | Agent driver to run. Falls back to node-level `agent` when absent (`explicit_agent_for_node`, `src/model.rs`). Validation requires an agent from either this key or node-level `agent` (`validate_graph_body`, `src/model.rs`, `explicit_agent_for_node`). |
| `prompt` | string or absent | No | absent | Prompt text for the agent. Falls back to node-level `prompt` when absent (`validate_graph_body`, `src/model.rs`, `unwrap_or(node.prompt.as_str())`). |
| `cwd` | string or absent | No | absent | Working directory override. Must be absolute when set. Falls back to the active workflow cwd when absent (`run_agent_interactive`, `src/tmux_exec.rs`) (`execute_workflow`, `src/runtime.rs`, `scoped_workflow_cwd`). Node-level `cwd` does not feed this chain at execution time — the config-then-node-then-workflow resolution in `working_directory_for_node` (`working_directory_for_node`, `src/model.rs`) is reached only by `continueSessionFrom` validation (`validate_graph_body`, `src/model.rs`, `working_directory_for_node`). |
| `access` | string or absent | No | absent | Named access profile; distinct from `accessMode` on agent defaults. |
| `extraArgs` | array of strings | No | `[]` | |
| `name` | string or absent | No | absent | Pane display name. |
| `timeout` | non-negative integer or absent | No | absent | Timeout in seconds. Falls back to node-level `timeout` when absent (`timeout_for_node`, `src/runtime.rs`). |
| `idleSeconds` | number or absent | No | absent | Runtime uses `2.0` seconds when unset (`run_agent_interactive`, `src/tmux_exec.rs`). |
| `readyStableSeconds` | number or absent | No | absent | Runtime uses `DEFAULT_READY_STABLE_SECONDS` (`2.0`) when unset (`run_agent_interactive`, `src/tmux_exec.rs`). |
| `until` | string or absent | No | absent | Marker string for ready/until waits. |
| `killAfter` | boolean | No | `true` | Tear down the pane after the agent exits. |
| `agentConfig` | object or absent | No | absent | Agent overrides alongside `runAgentConfig`; documented in **Templates and agent config**. Sibling payload of the variant, not a field of `RunAgentConfig`. |

*Source: `RunAgentConfig` (`RunAgentConfig`, `src/model.rs`), `NodeKind::RunAgent` (`NodeKind::RunAgent`, `src/model.rs`).*

## Edges and conditions

An edge deserializes to `WorkflowEdge` (`src/model.rs`). Wire keys use camelCase
(`rename_all = "camelCase"` on the struct). There are no serde aliases on endpoint fields — the
wire names are `from` and `to`, not `source` or `target`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `id` | string | Yes | — | Edge identifier; must be unique within the workflow. Duplicate ids are a validation error (`validate_graph_body`, `src/model.rs`, `Duplicate edge id`). |
| `from` | string | Yes | — | Source node id. |
| `to` | string | Yes | — | Target node id. |
| `outcome` | `success` \| `reject` \| `branch` \| `loop_continue` \| `loop_exit` | Yes | — | Traversal channel (`WorkflowEdgeOutcome`, `WorkflowEdgeOutcome::Success`, `src/model.rs`). |
| `label` | string or absent | No | absent | Branch label for `branch` edges (matched against decide outcomes) and collector merge keys (see below). |
| `branchId` | string or absent | No | absent | Branch identifier reported as `chosenBranch` on the `branch_decision` event, falling back to the edge `id` when absent (`select_next_decision`, `src/runtime.rs`, `chosenBranch`). |
| `condition` | `{field, operator, value}` or absent | No | absent | Post-execution deterministic guard (`StructuredCondition`, `src/model.rs`). Only `branch`-outcome edges on non-`decide` nodes consult it (`select_next_decision`, `src/runtime.rs`, `is_some_and`); `success`, `reject`, `loop_continue`, and `loop_exit` edges deserialize it but never evaluate it. Validation warns only about `branch` edges (`validate_graph_body`, `src/model.rs`, `deterministic branch conditions`). `decide` nodes return before condition evaluation (`select_next_decision`, `src/runtime.rs`, `decide outcome`). Evaluation runs only when `result.parsed_output` is present (`select_next_decision`, `src/runtime.rs`); `responseFormat: json` produces parsed output for prompt-driven nodes (`parse_structured_output`, `strip_markdown_json_fence`, `src/runtime.rs`), while system-output kinds (`capture`, `parallel_batch`, `decide`, `split`, `collector`) populate it unconditionally. See **Condition leaf**. |

*Source: `WorkflowEdge` (`WorkflowEdge`, `src/model.rs`).*

### Edge outcomes

There is no `failure` outcome in v4 — a node failure always ends its cursor; outside a split
family that fails the whole run. A sixth `failure` outcome is planned in
[ADR-260815-2009-02](adr/260815-2009-single-condition-dialect.md) and is not present behavior.

Saving a workflow does not validate it (`save_workflow`, `src/api.rs`). Graph rules below are
enforced by `validate_workflow` on an explicit validate request and at run start, where
error-severity issues refuse the run. A document with a branch edge leaving a split loads and
saves cleanly and fails when someone tries to run it.

### Split and collector graph rules

**Split.** Fan-out is over **success** edges only. Any `branch`, `loop_continue`, `loop_exit`, or
`reject` edge leaving a split is a validation error (`validate_graph_body`, `src/model.rs`, `only use success edges for split fan-out`). A split with zero
outbound success edges is an error (`validate_graph_body`, `src/model.rs`, `has no outbound split edges`); exactly one success edge is a warning
(`validate_graph_body`, `src/model.rs`, `fans out to fewer than two branches`).

**Collector.** A collector needs at least one inbound edge; emptiness is an error
(`validate_graph_body`, `src/model.rs`, `no inbound branches to collect`). It needs exactly one outbound **success** edge; any other count is an error (`validate_graph_body`, `src/model.rs`, `exactly one outbound success edge`),
and no `branch`, `loop_continue`, `loop_exit`, or `reject` outbound edges (`validate_graph_body`, `src/model.rs`, `only use a success edge after collecting inputs`). Expected
inputs are keyed by each inbound edge's `label`, falling back to the source node `from` id
(`validate_graph_body`, `src/model.rs`, `unwrap_or_else(|| edge.from.clone())`). Two inbound edges sharing a merge key are a **validation error**, not a silent
merge (`validate_graph_body`, `src/model.rs`, `duplicate collector input key`).

If validation were bypassed, the barrier builder would dedupe duplicate merge keys into a set
behind a log warning (`collector_required_inputs`, `src/runtime.rs`) — that collapse is
not behavior an author will meet on a validated document.

### Condition leaf

Edge conditions and `loopCondition` share one flat leaf shape — `StructuredCondition`
(`StructuredCondition`, `src/model.rs`). All three fields are required when the object is present; there is no
nested condition AST in v4 (a planned replacement is described in
[ADR-260815-2009-02](adr/260815-2009-single-condition-dialect.md)).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `field` | string | Yes | — | Dot-path field lookup against the source node's parsed JSON output (`get_nested_field`, `evaluate_condition`, `src/model.rs`, `field not found`). |
| `operator` | string | Yes | — | Comparison operator; see **Operators** below. |
| `value` | string | Yes | — | Right-hand operand, always a string on the wire. |

*Source: `StructuredCondition` (`StructuredCondition`, `src/model.rs`), `evaluate_condition`
(`evaluate_condition`, `src/model.rs`, `get_nested_field`).*

#### Operators

The operator set exists only inside `evaluate_condition` as string comparisons — there is no enum
to enumerate. The complete set of recognised operator strings is:

| Operator | Behaviour |
| --- | --- |
| `==` | String equality after coercing the field value to string. |
| `!=` | String inequality. |
| `contains` | `value_as_string.contains(target)`. |
| `matches` | Regex match; patterns longer than 256 UTF-8 bytes are rejected at run time with an error string (`evaluate_condition`, `src/model.rs`, `regex pattern too long`); the runtime message says "chars". Invalid regex syntax returns an error string. |
| `>` `<` `>=` `<=` | Numeric comparison after parsing both sides as `f64`; non-numeric operands return an error string (`evaluate_condition`, `src/model.rs`, `parse::<f64>()`). |

An **unrecognised** operator does not match — `evaluate_condition` returns
`(false, Some("unknown operator: …"))` (`evaluate_condition`, `src/model.rs`), not a silent false. A missing
field returns `(false, Some("field not found: …"))` (`evaluate_condition`, `src/model.rs`, `field not found`).

## Document-level fields

A workflow document deserializes to `WorkflowV3` (`WorkflowV3`, `src/model.rs`). Wire keys use camelCase.
Only two top-level keys are genuinely required at deserialization time: `version` and `entryNodeId`
(see **Required top-level keys** in **Document grammar**). Every other field carries a serde
default or is optional.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `version` | non-negative integer | Yes | — | Canonical value is `4` (`WORKFLOW_SCHEMA_VERSION`, `src/model.rs`). Rewritten by `ensure_defaults` (`ensure_defaults`, `src/model.rs`, `WORKFLOW_SCHEMA_VERSION`) and migration (`migrate_workflow_value_to_v4`, `src/model.rs`, `workflow.insert`). |
| `name` | string or absent | No | absent | Skipped on serialize when absent (`WorkflowV3::name`, `src/model.rs`). |
| `goal` | string | No | `""` | Always emitted. |
| `cwd` | string | No | `""` | Workflow working directory. Always emitted. |
| `useOrchestrator` | boolean | No | `false` | Always emitted. |
| `runAs` | `{user?, command?}` or absent | No | absent | Skipped on serialize when absent (`WorkflowV3::run_as`, `src/model.rs`). |
| `entryNodeId` | string | Yes | — | Id of the first node to execute. |
| `variables` | array of `{name, default}` | No | `[]` | Always emitted. See **Variables** below. |
| `limits` | `{maxTotalSteps, maxVisitsPerNode}` | No | see **Limits** | Always emitted. |
| `nodes` | array of node objects | No | `[]` | Always emitted. |
| `edges` | array of edge objects | No | `[]` | Always emitted. |
| `agentDefaults` | map of agent name → config object | No | `{}` | `BTreeMap` — keys are sorted and deterministic on serialize (`WorkflowV3::agent_defaults`, `src/model.rs`). Skipped when empty. |
| `subflows` | map of name → workflow body | No | `{}` | Root-level subflow catalog (`WorkflowV3::subflows`, `src/model.rs`). Skipped when empty. See **Subflow catalog**. |
| `ui` | `{canvas?}` or absent | No | absent | Editor canvas state. Skipped when absent (`WorkflowV3::ui`, `src/model.rs`). |

*Source: `WorkflowV3` (`WorkflowV3`, `src/model.rs`).*

### Limits

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `maxTotalSteps` | non-negative integer | No | `50` | `default_max_total_steps` (`default_max_total_steps`, `src/model.rs`). |
| `maxVisitsPerNode` | non-negative integer | No | `10` | `default_max_visits_per_node` (`default_max_visits_per_node`, `src/model.rs`). |

`ensure_defaults` (`ensure_defaults`, `src/model.rs`) rewrites any `0` limit back to its default on the
**root** workflow only — a zero does **not** mean unlimited. Subflow `limits` are ignored at
execution and are not recursively normalized; a `{0,0}` pair on a subflow body survives
serialization unchanged. On the root, the pair `{maxTotalSteps: 0, maxVisitsPerNode: 0}` is
treated as absent-equivalent by `limits_are_canonical` (`limits_are_canonical`, `src/model.rs`) and normalizes to
`50` / `10`.

*Source: `WorkflowLimits` (`WorkflowLimits`, `src/model.rs`), `ensure_defaults` (`ensure_defaults`, `src/model.rs`).*

### Variables

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `name` | string | Yes | — | Variable name within the cursor scope. |
| `default` | string | No | `""` | String-valued initial content (`WorkflowVariable`, `src/model.rs`). |

On a **subflow** body, a variable with an empty `default` is a **required input binding** — the
calling `subflow` or `call` node must supply an `inputs` entry for that name, or validation fails
at run start (`validate_subflow_node_config`, `src/model.rs`, `does not bind required subflow input`). Root-level variable names are not checked for uniqueness;
only subflow input bindings on a call node are deduped (`validate_subflow_node_config`, `src/model.rs`, `subflow node has duplicate input binding`).

*Source: `WorkflowVariable` (`WorkflowVariable`, `src/model.rs`).*

### Subflow catalog

Each entry in `subflows` is a full `WorkflowV3` body keyed by the subflow name. Names are globally
scoped at the root — a subflow body that contains its own `subflows` map is rejected outright
(`validate_workflow_body_for_scope`, `src/model.rs`). Subflow bodies warn when they
define root-only fields that execution ignores: non-canonical `limits` and a `runAs` block
(`validate_subflow_body_root_only_fields`, `src/model.rs`).

| Constraint | Severity | Notes |
| --- | --- | --- |
| Nested `subflows` on a catalog entry | error | Rejected at validation ingress (`validate_workflow_body_for_scope`, `src/model.rs`). |
| `runAs` on a subflow body | warning | Only the root workflow `runAs` is honored (`validate_subflow_body_root_only_fields`, `src/model.rs`). |
| Non-canonical `limits` on a subflow body | warning | Only root limits are honored (`validate_subflow_body_root_only_fields`, `src/model.rs`). |

*Source: `WorkflowV3::subflows` (`WorkflowV3::subflows`, `src/model.rs`), `validate_workflow_body_for_scope`
(`validate_workflow_body_for_scope`, `src/model.rs`).*

### UI viewport

`ui.canvas.viewport` (`WorkflowCanvasViewport`, `src/model.rs`) has three required
sub-fields (`x`, `y`, `zoom`) with no per-field serde defaults. Omitting the whole `viewport`
object deserializes to `WorkflowCanvasViewport::default()` via the parent `#[serde(default)]`
(`WorkflowCanvasUi`, `WorkflowCanvasUi::viewport`, `src/model.rs`), but a **partial** viewport object (for example only
`x`) fails deserialization.

*Source: `WorkflowCanvasViewport` (`WorkflowCanvasViewport`, `src/model.rs`), `WorkflowCanvasUi`
(`WorkflowCanvasUi::viewport`, `src/model.rs`).*

### UI per-node canvas state

`ui.canvas.nodes` (`WorkflowCanvasUi::nodes`, `src/model.rs`) is a map keyed by **node id**
to per-node canvas position (`WorkflowCanvasNodeState`, `src/model.rs`). Keys are intended to
match `nodes[].id` in the same workflow body, but that correspondence is not validated — an
entry whose key no longer names a node in `nodes[]` is neither rejected nor pruned and
round-trips unchanged. The map is optional: it defaults to empty via `#[serde(default)]` and is
**skipped on serialize when empty**.

Each value has two required sub-fields (`x`, `y`) with no per-field serde defaults. A **partial**
value object (for example only `x`) fails deserialization.

*Source: `WorkflowCanvasNodeState` (`WorkflowCanvasNodeState`, `src/model.rs`), `WorkflowCanvasUi`
(`WorkflowCanvasUi::nodes`, `src/model.rs`).*

## Templates and agent config

### Template tokens

Prompt text is scanned for `{{…}}` substitution forms by `resolve_template_vars`
(`resolve_template_vars`, `src/runtime.rs`). There are exactly ten forms and no others:

| Form | Resolves from |
| --- | --- |
| `{{var:<name>}}` | Cursor variable map (`var_map`). |
| `{{<nodeId>}}` | Named node's `output` string. |
| `{{node:<nodeId>.output}}` | Same as the bare node-id form. |
| `{{node:<nodeId>.output.<path>}}` | Dot-path field inside the node's JSON `output` (parsed when possible). |
| `{{node:<nodeId>.parsedOutput.<path>}}` | Dot-path field inside the node's `parsedOutput` value. |
| `{{context:<name>}}` | Output of the node registered under that name in `contextSources`. |
| `{{previous_output}}` | The cursor's `last_output`. |
| `{{branch_origin}}` | The cursor's `last_branch_origin_id`, or empty. |
| `{{branch_choice}}` | The cursor's `last_branch_choice`, or empty. |
| `{{all_predecessors}}` | Direct inbound predecessors only — see below. |

Node ids carry no constraint beyond duplicate detection within a workflow
(`validate_graph_body`, `src/model.rs`, `Duplicate node id`). **Which form matches.** `resolve_template_vars` applies forms in this
order: `{{var:<name>}}`, then bare `{{<nodeId>}}` and `{{node:<nodeId>.output}}`, then the two
dot-path regexes, then `{{context:<name>}}`, then `{{previous_output}}`, `{{branch_origin}}`,
`{{branch_choice}}`, and finally `{{all_predecessors}}` (`resolve_template_vars`, `src/runtime.rs`). An earlier
match shadows later forms. A completed node whose id is `previous_output` or `context:name`
consumes that token first; a node whose id is `var:name` consumes a `{{var:name}}` token only when
no variable of that name is bound. The dot-path regexes capture the node id as `[^.}]+`
(`resolve_template_vars`, `src/runtime.rs`, `replace_all`); a token whose node id contains `.` (for example
`build.step`) matches neither regex and is left in the prompt verbatim.

**Miss behaviour, given a form matched.** Nothing errors on a miss. The two dot-path forms
(`output.<path>` and `parsedOutput.<path>`) substitute the **empty string** when the node or field
is absent. The four name-keyed forms (`{{var:<name>}}`, `{{<nodeId>}}`,
`{{node:<nodeId>.output}}`, `{{context:<name>}}`) are left in the prompt **verbatim** when their
binding is missing. The remaining forms (`{{previous_output}}`, `{{branch_origin}}`,
`{{branch_choice}}`, `{{all_predecessors}}`) always substitute, using an empty string when there
is no value.

`contextSources` on a node registers a `{{context:<name>}}` substitution — it does **not** inject
text into the prompt. The token has no effect unless the prompt names it
(`resolve_template_vars`, `src/runtime.rs`).

`{{all_predecessors}}` covers **direct** inbound predecessors only (`resolve_template_vars`, `src/runtime.rs`, `all_predecessors`). Empty outputs are dropped. Non-empty outputs are joined with
`\n---\n`. Results marked stale are prefixed with `[preserved from prior run]\n`.

*Source: `resolve_template_vars` (`resolve_template_vars`, `src/runtime.rs`), `CONTEXT.md` **Template Token**.*

### `skipCondition`

Pre-execution guard on a node (`SkipCondition`, `src/model.rs`). Distinct from edge and
loop conditions — evaluated before the node runs (`should_skip_cursor_node`, `src/runtime.rs`).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `source` | string | No | `"previous_output"` | When `"previous_output"`, uses the cursor's `last_output`. Any other value is treated as a node id; a missing result defaults to `""` (`should_skip_cursor_node`, `src/runtime.rs`). |
| `type` | `contains` \| `not_contains` \| `regex` | Yes | — | Wire key is `type`; the Rust field is `kind` (`SkipCondition`, `src/model.rs`, `rename = "type"`). |
| `value` | string | Yes | — | Operand for the chosen type. |

An unrecognised `type` evaluates **false** silently — the node runs (`should_skip_cursor_node`, `src/runtime.rs`).
Only `regex` is compile-checked at validation time; an invalid pattern is an error-severity issue
(`validate_graph_body`, `src/model.rs`, `invalid skip condition regex`) and fails the run before any node executes.

*Source: `SkipCondition` (`SkipCondition`, `src/model.rs`), `should_skip_cursor_node`
(`should_skip_cursor_node`, `src/runtime.rs`).*

### Agent configuration

Workflow-level defaults live in `agentDefaults`, keyed by agent name (`AgentDefaults`, `src/model.rs`). Per-node overrides use `agentConfig` inside `kind` on `task` and
`run_agent` nodes (`AgentNodeConfig`, `src/model.rs`), which `#[serde(flatten)]`s the
shared defaults and adds node-exclusive tool lists.

Resolution order for shared fields: node `agentConfig` → workflow `agentDefaults[agent]` →
built-in defaults (`resolve_agent_config`, `src/model.rs`, `unwrap_or_default`). The `access` field on
`spawnConfig` and `runAgentConfig` is applied **outside** this merge chain as
`access_profile_override` (`resolve_agent_config`, `src/model.rs`) — distinct from `accessMode`; see
`CONTEXT.md` **Access Profile**.

#### Workflow `agentDefaults` fields

All `or absent` fields below are omitted on serialize (`AgentDefaults`, `src/model.rs`, `skip_serializing_if`).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `model` | string or absent | No | absent | |
| `reasoningLevel` | `low` \| `medium` \| `high` or absent | No | absent | `ReasoningLevel` (`ReasoningLevel::Low`, `src/driver.rs`). |
| `systemPrompt` | string or absent | No | absent | |
| `accessMode` | `read_only` \| `edit` \| `execute` \| `unrestricted` or absent | No | absent | Resolves to `execute` — not `read_only` — when unset (`AccessMode`, `src/driver.rs`, `Execute`). |
| `toolToggles` | `{webSearch?: boolean}` or absent | No | absent | Exactly one toggle key (`ToolToggles`, `ToolToggles::web_search`, `src/driver.rs`). |
| `maxTurns` | non-negative integer or absent | No | absent | |
| `maxBudgetUsd` | number or absent | No | absent | |
| `autoApprove` | boolean or absent | No | absent | Resolves to `false` when unset (`resolve_agent_config`, `src/model.rs`). |
| `orchestrator` | object or absent | No | absent | See **Orchestrator config**. |

*Source: `AgentDefaults` (`AgentDefaults`, `src/model.rs`).*

#### Node `agentConfig` fields

All `agentDefaults` fields above, plus:

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `allowedTools` | array of strings or absent | No | absent | Node-exclusive allow list. |
| `disallowedTools` | array of strings or absent | No | absent | Node-exclusive deny list. |

*Source: `AgentNodeConfig` (`AgentNodeConfig`, `src/model.rs`).*

#### Orchestrator config

Nested under `orchestrator` on `agentDefaults` or node `agentConfig` (`OrchestratorConfig`, `src/model.rs`). `enabled` and `activation` are always emitted; all other fields are omitted
on serialize when absent (`OrchestratorConfig`, `src/model.rs`).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `enabled` | boolean | No | `false` | |
| `model` | string or absent | No | absent | |
| `activation` | `stale_only` \| `always_on` | No | `stale_only` | `OrchestratorActivation` (`OrchestratorActivation::StaleOnly`, `src/model.rs`). |
| `systemPrompt` | string or absent | No | absent | |
| `staleTimeoutSecs` | non-negative integer or absent | No | absent | |
| `subagentTimeoutSecs` | non-negative integer or absent | No | absent | |

*Source: `OrchestratorConfig` (`OrchestratorConfig`, `src/model.rs`).*

## Validation catalog

`validate_workflow` (`validate_workflow`, `src/model.rs`, `ensure_defaults`) is the validation entry point. Saving a workflow does not
run it (`save_workflow`, `src/api.rs`). Validation runs on an explicit `POST /api/validate-workflow`
request (`validate_workflow_route`, `src/api.rs`), where it is advisory: the response is 200
and carries every issue, errors included. Three earlier failures short-circuit the request before the
full validation result is assembled — an ingest failure returns 400 (`validate_workflow_route`, `src/api.rs`), a
subflow-hydration failure returns 500 (`validate_workflow_route`, `src/api.rs`, `hydrate_saved_subflows`), and an
input-bound rejection returns 422 carrying that single issue in its `details` body
(`validate_workflow_route`, `enforce_workflow_input_bounds`, `src/api.rs`). Validation is **enforced at run start** (`create_run`,
`src/api.rs`, `issue.severity == "error"`),
where any `error`-severity issue refuses the run. A clean catalog therefore predicts run refusal, not
save success.

Each issue is a `ValidationIssue`: `{severity, nodeId?, scope?, message}` (`ValidationIssue::severity`, `src/model.rs`).
`severity` is an unconstrained string the engine only ever sets to `error` or `warning` — there is
no informational severity. Warnings are issues too; they do not block run start.

Besides `issues`, a `ValidationResult` carries the normalized `workflow`, ingest `notices`, and
`graph` metadata (`ValidationResult::workflow`, `src/model.rs`). Graph metadata — `reachableNodeIds`,
`unreachableNodeIds`, and `deadEndNodeIds` (`GraphMetadata`, `GraphMetadata::reachable_node_ids`, `src/model.rs`) — is a
separate field, not an issue and not a severity. Unreachable nodes also produce warning issues
(`validate_workflow`, `src/model.rs`, `compute_graph_metadata`); the reachable and dead-end lists are computed for inspection only. On the
root graph, the **terminal node** warning predicate (`validate_graph_body`, `src/model.rs`, `is a terminal node`) and membership in
`deadEndNodeIds` (`compute_graph_metadata`, `src/model.rs`, `dead_end_node_ids`) currently coincide — every root dead end is warned. The
genuine asymmetry is coverage: `deadEndNodeIds` is computed for the root document only
(`validate_workflow`, `src/model.rs`, `compute_graph_metadata`), while the terminal warning is also emitted inside every catalogued subflow
body by re-entering `validate_graph_body` (`validate_graph_body`, `validate_subflow_catalog`, `src/model.rs`).

`scope` names the subflow a nested issue came from (for example `subflow:myFlow`). Root-graph
issues have no `scope`. Issues raised while validating a subflow graph body are stamped with `scope`
and prefixed in the message (`scope_graph_body_issues`, `src/model.rs`). Exceptions:
issues raised *about* a subflow from the root pass (for example subflow `runAs`/`limits` warnings,
subflow `entryNodeId` errors) and input-bound failures name the subflow in the message where
applicable but carry no `scope`.

A missing or unsupported `version` is an ingest error (`migrate_workflow_value_to_v4`, `src/model.rs`) — it never becomes a `ValidationIssue` and never appears on the issues
channel. See **Version and migration contract** in **Document grammar**.

Entries below are grouped by the subject being validated. The unit is one row per **distinct
trigger condition** — a message an author could be holding — not per Rust construction site. One
site fed by several labels or reason strings expands to several rows.

### Document and input bounds

Input-bound rejections run in `validate_workflow_input_bounds` (`validate_workflow_input_bounds`, `src/model.rs`, `nodes across the root workflow`) before the
main pass. When one fires inside `validate_workflow`, the function returns immediately with that
single issue and default graph metadata (`validate_workflow`, `src/model.rs`, `validate_workflow_input_bounds`). The validate and run-start
routes also call `enforce_workflow_input_bounds` (`validate_workflow_route`, `create_run`, `src/api.rs`, `enforce_workflow_input_bounds`) so oversized documents
fail with HTTP 422 before the full result is assembled.

| Severity | Condition | Source |
| --- | --- | --- |
| error | Subflow body contains a nested `subflows` catalog | `validate_workflow_body_for_scope`, `src/model.rs`, `nested subflow catalog` |
| error | Internal invariant: subflow-scope validation invoked without a subflow name (defensive; unreachable from current callers — `reject_nested_subflow_catalogs` always supplies one; `reject_nested_subflow_catalogs` (`src/model.rs`)) | `validate_workflow_body_for_scope`, `src/model.rs`, `subflow scope requires a subflow name` |
| error | Subflow count exceeds `MAX_WORKFLOW_SUBFLOWS` (1024) across the root catalog | `validate_workflow_input_bounds`, `src/model.rs`, `subflows; at most` |
| error | Subflow/call node count (only nodes with a non-blank `subflowConfig.workflowName`) exceeds `MAX_SUBFLOW_CALL_EDGES` (4096) across root and catalog | `validate_workflow_input_bounds`, `src/model.rs`, `subflow call edges` |
| error | Node count exceeds `MAX_WORKFLOW_NODES` (50_000) across root and catalog | `validate_workflow_input_bounds`, `src/model.rs`, `nodes across the root workflow` |
| error | Edge count exceeds `MAX_WORKFLOW_EDGES` (100_000) across root and catalog | `validate_workflow_input_bounds`, `src/model.rs`, `edges across the root workflow` |
| error | Node `retryCount` exceeds `MAX_NODE_RETRY_COUNT` (10) | `validate_workflow_input_bounds`, `src/model.rs`, `retryCount is` |

Constants: (`MAX_WORKFLOW_SUBFLOWS`, `MAX_SUBFLOW_CALL_EDGES`, `MAX_WORKFLOW_NODES`, `MAX_WORKFLOW_EDGES`, `MAX_NODE_RETRY_COUNT`, `src/model.rs`).

### Graph topology and edges

| Severity | Condition | Source |
| --- | --- | --- |
| error | `entryNodeId` references a non-existent node | `validate_workflow`, `src/model.rs`, `entryNodeId` |
| warning | Node is unreachable from the entry node (root graph only — subflow bodies are not checked for reachability) | `validate_workflow`, `src/model.rs`, `compute_graph_metadata` |
| error | Duplicate node id | `validate_graph_body`, `src/model.rs`, `Duplicate node id` |
| error | Duplicate edge id | `validate_graph_body`, `src/model.rs`, `Duplicate edge id` |
| error | Edge references an unknown source node | `validate_graph_body`, `src/model.rs`, `references unknown source node` |
| error | Edge references an unknown target node | `validate_graph_body`, `src/model.rs`, `references unknown target node` |
| error | Non-split node has more than one `success` edge | `validate_graph_body`, `src/model.rs`, `more than one success edge` |
| error | Node has more than one `reject` edge | `validate_graph_body`, `src/model.rs`, `more than one reject edge` |
| error | Node mixes `branch` and loop control edges | `validate_graph_body`, `src/model.rs`, `mixes branch and loop control edges` |
| error | Node has a `loop_continue` edge but no `loop_exit` edge | `validate_graph_body`, `src/model.rs`, `loop_continue edge but no loop_exit edge` |
| error | `approval` node has `branch` outbound edges | `validate_graph_body`, `src/model.rs`, `Approval node` |
| warning | Node has `loopCondition` but `responseFormat` is not `json` | `validate_graph_body`, `src/model.rs`, `loop condition but responseFormat is not json` |
| warning | Node has deterministic `branch` edge conditions but `responseFormat` is not `json` | `validate_graph_body`, `src/model.rs`, `deterministic branch conditions but responseFormat is not json` |
| warning | Node is a terminal node (no outbound control edges) | `validate_graph_body`, `src/model.rs`, `is a terminal node` |
| warning | `split` node carries task-execution fields that are ignored | `validate_graph_body`, `src/model.rs`, `NodeKind::Split` |
| warning | `collector` node carries task-execution fields that are ignored | `validate_graph_body`, `src/model.rs`, `NodeKind::Collector` |
| error | `split` node has outbound edges other than `success` | `validate_graph_body`, `src/model.rs`, `only use success edges for split fan-out` |
| error | `split` node has no outbound `success` edges | `validate_graph_body`, `src/model.rs`, `has no outbound split edges` |
| warning | `split` node fans out to fewer than two `success` branches | `validate_graph_body`, `src/model.rs`, `fans out to fewer than two branches` |
| error | `collector` node has no inbound edges | `validate_graph_body`, `src/model.rs`, `has no inbound branches to collect` |
| error | `collector` node does not have exactly one outbound `success` edge | `validate_graph_body`, `src/model.rs`, `exactly one outbound success edge` |
| error | `collector` node has outbound edges other than `success` | `validate_graph_body`, `src/model.rs`, `only use a success edge after collecting inputs` |
| error | `collector` node has duplicate inbound merge keys | `validate_graph_body`, `src/model.rs`, `duplicate collector input key` |
| error | `continueSessionFrom` source node is not `task` or `run_agent` | `validate_graph_body`, `src/model.rs`, `not an agent-running node` |
| error | `continueSessionFrom` target and source use different agents | `validate_graph_body`, `src/model.rs`, `they use different agents` |
| error | `continueSessionFrom` source has a broader access profile than the target | `validate_graph_body`, `src/model.rs`, `with a broader access profile` |
| warning | `continueSessionFrom` cannot verify the target agent's access profile (unregistered agent or resolve failure); pane adoption is refused at run time | `validate_graph_body`, `src/model.rs`, `agent {current_agent} {e}` |
| warning | `continueSessionFrom` cannot verify the source agent's access profile (unregistered agent or resolve failure); pane adoption is refused at run time | `validate_graph_body`, `src/model.rs`, `agent {source_agent} {e}` |
| error | `continueSessionFrom` target and source resolve to different working directories | `validate_graph_body`, `src/model.rs`, `different working directories` |
| error | `continueSessionFrom` references an unknown node id | `validate_graph_body`, `src/model.rs`, `references unknown node` |

### Node kinds

Checked for every kind (node-field rules independent of `kind`):

| Severity | Condition | Source |
| --- | --- | --- |
| warning | `outputSchema` is set but `responseFormat` is not `json` | `validate_graph_body`, `src/model.rs`, `output schema but responseFormat is not json` |
| error | `skipCondition` regex is invalid (only `kind: "regex"` is compile-checked) | `validate_graph_body`, `src/model.rs`, `invalid skip condition regex` |

Kind-specific rules:

| Severity | Condition | Source |
| --- | --- | --- |
| error | `task` node has no agent assigned | `validate_graph_body`, `src/model.rs`, `has no agent assigned` |
| warning | `task` node has an empty prompt | `validate_graph_body`, `src/model.rs`, `has an empty prompt` |
| error | `spawn` node has neither an agent nor a `command` | `validate_graph_body`, `src/model.rs`, `spawn node requires an agent or command` |
| error | `send` node has neither `sendConfig.text` nor node `prompt` | `validate_graph_body`, `src/model.rs`, `send node requires text or prompt` |
| error | `run_agent` node has no agent | `validate_graph_body`, `src/model.rs`, `run_agent node requires an agent` |
| warning | `run_agent` node has an empty prompt | `validate_graph_body`, `src/model.rs`, `run_agent node has an empty prompt` |
| error | `wait` node with `until` mode has no marker | `validate_wait_timing_and_marker`, `src/model.rs`, `until mode requires a marker` |
| error | Any non-blank `waitConfig.marker` (regardless of wait mode) or non-blank `runAgentConfig.until` must be a valid regex | `validate_wait_timing_and_marker`, `src/model.rs`, `invalid wait marker regex` |
| error | `idleSeconds` (message: `idle_seconds`) is not a finite non-negative number (`wait` or `run_agent`) | `validate_wait_timing_and_marker`, `src/model.rs`, `("idle_seconds", idle_seconds)` |
| error | `readyStableSeconds` (message: `ready_stable_seconds`) is not a finite non-negative number (`wait` or `run_agent`) | `validate_wait_timing_and_marker`, `src/model.rs`, `("ready_stable_seconds", ready_stable_seconds)` |
| warning | `decide` node has an empty prompt | `validate_decide_node_config`, `src/model.rs`, `decide node has an empty prompt` |
| error | `decide` input binding has an empty name or source | `validate_decide_node_config`, `src/model.rs`, `input binding with an empty name or source` |
| error | `decide` node has duplicate input bindings | `validate_decide_node_config`, `src/model.rs`, `duplicate input binding` |
| error | `decide` node has no outcomes | `validate_decide_node_config`, `src/model.rs`, `requires at least one outcome` |
| error | `decide` outcome label is empty | `validate_decide_node_config`, `src/model.rs`, `has an empty outcome label` |
| error | `decide` outcome label has leading or trailing whitespace | `validate_decide_node_config`, `src/model.rs`, `decide_padded_label_issue` |
| error | `decide` node has duplicate outcome labels | `validate_decide_node_config`, `src/model.rs`, `has duplicate outcome` |
| error | `decide` outcome label does not match an outgoing `branch` edge label | `validate_decide_node_config`, `src/model.rs`, `does not match an outgoing branch edge label` |
| error | `decide` node does not have exactly one `branch` edge per outcome | `validate_decide_node_config`, `src/model.rs`, `one outgoing branch edge per outcome` |
| error | `decide` `branch` edge is missing a label matching an outcome | `validate_decide_node_config`, `src/model.rs`, `requires a label matching an outcome` |
| error | `decide` `branch` edge label has leading or trailing whitespace | `validate_decide_node_config`, `src/model.rs`, `node branch edge label` |
| error | `parallel_batch` node is missing `itemsBinding` | `validate_batch_node_config`, `src/model.rs`, `requires itemsBinding` |
| error | `parallel_batch` node is missing `itemVar` | `validate_batch_node_config`, `src/model.rs`, `requires itemVar` |
| error | `parallel_batch` node is missing `bodyEntry` | `validate_batch_node_config`, `src/model.rs`, `requires bodyEntry` |
| error | `parallel_batch` `bodyEntry` references a non-existent node (existence only; dispatchability is resolved at run time) | `validate_batch_node_config`, `src/model.rs`, `parallel_batch bodyEntry` |
| error | `parallel_batch` `maxConcurrent` is zero | `validate_batch_node_config`, `src/model.rs`, `maxConcurrent must be at least 1` |
| error | `parallel_batch` `collectorVar` is empty when set | `validate_batch_node_config`, `src/model.rs`, `collectorVar cannot be empty when set` |

### Subflows and calls

| Severity | Condition | Source |
| --- | --- | --- |
| warning | Subflow body defines `runAs` (only the root workflow `runAs` is honored) | `validate_subflow_body_root_only_fields`, `src/model.rs`, `defines runAs` |
| warning | Subflow body defines non-canonical `limits` (only root limits are honored; see **Limits**) | `validate_subflow_body_root_only_fields`, `src/model.rs`, `defines custom limits` |
| error | Subflow `entryNodeId` references a non-existent node | `validate_subflow_catalog`, `src/model.rs`, `entryNodeId` |
| error | `subflow`/`call` node is missing `subflowConfig.workflowName` | `validate_subflow_node_config`, `src/model.rs`, `requires subflowConfig.workflowName` |
| error | `subflow`/`call` node references an unknown subflow name | `validate_subflow_node_config`, `src/model.rs`, `references unknown subflow` |
| error | `subflow`/`call` references a subflow whose `entryNodeId` is invalid | `validate_subflow_node_config`, `src/model.rs`, `with missing entryNodeId` |
| error | Referenced subflow does not expose exactly one terminal exit node | `validate_subflow_node_config`, `src/model.rs`, `must expose exactly one exit node` |
| error | `subflowConfig.exitNodeId` is not terminal (has outbound edges) | `validate_subflow_node_config`, `src/model.rs`, `must be terminal` |
| error | `subflowConfig.exitNodeId` references a non-existent node | `validate_subflow_node_config`, `src/model.rs`, `subflow exitNodeId` |
| error | `subflowConfig.exitNodeId` is required when the referenced subflow has other than one terminal node | `validate_subflow_node_config`, `src/model.rs`, `exitNodeId is required unless` |
| error | `subflowConfig.maxDepth` is zero | `validate_subflow_node_config`, `src/model.rs`, `maxDepth must be at least 1` |
| error | `subflow`/`call` input binding has an empty name or source | `validate_subflow_node_config`, `src/model.rs`, `subflow node has an input binding with an empty name or source` |
| error | `subflow`/`call` node has duplicate input bindings | `validate_subflow_node_config`, `src/model.rs`, `subflow node has duplicate input binding` |
| error | `subflow`/`call` binds an unknown subflow variable name (name check only; resolution happens at call time) | `validate_subflow_node_config`, `src/model.rs`, `binds unknown subflow variable` |
| error | `subflow`/`call` does not bind a required subflow input (variable with empty `default`) | `validate_subflow_node_config`, `src/model.rs`, `does not bind required subflow input` |
| warning | Subflow call cycle detected among catalogued subflows (`maxDepth` bounds recursion at run time) | `validate_subflow_call_cycles`, `src/model.rs`, `Subflow call cycle detected among subflows` |

### Agent launch configuration

Absolute working-directory enforcement (`validate_absolute_cwd`, `src/model.rs`) applies
whenever a path is non-empty on these surfaces:

| Severity | Condition | Source |
| --- | --- | --- |
| error | Root workflow `cwd` is not an absolute path | `validate_graph_body`, `src/model.rs`, `Workflow cwd` |
| error | Subflow body `cwd` is not an absolute path (scoped) | `validate_graph_body`, `src/model.rs`, `format!("{prefix} cwd")` |
| error | Node-level `cwd` is not an absolute path | `validate_graph_body`, `src/model.rs`, `&format!("\"{}\" cwd", node.name)` |
| error | `spawnConfig.cwd` is not an absolute path | `validate_graph_body`, `src/model.rs`, `spawn cwd` |
| error | `runAgentConfig.cwd` is not an absolute path | `validate_graph_body`, `src/model.rs`, `run_agent cwd` |

`extraArgs` on `spawn` and `run_agent` nodes is validated by `validate_agent_launch_config`
(`validate_agent_launch_config`, `src/model.rs`, `while let Some(arg)`); each rejection is pushed by `reject_agent_extra_arg`
(`reject_agent_extra_arg`, `src/model.rs`). `access` is validated on `run_agent` nodes and on agent-launched `spawn`
nodes that do not have a non-blank custom `spawnConfig.command`; command-driven spawn passes
`access: None` and skips access checks (`validate_graph_body`, `src/model.rs`, `then_some`). Option-syntax arguments (any
token starting with `-` other than a lone `-`) are rejected unless allowlisted. Allowlisted options:
`--model` (and `--model=…`) for `claude` and `codex`; `--search` for `codex`; `-c` / `--config`
(and `--config=…`) for `codex` with `KEY=VALUE` validation. Any configuration key containing
`sandbox` or `approval` is rejected.

| Severity | Condition | Source |
| --- | --- | --- |
| error | `access` profile is not defined for a registered agent | `validate_agent_launch_config`, `src/model.rs`, `is not defined for agent` |
| warning | `access` profile cannot be verified because the agent is not registered | `validate_agent_launch_config`, `src/model.rs`, `cannot be verified because agent` |
| error | `extraArgs` `--model` requires a following non-option value (`claude`/`codex`) | `validate_agent_launch_config`, `src/model.rs`, `arg == "--model"` |
| error | `extraArgs` `-c` / `--config` requires a following non-option value (`codex`) | `validate_agent_launch_config`, `src/model.rs`, `"--config"` |
| error | `extraArgs` `--config` override is not `KEY=VALUE` syntax | `validate_agent_config_override`, `src/model.rs`, `must use KEY=VALUE syntax` |
| error | `extraArgs` `--config` override has an empty key | `validate_agent_config_override`, `src/model.rs`, `must include a non-empty key` |
| error | `extraArgs` `--config` override uses unsupported key syntax | `validate_agent_config_override`, `src/model.rs`, `uses unsupported config key syntax` |
| error | `extraArgs` `--config` override changes sandbox or approval settings | `validate_agent_config_override`, `src/model.rs`, `cannot override sandbox or approval settings` |
| error | `extraArgs` argument is option-syntax but not allowlisted | `validate_agent_launch_config`, `src/model.rs`, `is not an allowlisted option` |

`access` here is the named argv bundle selected by `spawnConfig.access` or `runAgentConfig.access`
— distinct from the `accessMode` enum on `agentConfig`.

### Run identity

| Severity | Condition | Source |
| --- | --- | --- |
| error | `runAs.command` is empty | `validate_run_as_config`, `src/model.rs`, `runAs.command must not be empty` |
| error | `runAs.command` contains blank tokens | `validate_run_as_config`, `src/model.rs`, `runAs.command must not contain blank tokens` |
| error | `runAs.user` is empty | `validate_run_as_config`, `src/model.rs`, `runAs.user must not be empty` |
| error | `runAs.user` contains shell metacharacters | `validate_run_as_config`, `src/model.rs`, `runAs.user contains shell metacharacters` |

## Regenerating this document

`src/model.rs` is the schema authority. The **Node catalog** section is generated: each block
between a matched HTML comment pair (block id in the comment) will be rewritten by the catalog
generator; all other sections are hand-written prose cross-referenced to the source. The per-kind
`### <wire-tag>` headings in **Node catalog** are hand-maintained and must be updated when the
`NodeKind` variant set changes.

The catalog generator lives in `tests/docs_catalog.rs`. Refresh generated blocks with:

```bash
just regen-docs
```

That recipe runs the catalog generator test with `SB_REGEN_DOCS=1` so it writes
`docs/workflow-schema.md`. A normal `cargo test` regenerates in memory only and asserts the
committed markdown matches — it does not rewrite tracked files.
