import {
  createEdgeId,
  createEmptyWorkflow,
  createNodeId,
  ensureCanvas,
} from "@/lib/types/workflow";
import type {
  ContextSource,
  InputBinding,
  RunEvent,
  RunObservability,
  SplitFailurePolicy,
  SubflowConfig,
  ValidationResponse,
  WorkflowDocument,
  WorkflowEdge,
  WorkflowNode,
  WorkflowNodeType,
  WorkflowVariable,
} from "@/lib/types/workflow";
import { defaultNodeKind, defaultNodeName } from "@/lib/stores/nodeMetadata";
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

type SaveCompoundFailureCode =
  | "missing_workflow"
  | "empty_selection"
  | "invalid_name"
  | "name_in_use"
  | "multiple_entry_nodes"
  | "multiple_exit_nodes"
  | "invalid_outbound_edge"
  | "non_producible_outcome"
  | "unsupported_dependency";

export type SaveSelectionAsCompoundResult =
  | { ok: true; nodeId: string }
  | { ok: false; code: SaveCompoundFailureCode; reason: string };

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

export type PaneStatus = "idle" | "connecting" | "open" | "closed" | "stalled" | "unavailable";

export interface InteractionState {
  sessionId: string;
  description: string;
  outputSoFar: string;
  interactionType: "permission" | "question" | "destructive_warning";
}

function nodeSubflowConfig(node: WorkflowNode | undefined): SubflowConfig | null {
  if (!node) return null;
  if (node.kind.type !== "subflow" && node.kind.type !== "call") return null;
  return node.kind.subflowConfig;
}

function defaultSplitFailurePolicy(): SplitFailurePolicy {
  return "best_effort_continue";
}

function defaultCanvas(
  workflow: WorkflowDocument,
): NonNullable<NonNullable<WorkflowDocument["ui"]>["canvas"]> {
  return {
    viewport: { x: 0, y: 0, zoom: 1 },
    nodes: Object.fromEntries(
      workflow.nodes.map((node, index) => [
        node.id,
        { x: 120 + index * 220, y: 120 + (index % 3) * 140 },
      ]),
    ),
  };
}

function compoundFailure(
  code: SaveCompoundFailureCode,
  reason: string,
): SaveSelectionAsCompoundResult {
  return { ok: false, code, reason };
}

const COMPOUND_VAR_RE = /\{\{var:([^}]+)\}\}/g;
const COMPOUND_NODE_OUTPUT_FIELD_RE = /\{\{node:([^.}]+)\.output\.([^}]+)\}\}/g;
const COMPOUND_NODE_PARSED_FIELD_RE = /\{\{node:([^.}]+)\.parsedOutput\.([^}]+)\}\}/g;
const COMPOUND_NODE_OUTPUT_RE = /\{\{node:([^.}]+)\.output\}\}/g;
const COMPOUND_CONTEXT_RE = /\{\{context:([^}]+)\}\}/g;

function compoundNodePrompts(node: WorkflowNode): string[] {
  const texts = [node.prompt];
  if (node.kind.type === "decide") {
    texts.push(node.kind.decideConfig.prompt);
  }
  return texts;
}

function compoundHasFieldPathRefs(text: string): boolean {
  COMPOUND_NODE_OUTPUT_FIELD_RE.lastIndex = 0;
  if (COMPOUND_NODE_OUTPUT_FIELD_RE.test(text)) return true;
  COMPOUND_NODE_PARSED_FIELD_RE.lastIndex = 0;
  return COMPOUND_NODE_PARSED_FIELD_RE.test(text);
}

function compoundOutboundOutcomeAllowed(edge: WorkflowEdge): boolean {
  if (edge.outcome === "success") return true;
  if (edge.outcome === "branch" && edge.condition != null) return true;
  return false;
}

function compoundUniqueVarName(base: string, used: Set<string>): string {
  const sanitized = base.replace(/[^a-zA-Z0-9_]/g, "_").replace(/^_+|_+$/g, "") || "ref";
  let candidate = sanitized;
  let suffix = 2;
  while (used.has(candidate)) {
    candidate = `${sanitized}_${suffix}`;
    suffix += 1;
  }
  used.add(candidate);
  return candidate;
}

interface CompoundDependencyPlan {
  compoundId: string;
  subflowVariables: WorkflowVariable[];
  callInputs: InputBinding[];
  movedNodes: WorkflowNode[];
  outsideNodePatches: Map<string, { prompt?: string; contextSources?: ContextSource[] }>;
}

