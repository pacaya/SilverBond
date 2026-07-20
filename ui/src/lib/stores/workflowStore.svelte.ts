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

type CompoundPromptField = "prompt" | "decidePrompt" | "runAgentPrompt" | "sendText";
type CompoundRefSide = "moved" | "outside" | "unknown";
type CompoundRefForm = "output" | "field" | "parsedField" | "bare";

interface CompoundPromptText {
  field: CompoundPromptField;
  text: string;
}

interface CompoundRef {
  nodeId: string;
  form: CompoundRefForm;
}

interface CompoundBoundary {
  side: (nodeId: string) => CompoundRefSide;
  refsIn: (text: string) => CompoundRef[];
  retarget: (text: string, fromId: string, toId: string) => string;
}

const COMPOUND_ALL_PREDECESSORS = "{{all_predecessors}}";
const COMPOUND_REPRESENTABLE_POSITIONAL_TOKENS = [
  { token: "{{previous_output}}", source: "previous_output" },
  { token: "{{branch_origin}}", source: "branch_origin" },
  { token: "{{branch_choice}}", source: "branch_choice" },
] as const;

function retargetNodeRefsInText(
  text: string,
  fromId: string,
  targets: {
    output: string;
    field: (path: string) => string;
    parsedField: (path: string) => string;
    bare: string;
  },
): string {
  let result = text;
  result = result.replace(COMPOUND_NODE_OUTPUT_FIELD_RE, (match, nodeId: string, path: string) =>
    nodeId === fromId ? targets.field(path) : match,
  );
  result = result.replace(COMPOUND_NODE_PARSED_FIELD_RE, (match, nodeId: string, path: string) =>
    nodeId === fromId ? targets.parsedField(path) : match,
  );
  if (result.includes(`{{node:${fromId}.output}}`)) {
    result = result.replaceAll(`{{node:${fromId}.output}}`, targets.output);
  }
  if (result.includes(`{{${fromId}}}`)) {
    result = result.replaceAll(`{{${fromId}}}`, targets.bare);
  }
  return result;
}

/** Mirrors prompt_template_for_node: every field that can carry template text. */
function compoundNodePromptTexts(node: WorkflowNode): CompoundPromptText[] {
  const texts: CompoundPromptText[] = [{ field: "prompt", text: node.prompt }];
  if (node.kind.type === "decide") {
    texts.push({ field: "decidePrompt", text: node.kind.decideConfig.prompt });
  }
  if (node.kind.type === "run_agent") {
    texts.push({
      field: "runAgentPrompt",
      text: node.kind.runAgentConfig?.prompt ?? "",
    });
  }
  if (node.kind.type === "send") {
    texts.push({ field: "sendText", text: node.kind.sendConfig.text });
  }
  return texts;
}

function applyCompoundPromptField(node: WorkflowNode, field: CompoundPromptField, text: string) {
  switch (field) {
    case "prompt":
      node.prompt = text;
      break;
    case "decidePrompt":
      if (node.kind.type === "decide") node.kind.decideConfig.prompt = text;
      break;
    case "runAgentPrompt":
      if (node.kind.type === "run_agent") {
        node.kind.runAgentConfig = {
          ...(node.kind.runAgentConfig ?? { killAfter: true }),
          prompt: text,
        };
      }
      break;
    case "sendText":
      if (node.kind.type === "send") node.kind.sendConfig.text = text;
      break;
  }
}

