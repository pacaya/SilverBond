export type WorkflowNodeType =
  | "task"
  | "approval"
  | "split"
  | "collector"
  | "decide"
  | "parallel_batch"
  | "subflow"
  | "call"
  | "spawn"
  | "send"
  | "wait"
  | "capture"
  | "kill"
  | "run_agent";

export type WaitMode = "idle" | "ready" | "until";
export type WorkflowEdgeOutcome =
  | "success"
  | "reject"
  | "branch"
  | "loop_continue"
  | "loop_exit";
export type ResponseFormat = "text" | "json";
export type SplitFailurePolicy =
  | "best_effort_continue"
  | "fail_fast_cancel"
  | "drain_then_fail";

export type AccessMode = "read_only" | "edit" | "execute" | "unrestricted";

export type NodeOutcome =
  | "success"
  | "error_execution"
  | "error_max_turns"
  | "error_max_budget"
  | "error_schema_validation"
  | "error_timeout"
  | "error_not_found";

export type ReasoningLevel = "low" | "medium" | "high";

export interface ToolToggles {
  webSearch?: boolean;
}

export interface AgentDefaults {
  model?: string;
  reasoningLevel?: ReasoningLevel;
  systemPrompt?: string;
  accessMode?: AccessMode;
  toolToggles?: ToolToggles;
  maxTurns?: number;
  maxBudgetUsd?: number;
  autoApprove?: boolean;
  orchestrator?: OrchestratorConfig;
}

export interface AgentNodeConfig extends AgentDefaults {
  allowedTools?: string[];
  disallowedTools?: string[];
}

export type OrchestratorActivation = "stale_only" | "always_on";

export interface OrchestratorConfig {
  enabled: boolean;
  model?: string;
  activation?: OrchestratorActivation;
  systemPrompt?: string;
  staleTimeoutSecs?: number;
  subagentTimeoutSecs?: number;
}

export interface WorkflowVariable {
  name: string;
  default: string;
}

export interface ContextSource {
  name: string;
  nodeId: string;
}

export interface StructuredCondition {
  field: string;
  operator: string;
  value: string;
}

export interface SkipCondition {
  source: string;
  type: string;
  value: string;
}

export interface WorkflowLimits {
  maxTotalSteps: number;
  maxVisitsPerNode: number;
}

/**
 * Workflow-level "run as" / sandbox configuration. Mirrors the backend
 * `RunAsConfig` (serialized camelCase). Controls how spawned panes are launched:
 * under a different user, via a custom command prefix, and on a dedicated tmux
 * socket.
 */
export interface RunAsConfig {
  user?: string;
  command?: string[];
  socket?: string;
}

export interface WorkflowUiCanvasNode {
  x: number;
  y: number;
}

export interface WorkflowUiCanvas {
  viewport: {
    x: number;
    y: number;
    zoom: number;
  };
  nodes: Record<string, WorkflowUiCanvasNode>;
}

export interface WorkflowUiState {
  canvas?: WorkflowUiCanvas;
}

/** A single input binding for decide / subflow / call nodes. */
export interface InputBinding {
  name: string;
  source: string;
}

/** LLM-routed branch decision node. */
export interface DecideConfig {
  inputs?: InputBinding[];
  prompt: string;
  model?: string | null;
  outcomes?: string[];
}

/** Fan-out over a collection, running a subgraph body per item. */
export interface BatchConfig {
  itemsBinding: string;
  maxConcurrent: number;
  itemVar: string;
  bodyEntry: string;
  collectorVar?: string | null;
}

/** Spawn an agent/command into a managed PTY pane. */
export interface SpawnConfig {
  agent?: string | null;
  command?: string | null;
  access?: string | null;
  extraArgs?: string[];
  cwd?: string | null;
  name?: string | null;
  sessionName?: string | null;
}

/** Send text/keys to a running pane. */
export interface SendConfig {
  target?: string | null;
  text: string;
  enter: boolean;
}

/** Wait on a pane until idle / ready / a marker appears. */
export interface WaitConfig {
  target?: string | null;
  mode: WaitMode;
  marker?: string | null;
  timeout?: number | null;
  idleSeconds?: number | null;
  readyStableSeconds?: number | null;
}

/** Capture pane output into the run context. */
export interface CaptureConfig {
  target?: string | null;
  lines?: number | null;
  all: boolean;
  ansi: boolean;
}

/** Terminate a running pane/session. */
export interface KillConfig {
  target?: string | null;
  sessionName?: string | null;
}

/** One-shot: spawn an agent, wait for completion, capture, optionally kill. */
export interface RunAgentConfig {
  agent?: string | null;
  prompt?: string | null;
  cwd?: string | null;
  access?: string | null;
  extraArgs?: string[];
  name?: string | null;
  timeout?: number | null;
  idleSeconds?: number | null;
  readyStableSeconds?: number | null;
  until?: string | null;
  killAfter: boolean;
}

