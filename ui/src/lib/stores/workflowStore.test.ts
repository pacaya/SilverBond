import { beforeEach, describe, expect, it } from "vitest";
import { store } from "@/lib/stores/workflowStore.svelte";
import type {
  NodeKind,
  WorkflowDocument,
  WorkflowEdge,
  WorkflowNode,
  WorkflowNodeType,
} from "@/lib/types/workflow";

function workflow(overrides: Partial<WorkflowDocument> = {}): WorkflowDocument {
  return {
    version: 4,
    name: "",
    goal: "",
    cwd: "",
    useOrchestrator: false,
    entryNodeId: "",
    variables: [],
    limits: { maxTotalSteps: 50, maxVisitsPerNode: 10 },
    nodes: [],
    edges: [],
    ui: {
      canvas: {
        viewport: { x: 0, y: 0, zoom: 1 },
        nodes: {},
      },
    },
    ...overrides,
  };
}

function node(
  id: string,
  type: WorkflowNodeType = "task",
  patch: Partial<WorkflowNode> = {},
): WorkflowNode {
  return {
    id,
    name: id,
    kind: kind(type),
    agent: type === "task" ? "claude" : null,
    prompt: "",
    contextSources: [],
    responseFormat: type === "task" ? "text" : null,
    ...patch,
  };
}

function kind(type: WorkflowNodeType): NodeKind {
  switch (type) {
    case "task":
      return { type: "task" };
    case "approval":
      return { type: "approval" };
    case "split":
      return { type: "split" };
    case "collector":
      return { type: "collector" };
    case "decide":
      return { type: "decide", decideConfig: { prompt: "", inputs: [], outcomes: [] } };
    case "parallel_batch":
      return {
        type: "parallel_batch",
        batchConfig: { itemsBinding: "", maxConcurrent: 4, itemVar: "item", bodyEntry: "" },
      };
    case "subflow":
      return { type: "subflow", subflowConfig: { workflowName: "", inputs: [], maxDepth: 10 } };
    case "call":
      return { type: "call", subflowConfig: { workflowName: "", inputs: [], maxDepth: 10 } };
    case "spawn":
      return { type: "spawn", spawnConfig: { agent: "claude" } };
    case "send":
      return { type: "send", sendConfig: { text: "", enter: true } };
    case "wait":
      return { type: "wait", waitConfig: { mode: "idle" } };
    case "capture":
      return { type: "capture", captureConfig: { all: false, ansi: false } };
    case "kill":
      return { type: "kill", killConfig: {} };
    case "run_agent":
      return { type: "run_agent", runAgentConfig: { killAfter: true } };
  }
}

function edge(id: string, from: string, to: string): WorkflowEdge {
  return {
    id,
    from,
    to,
    outcome: "success",
    label: null,
    branchId: null,
    condition: null,
  };
}

