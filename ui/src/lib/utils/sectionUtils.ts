import type { AgentCapabilities, AgentNodeConfig, WorkflowNode } from "@/lib/types/workflow";

export const SECTION_IDS = [
  "agent-tuning",
  "guards-retry",
  "loop-control",
  "output-schema",
  "tool-permissions",
  "skip-condition",
] as const;

export type SectionId = (typeof SECTION_IDS)[number];

export interface SectionDef {
  id: SectionId;
  label: string;
  tooltip: string;
}

export const SECTION_DEFS: SectionDef[] = [
  { id: "agent-tuning", label: "Agent tuning", tooltip: "Customize how the agent behaves for this node" },
  { id: "guards-retry", label: "Guards & retry", tooltip: "Add failure recovery and time limits" },
  { id: "loop-control", label: "Loop control", tooltip: "Make this node repeat until a condition is met" },
  { id: "output-schema", label: "Output schema", tooltip: "Define the structure of the response" },
  { id: "tool-permissions", label: "Tool permissions", tooltip: "Control which tools the agent can use" },
  { id: "skip-condition", label: "Skip condition", tooltip: "Skip this node based on previous output" },
];

export function isSectionAvailable(sectionId: string, agentCaps: AgentCapabilities | null): boolean {
  if (!agentCaps) return sectionId !== "agent-tuning" && sectionId !== "tool-permissions";
  if (sectionId === "tool-permissions") return !!agentCaps.toolAllowlist;
  return true;
}

export function sectionHasValues(node: WorkflowNode, sectionId: SectionId): boolean {
  const cfg =
    node.kind.type === "task" || node.kind.type === "run_agent"
      ? node.kind.agentConfig ?? {}
      : {};
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
