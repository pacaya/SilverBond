import { beforeEach, describe, expect, it, vi } from "vitest";
import { store } from "@/lib/stores/workflowStore.svelte";
import { branchEdge, buildDecideBranchWorkflow } from "@/test/fixtures/decideBranchWorkflow";
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

function edge(id: string, from: string, to: string, patch: Partial<WorkflowEdge> = {}): WorkflowEdge {
  return {
    id,
    from,
    to,
    outcome: "success",
    label: null,
    branchId: null,
    condition: null,
    ...patch,
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

  it("refuses forward branch transition when normalized label collides with sibling", () => {
    store.setWorkflow(
      buildDecideBranchWorkflow({
        edges: [
          branchEdge({ id: "branch_yes", label: "yes" }),
          branchEdge({ id: "branch_maybe", outcome: "success", label: " yes " }),
        ],
      }),
    );

    store.setEdgeOutcome("branch_maybe", "branch");

    const edge = store.workflow!.edges.find((item) => item.id === "branch_maybe")!;
    expect(edge.outcome).toBe("branch");
    expect(edge.label).toBe("yes");
  });

  it("normalizes forward branch transition when no sibling collision", () => {
    store.setWorkflow(
      buildDecideBranchWorkflow({
        edges: [
          branchEdge({ id: "branch_yes", label: "yes" }),
          branchEdge({ id: "branch_maybe", outcome: "success", label: " maybe " }),
        ],
      }),
    );

    store.setEdgeOutcome("branch_maybe", "branch");

    const edge = store.workflow!.edges.find((item) => item.id === "branch_maybe")!;
    expect(edge.label).toBe("maybe");
  });

  it("preserves label on reverse transition away from branch", () => {
    store.setWorkflow(
      buildDecideBranchWorkflow({
        edges: [branchEdge({ label: " yes " })],
      }),
    );

    store.setEdgeOutcome("branch_yes", "success");

    const edge = store.workflow!.edges.find((item) => item.id === "branch_yes")!;
    expect(edge.outcome).toBe("success");
    expect(edge.label).toBe(" yes ");
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

  it("does not mutate root runAs or limits when drilled into a subflow", () => {
    const subflowA = workflow({
      name: "A",
      entryNodeId: "a1",
      nodes: [node("a1")],
    });
    store.setWorkflow(
      workflow({
        runAs: { user: "root-user" },
        limits: { maxTotalSteps: 100, maxVisitsPerNode: 20 },
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

    store.updateWorkflow((wf) => {
      wf.runAs = { user: "subflow-user" };
      wf.limits = { maxTotalSteps: 999, maxVisitsPerNode: 999 };
    });

    expect(store.workflow!.runAs).toEqual({ user: "root-user" });
    expect(store.workflow!.limits).toEqual({ maxTotalSteps: 100, maxVisitsPerNode: 20 });
    expect(store.activeWorkflow!.runAs).toEqual({ user: "subflow-user" });
    expect(store.activeWorkflow!.limits).toEqual({ maxTotalSteps: 999, maxVisitsPerNode: 999 });
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

  it("copies agentDefaults and useOrchestrator into an extracted compound subflow", () => {
    store.setWorkflow(
      workflow({
        goal: "parent goal must not leak",
        useOrchestrator: true,
        agentDefaults: {
          claude: { accessMode: "read_only" },
        },
        entryNodeId: "a",
        nodes: [node("a"), node("b")],
        edges: [edge("a-b", "a", "b")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "ScopedDefaults");

    expect(result).toMatchObject({ ok: true });
    const subflow = store.workflow!.subflows?.ScopedDefaults;
    expect(subflow?.useOrchestrator).toBe(true);
    expect(subflow?.agentDefaults?.claude?.accessMode).toBe("read_only");
    expect(subflow?.goal).toBe("");
  });

  it("selects the external inbound node as entry for X→A, A→B", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "x",
        nodes: [node("x"), node("a"), node("b")],
        edges: [edge("x-a", "x", "a"), edge("a-b", "a", "b")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "MidGraph");

    expect(result).toMatchObject({ ok: true });
    expect(store.workflow!.subflows?.MidGraph?.entryNodeId).toBe("a");
  });

  it("rejects compound selections with multiple external entry points", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "x",
        nodes: [node("x"), node("y"), node("a"), node("b")],
        edges: [
          edge("x-a", "x", "a"),
          edge("y-b", "y", "b"),
          edge("a-b", "a", "b"),
        ],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "TwoEntries");

    expect(result.ok).toBe(false);
    if (result.ok) throw new Error("compound save unexpectedly succeeded");
    expect(result.code).toBe("multiple_entry_nodes");
  });

  it("rejects outbound edges that do not originate from the exit node", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [node("a", "split"), node("b"), node("x")],
        edges: [
          edge("a-b", "a", "b"),
          edge("a-x", "a", "x", { outcome: "branch", condition: { field: "ok", operator: "eq", value: "true" } }),
        ],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "SplitBranch");

    expect(result.ok).toBe(false);
    if (result.ok) throw new Error("compound save unexpectedly succeeded");
    expect(result.code).toBe("invalid_outbound_edge");
  });

  it("rejects non-producible outbound outcomes from the exit node", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [node("a"), node("b", "approval"), node("x")],
        edges: [edge("a-b", "a", "b"), edge("b-x", "b", "x", { outcome: "reject" })],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "RejectArm");

    expect(result.ok).toBe(false);
    if (result.ok) throw new Error("compound save unexpectedly succeeded");
    expect(result.code).toBe("non_producible_outcome");
  });

  it("preserves root variables and cross-boundary references when saving a compound", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "x",
        variables: [{ name: "ROOT_VAR", default: "root-value" }],
        nodes: [
          node("x", "task", { prompt: "before" }),
          node("a", "task", {
            prompt: "inside {{var:ROOT_VAR}} and {{node:x.output}}",
            contextSources: [{ name: "upstream", nodeId: "x" }],
          }),
          node("b", "task", { prompt: "exit" }),
          node("after", "task", { prompt: "after {{node:b.output}}", contextSources: [{ name: "compound_out", nodeId: "b" }] }),
        ],
        edges: [
          edge("x-a", "x", "a"),
          edge("a-b", "a", "b"),
          edge("b-after", "b", "after"),
        ],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "Bound");

    expect(result).toMatchObject({ ok: true });
    const subflow = store.workflow!.subflows?.Bound;
    expect(subflow?.variables.map((v) => v.name).sort()).toEqual(
      expect.arrayContaining(["ROOT_VAR", "from_x"]),
    );
    const compound = store.workflow!.nodes.find((n) => n.kind.type === "subflow");
    expect(compound?.kind.type).toBe("subflow");
    if (compound?.kind.type !== "subflow") throw new Error("expected subflow compound");
    expect(compound.kind.subflowConfig.inputs).toEqual(
      expect.arrayContaining([
        { name: "ROOT_VAR", source: "var:ROOT_VAR" },
        { name: "from_x", source: "node:x.output" },
      ]),
    );
    const movedA = subflow?.nodes.find((n) => n.id === "a");
    expect(movedA?.prompt).toContain("{{var:ROOT_VAR}}");
    expect(movedA?.prompt).not.toContain("{{node:x.output}}");
    expect(movedA?.contextSources ?? []).toHaveLength(0);
    const after = store.workflow!.nodes.find((n) => n.id === "after");
    expect(after?.prompt).toContain(`{{node:${compound.id}.output}}`);
    expect(after?.contextSources?.[0]?.nodeId).toBe(compound.id);
  });

  it("rejects field-path template references across a compound boundary", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "task", { prompt: "use {{node:x.output.field}}" }),
          node("b", "task"),
          node("x", "task"),
        ],
        edges: [edge("a-b", "a", "b"), edge("x-a", "x", "a")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "FieldPath");

    expect(result.ok).toBe(false);
    if (result.ok) throw new Error("compound save unexpectedly succeeded");
    expect(result.code).toBe("unsupported_dependency");
  });

  it("rejects outside references to non-exit nodes inside the selection", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "task"),
          node("b", "task", { prompt: "middle" }),
          node("c", "task", { prompt: "exit" }),
          node("after", "task", { prompt: "refs {{node:b.output}}" }),
        ],
        edges: [
          edge("a-b", "a", "b"),
          edge("b-c", "b", "c"),
          edge("c-after", "c", "after"),
        ],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b", "c"], "NonExitRef");

    expect(result.ok).toBe(false);
    if (result.ok) throw new Error("compound save unexpectedly succeeded");
    expect(result.code).toBe("unsupported_dependency");
  });

  it("preserves run_agent and send prompt fields across a compound boundary", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "x",
        nodes: [
          node("x", "task", { prompt: "upstream" }),
          node("a", "run_agent", {
            kind: {
              type: "run_agent",
              runAgentConfig: { killAfter: true, prompt: "agent {{node:x.output}}" },
            },
          }),
          node("b", "send", {
            kind: {
              type: "send",
              sendConfig: { target: "editor", text: "send {{node:x.output}}", enter: false },
            },
          }),
          node("after", "run_agent", {
            kind: {
              type: "run_agent",
              runAgentConfig: { killAfter: true, prompt: "after {{node:b.output}}" },
            },
          }),
        ],
        edges: [
          edge("x-a", "x", "a"),
          edge("a-b", "a", "b"),
          edge("b-after", "b", "after"),
        ],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "AgentSend");

    expect(result).toMatchObject({ ok: true });
    const subflow = store.workflow!.subflows?.AgentSend;
    const movedA = subflow?.nodes.find((n) => n.id === "a");
    const movedB = subflow?.nodes.find((n) => n.id === "b");
    expect(movedA?.kind.type).toBe("run_agent");
    if (movedA?.kind.type !== "run_agent") throw new Error("expected run_agent");
    expect(movedA.kind.runAgentConfig?.prompt).toContain("{{var:");
    expect(movedA.kind.runAgentConfig?.prompt).not.toContain("{{node:x.output}}");
    expect(movedB?.kind.type).toBe("send");
    if (movedB?.kind.type !== "send") throw new Error("expected send");
    if (!movedB.kind.sendConfig) throw new Error("expected send config");
    expect(movedB.kind.sendConfig.text).toContain("{{var:");
    expect(movedB.kind.sendConfig.text).not.toContain("{{node:x.output}}");
    expect(movedB.kind.sendConfig.target).toBe("editor");
    expect(movedB.kind.sendConfig.enter).toBe(false);
    const compound = store.workflow!.nodes.find((n) => n.kind.type === "subflow");
    const after = store.workflow!.nodes.find((n) => n.id === "after");
    expect(after?.kind.type).toBe("run_agent");
    if (after?.kind.type !== "run_agent") throw new Error("expected run_agent");
    expect(after.kind.runAgentConfig?.prompt).toContain(`{{node:${compound!.id}.output}}`);
  });

  it("extracts a compound containing a send node with omitted default config", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "send", { kind: { type: "send" } }),
          node("b", "task"),
        ],
        edges: [edge("a-b", "a", "b")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "DefaultSend");

    expect(result).toMatchObject({ ok: true });
    const movedSend = store.workflow!.subflows?.DefaultSend.nodes.find((n) => n.id === "a");
    expect(movedSend?.kind).toEqual({ type: "send" });
  });

  it("rejects cross-boundary field-path refs in run_agent and send prompt fields", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "run_agent", {
            kind: {
              type: "run_agent",
              runAgentConfig: { killAfter: true, prompt: "bad {{node:x.output.field}}" },
            },
          }),
          node("b", "task"),
          node("x", "task"),
        ],
        edges: [edge("a-b", "a", "b"), edge("x-a", "x", "a")],
      }),
    );

    const runAgentResult = store.saveSelectionAsCompound(["a", "b"], "RunAgentField");
    expect(runAgentResult.ok).toBe(false);
    if (runAgentResult.ok) throw new Error("compound save unexpectedly succeeded");
    expect(runAgentResult.code).toBe("unsupported_dependency");

    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "send", {
            kind: { type: "send", sendConfig: { text: "bad {{node:x.output.field}}", enter: true } },
          }),
          node("b", "task"),
          node("x", "task"),
        ],
        edges: [edge("a-b", "a", "b"), edge("x-a", "x", "a")],
      }),
    );

    const sendResult = store.saveSelectionAsCompound(["a", "b"], "SendField");
    expect(sendResult.ok).toBe(false);
    if (sendResult.ok) throw new Error("compound save unexpectedly succeeded");
    expect(sendResult.code).toBe("unsupported_dependency");
  });

  it("allocates distinct bindings when moved nodes share a context name with different sources", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "x",
        nodes: [
          node("x", "task", { prompt: "x-out" }),
          node("y", "task", { prompt: "y-out" }),
          node("a", "task", {
            prompt: "a {{context:notes}}",
            contextSources: [{ name: "notes", nodeId: "x" }],
          }),
          node("b", "task", {
            prompt: "b {{context:notes}}",
            contextSources: [{ name: "notes", nodeId: "y" }],
          }),
        ],
        edges: [edge("x-a", "x", "a"), edge("a-b", "a", "b")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "ContextAlias");

    expect(result).toMatchObject({ ok: true });
    const subflow = store.workflow!.subflows?.ContextAlias;
    const movedA = subflow?.nodes.find((n) => n.id === "a");
    const movedB = subflow?.nodes.find((n) => n.id === "b");
    const varNames = subflow?.variables.map((v) => v.name) ?? [];
    expect(varNames.filter((name) => name === "notes" || name.startsWith("notes_"))).toHaveLength(2);
    expect(movedA?.prompt).toMatch(/\{\{var:(notes|notes_\d+)\}\}/);
    expect(movedB?.prompt).toMatch(/\{\{var:(notes|notes_\d+)\}\}/);
    expect(movedA?.prompt).not.toBe(movedB?.prompt);
  });

  it("keeps context bindings separate from root variables with the same name", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "x",
        variables: [{ name: "notes", default: "root-notes" }],
        nodes: [
          node("x", "task", { prompt: "upstream" }),
          node("a", "task", {
            prompt: "ctx {{context:notes}} root {{var:notes}}",
            contextSources: [{ name: "notes", nodeId: "x" }],
          }),
          node("b", "task", { prompt: "exit" }),
        ],
        edges: [edge("x-a", "x", "a"), edge("a-b", "a", "b")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "ContextRootCollision");

    expect(result).toMatchObject({ ok: true });
    const subflow = store.workflow!.subflows?.ContextRootCollision;
    const compound = store.workflow!.nodes.find((n) => n.kind.type === "subflow");
    if (compound?.kind.type !== "subflow") throw new Error("expected subflow compound");
    expect(compound.kind.subflowConfig.inputs).toEqual(
      expect.arrayContaining([
        { name: "notes", source: "var:notes" },
        { name: "notes_2", source: "node:x.output" },
      ]),
    );
    const movedA = subflow?.nodes.find((n) => n.id === "a");
    expect(movedA?.prompt).toContain("{{var:notes_2}}");
    expect(movedA?.prompt).toContain("{{var:notes}}");
  });

  it("allows internal field-path refs and moved-to-moved context sources", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "task", { prompt: "use {{node:b.output.field}}" }),
          node("b", "task", {
            prompt: "exit {{context:notes}}",
            contextSources: [{ name: "notes", nodeId: "a" }],
          }),
        ],
        edges: [edge("a-b", "a", "b")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "InternalRefs");

    expect(result).toMatchObject({ ok: true });
    const subflow = store.workflow!.subflows?.InternalRefs;
    const movedB = subflow?.nodes.find((n) => n.id === "b");
    expect(movedB?.prompt).toContain("{{context:notes}}");
    expect(movedB?.contextSources).toEqual([{ name: "notes", nodeId: "a" }]);
  });

  it("does not reject unrelated outside field-path refs when saving a compound", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "task"),
          node("b", "task"),
          node("q", "task", { prompt: "use {{node:r.output.field}}" }),
          node("r", "task"),
        ],
        edges: [edge("a-b", "a", "b"), edge("q-r", "q", "r")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "UnrelatedFieldPath");

    expect(result).toMatchObject({ ok: true });
  });

  it("retargets exit field-path references from outside nodes across all prompt fields", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "x",
        nodes: [
          node("x", "task", { prompt: "upstream" }),
          node("a", "task", { prompt: "mid" }),
          node("b", "task", {
            prompt: "exit {{node:a.output.summary}}",
            responseFormat: "json",
          }),
          node("after", "task", {
            prompt: "whole {{node:b.output}} field {{node:b.output.total}}",
          }),
          node("after2", "decide", {
            kind: {
              type: "decide",
              decideConfig: {
                prompt: "field {{node:b.parsedOutput.key}}",
                inputs: [],
                outcomes: [],
              },
            },
          }),
          node("after3", "run_agent", {
            kind: {
              type: "run_agent",
              runAgentConfig: {
                killAfter: true,
                prompt: "agent field {{node:b.output.nested}}",
              },
            },
          }),
          node("after4", "send", {
            kind: {
              type: "send",
              sendConfig: { text: "send {{node:b.parsedOutput.msg}}", enter: true },
            },
          }),
        ],
        edges: [
          edge("x-a", "x", "a"),
          edge("a-b", "a", "b"),
          edge("b-after", "b", "after"),
          edge("b-after2", "b", "after2"),
          edge("b-after3", "b", "after3"),
          edge("b-after4", "b", "after4"),
        ],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "ExitFieldPaths");

    expect(result).toMatchObject({ ok: true });
    const compound = store.workflow!.nodes.find((n) => n.kind.type === "subflow");
    if (!compound) throw new Error("expected compound");
    const cid = compound.id;
    expect(store.workflow!.nodes.find((n) => n.id === "after")?.prompt).toBe(
      `whole {{node:${cid}.output}} field {{node:${cid}.output.total}}`,
    );
    const after2 = store.workflow!.nodes.find((n) => n.id === "after2");
    expect(after2?.kind.type).toBe("decide");
    if (after2?.kind.type !== "decide") throw new Error("expected decide");
    expect(after2.kind.decideConfig.prompt).toBe(`field {{node:${cid}.parsedOutput.key}}`);
    const after3 = store.workflow!.nodes.find((n) => n.id === "after3");
    if (after3?.kind.type !== "run_agent") throw new Error("expected run_agent");
    expect(after3.kind.runAgentConfig?.prompt).toBe(`agent field {{node:${cid}.output.nested}}`);
    const after4 = store.workflow!.nodes.find((n) => n.id === "after4");
    if (after4?.kind.type !== "send") throw new Error("expected send");
    const after4SendConfig = after4.kind.type === "send" ? after4.kind.sendConfig : undefined;
    expect(after4SendConfig?.text).toBe(`send {{node:${cid}.parsedOutput.msg}}`);
  });

  it("leaves unknown whole-output references verbatim in moved and outside nodes", () => {
    const typoRef = "{{node:typo-id.output}}";
    store.setWorkflow(
      workflow({
        entryNodeId: "x",
        nodes: [
          node("x", "task", { prompt: "upstream" }),
          node("a", "task", { prompt: `moved ${typoRef}` }),
          node("b", "task", { prompt: "exit" }),
          node("after", "task", { prompt: `outside ${typoRef}` }),
        ],
        edges: [
          edge("x-a", "x", "a"),
          edge("a-b", "a", "b"),
          edge("b-after", "b", "after"),
        ],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "UnknownWholeOutput");

    expect(result).toMatchObject({ ok: true });
    const subflow = store.workflow!.subflows?.UnknownWholeOutput;
    expect(subflow?.nodes.find((n) => n.id === "a")?.prompt).toContain(typoRef);
    expect(store.workflow!.nodes.find((n) => n.id === "after")?.prompt).toContain(typoRef);
  });

  it("rejects unknown field-path references from moved nodes without mutating the workflow", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "task", { prompt: "moved {{node:typo-id.output.field}}" }),
          node("b", "task", { prompt: "exit" }),
        ],
        edges: [edge("a-b", "a", "b")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "UnknownMovedFieldPath");

    expect(result.ok).toBe(false);
    if (result.ok) throw new Error("compound save unexpectedly succeeded");
    expect(result.code).toBe("unsupported_dependency");
    expect(store.workflow!.subflows?.UnknownMovedFieldPath).toBeUndefined();
    expect(store.workflow!.nodes.some((n) => n.kind.type === "subflow")).toBe(false);
  });

  it("rejects unknown context sources on moved nodes without mutating the workflow", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "task", {
            prompt: "moved {{context:notes}}",
            contextSources: [{ name: "notes", nodeId: "typo-id" }],
          }),
          node("b", "task", { prompt: "exit" }),
        ],
        edges: [edge("a-b", "a", "b")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "UnknownMovedContextSource");

    expect(result.ok).toBe(false);
    if (result.ok) throw new Error("compound save unexpectedly succeeded");
    expect(result.code).toBe("unsupported_dependency");
    expect(store.workflow!.subflows?.UnknownMovedContextSource).toBeUndefined();
    expect(store.workflow!.nodes.some((n) => n.kind.type === "subflow")).toBe(false);
  });

  it("rejects unknown field-path references from outside nodes without mutating the workflow", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "task", { prompt: "moved" }),
          node("b", "task", { prompt: "exit" }),
          node("after", "task", { prompt: "outside {{node:typo-id.parsedOutput.field}}" }),
        ],
        edges: [edge("a-b", "a", "b"), edge("b-after", "b", "after")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b"], "UnknownOutsideFieldPath");

    expect(result.ok).toBe(false);
    if (result.ok) throw new Error("compound save unexpectedly succeeded");
    expect(result.code).toBe("unsupported_dependency");
    expect(store.workflow!.subflows?.UnknownOutsideFieldPath).toBeUndefined();
    expect(store.workflow!.nodes.some((n) => n.kind.type === "subflow")).toBe(false);
  });

  it("binds positional tokens on the entry node when extracting a middle compound", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "task", { prompt: "upstream" }),
          node("b", "task", { prompt: "middle {{previous_output}}" }),
          node("c", "task", { prompt: "downstream" }),
        ],
        edges: [edge("a-b", "a", "b"), edge("b-c", "b", "c")],
      }),
    );

    const result = store.saveSelectionAsCompound(["b"], "MiddleEntryPositional");

    expect(result).toMatchObject({ ok: true });
    const compound = store.workflow!.nodes.find((n) => n.kind.type === "subflow");
    if (compound?.kind.type !== "subflow") throw new Error("expected subflow");
    expect(compound.kind.subflowConfig.inputs).toEqual(
      expect.arrayContaining([{ name: "previous_output", source: "previous_output" }]),
    );
    const subflow = store.workflow!.subflows?.MiddleEntryPositional;
    const movedB = subflow?.nodes.find((n) => n.id === "b");
    expect(movedB?.prompt).toContain("{{var:previous_output}}");
    expect(movedB?.prompt).not.toContain("{{previous_output}}");
  });

  it("rejects {{all_predecessors}} on the entry node without mutating the workflow", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "task", { prompt: "upstream" }),
          node("b", "task", { prompt: "entry {{all_predecessors}}" }),
          node("c", "task", { prompt: "exit" }),
        ],
        edges: [edge("a-b", "a", "b"), edge("b-c", "b", "c")],
      }),
    );

    const result = store.saveSelectionAsCompound(["b"], "AllPredecessorsEntry");

    expect(result.ok).toBe(false);
    if (result.ok) throw new Error("compound save unexpectedly succeeded");
    expect(result.code).toBe("unsupported_dependency");
    expect(store.workflow!.subflows?.AllPredecessorsEntry).toBeUndefined();
    expect(store.workflow!.nodes.some((n) => n.kind.type === "subflow")).toBe(false);
  });

  it("does not rewrite positional tokens on non-entry moved nodes", () => {
    store.setWorkflow(
      workflow({
        entryNodeId: "a",
        nodes: [
          node("a", "task", { prompt: "entry" }),
          node("b", "task", { prompt: "inner {{previous_output}}" }),
          node("c", "task", { prompt: "exit" }),
        ],
        edges: [edge("a-b", "a", "b"), edge("b-c", "b", "c")],
      }),
    );

    const result = store.saveSelectionAsCompound(["a", "b", "c"], "NonEntryPositional");

    expect(result).toMatchObject({ ok: true });
    const subflow = store.workflow!.subflows?.NonEntryPositional;
    expect(subflow?.nodes.find((n) => n.id === "b")?.prompt).toContain("{{previous_output}}");
  });

  it("clears stale pane selection when a new run stream begins without kickoff panes", () => {
    store.selectPane("cursor:stale-from-prior-run");
    store.setRunObservability({
      panes: [
        {
          pane: "cursor:stale-from-prior-run",
          sessionName: "old-sess",
          attachCommand: "tmux attach -t old-sess",
        },
      ],
    });

    store.beginRunStream();

    expect(store.selectedPane).toBe("active");
    expect(store.runObservability).toBeNull();

    store.setRunObservability({
      panes: [
        {
          pane: "cursor:fresh-node",
          sessionName: "new-sess",
          attachCommand: "tmux attach -t new-sess",
        },
      ],
    });
    expect(store.selectedPane).toBe("cursor:fresh-node");
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

  it("ignores stale run events after the run epoch advances", () => {
    const { epoch: epochA } = store.beginRunStream();
    store.setRunState({ runId: "run-a", running: true });
    store.beginRunStream();
    store.setRunState({ runId: "run-b", running: true });

    store.applyRunEvent({ type: "done", runId: "run-a" }, epochA);

    expect(store.runId).toBe("run-b");
    expect(store.running).toBe(true);
    expect(store.lines.some((line) => line.text === "Workflow complete")).toBe(false);
  });

  it("does not adopt runId from a different active run", () => {
    store.setRunState({ runId: "run-b", running: true });
    store.applyRunEvent({ type: "node_start", nodeId: "n1", runId: "run-a" });
    expect(store.runId).toBe("run-b");
  });

  it("upserts agent_interaction_required events for the same session", () => {
    store.applyRunEvent({
      type: "agent_interaction_required",
      sessionId: "sess-a",
      description: "first",
      outputSoFar: "out-1",
      interactionType: "question",
    });
    store.applyRunEvent({
      type: "agent_interaction_required",
      sessionId: "sess-a",
      description: "updated",
      outputSoFar: "out-2",
      interactionType: "permission",
    });

    expect(store.interactions).toHaveLength(1);
    expect(store.interactions[0]).toMatchObject({
      sessionId: "sess-a",
      description: "updated",
      outputSoFar: "out-2",
      interactionType: "permission",
    });
  });

  it("resolving one session leaves other pending interaction cards intact", () => {
    store.applyRunEvent({
      type: "agent_interaction_required",
      sessionId: "sess-a",
      description: "wait A",
      outputSoFar: "",
      interactionType: "question",
    });
    store.applyRunEvent({
      type: "agent_interaction_required",
      sessionId: "sess-b",
      description: "wait B",
      outputSoFar: "",
      interactionType: "question",
    });

    store.applyRunEvent({
      type: "agent_interaction_resolved",
      sessionId: "sess-b",
      description: "wait B",
    });

    expect(store.interactions.map((item) => item.sessionId)).toEqual(["sess-a"]);
  });

  it("ignores agent_interaction_resolved events without sessionId", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});

    store.applyRunEvent({
      type: "agent_interaction_required",
      sessionId: "sess-a",
      description: "wait A",
      outputSoFar: "",
      interactionType: "question",
    });
    store.applyRunEvent({
      type: "agent_interaction_resolved",
      description: "missing session",
    });

    expect(store.interactions).toHaveLength(1);
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });
});
