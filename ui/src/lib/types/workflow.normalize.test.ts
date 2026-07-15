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

  it("leaves v3 nodes untouched", () => {
    const node = v3Node("v1", { type: "approval" });
    expect(normalizeWorkflowNode(node)).toBe(node);
  });

  it("synthesizes a task kind for malformed nodes", () => {
    const malformed = { id: "m1", name: "m1", prompt: "" } as WorkflowNode;
    expect(normalizeWorkflowNode(malformed).kind).toEqual({ type: "task" });
  });
});

describe("normalizeWorkflowNodes", () => {
  it("normalizes every node in a workflow document", () => {
    const workflow = {
      version: 4,
      name: "wf",
      goal: "",
      cwd: "",
      useOrchestrator: false,
      entryNodeId: "n1",
      variables: [],
      limits: { maxTotalSteps: 50, maxVisitsPerNode: 10 },
      nodes: [legacyNode("n1", "task"), v3Node("n2", { type: "split" })],
      edges: [],
    } as WorkflowDocument;

    const normalized = normalizeWorkflowNodes(workflow);
    expect(normalized.nodes[0].kind).toEqual({ type: "task" });
    expect(normalized.nodes[1]).toBe(workflow.nodes[1]);
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
