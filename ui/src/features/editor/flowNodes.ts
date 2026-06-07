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
    const isCompound = node.type === "subflow" || node.type === "call";

    const data: Record<string, unknown> = isCompound
      ? (() => {
          const subflowName = node.subflowConfig?.workflowName ?? "";
          const inputs = (node.subflowConfig?.inputs ?? []).map((b) => b.name);
          const missing = !subflowName || !workflow.subflows?.[subflowName];
          return {
            label: node.name,
            nodeType: node.type,
            subflowName,
            inputs,
            output: subflowName ? `${subflowName}.result` : "result",
            missing,
          };
        })()
      : { label: node.name };

    return {
      id: node.id,
      position,
      data,
      draggable: true,
      selected: selectedNodeId === node.id,
      type: isCompound ? "subflow" : "default",
      initialWidth: 220,
      initialHeight: isCompound ? 120 : 72,
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
    decide: "rgba(168, 85, 247, 0.78)",
    parallel_batch: "rgba(234, 179, 8, 0.78)",
    subflow: "rgba(56, 189, 248, 0.82)",
    call: "rgba(14, 165, 233, 0.78)",
    spawn: "rgba(34, 197, 94, 0.72)",
    send: "rgba(132, 204, 22, 0.72)",
    wait: "rgba(148, 163, 184, 0.72)",
    capture: "rgba(20, 184, 166, 0.72)",
    kill: "rgba(239, 68, 68, 0.78)",
    run_agent: "rgba(129, 140, 248, 0.78)",
  };

  return [
    "width: 220px",
    "border-radius: 16px",
    "border: 1px solid rgba(148, 163, 184, 0.28)",
    `border-left: 4px solid ${accents[type]}`,
    "padding: 14px",
  ].join("; ");
}