function planCompoundDependencies(
  active: WorkflowDocument,
  selected: Set<string>,
  selNodes: WorkflowNode[],
  exit: WorkflowNode,
): CompoundDependencyPlan | SaveSelectionAsCompoundResult {
  const compoundId = createNodeId();
  const varNames = new Set<string>();
  const subflowVariables = new Map<string, WorkflowVariable>();
  const callInputs = new Map<string, InputBinding>();
  const movedNodes = structuredClone(selNodes) as WorkflowNode[];
  const movedById = new Map(movedNodes.map((node) => [node.id, node]));
  const outsideNodePatches = new Map<string, { prompt?: string; contextSources?: ContextSource[] }>();

  const declareBinding = (name: string, source: string) => {
    if (!subflowVariables.has(name)) {
      subflowVariables.set(name, { name, default: "" });
      callInputs.set(name, { name, source });
    }
  };

  const reject = (reason: string): SaveSelectionAsCompoundResult =>
    compoundFailure("unsupported_dependency", reason);

  const rewriteMovedText = (nodeId: string, rewrite: (text: string) => string) => {
    const node = movedById.get(nodeId);
    if (!node) return;
    node.prompt = rewrite(node.prompt);
    if (node.kind.type === "decide") {
      node.kind.decideConfig.prompt = rewrite(node.kind.decideConfig.prompt);
    }
  };

  const outsideVarByNodeId = new Map<string, string>();

  const bindOutsideNodeOutput = (outsideId: string, targetNodeId: string) => {
    let varName = outsideVarByNodeId.get(outsideId);
    if (!varName) {
      varName = compoundUniqueVarName(`from_${outsideId}`, varNames);
      outsideVarByNodeId.set(outsideId, varName);
      declareBinding(varName, `node:${outsideId}.output`);
    }
    rewriteMovedText(targetNodeId, (text) =>
      text
        .replaceAll(`{{node:${outsideId}.output}}`, `{{var:${varName}}}`)
        .replaceAll(`{{${outsideId}}}`, `{{var:${varName}}}`),
    );
  };

  for (const node of movedNodes) {
    for (const text of compoundNodePrompts(node)) {
      if (compoundHasFieldPathRefs(text)) {
        return reject(
          `Selection contains a field-path template reference that cannot be preserved across a compound boundary.`,
        );
      }
    }

    for (const text of compoundNodePrompts(node)) {
      for (const match of text.matchAll(COMPOUND_VAR_RE)) {
        const varName = match[1];
        declareBinding(varName, `var:${varName}`);
      }
    }

    for (const text of compoundNodePrompts(node)) {
      for (const match of text.matchAll(COMPOUND_NODE_OUTPUT_RE)) {
        const refNodeId = match[1];
        if (selected.has(refNodeId)) continue;
        bindOutsideNodeOutput(refNodeId, node.id);
      }
      for (const refNodeId of active.nodes.map((candidate) => candidate.id)) {
        if (selected.has(refNodeId)) continue;
        if (!text.includes(`{{${refNodeId}}}`)) continue;
        bindOutsideNodeOutput(refNodeId, node.id);
      }
    }

    for (const context of node.contextSources ?? []) {
      if (!context.name) continue;
      if (selected.has(context.nodeId)) {
        return reject(
          `Context source "${context.name}" references a node inside the selection; move the reference to the exit output or remove it before saving.`,
        );
      }
      declareBinding(context.name, `node:${context.nodeId}.output`);
      rewriteMovedText(node.id, (text) =>
        text.replaceAll(`{{context:${context.name}}}`, `{{var:${context.name}}}`),
      );
      node.contextSources = (node.contextSources ?? []).filter((entry) => entry !== context);
    }
  }

  for (const node of active.nodes) {
    if (selected.has(node.id)) continue;

    for (const text of compoundNodePrompts(node)) {
      if (compoundHasFieldPathRefs(text)) {
        return reject(
          `A node outside the selection uses a field-path template reference to a moved node; compound extraction cannot preserve it.`,
        );
      }
    }

    for (const context of node.contextSources ?? []) {
      if (!selected.has(context.nodeId)) continue;
      if (context.nodeId !== exit.id) {
        return reject(
          `Context source "${context.name}" on "${node.name}" references a non-exit node inside the selection.`,
        );
      }
    }

    let prompt = node.prompt;
    let contextSources = node.contextSources;
    let changed = false;

    for (const match of prompt.matchAll(COMPOUND_NODE_OUTPUT_RE)) {
      const refNodeId = match[1];
      if (!selected.has(refNodeId)) continue;
      if (refNodeId !== exit.id) {
        return reject(
          `Node "${node.name}" references "${refNodeId}" inside the selection, but only the exit node may be referenced from outside.`,
        );
      }
      prompt = prompt
        .replaceAll(`{{node:${refNodeId}.output}}`, `{{node:${compoundId}.output}}`)
        .replaceAll(`{{${refNodeId}}}`, `{{${compoundId}}}`);
      changed = true;
    }

    for (const refNodeId of selNodes.map((candidate) => candidate.id)) {
      if (!prompt.includes(`{{${refNodeId}}}`)) continue;
      if (refNodeId !== exit.id) {
        return reject(
          `Node "${node.name}" references "${refNodeId}" inside the selection, but only the exit node may be referenced from outside.`,
        );
      }
      prompt = prompt.replaceAll(`{{${refNodeId}}}`, `{{${compoundId}}}`);
      changed = true;
    }

    if (contextSources?.some((context) => selected.has(context.nodeId))) {
      contextSources = contextSources.map((context) =>
        selected.has(context.nodeId) ? { ...context, nodeId: compoundId } : context,
      );
      changed = true;
    }

    if (changed) {
      outsideNodePatches.set(node.id, {
        ...(prompt !== node.prompt ? { prompt } : {}),
        ...(contextSources !== node.contextSources ? { contextSources } : {}),
      });
    }
  }

  return {
    compoundId,
    subflowVariables: [...subflowVariables.values()],
    callInputs: [...callInputs.values()],
    movedNodes,
    outsideNodePatches,
  };
}

