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

The fourteen `type` values and their config shapes are listed in the generated node catalog
below; `NodeKind` (`src/model.rs`) remains the schema authority for fields not shown in an
example. `/api/capabilities` publishes the same wire tags as `supportedNodeTypes` (`src/api.rs`).

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
| `contextSources` | array of `{name, nodeId}` | No | `[]` | `ContextSource` entries (`src/model.rs:121-126`); `nodeId` is the referenced node identifier. Registers `{{context:<name>}}` substitutions; does not inject text into the prompt. |
| `responseFormat` | `"text"` \| `"json"` or absent | No | absent | |
| `outputSchema` | JSON value or absent | No | absent | Raw, unvalidated JSON value (legacy `{field: type}` shorthand is converted only during v2/v3→v4 migration, `src/model.rs:372-379`). Setting it while `responseFormat != "json"` produces a warning (`src/model.rs:1608-1618`). |
| `retryCount` | non-negative integer or absent | No | absent | Treated as `0` when absent. Values above `MAX_NODE_RETRY_COUNT` (10, `src/model.rs`) are a hard validation error that short-circuits the whole validation pass. |
| `retryDelay` | non-negative integer or absent | No | absent | Delay in seconds between retries. Runtime uses `2` seconds when unset (`src/runtime.rs:3970`). |
| `timeout` | non-negative integer or absent | No | absent | Per-node execution timeout in seconds. |
| `skipCondition` | object or absent | No | absent | Pre-execution guard; shape is documented in **Templates and agent config**. |
| `loopMaxIterations` | non-negative integer or absent | No | absent | Runtime default is `5` when unset (`src/runtime.rs`). Reaching the cap without a `loop_exit` edge fails the run. |
| `loopCondition` | `{field, operator, value}` or absent | No | absent | `StructuredCondition` (`src/model.rs`) — a flat deterministic leaf evaluated by the engine, never by an LLM. Not a prompt string. |
| `splitFailurePolicy` | `"best_effort_continue"` \| `"fail_fast_cancel"` \| `"drain_then_fail"` | No | `best_effort_continue` | Universal node field (`WorkflowNode::split_failure_policy`, `src/model.rs`). Explicit JSON `null` deserializes to `best_effort_continue` via a custom deserializer. |
| `cwd` | string or absent | No | absent | Validated as absolute when set (`src/model.rs:1620`). At execution time it is read only by `spawn` nodes (`src/tmux_exec.rs:767-771`); at validation time it also feeds `continueSessionFrom`'s matching-working-directory check (`src/model.rs:2111-2112`). `task` and `run_agent` execution ignores it: a `task` node runs in the scoped workflow cwd (`src/runtime.rs:2925`), and a `run_agent` node uses `runAgentConfig.cwd` when set, falling back to that same scoped workflow cwd (`src/tmux_exec.rs:1051-1053`). |
| `continueSessionFrom` | string (node id) or absent | No | absent | Adopts an agent session from another node. Validation enforces five constraints: the source node must exist; its kind must be `task` or `run_agent`; resolved agents must match; the source's access profile must be no broader than the target's; resolved working directories must match. |

*Source: `WorkflowNode` (`src/model.rs:862-910`).*

### Per-kind config fields

Subsections follow the **Node catalog** order. Wire keys use camelCase (`rename_all = "camelCase"`
on each config struct). Config *objects* omitted from serialized output when default-valued are
noted; individual fields are additionally skipped when default-valued — see the struct's serde
attributes. Requiredness reflects serde attributes, not runtime validation — a field marked
optional may still trigger a validation error for a specific kind. An absent key and an explicit
default deserialize the same.

#### task

The only in-`kind` payload is `agentConfig` (agent overrides). That object is documented in
**Templates and agent config** (ISSUE-260826-0637-06).

*Source: `NodeKind::Task` (`src/model.rs:696-703`).*

#### approval

Carries no config object — `approval` is a bare unit variant (`NodeKind::Approval`,
`src/model.rs:704`).

#### split

Carries no config object — `split` is a bare unit variant (`NodeKind::Split`, `src/model.rs:705`).

#### collector

