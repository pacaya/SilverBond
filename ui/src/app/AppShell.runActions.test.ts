import { cleanup, render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { tick } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "@/lib/api/client";
import RunPanel from "@/features/runtime/RunPanel.svelte";
import { store } from "@/lib/stores/workflowStore.svelte";

const approveRunMock = vi.hoisted(() => vi.fn());
const respondToInteractionMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/client")>();
  return {
    ...actual,
    api: {
      ...actual.api,
      approveRun: approveRunMock,
      respondToInteraction: respondToInteractionMock,
    },
  };
});

/** Mirrors AppShell.svelte guard + RunPanel callback wiring for regression coverage. */
function guard(label: string, p: Promise<unknown>) {
  void p.catch((e) =>
    store.setError(`${label} failed: ${e instanceof Error ? e.message : "Unknown error"}`),
  );
}

describe("AppShell run action errors", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("scrollIntoView", vi.fn());
    Element.prototype.scrollIntoView = vi.fn();
    store.createWorkflow();
    store.clearLines();
    store.resetRun();
    store.setRunState({
      runId: "run-1",
      streamToken: "token-1",
      running: false,
      approval: null,
    });
    approveRunMock.mockReset();
    respondToInteractionMock.mockReset();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  function renderRunPanel() {
    return render(RunPanel, {
      props: {
        workflow: store.workflow,
        isRunning: false,
        onRun: () => {},
        onAbort: () => {},
        onApproval: (approved, userInput) => {
          if (store.runId) guard("Approve", approveRunMock(store.runId, approved, userInput));
        },
        onInteractionResponse: (sessionId, response) => {
          if (store.runId && sessionId) {
            guard("Respond", respondToInteractionMock(store.runId, sessionId, response));
          }
        },
      },
    });
  }

  it("surfaces approveRun failures in the header error pill", async () => {
    approveRunMock.mockRejectedValue(new ApiError("No pending approval", 404, { error: "No pending approval" }));
    store.setRunState({
      approval: {
        nodeId: "node-1",
        nodeName: "Gate",
        prompt: "Continue?",
        lastOutput: "",
      },
    });

    renderRunPanel();
    await tick();

    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    await user.click(screen.getByTestId("approve-run"));
    await tick();

    expect(store.errorMessage).toBe("Approve failed: No pending approval");
  });

  it("surfaces respondToInteraction failures in the header error pill", async () => {
    respondToInteractionMock.mockRejectedValue(
      new ApiError("Stale interaction session", 409, { error: "Stale interaction session" }),
    );
    store.interactions = [
      {
        sessionId: "sess-1",
        description: "Allow tool?",
        outputSoFar: "",
        interactionType: "permission",
      },
    ];

    renderRunPanel();
    await tick();

    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    await user.click(screen.getByRole("button", { name: "Approve" }));
    await tick();

    expect(store.errorMessage).toBe("Respond failed: Stale interaction session");
  });
});