/* ── Undo stack ─────────────────────────────────────────────────────── */

interface UndoEntry {
  workflow: WorkflowDocument;
}

const MAX_UNDO = 50;

/** Match PaneTerminal xterm scrollback — bounds RunPanel DOM nodes and retained strings. */
const MAX_RUN_LINES = 5000;
const MAX_RUN_LINE_CHARS = 8_000;

function truncateRunLineText(text: string): string {
  if (text.length <= MAX_RUN_LINE_CHARS) return text;
  const remaining = text.length - MAX_RUN_LINE_CHARS;
  return `${text.slice(0, MAX_RUN_LINE_CHARS)}… truncated, ${remaining.toLocaleString()} more chars`;
}

function capRunLines(lines: RunLine[]): void {
  if (lines.length > MAX_RUN_LINES) {
    lines.splice(0, lines.length - MAX_RUN_LINES);
  }
}

class WorkflowStore {
  workflow = $state<WorkflowDocument | null>(null);
  validation = $state<ValidationResponse | null>(null);
  selection = $state<Selection>({ kind: "workflow", id: null });
  panelTab = $state<"output" | "history" | "reference" | "terminal">("output");
  dirty = $state(false);
  runId = $state<string | null>(null);
  streamToken = $state<string | null>(null);
  running = $state(false);
  lines = $state<RunLine[]>([]);
  nodeStates = $state<Record<string, NodeRuntimeState>>({});
  approval = $state<ApprovalState | null>(null);
  interactions = $state<InteractionState[]>([]);
  errorMessage = $state<string>("");
  #errorTimer: ReturnType<typeof setTimeout> | undefined;
  #runStreamAbort: AbortController | null = null;
  runEpoch = 0;

  /* ── live pane terminal ───────────────────────────────────────────── */
  selectedPane = $state<string>("active");
  paneStatus = $state<PaneStatus>("idle");
  paneError = $state<string>("");
  runObservability = $state<RunObservability | null>(null);

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