Carries no config object — `collector` is a bare unit variant (`NodeKind::Collector`,
`src/model.rs:706`).

#### decide

Config key: `decideConfig` (inside `kind`).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `inputs` | array of `{name, source}` | No | `[]` | Input bindings for the decide prompt. |
| `prompt` | string | No | `""` | |
| `model` | string or absent | No | absent | When absent, the runtime resolves `"claude-haiku-4-5"` (`default_decide_model`, `src/model.rs`). |
| `outcomes` | array of strings | No | `[]` | Branch labels; each needs a matching `branch` edge. |

*Source: `DecideConfig` (`src/model.rs:441-452`).*

#### parallel_batch

Config key: `batchConfig` inside `kind`. The wrapper also accepts the legacy alias
`parallelBatchConfig` (`NodeKind::ParallelBatch`, `src/model.rs`).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `itemsBinding` | string | Yes* | `""` if `batchConfig` omitted | Variable name holding the array to iterate. |
| `maxConcurrent` | non-negative integer | No | `4` | Clamped to `1`…`MAX_PARALLEL_BATCH_CONCURRENT` (32) at runtime (`src/runtime.rs`). |
| `itemVar` | string | Yes* | `""` if `batchConfig` omitted | Loop variable name for each item. |
| `bodyEntry` | string | Yes* | `""` if `batchConfig` omitted | Node id of the batch body entry point. |
| `collectorVar` | string or absent | No | absent | Optional variable to collect body outputs. |

\*When `batchConfig` is present, `itemsBinding`, `itemVar`, and `bodyEntry` have no per-field
serde default and must be supplied. When the whole `batchConfig` object is omitted, the struct's
`Default` supplies empty strings.

*Source: `BatchConfig` (`src/model.rs:461-471`), wrapper alias (`src/model.rs:711-713`).*

#### subflow

Config key: `subflowConfig` (inside `kind`).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `workflowName` | string | No | `""` | Also accepted as `workflow` or `subflowName` (serde aliases, `src/model.rs`). |
| `exitNodeId` | string or absent | No | absent | Optional early-exit node inside the subflow body. |
| `inputs` | array of `{name, source}` | No | `[]` | Bindings passed into the subflow body. |
| `maxDepth` | non-negative integer | No | `10` | Nesting depth cap (`default_max_call_depth`, `src/model.rs`). |

*Source: `SubflowConfig` (`src/model.rs:628-639`), `NodeKind::Subflow` (`src/model.rs:715-717`).*

#### call

Config key: `subflowConfig` (inside `kind`) — same struct as `subflow`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `workflowName` | string | No | `""` | Also accepted as `workflow` or `subflowName` (serde aliases, `src/model.rs`). |
| `exitNodeId` | string or absent | No | absent | Optional early-exit node inside the called workflow body. |
| `inputs` | array of `{name, source}` | No | `[]` | Bindings passed into the called workflow. |
| `maxDepth` | non-negative integer | No | `10` | Nesting depth cap (`default_max_call_depth`, `src/model.rs`). |

*Source: `SubflowConfig` (`src/model.rs:628-639`), `NodeKind::Call` (`src/model.rs:719-721`).*

#### spawn

Config key: `spawnConfig` (inside `kind`). The whole object is skipped on serialize when equal to
`SpawnConfig::default()`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `agent` | string or absent | No | absent | Agent driver to spawn. Falls back to node-level `agent` when absent (`src/model.rs:60-71`). |
| `command` | string or absent | No | absent | Shell command alternative to an agent launch. |
| `access` | string or absent | No | absent | Named access profile from the Agents Registry; distinct from `accessMode` on agent defaults. |
| `extraArgs` | array of strings | No | `[]` | |
| `cwd` | string or absent | No | absent | Must be absolute when set. Falls back to node-level `cwd`, then workflow cwd, when absent (`src/tmux_exec.rs:767-771`). |
| `name` | string or absent | No | absent | Pane display name. |
| `sessionName` | string or absent | No | absent | Tmux session name. |

*Source: `SpawnConfig` (`src/model.rs:485-502`).*

#### send

