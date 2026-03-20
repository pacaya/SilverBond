import {
  createEdgeId,
  createEmptyWorkflow,
  createNodeId,
  ensureCanvas,
} from "@/lib/types/workflow";
import type {
  RunEvent,
  SplitFailurePolicy,
  ValidationResponse,
  WorkflowDocument,
  WorkflowEdge,
  WorkflowNode,
  WorkflowNodeType,
} from "@/lib/types/workflow";
import { formatTokens } from "@/lib/utils/format";

type Selection =
  | { kind: "workflow"; id: null }
  | { kind: "node"; id: string }
  | { kind: "edge"; id: string };

type NodeRuntimeState =
  | "idle"
  | "running"
  | "success"
  | "failed"
  | "skipped"
  | "orchestrating";

export interface RunLine {
  tone: "system" | "success" | "warning" | "error" | "detail";
  text: string;
}

export interface ApprovalState {
  nodeId: string;
  nodeName: string;
  prompt: string;
  lastOutput: string;
}

export interface InteractionState {
  sessionId: string;
  description: string;
  outputSoFar: string;
  interactionType: "permission" | "question" | "destructive_warning";
}

function defaultNodeName(type: WorkflowNodeType, count: number): string {
  switch (type) {
    case "approval":
      return `Approval ${count}`;
    case "split":
      return `Split ${count}`;
    case "collector":
      return `Collector ${count}`;
    case "task":
    default:
      return `Task ${count}`;
  }
}

function defaultSplitFailurePolicy(): SplitFailurePolicy {
  return "best_effort_continue";
}

/* ── Undo stack ─────────────────────────────────────────────────────── */

interface UndoEntry {
  workflow: WorkflowDocument;
}

const MAX_UNDO = 50;

class WorkflowStore {
  workflow = $state<WorkflowDocument | null>(null);
  validation = $state<ValidationResponse | null>(null);
  selection = $state<Selection>({ kind: "workflow", id: null });
  panelTab = $state<"output" | "history" | "reference">("output");
  dirty = $state(false);
  runId = $state<string | null>(null);
  running = $state(false);
  lines = $state<RunLine[]>([]);
  nodeStates = $state<Record<string, NodeRuntimeState>>({});
  approval = $state<ApprovalState | null>(null);
  interaction = $state<InteractionState | null>(null);
  errorMessage = $state<string>("");

  /* ── undo/redo ────────────────────────────────────────────────────── */
  private undoStack: UndoEntry[] = [];
  private redoStack: UndoEntry[] = [];

  private pushUndo() {
    if (!this.workflow) return;
    this.undoStack.push({ workflow: structuredClone($state.snapshot(this.workflow)) });
    if (this.undoStack.length > MAX_UNDO) this.undoStack.shift();
    this.redoStack = [];
  }

  get canUndo() {
    return this.undoStack.length > 0;
  }

  get canRedo() {
    return this.redoStack.length > 0;
  }

  undo() {
    const entry = this.undoStack.pop();
    if (!entry || !this.workflow) return;
    this.redoStack.push({ workflow: structuredClone($state.snapshot(this.workflow)) });
    this.workflow = entry.workflow;
    this.dirty = true;
  }

  redo() {
    const entry = this.redoStack.pop();
    if (!entry || !this.workflow) return;
    this.undoStack.push({ workflow: structuredClone($state.snapshot(this.workflow)) });
    this.workflow = entry.workflow;
    this.dirty = true;
  }

  /* ── workflow lifecycle ───────────────────────────────────────────── */

  setWorkflow(workflow: WorkflowDocument) {
    this.workflow = ensureCanvas(workflow);
    this.validation = null;
    this.selection = { kind: "workflow", id: null };
    this.dirty = false;
    this.nodeStates = {};
    this.undoStack = [];
    this.redoStack = [];
  }

  createWorkflow() {
    this.workflow = createEmptyWorkflow();
    this.validation = null;
    this.selection = { kind: "workflow", id: null };
    this.dirty = false;
    this.nodeStates = {};
    this.undoStack = [];
    this.redoStack = [];
  }

  updateWorkflow(updater: (workflow: WorkflowDocument) => void) {
    if (!this.workflow) return;
    this.pushUndo();
    updater(this.workflow);
    this.dirty = true;
  }

  setValidation(validation: ValidationResponse | null) {
    this.validation = validation;
  }

  /* ── selection ────────────────────────────────────────────────────── */

  selectWorkflow() {
    this.selection = { kind: "workflow", id: null };
  }

  selectNode(nodeId: string) {
    this.selection = { kind: "node", id: nodeId };
  }

  selectEdge(edgeId: string) {
    this.selection = { kind: "edge", id: edgeId };
  }

  setPanelTab(tab: "output" | "history" | "reference") {
    this.panelTab = tab;
  }

