import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api, ApiError } from "@/lib/api/client";
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
    vi.restoreAllMocks();
    cleanup();
    store.createWorkflow();
  });

  it("prompts for an unlock secret and retries a privileged node preview", async () => {
    const taskNode: WorkflowNode = {
      id: "preview-task",
      name: "Preview task",
      kind: { type: "task" },
      agent: "echo",
      prompt: "Preview task",
      contextSources: [],
      responseFormat: null,
    };
    const testNode = vi.spyOn(api, "testNode")
      .mockRejectedValueOnce(new ApiError(
        "Privileged run requires unlock password.",
        403,
        { code: "privileged_unlock_required" },
      ))
      .mockResolvedValueOnce({ success: true } as never);

    renderInspector(workflow({
      entryNodeId: taskNode.id,
      nodes: [taskNode],
    }), taskNode.id);

    await fireEvent.click(screen.getByRole("button", { name: /Test/ }));
    await fireEvent.click(screen.getByRole("button", { name: "Run preview" }));

    // The masked dialog replaces window.prompt: type the secret and submit.
    const input = await screen.findByLabelText("Password");
    expect(input).toHaveProperty("type", "password");
    await fireEvent.input(input, { target: { value: "preview-unlock" } });
    await fireEvent.click(screen.getByRole("button", { name: "Unlock" }));

    await waitFor(() => expect(testNode).toHaveBeenCalledTimes(2));
    expect(testNode).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ id: "preview-task" }),
      "",
      { previousOutput: "" },
      "preview-unlock",
    );
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

  it("edits run_agent access through capability-backed registry profiles", async () => {
    const runAgentNode: WorkflowNode = {
      id: "reviewer-a",
      name: "Reviewer A",
      kind: {
        type: "run_agent",
        runAgentConfig: {
          agent: "codex",
          access: "read-only",
          killAfter: true,
        },
      },
      agent: "",
      prompt: "",
      contextSources: [],
      responseFormat: null,
    };

    renderInspector(workflow({
      entryNodeId: runAgentNode.id,
      nodes: [runAgentNode],
    }), runAgentNode.id);

    const access = screen.getByRole("combobox", { name: /^Access$/ }) as HTMLSelectElement;
    expect(access).toHaveValue("read-only");
    expect(screen.getByRole("option", { name: "registry default" })).toHaveValue("");

    await fireEvent.change(access, { target: { value: "full-access" } });

    const node = store.workflow!.nodes[0];
    if (node.kind.type !== "run_agent") throw new Error("node was not run_agent");
    expect(node.kind.runAgentConfig?.access).toBe("full-access");
  });

  it("edits spawn access through capability-backed registry profiles", async () => {
    const spawnNode: WorkflowNode = {
      id: "worker",
      name: "Worker",
      kind: {
        type: "spawn",
        spawnConfig: {
          agent: "claude",
          access: "workspace-write",
        },
      },
      agent: null,
      prompt: "",
      contextSources: [],
      responseFormat: null,
    };

    renderInspector(workflow({
      entryNodeId: spawnNode.id,
      nodes: [spawnNode],
    }), spawnNode.id);

    const access = screen.getByRole("combobox", { name: /^Access$/ }) as HTMLSelectElement;
    expect(access).toHaveValue("workspace-write");

    await fireEvent.change(access, { target: { value: "" } });

    const node = store.workflow!.nodes[0];
    if (node.kind.type !== "spawn") throw new Error("node was not spawn");
    expect(node.kind.spawnConfig?.access).toBeUndefined();
  });

  it("renders defaults for a configless capture node", () => {
    const captureNode: WorkflowNode = {
      id: "capture-output",
      name: "Capture output",
      kind: { type: "capture" },
      agent: null,
      prompt: "",
      contextSources: [],
      responseFormat: null,
    };

    renderInspector(workflow({
      entryNodeId: captureNode.id,
      nodes: [captureNode],
    }), captureNode.id);

    expect(screen.getByRole("textbox", { name: "Target pane" })).toHaveValue("");
    expect(screen.getByRole("checkbox", { name: "Capture all scrollback" })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Include ANSI" })).not.toBeChecked();
  });

  it("renders defaults for a configless kill node", () => {
    const killNode: WorkflowNode = {
      id: "kill-pane",
      name: "Kill pane",
      kind: { type: "kill" },
      agent: null,
      prompt: "",
      contextSources: [],
      responseFormat: null,
    };

    renderInspector(workflow({
      entryNodeId: killNode.id,
      nodes: [killNode],
    }), killNode.id);

    expect(screen.getByRole("textbox", { name: "Target pane" })).toHaveValue("");
    expect(screen.getByRole("textbox", { name: "Session name" })).toHaveValue("");
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

  it("persists run-as user and clears empty runAs", async () => {
    const { container } = renderInspector(workflow());

    const userField = Array.from(container.querySelectorAll("label.field")).find(
      (label) => label.textContent?.includes("User"),
    );
    const userInput = userField?.querySelector("input") as HTMLInputElement;
    await fireEvent.input(userInput, { target: { value: "sandbox" } });
    expect(store.workflow!.runAs?.user).toBe("sandbox");

    await fireEvent.input(userInput, { target: { value: "  " } });
    expect(store.workflow!.runAs).toBeUndefined();
  });

  it("round-trips run-as command prefix as one argv element per line", async () => {
    const { container } = renderInspector(workflow({
      runAs: {
        user: "sandbox",
        command: ["sudo", "-u", "Agent User"],
      },
    }));

    const commandField = Array.from(container.querySelectorAll("label.field")).find(
      (label) => label.textContent?.includes("Command prefix"),
    );
    const commandTextarea = commandField?.querySelector("textarea") as HTMLTextAreaElement;
    expect(commandTextarea).toHaveValue("sudo\n-u\nAgent User");
    expect(screen.getByText("sudo -u Agent User tmux -L silverbond-<run-id> attach -t <session>")).toBeInTheDocument();

    await fireEvent.input(commandTextarea, { target: { value: "doas\n-u\nAgent User" } });

    expect(store.workflow!.runAs?.command).toEqual(["doas", "-u", "Agent User"]);
    expect(screen.getByText("doas -u Agent User tmux -L silverbond-<run-id> attach -t <session>")).toBeInTheDocument();
  });

  it("hides Run As and Limits when drilled into a subflow", async () => {
    const subflowA = workflow({
      name: "A",
      entryNodeId: "a1",
      nodes: [{
        id: "a1",
        name: "Step",
        kind: { type: "task" },
        agent: "claude",
        prompt: "Do work",
        contextSources: [],
        responseFormat: null,
      }],
    });
    renderInspector(workflow({
      runAs: { user: "root-user" },
      limits: { maxTotalSteps: 100, maxVisitsPerNode: 20 },
      entryNodeId: "call_a",
      nodes: [{
        id: "call_a",
        name: "Call A",
        kind: {
          type: "subflow",
          subflowConfig: { workflowName: "A", inputs: [], maxDepth: 10 },
        },
        agent: null,
        prompt: "",
        contextSources: [],
        responseFormat: null,
      }],
      subflows: { A: subflowA },
    }));

    expect(screen.getByText("Run As / Sandbox")).toBeInTheDocument();
    expect(screen.getByText("Limits")).toBeInTheDocument();

    expect(store.drillIntoSubflow("call_a")).toBe(true);
    // drillIntoSubflow is called directly here (not via the "Open subgraph"
    // button's onclick), bypassing the DOM event dispatch that would
    // otherwise trigger Svelte's synchronous effect flush. A real user
    // interaction re-renders immediately; a direct store mutation needs an
    // explicit tick(), matching the pattern used elsewhere in this codebase
    // (see AppShell.validation.test.ts).
    await tick();

    expect(screen.queryByText("Run As / Sandbox")).not.toBeInTheDocument();
    expect(screen.queryByText("Limits")).not.toBeInTheDocument();

    store.updateWorkflow((wf) => {
      wf.runAs = { user: "subflow-user" };
      wf.limits = { maxTotalSteps: 999, maxVisitsPerNode: 999 };
    });

    expect(store.workflow!.runAs).toEqual({ user: "root-user" });
    expect(store.workflow!.limits).toEqual({ maxTotalSteps: 100, maxVisitsPerNode: 20 });
    expect(store.activeWorkflow!.runAs).toEqual({ user: "subflow-user" });
    expect(store.activeWorkflow!.limits).toEqual({ maxTotalSteps: 999, maxVisitsPerNode: 999 });
  });
});