Config key: `sendConfig` (inside `kind`). The whole object is skipped on serialize when equal to
`SendConfig::default()`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `target` | string or absent | No | absent | Session or pane target. |
| `text` | string | No | `""` | Text to send. |
| `enter` | boolean | No | `true` | Press Enter after sending. |

*Source: `SendConfig` (`src/model.rs:504-521`).*

#### wait

Config key: `waitConfig` (inside `kind`). The whole object is skipped on serialize when equal to
`WaitConfig::default()`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `target` | string or absent | No | absent | Session or pane target. |
| `mode` | `"idle"` \| `"ready"` \| `"until"` | No | `idle` | `WaitMode` (`src/model.rs`). |
| `marker` | string or absent | No | absent | Marker string for `until` mode. |
| `timeout` | non-negative integer or absent | No | absent | Timeout in seconds. Falls back to node-level `timeout` when absent (`src/runtime.rs:2132`). |
| `idleSeconds` | number or absent | No | absent | Idle duration for `idle` mode. Runtime uses `2.0` seconds when unset (`src/tmux_exec.rs:2338`). |
| `readyStableSeconds` | number or absent | No | absent | Stability window for `ready` mode. Runtime uses `DEFAULT_READY_STABLE_SECONDS` (`2.0`) when unset (`src/tmux_exec.rs:2339-2341`). |

*Source: `WaitConfig` (`src/model.rs:532-560`).*

#### capture

Config key: `captureConfig` (inside `kind`). The whole object is skipped on serialize when equal to
`CaptureConfig::default()`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `target` | string or absent | No | absent | Session or pane target. |
| `lines` | non-negative integer or absent | No | absent | Line count when `all` is false. |
| `all` | boolean | No | `false` | Capture entire scrollback. |
| `ansi` | boolean | No | `false` | Include ANSI escape sequences. |

*Source: `CaptureConfig` (`src/model.rs:562-573`).*

#### kill

Config key: `killConfig` (inside `kind`). The whole object is skipped on serialize when equal to
`KillConfig::default()`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `target` | string or absent | No | absent | Session or pane target. |
| `sessionName` | string or absent | No | absent | Alternative session identifier. |

*Source: `KillConfig` (`src/model.rs:575-582`).*

#### run_agent

Config keys: `runAgentConfig` and `agentConfig` (both inside `kind`). The `runAgentConfig` object
is skipped on serialize when equal to `RunAgentConfig::default()`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `agent` | string or absent | No | absent | Agent driver to run. Falls back to node-level `agent` when absent (`src/model.rs:60-71`). Validation requires an agent from either this key or node-level `agent` (`src/model.rs:1772-1783`). |
| `prompt` | string or absent | No | absent | Prompt text for the agent. Falls back to node-level `prompt` when absent (`src/model.rs:1784-1787`). |
| `cwd` | string or absent | No | absent | Working directory override. Must be absolute when set. Falls back to the active workflow cwd when absent (`src/tmux_exec.rs:1051-1053`, with the scoped workflow cwd resolved at `src/runtime.rs:2925`). Node-level `cwd` does not feed this chain at execution time — the config-then-node-then-workflow resolution in `working_directory_for_node` (`src/model.rs:97-110`) is reached only by `continueSessionFrom` validation (`src/model.rs:2111-2112`). |
| `access` | string or absent | No | absent | Named access profile; distinct from `accessMode` on agent defaults. |
| `extraArgs` | array of strings | No | `[]` | |
| `name` | string or absent | No | absent | Pane display name. |
| `timeout` | non-negative integer or absent | No | absent | Timeout in seconds. Falls back to node-level `timeout` when absent (`src/runtime.rs:2131`). |
| `idleSeconds` | number or absent | No | absent | Runtime uses `2.0` seconds when unset (`src/tmux_exec.rs:1058-1061`). |
| `readyStableSeconds` | number or absent | No | absent | Runtime uses `DEFAULT_READY_STABLE_SECONDS` (`2.0`) when unset (`src/tmux_exec.rs:1059-1061`). |
| `until` | string or absent | No | absent | Marker string for ready/until waits. |
| `killAfter` | boolean | No | `true` | Tear down the pane after the agent exits. |
| `agentConfig` | object or absent | No | absent | Agent overrides alongside `runAgentConfig`; documented in **Templates and agent config** (ISSUE-260826-0637-06). Sibling payload of the variant, not a field of `RunAgentConfig`. |