  /* ── node/edge mutations ──────────────────────────────────────────── */

  addNode(type: WorkflowNodeType, position?: { x: number; y: number }) {
    if (!this.workflow) {
      this.createWorkflow();
    }
    this.pushUndo();
    const wf = this.workflow!;
    const id = createNodeId();
    const nodeCount = wf.nodes.filter((n) => n.type === type).length + 1;
    const node: WorkflowNode = {
      id,
      name: defaultNodeName(type, nodeCount),
      type,
      agent: type === "task" ? "claude" : null,
      prompt: "",
      contextSources: [],
      responseFormat: type === "task" ? "text" : null,
      splitFailurePolicy: type === "split" ? defaultSplitFailurePolicy() : undefined,
    };
    wf.nodes.push(node);
    const canvas = ensureCanvas(wf).ui!.canvas!;
    canvas.nodes[id] = position ?? {
      x: 160 + wf.nodes.length * 28,
      y: 140 + wf.nodes.length * 24,
    };
    if (!wf.entryNodeId) wf.entryNodeId = id;
    this.dirty = true;
    this.selection = { kind: "node", id };
  }

  removeNode(nodeId: string) {
    if (!this.workflow) return;
    this.pushUndo();
    const wf = this.workflow;
    wf.nodes = wf.nodes.filter((n) => n.id !== nodeId);
    wf.edges = wf.edges.filter((e) => e.from !== nodeId && e.to !== nodeId);
    delete wf.ui?.canvas?.nodes[nodeId];
    wf.nodes.forEach((n) => {
      n.contextSources = (n.contextSources ?? []).filter((c) => c.nodeId !== nodeId);
    });
    if (wf.entryNodeId === nodeId) wf.entryNodeId = "";
    this.dirty = true;
    this.selection = { kind: "workflow", id: null };
  }

  addEdge(edge: Omit<WorkflowEdge, "id">) {
    if (!this.workflow) return;
    this.pushUndo();
    const nextEdge: WorkflowEdge = { ...edge, id: createEdgeId() };
    this.workflow.edges.push(nextEdge);
    this.dirty = true;
    this.selection = { kind: "edge", id: nextEdge.id };
  }

  removeEdge(edgeId: string) {
    if (!this.workflow) return;
    this.pushUndo();
    this.workflow.edges = this.workflow.edges.filter((e) => e.id !== edgeId);
    this.dirty = true;
    this.selection = { kind: "workflow", id: null };
  }

  setNodePosition(nodeId: string, position: { x: number; y: number }) {
    if (!this.workflow) return;
    const canvas = ensureCanvas(this.workflow).ui!.canvas!;
    canvas.nodes[nodeId] = position;
    this.dirty = true;
  }

  /* ── runtime ──────────────────────────────────────────────────────── */

  appendLine(line: RunLine) {
    this.lines.push(line);
  }

  clearLines() {
    this.lines = [];
  }

  resetRun() {
    this.runId = null;
    this.running = false;
    this.approval = null;
    this.interaction = null;
    this.nodeStates = {};
  }

  setRunState(patch: { runId?: string | null; running?: boolean; approval?: ApprovalState | null; interaction?: InteractionState | null }) {
    if (patch.runId !== undefined) this.runId = patch.runId;
    if (patch.running !== undefined) this.running = patch.running;
    if (patch.approval !== undefined) this.approval = patch.approval;
    if (patch.interaction !== undefined) this.interaction = patch.interaction;
  }

  setNodeRuntimeState(nodeId: string, state: NodeRuntimeState) {
    this.nodeStates[nodeId] = state;
  }

  setError(message: string) {
    this.errorMessage = message;
    if (message) setTimeout(() => { this.errorMessage = ""; }, 5000);
  }

