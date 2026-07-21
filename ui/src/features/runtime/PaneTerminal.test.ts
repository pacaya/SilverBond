import { cleanup, render, screen, within } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { store } from "@/lib/stores/workflowStore.svelte";
import type { WorkflowDocument } from "@/lib/types/workflow";
import PaneTerminal from "./PaneTerminal.svelte";

const streamPaneMock = vi.hoisted(() =>
  vi.fn(() => ({
    requestResync: vi.fn(),
    reconnect: vi.fn(),
    close: vi.fn(),
  })),
);

const terminalResetMock = vi.hoisted(() => vi.fn());

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    loadAddon = vi.fn();
    open = vi.fn();
    reset = terminalResetMock;
    write = vi.fn();
    dispose = vi.fn();
  },
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = vi.fn();
  },
}));

vi.mock("@/lib/api/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/client")>();
  return {
    ...actual,
    streamPane: streamPaneMock,
  };
});

const dualReviewWorkflow: WorkflowDocument = {
  version: 4,
  name: "Dual Review",
  goal: "Produce a merged adversarial code review from dual independent reviewers",
  cwd: "",
  useOrchestrator: false,
  entryNodeId: "split-reviewers",
  variables: [
    { name: "review_scope", default: "Review the code changes in the working directory" },
  ],
  limits: {
    maxTotalSteps: 50,
    maxVisitsPerNode: 10,
  },
  nodes: [
    {
      id: "split-reviewers",
      name: "Split Reviewers",
      kind: { type: "split" },
      prompt: "",
    },
    {
      id: "reviewer-a",
      name: "Reviewer A",
      kind: {
        type: "run_agent",
        runAgentConfig: {
          agent: "claude",
          prompt: "Perform an adversarial code review focused on correctness and security.",
          killAfter: true,
        },
      },
      prompt: "",
    },
    {
      id: "reviewer-b",
      name: "Reviewer B",
      kind: {
        type: "run_agent",
        runAgentConfig: {
          agent: "claude",
          prompt: "Perform an adversarial code review focused on code quality and maintainability.",
          killAfter: true,
        },
      },
      prompt: "",
    },
    {
      id: "collect-reviews",
      name: "Collect Reviews",
      kind: { type: "collector" },
      prompt: "",
    },
  ],
  edges: [],
};

class MockResizeObserver implements ResizeObserver {
  readonly [Symbol.toStringTag] = "ResizeObserver";

  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
}

describe("PaneTerminal", () => {
  beforeEach(() => {
    store.resetRun();
    store.selectPane("active");
    store.setPaneStatus("idle");
    store.setPaneError("");
    streamPaneMock.mockClear();
    terminalResetMock.mockClear();
    vi.stubGlobal("ResizeObserver", MockResizeObserver);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("offers both dual-review reviewer run_agent panes as selectable options", () => {
    render(PaneTerminal, {
      props: {
        runId: "run-1",
        streamToken: "stream-token-1",
        workflow: dualReviewWorkflow,
      },
    });

    const select = screen.getByRole("combobox", { name: "Pane to stream" });
    const options = within(select);
    const reviewerA = options.getByRole("option", { name: "Reviewer A" }) as HTMLOptionElement;
    const reviewerB = options.getByRole("option", { name: "Reviewer B" }) as HTMLOptionElement;

    expect(select).toBeEnabled();
    expect(reviewerA.value).toBe("reviewer-a");
    expect(reviewerB.value).toBe("reviewer-b");
    expect(options.queryByRole("option", { name: "Split Reviewers" })).not.toBeInTheDocument();
    expect(streamPaneMock).toHaveBeenCalledWith(
      "run-1",
      "active",
      "stream-token-1",
      expect.any(Object),
    );
  });

  it("resync while connecting reconnects without clearing the terminal", async () => {
    store.setRunState({ runId: "run-1", streamToken: "stream-token-1", running: false });
    store.setPaneStatus("connecting");

    render(PaneTerminal, {
      props: {
        runId: "run-1",
        streamToken: "stream-token-1",
        workflow: dualReviewWorkflow,
      },
    });

    const handle = streamPaneMock.mock.results.at(-1)?.value as {
      requestResync: ReturnType<typeof vi.fn>;
      reconnect: ReturnType<typeof vi.fn>;
    };

    const resetsBefore = terminalResetMock.mock.calls.length;
    await screen.getByRole("button", { name: "Resync" }).click();

    expect(terminalResetMock.mock.calls.length).toBe(resetsBefore);
    expect(handle.requestResync).not.toHaveBeenCalled();
  });
});