*Source: `RunAgentConfig` (`src/model.rs:584-626`), `NodeKind::RunAgent` (`src/model.rs:743-752`).*

## Edges and conditions

An edge deserializes to `WorkflowEdge` (`src/model.rs`). Wire keys use camelCase
(`rename_all = "camelCase"` on the struct). There are no serde aliases on endpoint fields — the
wire names are `from` and `to`, not `source` or `target`.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `id` | string | Yes | — | Edge identifier; must be unique within the workflow. Duplicate ids are a validation error (`validate_graph_body`, `src/model.rs:2138-2145`). |
| `from` | string | Yes | — | Source node id. |
| `to` | string | Yes | — | Target node id. |
| `outcome` | `success` \| `reject` \| `branch` \| `loop_continue` \| `loop_exit` | Yes | — | Traversal channel (`WorkflowEdgeOutcome`, `src/model.rs:215-221`). |
| `label` | string or absent | No | absent | Branch label for `branch` edges (matched against decide outcomes) and collector merge keys (see below). |
| `branchId` | string or absent | No | absent | Branch identifier reported as `chosenBranch` on the `branch_decision` event, falling back to the edge `id` when absent (`src/runtime.rs:6575`). |
| `condition` | `{field, operator, value}` or absent | No | absent | Post-execution deterministic guard (`StructuredCondition`, `src/model.rs:128-134`). Only `branch`-outcome edges on non-`decide` nodes consult it (`src/runtime.rs:6527-6539`); `success`, `reject`, `loop_continue`, and `loop_exit` edges deserialize it but never evaluate it, and validation does not flag that (`src/model.rs:1896-1914` warns only for branch edges). `decide` nodes return before condition evaluation (`src/runtime.rs:6383-6443`). Evaluation runs only when `result.parsed_output` is present (`src/runtime.rs:6529`); `responseFormat: json` produces parsed output for prompt-driven nodes (`parse_structured_output`, `src/runtime.rs:7208-7226`), while system-output kinds (`capture`, `parallel_batch`, `decide`, `split`, `collector`) populate it unconditionally. See **Condition leaf**. |

*Source: `WorkflowEdge` (`src/model.rs:922-935`).*

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
`reject` edge leaving a split is a validation error (`src/model.rs:1944-1955`). A split with zero
outbound success edges is an error (`:1956-1962`); exactly one success edge is a warning
(`:1963-1970`).

**Collector.** A collector needs at least one inbound edge (error when empty,
`src/model.rs:1974-1980`), exactly one outbound **success** edge (error otherwise, `:1982-1991`),
and no `branch`, `loop_continue`, `loop_exit`, or `reject` outbound edges (`:1993-2003`). Expected
inputs are keyed by each inbound edge's `label`, falling back to the source node `from` id
(`:2005-2007`). Two inbound edges sharing a merge key are a **validation error**, not a silent
merge (`:2008-2018`).

If validation were bypassed, the barrier builder would dedupe duplicate merge keys into a set
behind a log warning (`collector_required_inputs`, `src/runtime.rs:1929-1942`) — that collapse is
not behavior an author will meet on a validated document.

### Condition leaf

Edge conditions and `loopCondition` share one flat leaf shape — `StructuredCondition`
(`src/model.rs:128-134`). All three fields are required when the object is present; there is no
nested condition AST in v4 (a planned replacement is described in
[ADR-260815-2009-02](adr/260815-2009-single-condition-dialect.md)).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `field` | string | Yes | — | Dot-path field lookup against the source node's parsed JSON output (`get_nested_field`, `src/model.rs:3174-3180`). |
| `operator` | string | Yes | — | Comparison operator; see **Operators** below. |
| `value` | string | Yes | — | Right-hand operand, always a string on the wire. |

