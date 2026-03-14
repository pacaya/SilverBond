import { expect, test } from "@playwright/test";

test("shows completed history for a real approval run", async ({ page }) => {
  const workflowName = `playwright-approval-${Date.now()}`;
  const workflow = {
    version: 3,
    name: workflowName,
    goal: "Verify approval flow through the real backend",
    cwd: "",
    useOrchestrator: false,
    entryNodeId: "approval_1",
    variables: [],
    limits: { maxTotalSteps: 10, maxVisitsPerNode: 5 },
    nodes: [
      {
        id: "approval_1",
        name: "Approval 1",
        type: "approval",
        prompt: "Ship this workflow?",
      },
    ],
    edges: [],
  };

  await page.goto("/");
  await page.evaluate(async ({ workflowName, workflow }) => {
    const createResponse = await fetch("/api/runs", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        workflow,
        variableOverrides: {},
        startNodeId: workflow.entryNodeId,
      }),
    });
    if (!createResponse.ok) {
      throw new Error(`run create failed: ${await createResponse.text()}`);
    }
    const { runId } = await createResponse.json() as { runId: string };
    const deadline = Date.now() + 15_000;
    while (Date.now() < deadline) {
      const eventsResponse = await fetch(`/api/runs/${encodeURIComponent(runId)}/events`);
      const events = await eventsResponse.json() as Array<{ type?: string }>;
      if (events.some((event) => event.type === "approval_required")) {
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }

    const approveResponse = await fetch(`/api/runs/${encodeURIComponent(runId)}/approve`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ approved: true, userInput: "approved from browser e2e" }),
    });
    if (!approveResponse.ok) {
      throw new Error(`run approve failed: ${await approveResponse.text()}`);
    }

    while (Date.now() < deadline) {
      const logsResponse = await fetch("/api/logs");
      const logs = await logsResponse.json() as Array<{ workflowName?: string }>;
      if (logs.some((log) => log.workflowName === workflowName)) {
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw new Error(`timed out waiting for completed log for ${workflowName}`);
  }, { workflowName, workflow });

  await expect(page.locator(".sidebar__header").first()).toHaveText("Workflows");
  await expect(page.locator(".workspace__header")).toContainText("Graph editor");
  await page.getByTestId("history-tab").click();

  const completedRun = page.getByTestId("completed-run-card").filter({ hasText: workflowName });
  await expect(completedRun).toBeVisible({ timeout: 15_000 });
  await completedRun.click();
  await expect(page.getByText("Node executions")).toBeVisible();
});