  get interaction() {
    return this.interactions[0] ?? null;
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
    this.selectedPane = "active";
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
   * Resolve the document currently shown on the canvas from the root subflow
   * catalog. Subflow names are globally scoped to match backend validation and
   * runtime resolution.
   */
  private resolveActiveDocument(root: WorkflowDocument): WorkflowDocument {
    let doc = root;
    for (const name of this.drillStack) {
      const next = root.subflows?.[name];
      if (!next) break;
      doc = next;
    }
    return doc;
  }

  private resolveActive(root: WorkflowDocument): WorkflowDocument {
    const doc = this.resolveActiveDocument(root);
    const canvas = doc.ui?.canvas ?? defaultCanvas(doc);
    const rootSubflows = root.subflows;
    const needsProjection = !doc.ui?.canvas || (doc !== root && doc.subflows !== rootSubflows);
    if (!needsProjection) return doc;
    return {
      ...doc,
      subflows: rootSubflows,
      ui: {
        ...doc.ui,
        canvas,
      },
    };
  }

  private resolveActiveMutable(root: WorkflowDocument): WorkflowDocument {
    const doc = this.resolveActiveDocument(root);
    if (!doc.ui?.canvas) {
      doc.ui = {
        ...doc.ui,
        canvas: defaultCanvas(doc),
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
    const name = nodeSubflowConfig(node)?.workflowName;
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
    updater(this.resolveActiveMutable(this.workflow));
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
    const wf = this.resolveActiveMutable(this.workflow!);
    const id = createNodeId();
    const nodeCount = wf.nodes.filter((n) => n.kind.type === type).length + 1;
    const node: WorkflowNode = {
      id,
      name: defaultNodeName(type, nodeCount),
      kind: defaultNodeKind(type),
      agent: type === "task" || type === "run_agent" ? "claude" : null,
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
    const wf = this.resolveActiveMutable(this.workflow);
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
    this.resolveActiveMutable(this.workflow).edges.push(nextEdge);
    this.dirty = true;
    this.selection = { kind: "edge", id: nextEdge.id };
  }

  removeEdge(edgeId: string) {
    if (!this.workflow) return;
    this.pushUndo();
    const wf = this.resolveActiveMutable(this.workflow);
    wf.edges = wf.edges.filter((e) => e.id !== edgeId);
    this.dirty = true;
    this.selection = { kind: "workflow", id: null };
  }

  setNodePosition(nodeId: string, position: { x: number; y: number }) {
    if (!this.workflow) return;
    const canvas = this.resolveActiveMutable(this.workflow).ui!.canvas!;
    canvas.nodes[nodeId] = position;
    this.dirty = true;
  }

  /* ── save selection as compound node ──────────────────────────────── */

  /**
   * Collapse the selected nodes into a reusable saved subflow stored on the
   * root workflow's `subflows` catalog, replacing the active region with a
   * single `subflow` node. The selected region must expose exactly one entry
   * point and one terminal node.
   *
   * Returns the new compound node id, or a human-readable failure reason.
   */
  saveSelectionAsCompound(nodeIds: string[], rawName: string): SaveSelectionAsCompoundResult {
    if (!this.workflow) {
      return compoundFailure("missing_workflow", "No workflow is loaded.");
    }
    if (nodeIds.length === 0) {
      return compoundFailure("empty_selection", "Select one or more nodes to save as a compound node.");
    }
    const name = rawName.trim();
    if (!name) {
      return compoundFailure("invalid_name", "Enter a name for the compound node.");
    }

    const active = this.resolveActiveDocument(this.workflow);
    if (this.workflow.subflows?.[name]) {
      return compoundFailure("name_in_use", `A root-level subflow named "${name}" already exists.`);
    }

    const selected = new Set(nodeIds);
    const selNodes = active.nodes.filter((n) => selected.has(n.id));
    if (selNodes.length === 0) {
      return compoundFailure("empty_selection", "The selected nodes are not in the active workflow.");
    }

    const internalEdges = active.edges.filter((e) => selected.has(e.from) && selected.has(e.to));
    const inboundEdges = active.edges.filter((e) => !selected.has(e.from) && selected.has(e.to));
    const outboundEdges = active.edges.filter((e) => selected.has(e.from) && !selected.has(e.to));

    const hasInternalInbound = new Set(internalEdges.map((e) => e.to));
    const inboundTargets = new Set(inboundEdges.map((e) => e.to));
    // Entry = targeted from outside the selection, or has no inbound edge from within it.
    const entryCandidates = selNodes.filter(
      (n) => inboundTargets.has(n.id) || !hasInternalInbound.has(n.id),
    );
    if (entryCandidates.length !== 1) {
      return compoundFailure(
        "multiple_entry_nodes",
        `Selection must expose exactly one entry point; found ${entryCandidates.length}. Add a single entry or merge point before saving as a compound node.`,
      );
    }

    const hasInternalOutbound = new Set(internalEdges.map((e) => e.from));
    const exitCandidates = selNodes.filter((n) => !hasInternalOutbound.has(n.id));
    if (exitCandidates.length !== 1) {
      return compoundFailure(
        "multiple_exit_nodes",
        `Selection must expose exactly one exit node; found ${exitCandidates.length}. Add a single merge point before saving as a compound node.`,
      );
    }

    const entry = entryCandidates[0];
    const exit = exitCandidates[0];

    for (const edge of outboundEdges) {
      if (edge.from !== exit.id) {
        return compoundFailure(
          "invalid_outbound_edge",
          `Selection has an outbound edge from "${edge.from}" that is not the exit node "${exit.id}". Only the exit node may connect outside the compound.`,
        );
      }
      if (!compoundOutboundOutcomeAllowed(edge)) {
        return compoundFailure(
          "non_producible_outcome",
          `Outbound edge "${edge.id}" uses outcome "${edge.outcome}" that a compound node cannot produce. Only success and conditioned branch edges are allowed from the exit.`,
        );
      }
    }

    const selNodesPlain = structuredClone($state.snapshot(selNodes)) as WorkflowNode[];
    const dependencyPlan = planCompoundDependencies(active, selected, selNodesPlain, exit);
    if ("ok" in dependencyPlan) {
      return dependencyPlan;
    }
    const plan = dependencyPlan;

    this.pushUndo();
    const wf = this.resolveActiveMutable(this.workflow);

    // Build the subflow document (deep clone so it is decoupled from the parent).
    const positions = (active.ui?.canvas ?? defaultCanvas(active)).nodes;
    const subflowDoc: WorkflowDocument = {
      version: 4,
      name,
      goal: "",
      cwd: active.cwd,
      useOrchestrator: false,
      entryNodeId: entry.id,
      variables: plan.subflowVariables,
      limits: { maxTotalSteps: 50, maxVisitsPerNode: 10 },
      nodes: structuredClone($state.snapshot(plan.movedNodes)) as WorkflowNode[],
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

    if (!this.workflow.subflows) this.workflow.subflows = {};
    this.workflow.subflows[name] = subflowDoc;

    // Place the compound node at the centroid of the selected region.
    const pts = selNodes.map((n) => positions[n.id]).filter(Boolean) as { x: number; y: number }[];
    const centroid = pts.length
      ? { x: pts.reduce((s, p) => s + p.x, 0) / pts.length, y: pts.reduce((s, p) => s + p.y, 0) / pts.length }
      : { x: 200, y: 200 };

    const compoundId = plan.compoundId;
    const compoundNode: WorkflowNode = {
      id: compoundId,
      name,
      kind: {
        type: "subflow",
        subflowConfig: {
          workflowName: name,
          exitNodeId: exit.id,
          inputs: plan.callInputs,
          maxDepth: 10,
        },
      },
      agent: null,
      prompt: "",
      contextSources: [],
      responseFormat: null,
    };

    // Remove selected nodes + their internal edges; keep boundary edges to rewire.
    wf.nodes = wf.nodes.filter((n) => !selected.has(n.id));
    wf.edges = wf.edges.filter((e) => !(selected.has(e.from) && selected.has(e.to)));
    wf.nodes.push(compoundNode);

    for (const [nodeId, patch] of plan.outsideNodePatches) {
      const outsideNode = wf.nodes.find((n) => n.id === nodeId);
      if (!outsideNode) continue;
      if (patch.prompt !== undefined) outsideNode.prompt = patch.prompt;
      if (patch.contextSources !== undefined) outsideNode.contextSources = patch.contextSources;
    }

    // Rewire boundary edges onto the compound node.
    for (const edge of wf.edges) {
      if (inboundEdges.includes(edge)) edge.to = compoundId;
      if (outboundEdges.includes(edge)) edge.from = compoundId;
    }

    // Canvas bookkeeping.
    const canvas = wf.ui!.canvas!;
    for (const n of selNodes) delete canvas.nodes[n.id];
    canvas.nodes[compoundId] = centroid;

    if (wf.entryNodeId && selected.has(wf.entryNodeId)) {
      wf.entryNodeId = compoundId;
    }

    this.dirty = true;
    this.multiSelectedNodeIds = [];
    this.selection = { kind: "node", id: compoundId };
    return { ok: true, nodeId: compoundId };
  }

  /* ── runtime ──────────────────────────────────────────────────────── */

  appendLine(line: RunLine) {
    this.lines.push(line);
    capRunLines(this.lines);
  }

  clearLines() {
    this.lines = [];
  }

  private cancelRunStream() {
    this.#runStreamAbort?.abort();
    this.#runStreamAbort = null;
  }

  beginRunStream(): { epoch: number; signal: AbortSignal } {
    this.cancelRunStream();
    this.runEpoch += 1;
    const epoch = this.runEpoch;
    const controller = new AbortController();
    this.#runStreamAbort = controller;
    return { epoch, signal: controller.signal };
  }

  isRunEpochStale(epoch: number): boolean {
    return epoch !== this.runEpoch;
  }

  resetRun() {
    this.cancelRunStream();
    this.runEpoch += 1;
    this.runId = null;
    this.streamToken = null;
    this.running = false;
    this.approval = null;
    this.interactions = [];
    this.nodeStates = {};
    this.selectedPane = "active";
    this.runObservability = null;
  }

  setRunState(patch: {
    runId?: string | null;
    streamToken?: string | null;
    running?: boolean;
    approval?: ApprovalState | null;
  }) {
    if (patch.runId !== undefined) {
      this.runId = patch.runId;
      if (patch.runId === null && patch.streamToken === undefined) this.streamToken = null;
    }
    if (patch.streamToken !== undefined) this.streamToken = patch.streamToken;
    if (patch.running !== undefined) this.running = patch.running;
    if (patch.approval !== undefined) this.approval = patch.approval;
  }

  setRunObservability(observability: RunObservability | null) {
    this.runObservability = observability;
    const panes = observability?.panes;
    if (panes?.length === 1) {
      this.selectedPane = panes[0].pane;
    } else if (panes && panes.length > 1) {
      if (!panes.some((p) => p.pane === this.selectedPane)) {
        this.selectedPane = panes[0]?.pane ?? "active";
      }
    }
  }

  setNodeRuntimeState(nodeId: string, state: NodeRuntimeState) {
    this.nodeStates[nodeId] = state;
  }

  setError(message: string) {
    clearTimeout(this.#errorTimer);
    this.errorMessage = message;
    if (message) {
      this.#errorTimer = setTimeout(() => {
        this.errorMessage = "";
      }, 5000);
    }
  }

  applyRunEvent(event: RunEvent, epoch?: number) {
    if (epoch !== undefined && this.isRunEpochStale(epoch)) return;

    const nodeId = typeof event.nodeId === "string" ? event.nodeId : null;
    const push = (tone: RunLine["tone"], text: string) => {
      this.lines.push({ tone, text });
      capRunLines(this.lines);
    };

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
        if (typeof result.output === "string" && result.output) {
          push("detail", truncateRunLineText(result.output));
        }
        if (typeof result.stderr === "string" && result.stderr) {
          push("error", truncateRunLineText(result.stderr));
        }
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
      case "subflow_start":
        push("system", `Entering subflow \`${String(event.subflowName ?? "")}\``);
        break;
      case "subflow_done":
        push("system", `Subflow \`${String(event.subflowName ?? "")}\` finished`);
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
        {
          const interaction = {
            sessionId: String(event.sessionId ?? ""),
            description: String(event.description ?? ""),
            outputSoFar: String(event.outputSoFar ?? ""),
            interactionType: (event.interactionType as InteractionState["interactionType"]) ?? "question",
          };
          const index = this.interactions.findIndex((item) => item.sessionId === interaction.sessionId);
          if (index === -1) {
            this.interactions.push(interaction);
          } else {
            this.interactions[index] = interaction;
          }
        }
        break;
      case "agent_interaction_resolved":
        push("detail", `Interaction resolved: ${String(event.description ?? "")}`);
        if (typeof event.sessionId === "string" && event.sessionId) {
          this.interactions = this.interactions.filter((item) => item.sessionId !== event.sessionId);
        } else {
          console.warn("agent_interaction_resolved event missing sessionId; ignoring", event);
        }
        break;
      case "done":
        push(event.aborted ? "error" : "success", event.aborted ? "Workflow aborted" : "Workflow complete");
        this.approval = null;
        this.interactions = [];
        this.running = false;
        break;
      default:
        break;
    }

    if (
      typeof event.runId === "string" &&
      (this.runId === null || event.runId === this.runId)
    ) {
      this.runId = event.runId;
    }
  }
}

export const store = new WorkflowStore();