*Source: `StructuredCondition` (`src/model.rs:128-134`), `evaluate_condition`
(`src/model.rs:3182-3236`).*

#### Operators

The operator set exists only inside `evaluate_condition` as string comparisons — there is no enum
to enumerate. The complete set of recognised operator strings is:

| Operator | Behaviour |
| --- | --- |
| `==` | String equality after coercing the field value to string. |
| `!=` | String inequality. |
| `contains` | `value_as_string.contains(target)`. |
| `matches` | Regex match; patterns longer than 256 UTF-8 bytes are rejected at run time with an error string (`src/model.rs:3201-3207`; the runtime message says "chars"). Invalid regex syntax returns an error string. |
| `>` `<` `>=` `<=` | Numeric comparison after parsing both sides as `f64`; non-numeric operands return an error string (`src/model.rs:3213-3233`). |

An **unrecognised** operator does not match — `evaluate_condition` returns
`(false, Some("unknown operator: …"))` (`src/model.rs:3234-3235`), not a silent false. A missing
field returns `(false, Some("field not found: …"))` (`:3186-3188`).

## Document-level fields

A workflow document deserializes to `WorkflowV3` (`src/model.rs:939-969`). Wire keys use camelCase.
Only two top-level keys are genuinely required at deserialization time: `version` and `entryNodeId`
(see **Required top-level keys** in **Document grammar**). Every other field carries a serde
default or is optional.

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `version` | non-negative integer | Yes | — | Canonical value is `4` (`WORKFLOW_SCHEMA_VERSION`, `src/model.rs`). Rewritten by `ensure_defaults` and migration (`src/model.rs:1116-1117`, `:1170`). |
| `name` | string or absent | No | absent | Skipped on serialize when absent (`skip_serializing_if = "Option::is_none"`, `src/model.rs:941-942`). |
| `goal` | string | No | `""` | Always emitted. |
| `cwd` | string | No | `""` | Workflow working directory. Always emitted. |
| `useOrchestrator` | boolean | No | `false` | Always emitted. |
| `runAs` | `{user?, command?}` or absent | No | absent | Skipped on serialize when absent (`src/model.rs:949-950`). |
| `entryNodeId` | string | Yes | — | Id of the first node to execute. |
| `variables` | array of `{name, default}` | No | `[]` | Always emitted. See **Variables** below. |
| `limits` | `{maxTotalSteps, maxVisitsPerNode}` | No | see **Limits** | Always emitted. |
| `nodes` | array of node objects | No | `[]` | Always emitted. |
| `edges` | array of edge objects | No | `[]` | Always emitted. |
| `agentDefaults` | map of agent name → config object | No | `{}` | `BTreeMap` — keys are sorted and deterministic on serialize (`src/model.rs:960-961`). Skipped when empty. |
| `subflows` | map of name → workflow body | No | `{}` | Root-level subflow catalog (`src/model.rs:962-966`). Skipped when empty. See **Subflow catalog**. |
| `ui` | `{canvas?}` or absent | No | absent | Editor canvas state. Skipped when absent (`src/model.rs:967-968`). |

*Source: `WorkflowV3` (`src/model.rs:939-969`).*

### Limits

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `maxTotalSteps` | non-negative integer | No | `50` | `default_max_total_steps` (`src/model.rs:232-234`). |
| `maxVisitsPerNode` | non-negative integer | No | `10` | `default_max_visits_per_node` (`src/model.rs:236-238`). |

`ensure_defaults` (`src/model.rs:1116-1131`) rewrites any `0` limit back to its default on the
**root** workflow only — a zero does **not** mean unlimited. Subflow `limits` are ignored at
execution and are not recursively normalized; a `{0,0}` pair on a subflow body survives
serialization unchanged. On the root, the pair `{maxTotalSteps: 0, maxVisitsPerNode: 0}` is
treated as absent-equivalent by `limits_are_canonical` (`src/model.rs:240-245`) and normalizes to
`50` / `10`.

*Source: `WorkflowLimits` (`src/model.rs:223-230`), `ensure_defaults` (`src/model.rs:1116-1131`).*