  applyRunEvent(event: RunEvent) {
    const nodeId = typeof event.nodeId === "string" ? event.nodeId : null;
    const push = (tone: RunLine["tone"], text: string) => this.lines.push({ tone, text });

    switch (event.type) {
      case "run_start":
      case "run_resumed":
        push("system", `Run ${String(event.runId ?? "")} started`);
        break;
      case "orchestrator_start":
        if (nodeId) this.nodeStates[nodeId] = "orchestrating";
        push("detail", `Orchestrator refining ${String(event.nodeName ?? nodeId ?? "")}`);
        break;
      case "orchestrator_done":
        push("detail", `Prompt refined in ${String(event.duration ?? "0")}s`);
        break;
      case "node_start":
        if (nodeId) this.nodeStates[nodeId] = "running";
        push("system", `Running ${String(event.nodeName ?? nodeId ?? "")}`);
        break;
      case "node_done": {
        const result = (event.result ?? {}) as Record<string, unknown>;
        if (nodeId) this.nodeStates[nodeId] = result.success ? "success" : "failed";
        const outcomeLabel = result.errorType ? ` (${String(result.errorType)})` : "";
        push(
          result.success ? "success" : "error",
          `${String(event.nodeName ?? nodeId ?? "")} ${result.success ? "completed" : "failed"}${outcomeLabel}`,
        );
        if (typeof result.output === "string" && result.output) push("detail", result.output);
        if (typeof result.stderr === "string" && result.stderr) push("error", result.stderr);
        // Show metadata summary when available
        const metaParts: string[] = [];
        if (result.duration) metaParts.push(`${String(result.duration)}s`);
        if (result.modelUsed) metaParts.push(String(result.modelUsed));
        if (typeof result.inputTokens === "number" || typeof result.outputTokens === "number") {
          const inp = (typeof result.inputTokens === "number" ? result.inputTokens : 0) as number;
          const out = (typeof result.outputTokens === "number" ? result.outputTokens : 0) as number;
          metaParts.push(`${formatTokens(inp)}→${formatTokens(out)} tok`);
        }
        if (typeof result.costUsd === "number") {
          metaParts.push(`$${(result.costUsd as number).toFixed(4)}`);
        }
        if (typeof result.numTurns === "number" && result.numTurns > 1) {
          metaParts.push(`${String(result.numTurns)} turns`);
        }
        if (metaParts.length > 0) push("detail", `  ↳ ${metaParts.join(" · ")}`);
        break;
      }
      case "node_retry":
        push("warning", `Retrying ${String(event.nodeName ?? nodeId ?? "")}`);
        break;
      case "node_skipped":
        if (nodeId) this.nodeStates[nodeId] = "skipped";
        push("warning", `Skipped ${String(event.nodeName ?? nodeId ?? "")}`);
        break;
      case "branch_decision":
        push("detail", `Branch: ${String(event.chosenLabel ?? event.chosenBranch ?? "")}`);
        break;
      case "cursor_spawned":
        push(
          "detail",
          `Spawned cursor ${String(event.cursorId ?? "")} from ${String(event.fromNodeId ?? nodeId ?? "")}`,
        );
        break;
      case "collector_waiting":
        if (nodeId) this.nodeStates[nodeId] = "running";
        push(
          "detail",
          `Collector ${String(event.nodeName ?? nodeId ?? "")} waiting on ${String(event.arrived ?? 0)}/${String(event.required ?? 0)} inputs`,
        );
        break;
      case "collector_released":
        if (nodeId) this.nodeStates[nodeId] = "success";
        push("detail", `Collector ${String(event.nodeName ?? nodeId ?? "")} released`);
        break;
      case "aggregate_merged":
        push("detail", `Merged collector inputs for ${String(nodeId ?? "")}`);
        break;
      case "approval_queued":
        push("warning", `Approval queued for ${String(event.nodeName ?? nodeId ?? "")}`);
        break;
      case "cursor_cancelled":
        push("warning", `Cursor ${String(event.cursorId ?? "")} ended with ${String(event.status ?? "cancelled")}`);
        break;
      case "loop_decision":
        push("detail", `Loop verdict: ${String(event.verdict ?? "")}`);
        break;
      case "approval_required":
        push("warning", `Approval required for ${String(event.nodeName ?? nodeId ?? "")}`);
        this.approval = {
          nodeId: String(event.nodeId ?? ""),
          nodeName: String(event.nodeName ?? ""),
          prompt: String(event.prompt ?? ""),
          lastOutput: String(event.lastOutput ?? ""),
        };
        break;
      case "transition":
        push("detail", `${String(event.fromNodeId ?? "")} -> ${String(event.toNodeId ?? "(end)")}`);
        break;
      case "workflow_error":
        push("error", String(event.message ?? "Workflow error"));
        break;
      case "sys_warn":
        push("warning", String(event.message ?? "Warning"));
        break;
      case "agent_interaction_required":
        push("warning", `Agent interaction required: ${String(event.description ?? "")}`);
        this.interaction = {
          sessionId: String(event.sessionId ?? ""),
          description: String(event.description ?? ""),
          outputSoFar: String(event.outputSoFar ?? ""),
          interactionType: (event.interactionType as InteractionState["interactionType"]) ?? "question",
        };
        break;
      case "agent_interaction_resolved":
        push("detail", `Interaction resolved: ${String(event.description ?? "")}`);
        this.interaction = null;
        break;
      case "done":
        push(event.aborted ? "error" : "success", event.aborted ? "Workflow aborted" : "Workflow complete");
        this.approval = null;
        this.interaction = null;
        this.running = false;
        break;
      default:
        break;
    }

    if (typeof event.runId === "string") this.runId = event.runId;
  }
}

export const store = new WorkflowStore();
