export type WorkflowNodeType = "task" | "approval" | "split" | "collector";
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
}

export interface AgentNodeConfig extends AgentDefaults {
  allowedTools?: string[];
  disallowedTools?: string[];
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

export interface WorkflowNode {
  id: string;
  name: string;
  type: WorkflowNodeType;
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
  agentConfig?: AgentNodeConfig | null;
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