### Variables

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `name` | string | Yes | — | Variable name within the cursor scope. |
| `default` | string | No | `""` | String-valued initial content (`WorkflowVariable`, `src/model.rs:115-119`). |

On a **subflow** body, a variable with an empty `default` is a **required input binding** — the
calling `subflow` or `call` node must supply an `inputs` entry for that name, or validation fails
at run start (`src/model.rs:2694-2705`). Root-level variable names are not checked for uniqueness;
only subflow input bindings on a call node are deduped (`src/model.rs:2656-2679`).

*Source: `WorkflowVariable` (`src/model.rs:115-119`).*

### Subflow catalog

Each entry in `subflows` is a full `WorkflowV3` body keyed by the subflow name. Names are globally
scoped at the root — a subflow body that contains its own `subflows` map is rejected outright
(`validate_workflow_body_for_scope`, `src/model.rs:1392-1400`). Subflow bodies warn when they
define root-only fields that execution ignores: non-canonical `limits` and a `runAs` block
(`validate_subflow_body_root_only_fields`, `src/model.rs:1348-1374`).

| Constraint | Severity | Notes |
| --- | --- | --- |
| Nested `subflows` on a catalog entry | error | Rejected at validation ingress (`src/model.rs:1392-1400`). |
| `runAs` on a subflow body | warning | Only the root workflow `runAs` is honored (`src/model.rs:1353-1361`). |
| Non-canonical `limits` on a subflow body | warning | Only root limits are honored (`src/model.rs:1364-1372`). |

*Source: `WorkflowV3::subflows` (`src/model.rs:962-966`), `validate_workflow_body_for_scope`
(`src/model.rs:1376-1407`).*

### UI viewport

`ui.canvas.viewport` (`WorkflowCanvasViewport`, `src/model.rs:652-658`) has three required
sub-fields (`x`, `y`, `zoom`) with no per-field serde defaults. Omitting the whole `viewport`
object deserializes to `WorkflowCanvasViewport::default()` via the parent `#[serde(default)]`
(`WorkflowCanvasUi`, `src/model.rs:680-681`), but a **partial** viewport object (for example only
`x`) fails deserialization.

*Source: `WorkflowCanvasViewport` (`src/model.rs:652-668`), `WorkflowCanvasUi`
(`src/model.rs:677-684`).*

## Templates and agent config

### Template tokens

