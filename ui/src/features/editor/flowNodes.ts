import clsx from "clsx";
import type { Node } from "@xyflow/svelte";
import type {
  ValidationResponse,
  WorkflowDocument,
  WorkflowNodeType,
} from "@/lib/types/workflow";

export interface ValidationFlags {
  error: boolean;
  warning: boolean;
}

export type ValidationIndex = Record<string, ValidationFlags>;

export function buildValidationIndex(validation: ValidationResponse | null): ValidationIndex {
  const issues = validation?.issues ?? [];
  return issues.reduce<ValidationIndex>((acc, issue) => {
    if (!issue.nodeId) return acc;
    const current = acc[issue.nodeId] ?? { error: false, warning: false };
    if (issue.severity === "error") current.error = true;
    if (issue.severity === "warning") current.warning = true;
    acc[issue.nodeId] = current;
    return acc;
  }, {});
}

export function buildFlowNodes(
  workflow: WorkflowDocument,
  validation: ValidationResponse | null,
  validationIndex: ValidationIndex,
  nodeStates: Record<string, string | undefined>,
  selectedNodeId: string | null,
): Node[] {
  return workflow.nodes.map((node) => {
    const canvasPosition = workflow.ui?.canvas?.nodes?.[node.id];
    const position = canvasPosition
      ? { x: canvasPosition.x, y: canvasPosition.y }
      : { x: 180, y: 180 };
    const status = validationIndex[node.id];

    return {
      id: node.id,
      position,
      data: { label: node.name },
      draggable: true,
      selected: selectedNodeId === node.id,
      type: "default",
      initialWidth: 220,
      initialHeight: 72,
      class: clsx("graphNode", `graphNode--${node.type}`, {
        "graphNode--entry": workflow.entryNodeId === node.id,
        "graphNode--error": status?.error,
        "graphNode--warning": !status?.error && status?.warning,
        "graphNode--runtime-running": nodeStates[node.id] === "running",
        "graphNode--runtime-success": nodeStates[node.id] === "success",
        "graphNode--runtime-failed": nodeStates[node.id] === "failed",
        "graphNode--runtime-skipped": nodeStates[node.id] === "skipped",
        "graphNode--runtime-orchestrating": nodeStates[node.id] === "orchestrating",
        "graphNode--unreachable": validation?.graph.unreachableNodeIds.includes(node.id),
        "graphNode--deadend": validation?.graph.deadEndNodeIds.includes(node.id),
      }),
      style: nodeStyle(node.type),
    };
  });
}

function nodeStyle(type: WorkflowNodeType): string {
  const accents: Record<WorkflowNodeType, string> = {
    task: "rgba(96, 165, 250, 0.62)",
    approval: "rgba(245, 158, 11, 0.72)",
    split: "rgba(249, 115, 22, 0.78)",
    collector: "rgba(45, 212, 191, 0.72)",
  };

  return [
    "width: 220px",
    "border-radius: 16px",
    "border: 1px solid rgba(148, 163, 184, 0.28)",
    `border-left: 4px solid ${accents[type]}`,
    "padding: 14px",
  ].join("; ");
}
