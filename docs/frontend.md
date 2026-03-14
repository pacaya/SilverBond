# Frontend Architecture

The Svelte 5 frontend is an authoring and inspection surface over the Rust runtime. It is not a second source of workflow truth — all validation, execution, and persistence happens on the backend.

## Tech Stack

| Technology | Version | Purpose |
|-----------|---------|---------|
| Svelte | 5.53 | UI framework (using runes: `$state`, `$derived`, `$effect`) |
| Vite | 7.3 | Build tool and dev server |
| TypeScript | 5.8 | Type safety |
| TanStack Svelte Query | 6.1 | Server state management |
| SvelteFlow (`@xyflow/svelte`) | 1.1 | Graph visualization and editing |
| clsx | 2.1 | Conditional CSS class composition |
| Vitest | 4.0 | Unit testing |
| Playwright | 1.52 | End-to-end testing |

## Source Layout

```
ui/src/
├── main.ts                           # Application entry point
├── styles.css                        # Global styles
├── app/
│   ├── App.svelte                    # Root component (QueryClientProvider)
│   └── AppShell.svelte               # Main layout, query orchestration, mutations
├── features/
│   ├── editor/
│   │   ├── GraphEditor.svelte        # SvelteFlow canvas and node interaction
│   │   ├── InspectorPanel.svelte     # Node/edge/workflow property editor
│   │   ├── flowNodes.ts              # Workflow → SvelteFlow node/edge conversion
│   │   └── flowNodes.test.ts         # Unit tests for flow node conversion
│   ├── runtime/
│   │   └── RunPanel.svelte           # Execution log and approval UI
│   ├── history/
│   │   └── HistoryPanel.svelte       # Past runs, logs, and execution details
│   ├── reference/
│   │   ├── ReferencePanel.svelte     # Schema and capability reference
│   │   ├── ReferenceSection.svelte   # Collapsible reference section
│   │   └── referenceData.ts          # Static reference content
│   └── workflows/
│       └── Sidebar.svelte            # Workflow list, create/load/delete
├── lib/
│   ├── api/
│   │   └── client.ts                 # HTTP API client
│   ├── stores/
│   │   ├── workflowStore.svelte.ts   # Client-side editor state
│   │   └── workflowStore.test.ts     # Store unit tests
│   ├── types/
│   │   └── workflow.ts               # TypeScript type definitions
│   ├── components/
│   │   ├── AutocompletePopup.svelte  # Template variable autocomplete
│   │   ├── ConfirmDialog.svelte      # Delete confirmation modal
│   │   └── PromptTextarea.svelte     # Prompt editor with autocomplete
│   └── utils/
│       ├── caretPosition.ts          # Textarea caret position measurement
│       ├── format.ts                 # Token count formatting (5.4k, 1.0M)
│       └── templateSuggestions.ts    # Autocomplete suggestion generation
└── test/
    └── setup.ts                      # Vitest setup (jsdom)
```

## Component Hierarchy

```
App.svelte
└── QueryClientProvider
    └── AppShell.svelte
        ├── Sidebar.svelte          (left panel: workflow list)
        ├── GraphEditor.svelte      (center: visual graph editor)
        ├── InspectorPanel.svelte   (right panel: property editor)
        ├── RunPanel.svelte         (bottom: execution output)
        ├── HistoryPanel.svelte     (tab: past runs and logs)
        ├── ReferencePanel.svelte   (tab: schema reference)
        └── ConfirmDialog.svelte    (modal: delete confirmation)
```

## Key Components

### AppShell.svelte

The main orchestration component. Responsibilities:

- Renders five TanStack queries: workflows, templates, capabilities, interrupted-runs, logs
- Debounced workflow validation (500ms delay after edits)
- Manages all mutations: save, delete, create run, approve, abort, resume, restart, dismiss
- Tracks active run state: `runId`, `running`, `lines`, `approval`, `errorMessage`
- Streams SSE events during active runs

### GraphEditor.svelte

The SvelteFlow-powered graph canvas. Key patterns:

- Uses `$state.raw` for nodes and edges (critical — see [Svelte Specifics](svelte-specifics.md))
- Converts workflow data to SvelteFlow nodes/edges via `buildFlowNodes()` and `toFlowEdges()`
- Merges with previous positions to preserve layout during reactive updates
- Color-coded edges by outcome type
- Validation badges (error/warning) and runtime state indicators (running/success/failed)
- Drag-and-drop node creation, click selection, double-click text editing
- Canvas viewport and node positions persisted in `workflow.ui.canvas`

### InspectorPanel.svelte

Context-sensitive property editor with three modes:

1. **Workflow selected**: Edit name, goal, cwd, orchestrator toggle, variables, limits, and per-agent defaults
2. **Node selected**: Edit all node properties including agent config, output schema, skip conditions, loop settings
3. **Edge selected**: Edit outcome type and label

