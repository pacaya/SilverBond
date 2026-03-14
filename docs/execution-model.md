# Execution Model

The runtime engine (`runtime.rs`) drives workflow execution using a multi-cursor, checkpoint-based model with durable concurrency.

## Run Lifecycle

1. Client submits a workflow to `POST /api/runs`
2. Backend validates the workflow
3. Runtime creates an initial checkpoint with one cursor at the entry node
4. Execution begins on the Tokio runtime
5. For each cursor, the runtime traverses the graph depth-first
6. Events are appended to SQLite and published to in-memory SSE subscribers
7. Terminal state and execution log are persisted when the run completes

## Cursor Model

Execution proceeds through **cursors** — independent execution pointers that traverse the workflow graph. A simple linear workflow uses a single cursor. Split nodes spawn multiple cursors for parallel branches.

### Cursor State

Each cursor tracks:

```
CursorState {
  cursor_id         — unique identifier
  node_id           — current position in the graph
  execution_epoch   — epoch counter for restart isolation
  parent_cursor_id  — cursor that spawned this one (for splits)
  incoming_edge_id  — edge that led to this node
  incoming_node_id  — previous node
  split_family_ids  — split families this cursor belongs to
  last_output       — output from the last executed node
  loop_counters     — per-node loop iteration counts
  visit_counters    — per-node visit counts
  last_branch_*     — last branch decision metadata
  cancel_requested  — whether cancellation was requested
  state             — Running | WaitingAtCollector | Done | Cancelled | Failed
}
```

### Cursor Lifecycle

```
                    ┌──────────┐
                    │ Running  │
                    └────┬─────┘
                         │
              ┌──────────┼──────────┐
              │          │          │
              ▼          ▼          ▼
     ┌────────────┐ ┌────────┐ ┌───────────────────┐
     │   Done     │ │ Failed │ │ WaitingAtCollector │
     └────────────┘ └────────┘ └─────────┬─────────┘
                                         │
                                         ▼
                                   ┌──────────┐
                                   │ Released │ (continues as Running)
                                   └──────────┘
```

## Node Execution

### Task Nodes

For each task node, the runtime:

1. **Resolves the prompt** — substitutes `{{var:name}}`, `{{previous_output}}`, `{{node_name:field}}`, `{{all_predecessors}}`
2. **Checks skip condition** — if the condition matches, skips the node and emits `node_skipped`
3. **Resolves agent config** — merges node-level → workflow defaults → driver defaults
4. **Optionally refines the prompt** — if `useOrchestrator` is enabled and the agent supports it
5. **Builds CLI arguments** — via the agent driver's `build_args()` method
6. **Spawns the agent subprocess** — runs the CLI command with configured timeout
7. **Parses output** — via the agent driver's `parse_output()` method
8. **Validates structured output** — if `outputSchema` is set
9. **Handles retries** — on failure, retries up to `retryCount` times with `retryDelay`
10. **Persists checkpoint** — saves cursor state and node result to SQLite
11. **Emits events** — `node_start`, `node_done` (or `node_retry`, `node_skipped`)

### Approval Nodes

1. Run pauses and emits `approval_required`
2. Checkpoint is persisted with pending approval state
3. Client calls `POST /api/runs/{id}/approve` with `approved` boolean and optional `userInput`
4. On approval: cursor follows `success` edges
5. On rejection: cursor follows `reject` edges

### Split Nodes

1. The split node executes (may use orchestrator for branch determination)
2. For each outgoing `branch` edge, a new cursor is spawned
3. Each cursor gets a unique ID, shares the parent's execution epoch
4. All cursors are registered in a **split family** with the configured failure policy
5. Cursors execute their branches independently and concurrently

### Collector Nodes

1. Collector implements **barrier semantics**
2. When a cursor arrives, it registers in the collector's barrier
3. If not all expected inputs have arrived, the cursor enters `WaitingAtCollector` state
4. When all inputs arrive, the barrier releases
5. The collected outputs are aggregated and available via `{{all_predecessors}}`
6. A single cursor continues past the collector

## Split Families and Failure Policies

Split families track related cursors spawned by a split node:

| Policy | Behavior |
|--------|----------|
| `best_effort_continue` | If some branches fail, continue with successful ones |
| `fail_fast_cancel` | On first branch failure, cancel all sibling cursors immediately |
| `drain_then_fail` | Wait for all branches to complete, then fail if any failed |

