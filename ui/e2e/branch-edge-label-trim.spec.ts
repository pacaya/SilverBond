import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { buildDecideBranchWorkflow } from "../src/test/fixtures/decideBranchWorkflow";

test("editor-trimmed branch edge label passes backend validation", async ({ page, request }) => {
  const workflow = buildDecideBranchWorkflow({
    name: "Branch label trim e2e",
    canvas: {
      decide: { x: 80, y: 200 },
      target_0: { x: 480, y: 200 },
    },
  });

  const workflowPath = join(tmpdir(), `branch-label-trim-${Date.now()}.json`);
  writeFileSync(workflowPath, JSON.stringify(workflow));

  await page.goto("/");
  await page.locator('input[type="file"][accept*="json"]').setInputFiles(workflowPath);

  await page.evaluate(() => {
    const edge = document.querySelector('[data-id="branch_yes"]');
    if (!edge) throw new Error("branch edge not found");
    edge.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
  });

  const labelInput = page.getByRole("textbox", { name: "Label" });
  await expect(labelInput).toBeVisible();

  const validateResponse = page.waitForResponse((response) => {
    if (!response.url().includes("/api/validate-workflow")) return false;
    if (response.request().method() !== "POST") return false;
    const payload = response.request().postDataJSON() as {
      workflow?: { edges?: Array<{ id?: string; label?: string | null }> };
    };
    const branchEdge = payload.workflow?.edges?.find((item) => item.id === "branch_yes");
    return branchEdge !== undefined && branchEdge.label !== null;
  });

  await labelInput.fill("yes ");
  await labelInput.blur();

  const response = await validateResponse;
  const { workflow: editedWorkflow } = response.request().postDataJSON() as {
    workflow: { edges: Array<{ id?: string; label?: string | null }> };
  };
  const branchEdge = editedWorkflow.edges.find((item) => item.id === "branch_yes");
  expect(branchEdge?.label).toBe("yes");

  const validation = await request.post("/api/validate-workflow", {
    headers: { Origin: "http://127.0.0.1:3333" },
    data: { workflow: editedWorkflow },
  });
  expect(validation.ok()).toBeTruthy();

  const body = (await validation.json()) as {
    issues?: Array<{ severity: string }>;
  };
  const errorIssues = (body.issues ?? []).filter((issue) => issue.severity === "error");
  expect(errorIssues).toEqual([]);
});
