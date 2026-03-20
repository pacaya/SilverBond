import type { AgentNodeConfig, WorkflowNode } from "@/lib/types/workflow";

export const SECTION_IDS = [
  "agent-tuning",
  "guards-retry",
  "loop-control",
  "output-schema",
  "tool-permissions",
  "skip-condition",
] as const;

export type SectionId = (typeof SECTION_IDS)[number];

export function sectionHasValues(node: WorkflowNode, sectionId: SectionId): boolean {
  const cfg = node.agentConfig ?? {};
  switch (sectionId) {
    case "agent-tuning":
      return !!(cfg.accessMode || cfg.model || cfg.reasoningLevel || cfg.systemPrompt ||
        cfg.maxTurns || cfg.maxBudgetUsd || cfg.toolToggles?.webSearch || node.cwd || node.continueSessionFrom);
    case "guards-retry":
      return !!(node.timeout || node.retryCount || node.retryDelay);
    case "loop-control":
      return !!(node.loopMaxIterations || node.loopCondition);
    case "output-schema":
      return !!(node.outputSchema);
    case "tool-permissions":
      return !!((cfg as AgentNodeConfig).allowedTools?.length || (cfg as AgentNodeConfig).disallowedTools?.length);
    case "skip-condition":
      return !!(node.skipCondition);
    default:
      return false;
  }
}