Features:
- Capability-gated fields — only shows config options the selected agent supports
- Shared `agentConfigFields` snippet used for both node-level and workflow-level defaults
- Output schema editor with JSON Schema validation
- Continue-session-from dropdown (filters to upstream task nodes with matching agent)

### RunPanel.svelte

Displays real-time execution output:
- Scrollable log of runtime event lines
- Approval card showing prompt, last output, and user input textarea
- Run/Abort action buttons
- Auto-scrolls to bottom on new lines

### HistoryPanel.svelte

Shows past execution history:
- Lists interrupted runs (resumable) and completed execution logs
- Detail view with per-node execution entries
- Run summary: node count, success/failed badges, total tokens, total cost, duration
- Per-node metadata: outcome badge, token breakdown, cost, model, session ID, turns, duration

### Sidebar.svelte

Workflow management:
- Lists saved workflows and templates
- New workflow creation
- Load and delete workflows
- Search/filter functionality

## Workflow Store

`workflowStore.svelte.ts` is a class-based Svelte 5 store using runes:

```typescript
class WorkflowStore {
  // Core state
  workflow = $state<WorkflowDocument | null>(null);
  validation = $state<ValidationResponse | null>(null);
  selection = $state<Selection>({ type: "workflow" });
  dirty = $state(false);

  // Run state
  running = $state(false);
  errorMessage = $state<string | null>(null);
  lines = $state<string[]>([]);
  nodeStates = $state<Record<string, string>>({});
  approval = $state<ApprovalState | null>(null);

  // Undo/redo (max 50 entries)
  undo(): void;
  redo(): void;
  get canUndo(): boolean;
  get canRedo(): boolean;

  // Mutations
  addNode(type, position): void;
  deleteNode(id): void;
  updateNode(id, changes): void;
  updateEdge(source, target, changes): void;
}

export const store = new WorkflowStore();
```

Undo/redo is backed by `$state.snapshot()` serialization. The dirty flag tracks unsaved changes.

## API Client

`client.ts` provides a typed API client using `fetch`:

```typescript
export const api = {
  capabilities(): Promise<RuntimeCapabilities>,
  workflows(): Promise<WorkflowItem[]>,
  saveWorkflow(name, workflow): Promise<void>,
  deleteWorkflow(name): Promise<void>,
  templates(): Promise<TemplateItem[]>,
  validateWorkflow(workflow): Promise<ValidationResponse>,
  testNode(node, cwd, mockContext): Promise<NodeTestPreview>,
  createRun(workflow, variableOverrides): Promise<{ runId: string }>,
  approveRun(runId, approved, userInput): Promise<void>,
  abortRun(runId): Promise<void>,
  resumeRun(runId): Promise<void>,
  restartFromNode(runId, nodeId): Promise<void>,
  dismissRun(runId): Promise<void>,
  interruptedRuns(): Promise<InterruptedRun[]>,
  logs(): Promise<LogListItem[]>,
  log(id): Promise<ExecutionLogDetail>,
  deleteLog(id): Promise<void>,
  runEvents(runId): Promise<RunEvent[]>,
};

export async function streamRun(runId, onEvent): Promise<void>;
```

`streamRun` uses `EventSource` for SSE streaming of runtime events.

## Utility Modules

### templateSuggestions.ts

Generates autocomplete suggestions for prompt editing:
- Workflow variables: `{{var:name}}`
- Context sources: `{{previous_output}}`
- Node outputs: `{{node_name:field}}` (extracts fields from JSON Schema properties)
- Predecessor outputs: `{{all_predecessors}}`

### flowNodes.ts

Converts between workflow data and SvelteFlow representation:
- `buildFlowNodes()` — creates positioned, styled nodes with validation/runtime badges
- `buildValidationIndex()` — indexes validation issues by node ID for fast lookup
- `mergeFlowNodes()` / `mergeFlowEdges()` — preserves positions during reactive updates

### format.ts

- `formatTokens(n)` — formats large numbers for display (`1000000` → `"1.0M"`, `5400` → `"5.4k"`)

## Build Configuration

### Vite (`ui/vite.config.ts`)

- Root: `./ui`
- Dev server: `127.0.0.1:5173` with `/api` proxy to `:3333`
- Build output: `../public` (embedded by Rust at compile time)
- Test: jsdom environment, CSS enabled, excludes e2e tests

### Svelte (`ui/svelte.config.js`)

Uses `vitePreprocess` for TypeScript support.

## Critical Patterns

See [Svelte Specifics](svelte-specifics.md) for essential rules around:
- Using `$state.raw` (not `$state`) for SvelteFlow nodes/edges
- Rebuilding nested objects to avoid Proxy leaks
- Pure template functions during render
- Required `bind:nodes` / `bind:edges` on SvelteFlow
