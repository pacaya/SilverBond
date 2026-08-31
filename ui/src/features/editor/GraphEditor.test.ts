import { cleanup, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it } from "vitest";
import { store } from "@/lib/stores/workflowStore.svelte";
import type { WorkflowDocument } from "@/lib/types/workflow";
import GraphEditor from "./GraphEditor.svelte";

const workflow: WorkflowDocument = {
  version: 4,
  name: "Test Workflow",
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
};

describe("GraphEditor", () => {
  afterEach(() => {
    cleanup();
    store.createWorkflow();
  });

  it("distinguishes unavailable node kinds from pending capabilities", async () => {
    store.setWorkflow(workflow);

    const { rerender } = render(GraphEditor, {
      props: {
        workflow,
        validation: null,
        capabilities: undefined,
        capabilitiesError: false,
      },
    });

    expect(screen.getByText("Loading node types…")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "+ Task" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "+ Approval" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "+ Split" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "+ Collector" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "+ More ▾" })).not.toBeInTheDocument();

    await rerender({
      workflow,
      validation: null,
      capabilities: undefined,
      capabilitiesError: true,
    });

    expect(screen.getByText("Node types unavailable.")).toBeInTheDocument();
    expect(screen.queryByText("Loading node types…")).not.toBeInTheDocument();
  }, 20_000);
});