describe("workflowStore", () => {
  beforeEach(() => {
    store.createWorkflow();
    store.clearLines();
    store.resetRun();
  });

  it("adds nodes, edges, and updates entry node", () => {
    store.addNode("task", { x: 100, y: 120 });
    store.addNode("approval", { x: 420, y: 120 });

    const workflow = store.workflow!;
    expect(workflow.nodes).toHaveLength(2);
    expect(workflow.entryNodeId).toBe(workflow.nodes[0].id);

    store.addEdge({
      from: workflow.nodes[0].id,
      to: workflow.nodes[1].id,
      outcome: "success",
      label: null,
      branchId: null,
      condition: null,
    });

    expect(store.workflow!.edges).toHaveLength(1);
    expect(store.workflow!.ui?.canvas?.nodes[workflow.nodes[0].id]).toEqual({ x: 100, y: 120 });
  });

  it("adds split and collector nodes with non-task defaults", () => {
    store.addNode("split", { x: 120, y: 120 });
    store.addNode("collector", { x: 360, y: 120 });

    const [split, collector] = store.workflow!.nodes;
    expect(split).toMatchObject({
      kind: { type: "split" },
      agent: null,
      responseFormat: null,
      splitFailurePolicy: "best_effort_continue",
    });
    expect(collector).toMatchObject({
      kind: { type: "collector" },
      agent: null,
      responseFormat: null,
      splitFailurePolicy: undefined,
    });
  });

  it("tracks runtime node states from node events", () => {
    store.applyRunEvent({
      type: "node_start",
      nodeId: "n1",
      nodeName: "Node 1",
    });
    store.applyRunEvent({
      type: "node_done",
      nodeId: "n1",
      nodeName: "Node 1",
      result: { success: true, output: "ok", stderr: "", nodeName: "Node 1" },
    });

    expect(store.nodeStates.n1).toBe("success");
    expect(store.lines.some((line) => line.text === "Node 1 completed")).toBe(true);
    expect(store.lines.at(-1)?.text).toContain("ok");
  });

  it("logs subflow boundary events", () => {
    store.applyRunEvent({
      type: "subflow_start",
      nodeId: "call_1",
      subflowName: "Review Loop",
    });
    store.applyRunEvent({
      type: "subflow_done",
      nodeId: "call_1",
      subflowName: "Review Loop",
      output: "subflow output should not appear in log",
    });

    expect(store.lines.some((line) => line.text === "Entering subflow `Review Loop`")).toBe(true);
    expect(store.lines.some((line) => line.text === "Subflow `Review Loop` finished")).toBe(true);
    expect(store.lines.some((line) => line.text.includes("subflow output"))).toBe(false);
  });

  it("tracks split and collector runtime events", () => {
    store.applyRunEvent({
      type: "cursor_spawned",
      cursorId: "cursor_1",
      fromNodeId: "split_1",
    });
    store.applyRunEvent({
      type: "collector_waiting",
      nodeId: "collector_1",
      nodeName: "Collector 1",
      arrived: 1,
      required: 2,
    });
    store.applyRunEvent({
      type: "aggregate_merged",
      nodeId: "collector_1",
    });
    store.applyRunEvent({
      type: "collector_released",
      nodeId: "collector_1",
      nodeName: "Collector 1",
    });

    expect(store.nodeStates.collector_1).toBe("success");
    expect(store.lines.map((line) => line.text)).toEqual(
      expect.arrayContaining([
        "Spawned cursor cursor_1 from split_1",
        "Collector Collector 1 waiting on 1/2 inputs",
        "Merged collector inputs for collector_1",
        "Collector Collector 1 released",
      ]),
    );
  });

  it("supports undo/redo", () => {
    store.updateWorkflow((wf) => { wf.name = "first"; });
    store.updateWorkflow((wf) => { wf.name = "second"; });

    expect(store.workflow!.name).toBe("second");

    store.undo();
    expect(store.workflow!.name).toBe("first");

    store.redo();
    expect(store.workflow!.name).toBe("second");
  });

  it("stores compounds created inside a drilled subflow in the root catalog", () => {
    const subflowA = workflow({
      name: "A",
      entryNodeId: "a1",
      nodes: [node("a1"), node("a2")],
      edges: [edge("a1-a2", "a1", "a2")],
      ui: {
        canvas: {
          viewport: { x: 0, y: 0, zoom: 1 },
          nodes: { a1: { x: 100, y: 100 }, a2: { x: 300, y: 100 } },
        },
      },
    });
    store.setWorkflow(
      workflow({
        entryNodeId: "call_a",
        nodes: [
          node("call_a", "subflow", {
            kind: {
              type: "subflow",
              subflowConfig: { workflowName: "A", inputs: [], maxDepth: 10 },
            },
          }),
        ],
        subflows: { A: subflowA },
      }),
    );

    expect(store.drillIntoSubflow("call_a")).toBe(true);
    store.setMultiSelection(["a1", "a2"]);
    const result = store.saveSelectionAsCompound(store.multiSelectedNodeIds, "B");

    expect(result).toMatchObject({ ok: true });
    expect(store.workflow!.subflows?.B).toBeDefined();
    expect(store.workflow!.subflows?.A?.subflows?.B).toBeUndefined();

    const compound = store.activeWorkflow!.nodes.find(
      (n) => n.kind.type === "subflow" && n.kind.subflowConfig.workflowName === "B",
    );
    expect(compound).toBeDefined();
    const compoundKind = compound!.kind;
    expect(compoundKind.type).toBe("subflow");
    if (compoundKind.type !== "subflow") throw new Error("compound node was not a subflow");
    expect(store.workflow!.subflows?.[compoundKind.subflowConfig.workflowName]).toBeDefined();
    expect(store.drillIntoSubflow(compound!.id)).toBe(true);
    expect(store.activeWorkflow?.name).toBe("B");
  });

  it("rejects compound selections with multiple terminal nodes", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [node("a"), node("b"), node("c")],
        edges: [edge("a-b", "a", "b"), edge("a-c", "a", "c")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b", "c"], "Invalid");

    expect(result.ok).toBe(false);
    if (result.ok) throw new Error("compound save unexpectedly succeeded");
    expect(result.code).toBe("multiple_exit_nodes");
    expect(result.reason).toContain("exactly one exit node");
    expect(store.workflow!.subflows?.Invalid).toBeUndefined();
    expect(store.workflow!.nodes.map((n) => n.id)).toEqual(["a", "b", "c"]);
  });

  it("saves a single-entry single-exit selection as a compound node", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [node("a"), node("b")],
        edges: [edge("a-b", "a", "b")],
        ui: {
          canvas: {
            viewport: { x: 0, y: 0, zoom: 1 },
            nodes: { a: { x: 80, y: 120 }, b: { x: 280, y: 120 } },
          },
        },
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "Valid");

    expect(result).toMatchObject({ ok: true });
    expect(store.workflow!.subflows?.Valid?.entryNodeId).toBe("a");
    expect(store.workflow!.subflows?.Valid?.nodes.map((n) => n.id)).toEqual(["a", "b"]);
    expect(store.workflow!.nodes).toHaveLength(1);
    const savedCompoundKind = store.workflow!.nodes[0].kind;
    expect(savedCompoundKind).toMatchObject({
      type: "subflow",
      subflowConfig: {
        workflowName: "Valid",
        exitNodeId: "b",
      },
    });
    expect(savedCompoundKind.type).toBe("subflow");
    if (savedCompoundKind.type !== "subflow") {
      throw new Error("saved compound node was not a subflow");
    }
    expect(savedCompoundKind.subflowConfig).toMatchObject({
      workflowName: "Valid",
      exitNodeId: "b",
    });
    expect(store.workflow!.entryNodeId).toBe(store.workflow!.nodes[0].id);
  });

  it("corrects stale selectedPane when observability panes change across runs", () => {
    store.selectPane("cursor:old-node");
    expect(store.selectedPane).toBe("cursor:old-node");

    store.setRunObservability({
      panes: [
        { pane: "cursor:new-a", sessionName: "sess-a", attachCommand: "tmux attach -t sess-a" },
        { pane: "cursor:new-b", sessionName: "sess-b", attachCommand: "tmux attach -t sess-b" },
      ],
    });

    expect(store.selectedPane).toBe("cursor:new-a");
  });

  it("preserves selectedPane when it is still in incoming observability panes", () => {
    store.selectPane("cursor:new-b");

    store.setRunObservability({
      panes: [
        { pane: "cursor:new-a", sessionName: "sess-a", attachCommand: "tmux attach -t sess-a" },
        { pane: "cursor:new-b", sessionName: "sess-b", attachCommand: "tmux attach -t sess-b" },
      ],
    });

    expect(store.selectedPane).toBe("cursor:new-b");
  });

  it("resets selectedPane to active on resetRun", () => {
    store.selectPane("node_abc");
    store.setRunState({ runId: "run-1", streamToken: "stream-token-1" });
    expect(store.selectedPane).toBe("node_abc");
    expect(store.streamToken).toBe("stream-token-1");

    store.resetRun();

    expect(store.selectedPane).toBe("active");
    expect(store.streamToken).toBeNull();
  });

  it("resets selectedPane to active on setWorkflow", () => {
    store.selectPane("node_abc");
    expect(store.selectedPane).toBe("node_abc");

    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [node("a")],
      }),
    );

    expect(store.selectedPane).toBe("active");
  });

  it("does not mutate missing subflow canvas state while reading activeWorkflow", () => {
    const child = workflow({
      name: "Child",
      entryNodeId: "child_task",
      nodes: [node("child_task")],
      ui: undefined,
    });
    store.setWorkflow(
      workflow({
        entryNodeId: "call_child",
        nodes: [
          node("call_child", "subflow", {
            kind: {
              type: "subflow",
              subflowConfig: { workflowName: "Child", inputs: [], maxDepth: 10 },
            },
          }),
        ],
        subflows: { Child: child },
      }),
    );

    expect(store.drillIntoSubflow("call_child")).toBe(true);
    const active = store.activeWorkflow;

    expect(active?.name).toBe("Child");
    expect(active?.ui?.canvas?.nodes.child_task).toEqual({ x: 120, y: 120 });
    expect(store.workflow!.subflows!.Child.ui?.canvas).toBeUndefined();

    store.addNode("task", { x: 8, y: 9 });
    const persistedChild = store.workflow!.subflows!.Child;
    const added = persistedChild.nodes.find((n) => n.id !== "child_task");
    expect(persistedChild.ui?.canvas).toBeDefined();
    expect(persistedChild.ui!.canvas!.nodes[added!.id]).toEqual({ x: 8, y: 9 });
  });
});
