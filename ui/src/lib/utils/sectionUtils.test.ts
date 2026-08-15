import { describe, expect, it } from "vitest";
import type { AgentNodeConfig, WorkflowNode } from "@/lib/types/workflow";
import { sectionHasValues } from "./sectionUtils";

function taskNode(agentConfig: AgentNodeConfig): WorkflowNode {
  return {
    id: "node-1",
    name: "Task",
    kind: { type: "task", agentConfig },
    prompt: "",
  };
}

describe("sectionHasValues", () => {
  it("reports agent tuning from webSearch presence, not truthiness", () => {
    expect(sectionHasValues(taskNode({}), "agent-tuning")).toBe(false);
    expect(sectionHasValues(taskNode({ toolToggles: {} }), "agent-tuning")).toBe(false);
    expect(sectionHasValues(taskNode({ toolToggles: { webSearch: false } }), "agent-tuning")).toBe(
      true,
    );
    expect(sectionHasValues(taskNode({ toolToggles: { webSearch: true } }), "agent-tuning")).toBe(
      true,
    );
  });
});
