import { beforeEach, describe, expect, it } from "vitest";
import { store } from "@/lib/stores/workflowStore.svelte";

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
      type: "split",
      agent: null,
      responseFormat: null,
      splitFailurePolicy: "best_effort_continue",
    });
    expect(collector).toMatchObject({
      type: "collector",
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
      result: { success: true, output: "ok", stderr: "" },
    });

    expect(store.nodeStates.n1).toBe("success");
    expect(store.lines.at(-1)?.text).toContain("ok");
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
});