/** Reference to a reusable saved subgraph (compound node). */
export interface SubflowConfig {
  workflowName: string;
  exitNodeId?: string | null;
  inputs?: InputBinding[];
  maxDepth: number;
}

// Internally-tagged kind variants (mirrors Rust #[serde(tag = "type")]).
export type NodeKind =
  | { type: "task"; agentConfig?: AgentNodeConfig | null }
  | { type: "approval" }
  | { type: "split" }
  | { type: "collector" }
  | { type: "decide"; decideConfig: DecideConfig }
  | { type: "parallel_batch"; batchConfig: BatchConfig }
  | { type: "subflow"; subflowConfig: SubflowConfig }
  | { type: "call"; subflowConfig: SubflowConfig }
  | { type: "spawn"; spawnConfig?: SpawnConfig }
  | { type: "send"; sendConfig?: SendConfig }
  | { type: "wait"; waitConfig?: WaitConfig }
  | { type: "capture"; captureConfig: CaptureConfig }
  | { type: "kill"; killConfig: KillConfig }
  | { type: "run_agent"; runAgentConfig?: RunAgentConfig; agentConfig?: AgentNodeConfig | null };

export interface WorkflowNode {
  id: string;
  name: string;
  kind: NodeKind;
  agent?: string | null;
  prompt: string;
  contextSources?: ContextSource[];
  responseFormat?: ResponseFormat | null;
  outputSchema?: Record<string, unknown> | null;
  retryCount?: number | null;
  retryDelay?: number | null;
  timeout?: number | null;
  skipCondition?: SkipCondition | null;
  loopMaxIterations?: number | null;
  loopCondition?: StructuredCondition | null;
  splitFailurePolicy?: SplitFailurePolicy | null;
  cwd?: string | null;
  continueSessionFrom?: string | null;
}

export interface WorkflowEdge {
  id: string;
  from: string;
  to: string;
  outcome: WorkflowEdgeOutcome;
  label?: string | null;
  branchId?: string | null;
  condition?: StructuredCondition | null;
}

export interface WorkflowDocument {
  version: 3;
  name?: string | null;
  goal: string;
  cwd: string;
  useOrchestrator: boolean;
  entryNodeId: string;
  variables: WorkflowVariable[];
  limits: WorkflowLimits;
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
  agentDefaults?: Record<string, AgentDefaults>;
  /** Optional "run as" / sandbox config applied when launching panes. */
  runAs?: RunAsConfig;
  /** Run-local catalog of saved subgraphs referenced by subflow/call nodes. */
  subflows?: Record<string, WorkflowDocument>;
  ui?: WorkflowUiState;
}

export interface ValidationIssue {
  severity: "error" | "warning";
  nodeId?: string | null;
  message: string;
}

export interface GraphMetadata {
  reachableNodeIds: string[];
  unreachableNodeIds: string[];
  deadEndNodeIds: string[];
}

export interface ValidationResponse {
  workflow: WorkflowDocument;
  issues: ValidationIssue[];
  graph: GraphMetadata;
}

export interface AgentCapabilities {
  workerExecution: boolean;
  promptRefinement: boolean;
  branchChoice: boolean;
  loopVerdict: boolean;
  structuredOutput: boolean;
  sessionReuse: boolean;
  nativeJsonSchema: boolean;
  modelSelection: boolean;
  reasoningConfig: boolean;
  systemPrompt: boolean;
  budgetLimit: boolean;
  turnLimit: boolean;
  costReporting: boolean;
  toolAllowlist: boolean;
  webSearch: boolean;
}

export interface RuntimeCapabilities {
  workflowVersion: number;
  supportedNodeTypes: WorkflowNodeType[];
  supportedEdgeOutcomes: WorkflowEdgeOutcome[];
  features: {
    split: boolean;
    collector: boolean;
  };
  agents: Record<
    string,
    {
      available: boolean;
      path?: string;
      capabilities: AgentCapabilities;
    }
  >;
}

export interface WorkflowItem {
  name: string;
  filename: string;
  workflow: WorkflowDocument;
}

export interface TemplateItem {
  name: string;
  description?: string | null;
  workflow: WorkflowDocument;
  templateFile: string;
}

export interface RunEvent {
  type: string;
  [key: string]: unknown;
}

export interface InterruptedRun {
  runId: string;
  status: string;
  workflowName: string;
  currentNodeId?: string | null;
  currentNodeName?: string | null;
  totalExecuted: number;
  updatedAt: string;
}

