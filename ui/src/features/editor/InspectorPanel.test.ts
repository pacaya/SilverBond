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
  workflowVersion: 3,
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
    },
    codex: {
      available: true,
      capabilities: agentCaps,
    },
  },
};

function workflow(patch: Partial<WorkflowDocument> = {}): WorkflowDocument {
  return {
    version: 3,
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

function renderInspector(document: WorkflowDocument, selectedNodeId?: string) {
  store.setWorkflow(document);
  if (selectedNodeId) {
    store.selectNode(selectedNodeId);
  }

  return render(InspectorPanel, {
    props: {
      workflow: store.workflow!,
      validation: null,
      capabilities,
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
      type: "run_agent",
      agent: "",
      prompt: "",
      contextSources: [],
      responseFormat: null,
      runAgentConfig: {
        agent: "claude",
        prompt: "Nested prompt from template",
        killAfter: true,
      },
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
    expect(node.runAgentConfig?.agent).toBe("codex");
    expect(node.runAgentConfig?.prompt).toBe("Edited nested prompt");
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
