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

export type PaneStatus = "idle" | "connecting" | "open" | "closed";

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
    case "decide":
      return `Decide ${count}`;
    case "parallel_batch":
      return `Batch ${count}`;
    case "subflow":
      return `Subflow ${count}`;
    case "call":
      return `Call ${count}`;
    case "spawn":
      return `Spawn ${count}`;
    case "send":
      return `Send ${count}`;
    case "wait":
      return `Wait ${count}`;
    case "capture":
      return `Capture ${count}`;
    case "kill":
      return `Kill ${count}`;
    case "run_agent":
      return `Run Agent ${count}`;
    case "task":
    default:
      return `Task ${count}`;
  }
}

/** Minimal valid per-type config so a freshly-dropped node round-trips through backend serde. */
function defaultNodeConfig(type: WorkflowNodeType): Partial<WorkflowNode> {
  switch (type) {
    case "decide":
      return { decideConfig: { prompt: "", inputs: [], outcomes: [] } };
    case "parallel_batch":
      return {
        batchConfig: { itemsBinding: "", maxConcurrent: 4, itemVar: "item", bodyEntry: "" },
      };
    case "run_agent":
      // agent lives on node.agent (set in addNode); prompt stays unset so it
      // does not shadow node.prompt on the backend (Some("") would win).
      return { runAgentConfig: { killAfter: true } };
    case "spawn":
      return { spawnConfig: { agent: "claude" } };
    case "send":
      return { sendConfig: { text: "", enter: true } };
    case "wait":
      return { waitConfig: { mode: "idle" } };
    case "capture":
      return { captureConfig: { all: false, ansi: false } };
    case "kill":
      return { killConfig: {} };
    case "subflow":
    case "call":
      return { subflowConfig: { workflowName: "", inputs: [], maxDepth: 10 } };
    default:
      return {};
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
  panelTab = $state<"output" | "history" | "reference" | "terminal">("output");
  dirty = $state(false);
  runId = $state<string | null>(null);
  running = $state(false);
  lines = $state<RunLine[]>([]);
  nodeStates = $state<Record<string, NodeRuntimeState>>({});
  approval = $state<ApprovalState | null>(null);
  interaction = $state<InteractionState | null>(null);
  errorMessage = $state<string>("");

  /* ── live pane terminal ───────────────────────────────────────────── */
  selectedPane = $state<string>("active");
  paneStatus = $state<PaneStatus>("idle");
  paneError = $state<string>("");

  /* ── compound-node drill-in (breadcrumb path of subflow names) ─────── */
  drillStack = $state<string[]>([]);
  /** Node ids in the current marquee/multi-selection (for "save as compound"). */
  multiSelectedNodeIds = $state<string[]>([]);

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
    this.drillStack = [];
    this.multiSelectedNodeIds = [];
  }

  createWorkflow() {
    this.workflow = createEmptyWorkflow();
    this.validation = null;
    this.selection = { kind: "workflow", id: null };
    this.dirty = false;
    this.nodeStates = {};
    this.undoStack = [];
    this.redoStack = [];
    this.drillStack = [];
    this.multiSelectedNodeIds = [];
  }

  /* ── compound-node drill-in ───────────────────────────────────────── */

  /**
   * Resolve the document currently shown on the canvas by walking the root
   * workflow's `subflows` catalog along the breadcrumb path. Returns the root
   * when not drilled in. Guarantees the returned doc has a `ui.canvas`.
   */
  private resolveActive(root: WorkflowDocument): WorkflowDocument {
    let doc = root;
    for (const name of this.drillStack) {
      const next = doc.subflows?.[name];
      if (!next) break;
      doc = next;
    }
    if (!doc.ui?.canvas) {
      doc.ui = {
        ...doc.ui,
        canvas: { viewport: { x: 0, y: 0, zoom: 1 }, nodes: {} },
      };
    }
    return doc;
  }

  /** The document currently shown on the canvas (root or a nested subflow). */
  get activeWorkflow(): WorkflowDocument | null {
    if (!this.workflow) return null;
    return this.resolveActive(this.workflow);
  }

  /** Whether the editor is currently drilled into a subflow. */
  get isDrilledIn(): boolean {
    return this.drillStack.length > 0;
  }

  /** Breadcrumb trail: ["root", ...subflow names]. */
  get breadcrumb(): string[] {
    return ["root", ...this.drillStack];
  }

  /** Double-click a subflow/call node to open its referenced subgraph. */
  drillIntoSubflow(nodeId: string): boolean {
    if (!this.workflow) return false;
    const active = this.resolveActive(this.workflow);
    const node = active.nodes.find((n) => n.id === nodeId);
    const name = node?.subflowConfig?.workflowName;
    if (!name || !this.workflow.subflows?.[name]) return false;
    this.drillStack = [...this.drillStack, name];
    this.selection = { kind: "workflow", id: null };
    this.multiSelectedNodeIds = [];
    return true;
  }

  /** Jump to a breadcrumb level. index 0 = root. */
  drillToLevel(index: number) {
    this.drillStack = this.drillStack.slice(0, Math.max(0, index));
    this.selection = { kind: "workflow", id: null };
    this.multiSelectedNodeIds = [];
  }

  setMultiSelection(ids: string[]) {
    this.multiSelectedNodeIds = ids;
  }

  updateWorkflow(updater: (workflow: WorkflowDocument) => void) {
    if (!this.workflow) return;
    this.pushUndo();
    // Mutations target the document currently on the canvas (root or subflow).
    updater(this.resolveActive(this.workflow));
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

  setPanelTab(tab: "output" | "history" | "reference" | "terminal") {
    this.panelTab = tab;
  }

  /* ── live pane terminal ───────────────────────────────────────────── */

  selectPane(pane: string) {
    this.selectedPane = pane;
  }

  setPaneStatus(status: PaneStatus) {
    this.paneStatus = status;
    // A healthy (re)connection clears any prior transient error.
    if (status === "open" || status === "idle") this.paneError = "";
  }

  setPaneError(message: string) {
    this.paneError = message;
  }

  /* ── node/edge mutations ──────────────────────────────────────────── */

  addNode(type: WorkflowNodeType, position?: { x: number; y: number }) {
    if (!this.workflow) {
      this.createWorkflow();
    }
    this.pushUndo();
    const wf = this.resolveActive(this.workflow!);
    const id = createNodeId();
    const nodeCount = wf.nodes.filter((n) => n.type === type).length + 1;
    const node: WorkflowNode = {
      id,
      name: defaultNodeName(type, nodeCount),
      type,
      agent: type === "task" || type === "run_agent" ? "claude" : null,
      prompt: "",
      contextSources: [],
      responseFormat: type === "task" ? "text" : null,
      splitFailurePolicy: type === "split" ? defaultSplitFailurePolicy() : undefined,
      ...defaultNodeConfig(type),
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
    const wf = this.resolveActive(this.workflow);
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
    this.resolveActive(this.workflow).edges.push(nextEdge);
    this.dirty = true;
    this.selection = { kind: "edge", id: nextEdge.id };
  }

  removeEdge(edgeId: string) {
    if (!this.workflow) return;
    this.pushUndo();
    const wf = this.resolveActive(this.workflow);
    wf.edges = wf.edges.filter((e) => e.id !== edgeId);
    this.dirty = true;
    this.selection = { kind: "workflow", id: null };
  }

  setNodePosition(nodeId: string, position: { x: number; y: number }) {
    if (!this.workflow) return;
    const canvas = this.resolveActive(this.workflow).ui!.canvas!;
    canvas.nodes[nodeId] = position;
    this.dirty = true;
  }

  /* ── save selection as compound node ──────────────────────────────── */

  /**
   * Collapse the selected nodes into a reusable saved subflow stored on the
   * (active) document's `subflows` catalog, replacing the region with a single
   * `subflow` node. Entry is the selected node with no internal inbound edge;
   * exit is the selected node with no internal outbound edge. External edges
   * are rewired to/from the new compound node.
   *
   * Returns the new compound node id, or null when the selection is invalid.
   */
  saveSelectionAsCompound(nodeIds: string[], rawName: string): string | null {
    if (!this.workflow || nodeIds.length === 0) return null;
    const name = rawName.trim();
    if (!name) return null;

    const wf = this.resolveActive(this.workflow);
    if (wf.subflows?.[name]) return null; // name collision

    const selected = new Set(nodeIds);
    const selNodes = wf.nodes.filter((n) => selected.has(n.id));
    if (selNodes.length === 0) return null;

    const internalEdges = wf.edges.filter((e) => selected.has(e.from) && selected.has(e.to));
    const inboundEdges = wf.edges.filter((e) => !selected.has(e.from) && selected.has(e.to));
    const outboundEdges = wf.edges.filter((e) => selected.has(e.from) && !selected.has(e.to));

    // Entry: targeted from outside, else a node with no internal inbound edge.
    const hasInternalInbound = new Set(internalEdges.map((e) => e.to));
    const inboundTargets = new Set(inboundEdges.map((e) => e.to));
    const entry =
      selNodes.find((n) => inboundTargets.has(n.id)) ??
      selNodes.find((n) => !hasInternalInbound.has(n.id)) ??
      selNodes[0];

    // Exit: a node with no internal outbound edge (terminal within the region).
    const hasInternalOutbound = new Set(internalEdges.map((e) => e.from));
    const exit =
      selNodes.find((n) => !hasInternalOutbound.has(n.id)) ?? selNodes[selNodes.length - 1];

    this.pushUndo();

    // Build the subflow document (deep clone so it is decoupled from the parent).
    const positions = wf.ui?.canvas?.nodes ?? {};
    const subflowDoc: WorkflowDocument = {
      version: 3,
      name,
      goal: "",
      cwd: wf.cwd,
      useOrchestrator: false,
      entryNodeId: entry.id,
      variables: [],
      limits: { maxTotalSteps: 50, maxVisitsPerNode: 10 },
      nodes: structuredClone($state.snapshot(selNodes)) as WorkflowNode[],
      edges: structuredClone($state.snapshot(internalEdges)) as WorkflowEdge[],
      ui: {
        canvas: {
          viewport: { x: 0, y: 0, zoom: 1 },
          nodes: Object.fromEntries(
            selNodes.map((n) => [n.id, positions[n.id] ?? { x: 160, y: 160 }]),
          ),
        },
      },
    };

    if (!wf.subflows) wf.subflows = {};
    wf.subflows[name] = subflowDoc;

    // Place the compound node at the centroid of the selected region.
    const pts = selNodes.map((n) => positions[n.id]).filter(Boolean) as { x: number; y: number }[];
    const centroid = pts.length
      ? { x: pts.reduce((s, p) => s + p.x, 0) / pts.length, y: pts.reduce((s, p) => s + p.y, 0) / pts.length }
      : { x: 200, y: 200 };

    const compoundId = createNodeId();
    const compoundNode: WorkflowNode = {
      id: compoundId,
      name,
      type: "subflow",
      agent: null,
      prompt: "",
      contextSources: [],
      responseFormat: null,
      subflowConfig: { workflowName: name, exitNodeId: exit.id, inputs: [], maxDepth: 10 },
    };

    // Remove selected nodes + their internal edges; keep boundary edges to rewire.
    wf.nodes = wf.nodes.filter((n) => !selected.has(n.id));
    wf.edges = wf.edges.filter((e) => !(selected.has(e.from) && selected.has(e.to)));
    wf.nodes.push(compoundNode);

    // Rewire boundary edges onto the compound node.
    for (const edge of wf.edges) {
      if (inboundEdges.includes(edge)) edge.to = compoundId;
      if (outboundEdges.includes(edge)) edge.from = compoundId;
    }

    // Canvas bookkeeping.
    const canvas = this.resolveActive(this.workflow).ui!.canvas!;
    for (const n of selNodes) delete canvas.nodes[n.id];
    canvas.nodes[compoundId] = centroid;

    if (wf.entryNodeId && selected.has(wf.entryNodeId)) {
      wf.entryNodeId = compoundId;
    }

    this.dirty = true;
    this.multiSelectedNodeIds = [];
    this.selection = { kind: "node", id: compoundId };
    return compoundId;
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

  setRunState(patch: { runId?: string | null; running?: boolean; approval?: ApprovalState | null }) {
    if (patch.runId !== undefined) this.runId = patch.runId;
    if (patch.running !== undefined) this.running = patch.running;
    if (patch.approval !== undefined) this.approval = patch.approval;
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
