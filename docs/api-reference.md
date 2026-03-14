# API Reference

All endpoints are served under `/api/` on the Rust backend (default port 3333).

## Health & Capabilities

### `GET /api/health`

Health check endpoint.

**Response:**
```json
{ "ok": true }
```

### `GET /api/capabilities`

Returns runtime capabilities including supported agents and their features.

**Response:**
```json
{
  "workflowVersion": 3,
  "supportedNodeTypes": ["task", "approval", "split", "collector"],
  "supportedEdgeOutcomes": ["success", "reject", "branch", "loop_continue", "loop_exit"],
  "features": {
    "split": true,
    "collector": true
  },
  "agents": {
    "claude": {
      "available": true,
      "path": "/usr/local/bin/claude",
      "capabilities": {
        "workerExecution": true,
        "promptRefinement": true,
        "branchChoice": true,
        "loopVerdict": true,
        "structuredOutput": true,
        "sessionReuse": true,
        "nativeJsonSchema": true,
        "modelSelection": true,
        "reasoningConfig": true,
        "systemPrompt": true,
        "budgetLimit": true,
        "turnLimit": true,
        "costReporting": true,
        "toolAllowlist": true,
        "webSearch": true
      }
    }
  }
}
```

## Workflow CRUD

### `GET /api/workflows`

List all saved workflows.

**Response:**
```json
[
  { "name": "My Workflow", "workflow": { ... } }
]
```

### `GET /api/workflows/{name}`

Get a specific workflow by name.

**Response:**
```json
{ "name": "My Workflow", "workflow": { ... } }
```

### `POST /api/workflows`

Save a workflow. Creates or overwrites.

**Request body:**
```json
{
  "name": "My Workflow",
  "workflow": { "version": 3, ... }
}
```

### `DELETE /api/workflows/{name}`

Delete a saved workflow.

## Templates

### `GET /api/templates`

List all available template workflows.

**Response:**
```json
[
  { "name": "Research and Summarize", "workflow": { ... } }
]
```

## Validation

### `POST /api/validate-workflow`

Validate a workflow and return issues with graph analysis.

**Request body:**
```json
{ "version": 3, "entryNodeId": "n1", "nodes": [...], "edges": [...] }
```

**Response:**
```json
{
  "issues": [
    {
      "severity": "error",
      "message": "Node 'n3' is unreachable from entry node",
      "nodeId": "n3"
    }
  ],
  "metadata": {
    "reachableNodes": ["n1", "n2"],
    "unreachableNodes": ["n3"]
  }
}
```

## Node Testing

### `POST /api/test-node`

Preview a task node's resolved prompt without executing it.

**Request body:**
```json
{
  "node": { "id": "n1", "type": "task", "prompt": "Research {{var:topic}}" },
  "cwd": "/workspace",
  "mockContext": { "topic": "AI safety" }
}
```

## Run Lifecycle

### `POST /api/runs`

Start a new workflow run.

**Request body:**
```json
{
  "workflow": { "version": 3, ... },
  "variableOverrides": { "topic": "quantum computing" }
}
```

**Response:**
```json
{ "runId": "550e8400-e29b-41d4-a716-446655440000" }
```

### `GET /api/runs/{runId}/stream`

Subscribe to real-time events via Server-Sent Events (SSE).

**Event format:**
```
data: {"type": "node_start", "nodeId": "n1", "nodeName": "Research", ...}

data: {"type": "node_done", "nodeId": "n1", "success": true, "output": "...", ...}

data: {"type": "done", "status": "completed"}
```

The connection stays open until the run completes, fails, or is aborted.

### `GET /api/runs/{runId}/events`

Get historical events for a run (for catch-up after reconnection).

**Response:**
```json
[
  { "type": "run_start", ... },
  { "type": "node_start", "nodeId": "n1", ... },
  { "type": "node_done", "nodeId": "n1", ... }
]
```

## Run Control

### `POST /api/runs/{runId}/approve`

Approve or reject a pending approval.

**Request body:**
```json
{
  "approved": true,
  "userInput": "Looks good, proceed."
}
```

### `POST /api/runs/{runId}/abort`

Abort a running workflow.

### `POST /api/runs/{runId}/resume`

Resume an interrupted run from its last checkpoint.

### `POST /api/runs/{runId}/restart-from/{nodeId}`

Restart a run from a specific node. Creates a new run ID and increments the execution epoch to prevent stale state contamination.

### `POST /api/runs/{runId}/dismiss`

Dismiss an interrupted run without resuming it.

## History & Logs

### `GET /api/interrupted-runs`

List runs that were interrupted (e.g., by process restart).

**Response:**
```json
[
  {
    "runId": "...",
    "workflowName": "My Workflow",
    "status": "paused",
    "currentNodeId": "n2",
    "totalExecuted": 3,
    "startedAt": "2026-03-14T10:00:00Z",
    "updatedAt": "2026-03-14T10:05:00Z"
  }
]
```

### `GET /api/logs`

List execution log summaries.

**Response:**
```json
[
  {
    "id": "...",
    "workflowName": "My Workflow",
    "goal": "Research topic",
    "startTime": "2026-03-14T10:00:00Z",
    "endTime": "2026-03-14T10:10:00Z",
    "totalDuration": 600000,
    "aborted": false,
    "runId": "..."
  }
]
```

### `GET /api/logs/{id}`

Get detailed execution log including per-node results.

**Response includes per-node metadata:**
```json
{
  "entries": [
    {
      "nodeId": "n1",
      "nodeName": "Research",
      "success": true,
      "output": "...",
      "duration": 5000,
      "metadata": {
        "outcome": "success",
        "costUsd": 0.05,
        "inputTokens": 1200,
        "outputTokens": 800,
        "thinkingTokens": 400,
        "modelUsed": "claude-opus-4",
        "numTurns": 3,
        "agentSessionId": "session-abc123"
      }
    }
  ]
}
```

### `DELETE /api/logs/{id}`

Delete an execution log entry.

## Error Handling

All endpoints return errors as JSON with appropriate HTTP status codes:

```json
{
  "error": "Workflow validation failed",
  "details": "Entry node 'n1' not found in nodes array"
}
```

Common status codes:
- `400` — Invalid request or validation failure
- `404` — Resource not found
- `500` — Internal server error