## Branching and Loops

### Branch Decisions

When a node has multiple outgoing `branch` edges, the runtime needs to choose which path to follow. If `useOrchestrator` is enabled and the agent supports `branchChoice`, the orchestrator evaluates the node's output and selects the appropriate branch.

### Loop Decisions

When a node has `loop_continue` and `loop_exit` edges:

1. The runtime checks `loopMaxIterations` — if exceeded, forces exit
2. If `useOrchestrator` is enabled, the orchestrator evaluates `loopCondition` against the node output
3. On `loop_continue`: cursor returns to the loop target, loop counter increments
4. On `loop_exit`: cursor follows the exit edge

## Checkpoints

After each significant state change, the runtime persists a checkpoint to SQLite. A checkpoint contains:

```
Checkpoint {
  executionEpoch      — monotonic epoch counter
  activeCursors[]     — all cursor states with outputs and counters
  splitFamilies[]     — split tracking with member cursors and failure policy
  collectorBarriers[] — barrier state with arrivals and waiting cursors
  nodeResults{}       — per-node execution results
  variables{}         — resolved workflow variables
  pendingApproval     — approval state if paused
  executionLog        — accumulated log entries
}
```

Checkpoints enable:
- **Resume after crash** — `POST /api/runs/{id}/resume` restores from last checkpoint
- **Restart from node** — `POST /api/runs/{id}/restart-from/{nodeId}` creates a new run with incremented epoch

### Execution Epochs

The execution epoch is a monotonic counter that increments on restart. It prevents stale split/collector state from the previous epoch from contaminating the restarted execution path.

## Session Persistence

Task nodes can continue an agent session from a previous node using `continueSessionFrom`. The runtime:

1. Pre-scans the workflow to identify which nodes need persistent sessions
2. When executing a node that needs session persistence, passes the `session_id` flag to the agent
3. When a downstream node references `continueSessionFrom`, passes the upstream node's `session_id` for session resumption

This enables multi-turn agent conversations across workflow nodes.

## Event Vocabulary

Events emitted during execution:

| Event | Description |
|-------|-------------|
| `run_start` | Workflow execution begins |
| `run_resumed` | Run resumed from checkpoint |
| `node_start` | Node execution begins |
| `node_done` | Node execution completed (success or failure) |
| `node_retry` | Node execution being retried |
| `node_skipped` | Node skipped due to skip condition |
| `branch_decision` | Orchestrator chose a branch path |
| `loop_decision` | Orchestrator decided loop continue/exit |
| `cursor_spawned` | New cursor created (split fan-out) |
| `collector_waiting` | Cursor waiting at collector barrier |
| `aggregate_merged` | Collector merged an arriving input |
| `collector_released` | All inputs arrived, collector proceeding |
| `approval_queued` | Approval node reached |
| `approval_required` | Run paused waiting for user approval |
| `cursor_cancelled` | Cursor cancelled (failure policy) |
| `transition` | Cursor moving to next node |
| `workflow_error` | Runtime error occurred |
| `done` | Run completed (success, failed, or aborted) |

## Run Statuses

| Status | Description |
|--------|-------------|
| `running` | Actively executing |
| `paused` | Waiting for approval or external input |
| `completed` | All cursors finished successfully |
| `failed` | Execution failed (error or limit exceeded) |
| `aborted` | User-initiated abort |
| `restarted` | Run was restarted from a specific node |

## Orchestrator

When `useOrchestrator` is enabled, the runtime uses the orchestrator agent (Claude by default) for three decision types:

1. **Prompt refinement** — enhances task prompts before execution
2. **Branch choice** — selects which branch to follow at decision points
3. **Loop verdict** — decides whether to continue or exit a loop

The orchestrator is only used when the selected agent supports the relevant capability.

## Limits and Guards

- `maxTotalSteps` — maximum total node executions across all cursors
- `maxVisitsPerNode` — maximum times any single node can be visited
- `loopMaxIterations` — maximum iterations for a loop
- `timeout` — per-node execution timeout
- `maxBudgetUsd` — per-node cost limit
- `maxTurns` — per-node agent turn limit

When any limit is exceeded, the node fails with the corresponding error outcome (`error_timeout`, `error_max_budget`, `error_max_turns`).