function compoundBoundary(active: WorkflowDocument, selected: Set<string>): CompoundBoundary {
  const knownNodeIds = new Set(active.nodes.map((node) => node.id));
  return {
    side(nodeId: string): CompoundRefSide {
      if (selected.has(nodeId)) return "moved";
      if (knownNodeIds.has(nodeId)) return "outside";
      return "unknown";
    },
    refsIn(text: string): CompoundRef[] {
      const refs: CompoundRef[] = [];
      for (const match of text.matchAll(COMPOUND_NODE_OUTPUT_FIELD_RE)) {
        refs.push({ nodeId: match[1], form: "field" });
      }
      for (const match of text.matchAll(COMPOUND_NODE_PARSED_FIELD_RE)) {
        refs.push({ nodeId: match[1], form: "parsedField" });
      }
      for (const match of text.matchAll(COMPOUND_NODE_OUTPUT_RE)) {
        refs.push({ nodeId: match[1], form: "output" });
      }
      for (const nodeId of knownNodeIds) {
        if (text.includes(`{{${nodeId}}}`)) {
          refs.push({ nodeId, form: "bare" });
        }
      }
      return refs;
    },
    retarget(text: string, fromId: string, toId: string): string {
      return retargetNodeRefsInText(text, fromId, {
        output: `{{node:${toId}.output}}`,
        field: (path) => `{{node:${toId}.output.${path}}}`,
        parsedField: (path) => `{{node:${toId}.parsedOutput.${path}}}`,
        bare: `{{${toId}}}`,
      });
    },
  };
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

interface OutsideNodePatch {
  prompts?: CompoundPromptText[];
  contextSources?: ContextSource[];
}

interface CompoundDependencyPlan {
  compoundId: string;
  subflowVariables: WorkflowVariable[];
  callInputs: InputBinding[];
  movedNodes: WorkflowNode[];
  outsideNodePatches: Map<string, OutsideNodePatch>;
}

function planCompoundDependencies(
  active: WorkflowDocument,
  selected: Set<string>,
  selNodes: WorkflowNode[],
  entry: WorkflowNode,
  exit: WorkflowNode,
): CompoundDependencyPlan | SaveSelectionAsCompoundResult {
  const compoundId = createNodeId();
  const boundary = compoundBoundary(active, selected);
  const varNames = new Set<string>();
  const sourceToVarName = new Map<string, string>();
  const subflowVariables = new Map<string, WorkflowVariable>();
  const callInputs = new Map<string, InputBinding>();
  const movedNodes = structuredClone(selNodes) as WorkflowNode[];
  const movedById = new Map(movedNodes.map((node) => [node.id, node]));
  const outsideNodePatches = new Map<string, OutsideNodePatch>();

  const reject = (reason: string): SaveSelectionAsCompoundResult =>
    compoundFailure("unsupported_dependency", reason);

  const bindSource = (preferredBase: string, source: string): string => {
    const existing = sourceToVarName.get(source);
    if (existing) return existing;

    let varName: string;
    if (source.startsWith("var:")) {
      varName = source.slice(4);
      varNames.add(varName);
    } else {
      varName = compoundUniqueVarName(preferredBase, varNames);
    }

    sourceToVarName.set(source, varName);
    subflowVariables.set(varName, { name: varName, default: "" });
    callInputs.set(varName, { name: varName, source });
    return varName;
  };

  const rewriteMovedPrompts = (nodeId: string, rewrite: (text: string) => string) => {
    const node = movedById.get(nodeId);
    if (!node) return;
    for (const entry of compoundNodePromptTexts(node)) {
      const next = rewrite(entry.text);
      if (next !== entry.text) {
        applyCompoundPromptField(node, entry.field, next);
      }
    }
  };

  const bindOutsideNodeOutput = (outsideId: string, targetNodeId: string) => {
    const varName = bindSource(`from_${outsideId}`, `node:${outsideId}.output`);
    const varRef = `{{var:${varName}}}`;
    rewriteMovedPrompts(targetNodeId, (text) =>
      retargetNodeRefsInText(text, outsideId, {
        output: varRef,
        field: () => varRef,
        parsedField: () => varRef,
        bare: varRef,
      }),
    );
  };

  const isFieldPathRef = (form: CompoundRefForm) => form === "field" || form === "parsedField";

  // Reserve root {{var:}} names before any other allocation.
  for (const node of movedNodes) {
    for (const { text } of compoundNodePromptTexts(node)) {
      for (const match of text.matchAll(COMPOUND_VAR_RE)) {
        varNames.add(match[1]);
      }
    }
  }

  for (const node of movedNodes) {
    for (const { text } of compoundNodePromptTexts(node)) {
      for (const ref of boundary.refsIn(text)) {
        if (!isFieldPathRef(ref.form)) continue;
        const refSide = boundary.side(ref.nodeId);
        if (refSide === "outside") {
          return reject(
            `Selection node "${node.name}" references a field-path template on outside node "${ref.nodeId}" that cannot be preserved across a compound boundary.`,
          );
        }
        if (refSide === "unknown") {
          return reject(
            `Selection node "${node.name}" references a field-path template on unknown node "${ref.nodeId}".`,
          );
        }
      }
    }

    for (const { text } of compoundNodePromptTexts(node)) {
      for (const match of text.matchAll(COMPOUND_VAR_RE)) {
        bindSource(match[1], `var:${match[1]}`);
      }
    }

    // Unknown-side whole-output/bare refs are left verbatim (runtime keeps non-matching
    // {{node:…}} / {{id}} literals); field paths must be rejected because they resolve to "".
    for (const { text } of compoundNodePromptTexts(node)) {
      for (const ref of boundary.refsIn(text)) {
        if (ref.form !== "output" && ref.form !== "bare") continue;
        if (boundary.side(ref.nodeId) !== "outside") continue;
        bindOutsideNodeOutput(ref.nodeId, node.id);
      }
    }

    for (const context of node.contextSources ?? []) {
      if (!context.name) continue;
      const sourceSide = boundary.side(context.nodeId);
      if (sourceSide === "moved") continue;
      if (sourceSide === "unknown") {
        return reject(
          `Context source "${context.name}" on "${node.name}" references unknown node "${context.nodeId}".`,
        );
      }
      const varName = bindSource(context.name, `node:${context.nodeId}.output`);
      rewriteMovedPrompts(node.id, (text) =>
        text.replaceAll(`{{context:${context.name}}}`, `{{var:${varName}}}`),
      );
      node.contextSources = (node.contextSources ?? []).filter((entry) => entry !== context);
    }
  }

  for (const { text } of compoundNodePromptTexts(entry)) {
    if (text.includes(COMPOUND_ALL_PREDECESSORS)) {
      return reject(
        `Entry node "${entry.name}" uses ${COMPOUND_ALL_PREDECESSORS}, which cannot be preserved when extracting a compound subflow.`,
      );
    }
  }
  for (const { token, source } of COMPOUND_REPRESENTABLE_POSITIONAL_TOKENS) {
    for (const { text } of compoundNodePromptTexts(entry)) {
      if (!text.includes(token)) continue;
      const varName = bindSource(source, source);
      rewriteMovedPrompts(entry.id, (t) => t.replaceAll(token, `{{var:${varName}}}`));
    }
  }

  for (const node of active.nodes) {
    if (boundary.side(node.id) === "moved") continue;

    for (const { text } of compoundNodePromptTexts(node)) {
      for (const ref of boundary.refsIn(text)) {
        if (!isFieldPathRef(ref.form)) continue;
        const refSide = boundary.side(ref.nodeId);
        if (refSide === "moved" && ref.nodeId !== exit.id) {
          return reject(
            `Node "${node.name}" references a field-path template on moved node "${ref.nodeId}" inside the selection; only the exit node may be referenced from outside.`,
          );
        }
        if (refSide === "unknown") {
          return reject(
            `Node "${node.name}" references a field-path template on unknown node "${ref.nodeId}".`,
          );
        }
      }
    }

    for (const context of node.contextSources ?? []) {
      if (boundary.side(context.nodeId) !== "moved") continue;
      if (context.nodeId !== exit.id) {
        return reject(
          `Context source "${context.name}" on "${node.name}" references a non-exit node inside the selection.`,
        );
      }
    }

    const patch: OutsideNodePatch = {};
    let changed = false;

    for (const entry of compoundNodePromptTexts(node)) {
      let updated = entry.text;
      let fieldChanged = false;
      for (const ref of boundary.refsIn(entry.text)) {
        if (boundary.side(ref.nodeId) !== "moved") continue;
        if (ref.nodeId !== exit.id) {
          return reject(
            `Node "${node.name}" references "${ref.nodeId}" inside the selection, but only the exit node may be referenced from outside.`,
          );
        }
        updated = boundary.retarget(updated, ref.nodeId, compoundId);
        fieldChanged = true;
      }
      if (fieldChanged) {
        (patch.prompts ??= []).push({ field: entry.field, text: updated });
        changed = true;
      }
    }

    let contextSources = node.contextSources;
    if (contextSources?.some((context) => boundary.side(context.nodeId) === "moved")) {
      contextSources = contextSources.map((context) =>
        boundary.side(context.nodeId) === "moved" ? { ...context, nodeId: compoundId } : context,
      );
      patch.contextSources = contextSources;
      changed = true;
    }

    if (changed) {
      outsideNodePatches.set(node.id, patch);
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
    const dependencyPlan = planCompoundDependencies(active, selected, selNodesPlain, entry, exit);
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
      if (patch.prompts) {
        for (const p of patch.prompts) {
          applyCompoundPromptField(outsideNode, p.field, p.text);
        }
      }
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
