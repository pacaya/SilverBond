import { describe, expect, it } from "vitest";
import {
  duplicateWorkflowForEditing,
  normalizeWorkflowNode,
  normalizeWorkflowNodes,
  type WorkflowDocument,
  type WorkflowNode,
} from "@/lib/types/workflow";

function legacyNode(
  id: string,
  type: string,
  extra: Record<string, unknown> = {},
): WorkflowNode {
  return { id, name: id, prompt: "", ...extra, type } as unknown as WorkflowNode;
}

function v3Node(id: string, kind: WorkflowNode["kind"]): WorkflowNode {
  return { id, name: id, prompt: "", kind };
}

describe("normalizeWorkflowNode", () => {
  it("migrates legacy task nodes with top-level agentConfig", () => {
    const agentConfig = { orchestrator: { enabled: false } };
    const result = normalizeWorkflowNode(
      legacyNode("n1", "task", { agentConfig }),
    );

    expect(result.kind).toEqual({ type: "task", agentConfig });
    expect(result).not.toHaveProperty("type");
    expect(result).not.toHaveProperty("agentConfig");
  });

  it("migrates legacy decide and run_agent nodes", () => {
    const decideConfig = { branches: [] };
    const decide = normalizeWorkflowNode(
      legacyNode("d1", "decide", { decideConfig }),
    );
    expect(decide.kind).toEqual({ type: "decide", decideConfig });
    expect(decide).not.toHaveProperty("decideConfig");

    const runAgentConfig = { agentId: "a1" };
    const agentConfig = { orchestrator: { enabled: true } };
    const runAgent = normalizeWorkflowNode(
      legacyNode("r1", "run_agent", { runAgentConfig, agentConfig }),
    );
    expect(runAgent.kind).toEqual({
      type: "run_agent",
      runAgentConfig,
      agentConfig,
    });
    expect(runAgent).not.toHaveProperty("runAgentConfig");
    expect(runAgent).not.toHaveProperty("agentConfig");
  });

  it("defaults capture and kill config objects when absent", () => {
    expect(normalizeWorkflowNode(legacyNode("c1", "capture")).kind).toEqual({
      type: "capture",
      captureConfig: {},
    });
    expect(normalizeWorkflowNode(legacyNode("k1", "kill")).kind).toEqual({
      type: "kill",
      killConfig: {},
    });
  });

  it("defaults subflow config for legacy and kind-shaped nodes", () => {
    const expected = { workflowName: "", inputs: [], maxDepth: 10 };

    expect(normalizeWorkflowNode(legacyNode("s1", "subflow")).kind).toEqual({
      type: "subflow",
      subflowConfig: expected,
    });
    expect(
      normalizeWorkflowNode(
        v3Node("s2", { type: "subflow" } as WorkflowNode["kind"]),
      ).kind,
    ).toEqual({ type: "subflow", subflowConfig: expected });
  });

  it("defaults decide config for legacy and kind-shaped nodes", () => {
    const expected = { prompt: "", inputs: [], outcomes: [] };

    expect(normalizeWorkflowNode(legacyNode("d2", "decide")).kind).toEqual({
      type: "decide",
      decideConfig: expected,
    });
    expect(
      normalizeWorkflowNode(
        v3Node("d3", { type: "decide" } as WorkflowNode["kind"]),
      ).kind,
    ).toEqual({ type: "decide", decideConfig: expected });
  });

  it("defaults parallel batch config for legacy and kind-shaped nodes", () => {
    const expected = { itemsBinding: "", maxConcurrent: 4, itemVar: "item", bodyEntry: "" };

    expect(normalizeWorkflowNode(legacyNode("b1", "parallel_batch")).kind).toEqual({
      type: "parallel_batch",
      batchConfig: expected,
    });
    expect(
      normalizeWorkflowNode(
        v3Node("b2", { type: "parallel_batch" } as WorkflowNode["kind"]),
      ).kind,
    ).toEqual({ type: "parallel_batch", batchConfig: expected });
  });

  it("leaves v3 nodes untouched", () => {
    const node = v3Node("v1", { type: "approval" });
    expect(normalizeWorkflowNode(node)).toBe(node);
  });

  it("synthesizes a task kind for malformed nodes", () => {
    const malformed = { id: "m1", name: "m1", prompt: "" } as WorkflowNode;
    expect(normalizeWorkflowNode(malformed).kind).toEqual({ type: "task" });
  });
});

function baseWorkflow(
  patch: Partial<WorkflowDocument> = {},
): WorkflowDocument {
  return {
    version: 4,
    name: "wf",
    goal: "",
    cwd: "",
    useOrchestrator: false,
    entryNodeId: "n1",
    variables: [],
    limits: { maxTotalSteps: 50, maxVisitsPerNode: 10 },
    nodes: [],
    edges: [],
    ...patch,
  } as WorkflowDocument;
}

describe("normalizeWorkflowNodes", () => {
  it("normalizes every node in a workflow document", () => {
    const workflow = baseWorkflow({
      nodes: [legacyNode("n1", "task"), v3Node("n2", { type: "split" })],
    });

    const normalized = normalizeWorkflowNodes(workflow);
    expect(normalized.nodes[0].kind).toEqual({ type: "task" });
    expect(normalized.nodes[1]).toBe(workflow.nodes[1]);
  });

  it("normalizes legacy nodes inside subflow bodies", () => {
    const subflows = {
      Child: baseWorkflow({
        entryNodeId: "s1",
        nodes: [legacyNode("s1", "task")],
      }),
    };
    const workflow = baseWorkflow({
      nodes: [v3Node("n1", { type: "split" })],
      subflows,
    });

    const normalized = normalizeWorkflowNodes(workflow);
    expect(normalized.subflows!.Child.nodes[0].kind).toEqual({ type: "task" });
    expect(subflows.Child.nodes[0]).toHaveProperty("type", "task");
  });

  it("keeps an already-normalized subflows record reference-identical", () => {
    const subflows = {
      Child: baseWorkflow({
        entryNodeId: "s1",
        nodes: [v3Node("s1", { type: "task" })],
      }),
    };
    const workflow = baseWorkflow({
      nodes: [v3Node("n1", { type: "split" })],
      subflows,
    });

    const normalized = normalizeWorkflowNodes(workflow);
    expect(normalized.subflows).toBe(subflows);
  });
});

describe("duplicateWorkflowForEditing", () => {
  it("clones and normalizes legacy nodes at ingress", () => {
    const workflow = {
      version: 4,
      name: "wf",
      goal: "",
      cwd: "",
      useOrchestrator: false,
      entryNodeId: "n1",
      variables: [],
      limits: { maxTotalSteps: 50, maxVisitsPerNode: 10 },
      nodes: [legacyNode("n1", "decide", { decideConfig: { branches: [] } })],
      edges: [],
    } as WorkflowDocument;

    const duplicated = duplicateWorkflowForEditing(workflow);
    expect(duplicated).not.toBe(workflow);
    expect(duplicated.nodes[0].kind).toEqual({
      type: "decide",
      decideConfig: { branches: [] },
    });
    expect(workflow.nodes[0]).toHaveProperty("type", "decide");
  });
});
