# Workflow Schema Reference (v3)

SilverBond uses a graph-native workflow schema. The only accepted format is version `3`. Legacy formats are rejected at validation time.

## Top-Level Document

```json
{
  "version": 3,
  "name": "My Workflow",
  "goal": "What this workflow should accomplish",
  "cwd": "/path/to/working/directory",
  "useOrchestrator": true,
  "entryNodeId": "node-1",
  "variables": [
    { "name": "topic", "default": "AI safety" }
  ],
  "limits": {
    "maxTotalSteps": 50,
    "maxVisitsPerNode": 10
  },
  "nodes": [],
  "edges": [],
  "agentDefaults": {},
  "ui": {}
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `version` | `number` | Yes | Must be `3` |
| `name` | `string` | No | Display name of the workflow |
| `goal` | `string` | Yes | High-level description of what the workflow achieves |
| `cwd` | `string` | No | Default working directory for agent execution |
| `useOrchestrator` | `boolean` | No | Enable orchestrator for prompt refinement, branch choice, and loop verdicts |
| `entryNodeId` | `string` | Yes | ID of the first node to execute |
| `variables` | `Variable[]` | No | Workflow-level variables available in prompt templates |
| `limits` | `Limits` | No | Execution guardrails |
| `nodes` | `Node[]` | Yes | Array of workflow nodes |
| `edges` | `Edge[]` | Yes | Array of directed edges between nodes |
| `agentDefaults` | `Record<string, AgentDefaults>` | No | Per-agent default configuration |
| `ui` | `UiMetadata` | No | Canvas layout metadata (does not affect runtime) |

### Variables

```json
{ "name": "topic", "default": "machine learning" }
```

Variables are referenced in prompts with `{{var:name}}` syntax.

### Limits

```json
{
  "maxTotalSteps": 50,
  "maxVisitsPerNode": 10
}
```

Guards against runaway execution. If either limit is hit, the run fails.

## Node Types

### Task Node

Executes a prompt using a local agent CLI.

```json
{
  "id": "research",
  "name": "Research Phase",
  "type": "task",
  "agent": "claude",
  "prompt": "Research {{var:topic}} and provide a summary.",
  "contextSources": [
    { "name": "previousContext", "nodeId": "prior-node" }
  ],
  "responseFormat": "text",
  "outputSchema": {
    "type": "object",
    "properties": {
      "summary": { "type": "string" },
      "confidence": { "type": "number" }
    },
    "required": ["summary"]
  },
  "retryCount": 2,
  "retryDelay": 1000,
  "timeout": 30000,
  "skipCondition": {
    "source": "previous_output",
    "type": "contains",
    "value": "SKIP"
  },
  "loopMaxIterations": 5,
  "loopCondition": "Evaluate if more research is needed",
  "agentConfig": {
    "model": "claude-opus-4",
    "reasoningLevel": "medium",
    "systemPrompt": "You are an expert researcher.",
    "accessMode": "read_only",
    "toolToggles": { "webSearch": true },
    "maxTurns": 10,
    "maxBudgetUsd": 5.0,
    "allowedTools": ["Read", "Grep", "Glob"],
    "disallowedTools": ["Bash"]
  },
  "cwd": "/custom/working/directory",
  "continueSessionFrom": "prior-node-id"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | `string` | Yes | Unique node identifier |
| `name` | `string` | Yes | Display name |
| `type` | `"task"` | Yes | Node type |
| `agent` | `string` | No | Agent to use (`claude`, `codex`, `gemini`). Defaults to `claude` |
| `prompt` | `string` | Yes | Prompt text with template substitution support |
| `contextSources` | `ContextSource[]` | No | Additional context from other nodes |
| `responseFormat` | `"text" \| "json"` | No | Expected response format |
| `outputSchema` | `object` | No | JSON Schema for structured output validation |
| `retryCount` | `number` | No | Number of retries on failure |
| `retryDelay` | `number` | No | Milliseconds between retries |
| `timeout` | `number` | No | Execution timeout in milliseconds |
| `skipCondition` | `SkipCondition` | No | Condition to skip this node |
| `loopMaxIterations` | `number` | No | Maximum loop iterations (when node has `loop_continue` edge) |
| `loopCondition` | `string` | No | Prompt for orchestrator to evaluate loop exit |
| `agentConfig` | `AgentNodeConfig` | No | Per-node agent configuration override |
| `cwd` | `string` | No | Working directory override for this node |
| `continueSessionFrom` | `string` | No | Node ID whose agent session to continue |

### Approval Node

Pauses execution and waits for human approval.

```json
{
  "id": "approve",
  "name": "Review Results",
  "type": "approval",
  "prompt": "Do the generated results look correct?"
}
```

When the runtime reaches an approval node:
1. The run pauses and emits an `approval_required` event
2. The frontend shows an approval card with the prompt and last node output
3. The user approves (follows `success` edges) or rejects (follows `reject` edges)
4. Optional user input can be attached to the approval decision

### Split Node

Fans out execution to multiple parallel branches.

```json
{
  "id": "split",
  "name": "Branch by topic",
  "type": "split",
  "prompt": "Determine which research path to take",
  "splitFailurePolicy": "best_effort_continue"
}
```

| `splitFailurePolicy` | Description |
|----------------------|-------------|
| `best_effort_continue` | Continue with successful branches even if some fail |
| `fail_fast_cancel` | Cancel all branches immediately on first failure |
| `drain_then_fail` | Wait for all branches to finish, then fail if any failed |

Split nodes spawn multiple cursors, one per outgoing `branch` edge.

### Collector Node

Convergence point that waits for all incoming branches to arrive.

```json
{
  "id": "collect",
  "name": "Merge Results",
  "type": "collector",
  "prompt": "Synthesize all research findings into a final report.",
  "responseFormat": "json"
}
```

Collectors implement barrier semantics — they wait until all expected inputs have arrived before executing. The `{{all_predecessors}}` template variable provides all collected outputs.

## Edges

```json
{
  "source": "node-1",
  "target": "node-2",
  "outcome": "success",
  "label": "On success"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `source` | `string` | Yes | Source node ID |
| `target` | `string` | Yes | Target node ID |
| `outcome` | `string` | Yes | Edge outcome type |
| `label` | `string` | No | Display label |

### Edge Outcomes

| Outcome | Usage | Visual |
|---------|-------|--------|
| `success` | Normal forward flow from any node | Solid green line |
| `reject` | Rejection path from approval nodes | Dashed red line |
| `branch` | Split branch (one per parallel path) | Dashed orange line |
| `loop_continue` | Loop iteration (back-edge to earlier node) | Dashed teal line |
| `loop_exit` | Exit from a loop | Dashed teal line |

## Prompt Templates

Prompts support template substitution with `{{...}}` syntax:

| Pattern | Description |
|---------|-------------|
| `{{var:name}}` | Substitutes the value of workflow variable `name` |
| `{{previous_output}}` | Output from the immediately preceding node |
| `{{node_name:field}}` | Specific field from a named node's structured output |
| `{{all_predecessors}}` | All ancestor node outputs (useful in collectors) |

### Context Sources

Context sources let a node explicitly reference output from another node:

```json
{
  "contextSources": [
    { "name": "previousContext", "nodeId": "research" }
  ]
}
```

The referenced node's output is injected as additional context in the prompt.

## Agent Defaults

Workflow-level default configuration per agent:

```json
{
  "agentDefaults": {
    "claude": {
      "model": "claude-opus-4",
      "reasoningLevel": "medium",
      "systemPrompt": "You are a helpful assistant.",
      "accessMode": "execute",
      "toolToggles": { "webSearch": false },
      "maxTurns": 20,
      "maxBudgetUsd": 10.0
    },
    "codex": {
      "model": "o4-mini",
      "accessMode": "edit"
    }
  }
}
```

Resolution order: node-level `agentConfig` overrides → workflow-level `agentDefaults` → driver defaults.

### Access Modes

| Mode | Description |
|------|-------------|
| `read_only` | Agent can only read files |
| `edit` | Agent can read and edit files |
| `execute` | Agent can read, edit, and execute commands |
| `unrestricted` | No restrictions on agent actions |

### Agent Config Fields

| Field | Type | Description |
|-------|------|-------------|
| `model` | `string` | Model to use (e.g., `claude-opus-4`, `o4-mini`) |
| `reasoningLevel` | `"low" \| "medium" \| "high"` | Reasoning effort level |
| `systemPrompt` | `string` | System prompt for the agent |
| `accessMode` | `string` | Permission level (see above) |
| `toolToggles` | `{ webSearch?: boolean }` | Toggle specific tools |
| `maxTurns` | `number` | Maximum conversation turns |
| `maxBudgetUsd` | `number` | Maximum cost in USD |
| `allowedTools` | `string[]` | Whitelist of allowed tools (node-level only) |
| `disallowedTools` | `string[]` | Blacklist of disallowed tools (node-level only) |

Not all agents support all fields. The frontend shows only capability-supported fields per agent. See [Agent Drivers](agent-drivers.md) for capability details.

## UI Metadata

Layout metadata that does not affect runtime behavior:

```json
{
  "ui": {
    "canvas": {
      "viewport": { "x": 0, "y": 0, "zoom": 1 },
      "nodes": {
        "node-1": { "x": 180, "y": 180 },
        "node-2": { "x": 400, "y": 300 }
      }
    }
  }
}
```

## Validation

The backend validates workflows and returns issues with severity and location:

- **Errors**: Missing entry node, duplicate node IDs, edges to nonexistent targets, task nodes without prompts
- **Warnings**: Unreachable nodes, dead-end nodes (no outgoing edges)
- **Info**: Graph metadata (reachable nodes, entry point analysis)

Legacy `outputSchema` in `{"field": "type"}` format is automatically migrated to full JSON Schema format.
