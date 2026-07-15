import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it } from "vitest";
import { store } from "@/lib/stores/workflowStore.svelte";
import type {
  AgentCapabilities,
  RuntimeCapabilities,
  WorkflowDocument,
  WorkflowNode,
} from "@/lib/types/workflow";
import InspectorPanel from "./InspectorPanel.svelte";

const agentCaps: AgentCapabilities = {
  workerExecution: true,
  promptRefinement: true,
  branchChoice: true,
  loopVerdict: true,
  structuredOutput: true,
  sessionReuse: true,
  nativeJsonSchema: true,
  modelSelection: true,
  reasoningConfig: true,
  systemPrompt: true,
  budgetLimit: true,
  turnLimit: true,
  costReporting: true,
  toolAllowlist: true,
  webSearch: true,
};

const capabilities: RuntimeCapabilities = {
  workflowVersion: 4,
  supportedNodeTypes: [
    "task",
    "approval",
    "split",
    "collector",
    "decide",
    "parallel_batch",
    "subflow",
    "call",
    "spawn",
    "send",
    "wait",
    "capture",
    "kill",
    "run_agent",
  ],
  supportedEdgeOutcomes: ["success", "reject", "branch", "loop_continue", "loop_exit"],
  features: {
    split: true,
    collector: true,
  },
  agents: {
    claude: {
      available: true,
      capabilities: agentCaps,
      accessProfiles: ["default", "full-access", "read-only", "workspace-write"],
    },
    codex: {
      available: true,
      capabilities: agentCaps,
      accessProfiles: ["default", "full-access", "read-only", "workspace-write"],
    },
  },
};

function workflow(patch: Partial<WorkflowDocument> = {}): WorkflowDocument {
  return {
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
    ...patch,
  };
}

function renderInspector(
  document: WorkflowDocument,
  selectedNodeId?: string,
  runtimeCapabilities: RuntimeCapabilities = capabilities,
) {
  store.setWorkflow(document);
  if (selectedNodeId) {
    store.selectNode(selectedNodeId);
  }

  return render(InspectorPanel, {
    props: {
      workflow: store.workflow!,
      validation: null,
      capabilities: runtimeCapabilities,
    },
  });
}

describe("InspectorPanel", () => {
  afterEach(() => {
    cleanup();
    store.createWorkflow();
  });

  it("edits run_agent agent and prompt through runAgentConfig", async () => {
    const runAgentNode: WorkflowNode = {
      id: "reviewer-a",
      name: "Reviewer A",
      kind: {
        type: "run_agent",
        runAgentConfig: {
          agent: "claude",
          prompt: "Nested prompt from template",
          killAfter: true,
        },
      },
      agent: "",
      prompt: "",
      contextSources: [],
      responseFormat: null,
    };

    const { container } = renderInspector(workflow({
      entryNodeId: runAgentNode.id,
      nodes: [runAgentNode],
    }), runAgentNode.id);

    const agentSelect = screen.getByRole("combobox", { name: /^Agent/ }) as HTMLSelectElement;
    expect(agentSelect.value).toBe("claude");

    await fireEvent.change(agentSelect, { target: { value: "codex" } });

    const promptTextarea = container.querySelector(".promptTextarea-wrapper textarea") as HTMLTextAreaElement;
    expect(promptTextarea).toHaveValue("Nested prompt from template");

    await fireEvent.input(promptTextarea, { target: { value: "Edited nested prompt" } });

    const node = store.workflow!.nodes[0];
    expect(node.agent).toBe("");
    expect(node.prompt).toBe("");
    expect(node.kind.type).toBe("run_agent");
    if (node.kind.type !== "run_agent") throw new Error("node was not run_agent");
    expect(node.kind.runAgentConfig?.agent).toBe("codex");
    expect(node.kind.runAgentConfig?.prompt).toBe("Edited nested prompt");
  });

  it("persists and displays read-only for a read-only-only agent", async () => {
    const readOnlyCapabilities: RuntimeCapabilities = {
      ...capabilities,
      agents: {
        observer: {
          available: true,
          capabilities: agentCaps,
          accessProfiles: ["read-only"],
        },
      },
    };
    const taskNode: WorkflowNode = {
      id: "observer-task",
      name: "Observer task",
      kind: { type: "task" },
      agent: "observer",
      prompt: "Inspect the repository",
      contextSources: [],
      responseFormat: null,
    };

    renderInspector(workflow({
      entryNodeId: taskNode.id,
      nodes: [taskNode],
    }), taskNode.id, readOnlyCapabilities);

    const accessMode = await screen.findByDisplayValue("read_only") as HTMLSelectElement;
    expect(accessMode).toHaveValue("read_only");
    const storedNode = store.workflow!.nodes[0];
    expect(storedNode.kind.type).toBe("task");
    if (storedNode.kind.type !== "task") throw new Error("node was not task");
    expect(storedNode.kind.agentConfig?.accessMode).toBe("read_only");
  });

  it("persists and displays unrestricted in defaults for a full-access-only agent", async () => {
    const fullAccessCapabilities: RuntimeCapabilities = {
      ...capabilities,
      agents: {
        deployer: {
          available: true,
          capabilities: agentCaps,
          accessProfiles: ["full-access"],
        },
      },
    };

    renderInspector(workflow(), undefined, fullAccessCapabilities);
    await fireEvent.click(screen.getByRole("button", { name: /deployer/ }));

    const accessMode = await screen.findByDisplayValue("unrestricted") as HTMLSelectElement;
    expect(accessMode).toHaveValue("unrestricted");
    expect(store.workflow!.agentDefaults?.deployer?.accessMode).toBe("unrestricted");
  });

  it("surfaces an agent with no supported access modes without persisting a fallback", async () => {
    const noAccessCapabilities: RuntimeCapabilities = {
      ...capabilities,
      agents: {
        broken: {
          available: true,
          capabilities: agentCaps,
          accessProfiles: [],
        },
      },
    };
    const taskNode: WorkflowNode = {
      id: "broken-task",
      name: "Broken task",
      kind: { type: "task" },
      agent: "broken",
      prompt: "Cannot launch",
      contextSources: [],
      responseFormat: null,
    };

    renderInspector(workflow({ nodes: [taskNode] }), taskNode.id, noAccessCapabilities);

    expect(await screen.findByText("Agent has no supported access profiles.")).toBeInTheDocument();
    const storedNode = store.workflow!.nodes[0];
    expect(storedNode.kind.type).toBe("task");
    if (storedNode.kind.type !== "task") throw new Error("node was not task");
    expect(storedNode.kind.agentConfig?.accessMode).toBeUndefined();
  });

  it("round-trips run-as command prefix as one argv element per line", async () => {
    const { container } = renderInspector(workflow({
      runAs: {
        user: "sandbox",
        command: ["sudo", "-u", "Agent User"],
        socket: "silver",
      },
    }));

    const commandField = Array.from(container.querySelectorAll("label.field")).find(
      (label) => label.textContent?.includes("Command prefix"),
    );
    const commandTextarea = commandField?.querySelector("textarea") as HTMLTextAreaElement;
    expect(commandTextarea).toHaveValue("sudo\n-u\nAgent User");
    expect(screen.getByText("sudo -u Agent User tmux -L silver attach -t <session>")).toBeInTheDocument();

    await fireEvent.input(commandTextarea, { target: { value: "doas\n-u\nAgent User" } });

    expect(store.workflow!.runAs?.command).toEqual(["doas", "-u", "Agent User"]);
    expect(screen.getByText("doas -u Agent User tmux -L silver attach -t <session>")).toBeInTheDocument();
  });
});
