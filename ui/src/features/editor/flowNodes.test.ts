import { beforeEach, describe, expect, it } from "vitest";
import { buildFlowNodes, buildValidationIndex } from "@/features/editor/flowNodes";
import { store } from "@/lib/stores/workflowStore.svelte";

describe("buildFlowNodes", () => {
  beforeEach(() => {
    store.createWorkflow();
    store.clearLines();
    store.resetRun();
  });

  it("returns plain nodes from rune-backed workflow data", () => {
    store.addNode("task", { x: 100, y: 120 });
    store.addNode("split", { x: 320, y: 120 });
    store.addNode("collector", { x: 540, y: 120 });

    const workflow = store.workflow!;
    const nodes = buildFlowNodes(
      workflow,
      null,
      buildValidationIndex(null),
      store.nodeStates,
      workflow.nodes[0].id,
    );

    expect(nodes).toHaveLength(3);
    expect(nodes[0]).toMatchObject({
      id: workflow.nodes[0].id,
      position: { x: 100, y: 120 },
      data: { label: "Task 1" },
      selected: true,
    });
    expect(nodes[1].class).toContain("graphNode--split");
    expect(nodes[2].class).toContain("graphNode--collector");
    expect(() => structuredClone(nodes[0])).not.toThrow();
  });

  it("assigns split defaults and distinct inline accents for new node types", () => {
    store.addNode("split", { x: 80, y: 100 });
    store.addNode("collector", { x: 320, y: 100 });

    const workflow = store.workflow!;
    const [splitNode, collectorNode] = workflow.nodes;
    const nodes = buildFlowNodes(
      workflow,
      null,
      buildValidationIndex(null),
      store.nodeStates,
      null,
    );

    expect(splitNode.splitFailurePolicy).toBe("best_effort_continue");
    expect(splitNode.responseFormat).toBeNull();
    expect(collectorNode.responseFormat).toBeNull();
    expect(nodes[0].style).toContain("border-left: 4px solid rgba(249, 115, 22, 0.78)");
    expect(nodes[1].style).toContain("border-left: 4px solid rgba(45, 212, 191, 0.72)");
  });
});
