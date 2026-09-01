import type { WorkflowDocument, WorkflowEdge, WorkflowNode } from "@/lib/types/workflow";

export type CanvasPositions = Record<string, { x: number; y: number }>;

export function decideNode(): WorkflowNode {
  return {
    id: "decide",
    name: "Route",
    kind: {
      type: "decide",
      decideConfig: {
        prompt: "Pick a path",
        inputs: [],
        outcomes: ["yes"],
      },
    },
    agent: null,
    prompt: "",
    contextSources: [],
    responseFormat: null,
  };
}

export function targetNode(): WorkflowNode {
  return {
    id: "target_0",
    name: "Target",
    kind: { type: "task" },
    agent: "claude",
    prompt: "Done",
    contextSources: [],
    responseFormat: null,
  };
}

export function branchEdge(patch: Partial<WorkflowEdge> = {}): WorkflowEdge {
  return {
    id: "branch_yes",
    from: "decide",
    to: "target_0",
    outcome: "branch",
    label: null,
    branchId: null,
    condition: null,
    ...patch,
  };
}

export function buildDecideBranchWorkflow(options: {
  name?: string;
  canvas?: CanvasPositions;
  edge?: Partial<WorkflowEdge>;
  edges?: WorkflowEdge[];
} = {}): WorkflowDocument {
  const canvas = options.canvas ?? {
    decide: { x: 0, y: 0 },
    target_0: { x: 200, y: 0 },
  };

  return {
    version: 4,
    name: options.name ?? "Test Workflow",
    goal: "",
    cwd: "",
    useOrchestrator: false,
    entryNodeId: "decide",
    variables: [],
    limits: { maxTotalSteps: 50, maxVisitsPerNode: 10 },
    nodes: [decideNode(), targetNode()],
    edges: options.edges ?? [branchEdge(options.edge)],
    ui: {
      canvas: {
        viewport: { x: 0, y: 0, zoom: 1 },
        nodes: canvas,
      },
    },
  };
}