export interface LogListItem {
  id: string;
  filename: string;
  workflowName: string;
  goal: string;
  startTime: string;
  endTime?: string | null;
  totalDuration: string;
  nodeExecutionCount?: number;
  aborted: boolean;
  runId?: string | null;
  totalCostUsd?: number | null;
  totalInputTokens?: number | null;
  totalOutputTokens?: number | null;
  nodesSucceeded?: number;
  nodesFailed?: number;
}

export interface NodeExecutionEntry {
  cursorId?: string | null;
  nodeId?: string;
  nodeName?: string;
  nodeType?: string;
  agent?: string;
  output?: string;
  stderr?: string;
  exitCode?: number;
  success?: boolean;
  duration?: string;
  iteration?: number;
  attempts?: number;
  timestamp?: string;
  outcome?: NodeOutcome | null;
  errorType?: string | null;
  agentSessionId?: string | null;
  costUsd?: number | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
  thinkingTokens?: number | null;
  cacheReadTokens?: number | null;
  cacheWriteTokens?: number | null;
  modelUsed?: string | null;
  numTurns?: number | null;
}

export interface ExecutionLogDetail {
  runId?: string | null;
  workflowName: string;
  goal: string;
  cwd: string;
  startTime: string;
  endTime?: string | null;
  totalDuration: string;
  aborted: boolean;
  nodeExecutions?: NodeExecutionEntry[];
  decisions?: Array<Record<string, unknown>>;
  transitions?: Array<Record<string, unknown>>;
  terminalReason?: string | null;
}

export interface NodeTestContext {
  variables?: Record<string, string>;
  nodeOutputs?: Record<string, string>;
  previousOutput?: string;
  branchOrigin?: string;
  branchChoice?: string;
}

export interface NodeTestPreview {
  resolvedPrompt: string;
  parsedOutput?: unknown;
  parseError?: string;
  routingPreview?: unknown;
  output: string;
  stderr: string;
  success: boolean;
  duration: string;
}

export function createEmptyWorkflow(): WorkflowDocument {
  return {
    version: 3,
    name: "",
    goal: "",
    cwd: "",
    useOrchestrator: false,
    entryNodeId: "",
    variables: [],
    limits: {
      maxTotalSteps: 50,
      maxVisitsPerNode: 10,
    },
    nodes: [],
    edges: [],
    ui: {
      canvas: {
        viewport: {
          x: 0,
          y: 0,
          zoom: 1,
        },
        nodes: {},
      },
    },
  };
}

export function ensureCanvas(workflow: WorkflowDocument): WorkflowDocument {
  if (workflow.ui?.canvas) {
    return workflow;
  }
  return {
    ...workflow,
    ui: {
      ...workflow.ui,
      canvas: {
        viewport: { x: 0, y: 0, zoom: 1 },
        nodes: Object.fromEntries(
          workflow.nodes.map((node, index) => [
            node.id,
            { x: 120 + index * 220, y: 120 + (index % 3) * 140 },
          ]),
        ),
      },
    },
  };
}

export function cloneWorkflow<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

export function createNodeId(): string {
  return `node_${crypto.randomUUID().slice(0, 8)}`;
}

export function createEdgeId(): string {
  return `edge_${crypto.randomUUID().slice(0, 8)}`;
}

export function duplicateWorkflowForEditing(workflow: WorkflowDocument): WorkflowDocument {
  return ensureCanvas(cloneWorkflow(workflow));
}

export function remapWorkflowIds(workflow: WorkflowDocument): WorkflowDocument {
  const idMap = new Map<string, string>();
  workflow.nodes.forEach((node) => {
    idMap.set(node.id, createNodeId());
  });

  const nextNodes = workflow.nodes.map((node) => ({
    ...node,
    id: idMap.get(node.id)!,
    contextSources: (node.contextSources ?? []).map((context) => ({
      ...context,
      nodeId: idMap.get(context.nodeId) ?? context.nodeId,
    })),
  }));

  const nextEdges = workflow.edges.map((edge) => ({
    ...edge,
    id: createEdgeId(),
    from: idMap.get(edge.from) ?? edge.from,
    to: idMap.get(edge.to) ?? edge.to,
  }));

  const nextCanvasNodes = Object.fromEntries(
    Object.entries(workflow.ui?.canvas?.nodes ?? {}).map(([nodeId, position]) => [
      idMap.get(nodeId) ?? nodeId,
      position,
    ]),
  );

  return ensureCanvas({
    ...workflow,
    name: workflow.name ? `${workflow.name} copy` : "Untitled copy",
    entryNodeId: idMap.get(workflow.entryNodeId) ?? "",
    nodes: nextNodes,
    edges: nextEdges,
    ui: {
      ...workflow.ui,
      canvas: {
        viewport: workflow.ui?.canvas?.viewport ?? { x: 0, y: 0, zoom: 1 },
        nodes: nextCanvasNodes,
      },
    },
  });
}