Prompt text is scanned for `{{…}}` substitution forms by `resolve_template_vars`
(`src/runtime.rs:7063-7154`). There are exactly ten forms and no others:

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
(`src/model.rs:1598-1606`). **Which form matches.** `resolve_template_vars` applies forms in this
order: `{{var:<name>}}`, then bare `{{<nodeId>}}` and `{{node:<nodeId>.output}}`, then the two
dot-path regexes, then `{{context:<name>}}`, then `{{previous_output}}`, `{{branch_origin}}`,
`{{branch_choice}}`, and finally `{{all_predecessors}}` (`src/runtime.rs:7065-7152`). An earlier
match shadows later forms. A completed node whose id is `previous_output` or `context:name`
consumes that token first; a node whose id is `var:name` consumes a `{{var:name}}` token only when
no variable of that name is bound. The dot-path regexes capture the node id as `[^.}]+`
(`src/runtime.rs:7073-7075`, `:7096-7098`); a token whose node id contains `.` (for example
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
(`src/runtime.rs:7119-7123`).

`{{all_predecessors}}` covers **direct** inbound predecessors only (`inbound_map` for the current
node, `src/runtime.rs:7135-7152`). Empty outputs are dropped. Non-empty outputs are joined with
`\n---\n`. Results marked stale are prefixed with `[preserved from prior run]\n`.

*Source: `resolve_template_vars` (`src/runtime.rs:7063-7154`), `CONTEXT.md` **Template Token**.*

### `skipCondition`

Pre-execution guard on a node (`SkipCondition`, `src/model.rs:136-144`). Distinct from edge and
loop conditions — evaluated before the node runs (`should_skip_cursor_node`,
`src/runtime.rs:2702-2733`).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `source` | string | No | `"previous_output"` | When `"previous_output"`, uses the cursor's `last_output`. Any other value is treated as a node id; a missing result defaults to `""` (`src/runtime.rs:2711-2718`). |
| `type` | `contains` \| `not_contains` \| `regex` | Yes | — | Wire key is `type`; the Rust field is `kind` (`#[serde(rename = "type")]`, `src/model.rs:141-142`). |
| `value` | string | Yes | — | Operand for the chosen type. |

An unrecognised `type` evaluates **false** silently — the node runs (`src/runtime.rs:2731-2732`).
Only `regex` is compile-checked at validation time; an invalid pattern is an error-severity issue
(`src/model.rs:1629-1642`) and fails the run before any node executes.

*Source: `SkipCondition` (`src/model.rs:136-144`), `should_skip_cursor_node`
(`src/runtime.rs:2702-2733`).*

### Agent configuration

Workflow-level defaults live in `agentDefaults`, keyed by agent name (`AgentDefaults`,
`src/model.rs:256-277`). Per-node overrides use `agentConfig` inside `kind` on `task` and
`run_agent` nodes (`AgentNodeConfig`, `src/model.rs:287-296`), which `#[serde(flatten)]`s the
shared defaults and adds node-exclusive tool lists.

Resolution order for shared fields: node `agentConfig` → workflow `agentDefaults[agent]` →
built-in defaults (`resolve_agent_config`, `src/model.rs:298-354`). The `access` field on
`spawnConfig` and `runAgentConfig` is applied **outside** this merge chain as
`access_profile_override` (`src/model.rs:324-330`) — distinct from `accessMode`; see
`CONTEXT.md` **Access Profile**.

#### Workflow `agentDefaults` fields

All `or absent` fields below are omitted on serialize (`skip_serializing_if = "Option::is_none"`,
`src/model.rs:259-276`).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `model` | string or absent | No | absent | |
| `reasoningLevel` | `low` \| `medium` \| `high` or absent | No | absent | `ReasoningLevel` (`src/driver.rs:494-500`). |
| `systemPrompt` | string or absent | No | absent | |
| `accessMode` | `read_only` \| `edit` \| `execute` \| `unrestricted` or absent | No | absent | Resolves to `execute` — not `read_only` — when unset (`AccessMode::default()`, `src/driver.rs:503-511`). |
| `toolToggles` | `{webSearch?: boolean}` or absent | No | absent | Exactly one toggle key (`ToolToggles`, `src/driver.rs:514-519`). |
| `maxTurns` | non-negative integer or absent | No | absent | |
| `maxBudgetUsd` | number or absent | No | absent | |
| `autoApprove` | boolean or absent | No | absent | Resolves to `false` when unset (`src/model.rs:351`). |
| `orchestrator` | object or absent | No | absent | See **Orchestrator config**. |

*Source: `AgentDefaults` (`src/model.rs:256-277`).*

#### Node `agentConfig` fields

All `agentDefaults` fields above, plus:

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `allowedTools` | array of strings or absent | No | absent | Node-exclusive allow list. |
| `disallowedTools` | array of strings or absent | No | absent | Node-exclusive deny list. |

*Source: `AgentNodeConfig` (`src/model.rs:287-296`).*

#### Orchestrator config

Nested under `orchestrator` on `agentDefaults` or node `agentConfig` (`OrchestratorConfig`,
`src/model.rs:40-55`). `enabled` and `activation` are always emitted; all other fields are omitted
on serialize when absent (`src/model.rs:43-54`).

| Wire field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `enabled` | boolean | No | `false` | |
| `model` | string or absent | No | absent | |
| `activation` | `stale_only` \| `always_on` | No | `stale_only` | `OrchestratorActivation` (`src/model.rs:29-37`). |
| `systemPrompt` | string or absent | No | absent | |
| `staleTimeoutSecs` | non-negative integer or absent | No | absent | |
| `subagentTimeoutSecs` | non-negative integer or absent | No | absent | |

*Source: `OrchestratorConfig` (`src/model.rs:40-55`).*

## Validation catalog

The validation-issue catalog is deferred to a later pass; `validate_workflow` and `ValidationIssue`
in `src/model.rs` are the authority until then.

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
