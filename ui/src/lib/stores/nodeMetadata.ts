import type { NodeKind, WorkflowNodeType } from "@/lib/types/workflow";

type NodeTypeMeta = {
  label: string;
  defaultKind: () => NodeKind;
};

/** Editor defaults for each workflow node type — mirrors backend node-kind serde shape. */
const NODE_TYPE_META: Record<WorkflowNodeType, NodeTypeMeta> = {
  task: { label: "Task", defaultKind: () => ({ type: "task" }) },
  approval: { label: "Approval", defaultKind: () => ({ type: "approval" }) },
  split: { label: "Split", defaultKind: () => ({ type: "split" }) },
  collector: { label: "Collector", defaultKind: () => ({ type: "collector" }) },
  decide: {
    label: "Decide",
    defaultKind: () => ({ type: "decide", decideConfig: { prompt: "", inputs: [], outcomes: [] } }),
  },
  parallel_batch: {
    label: "Batch",
    defaultKind: () => ({
      type: "parallel_batch",
      batchConfig: { itemsBinding: "", maxConcurrent: 4, itemVar: "item", bodyEntry: "" },
    }),
  },
  run_agent: {
    label: "Run Agent",
    // agent lives on node.agent (set in addNode); prompt stays unset so it
    // does not shadow node.prompt on the backend (Some("") would win).
    defaultKind: () => ({ type: "run_agent", runAgentConfig: { killAfter: true } }),
  },
  spawn: {
    label: "Spawn",
    defaultKind: () => ({ type: "spawn", spawnConfig: { agent: "claude" } }),
  },
  send: {
    label: "Send",
    defaultKind: () => ({ type: "send", sendConfig: { text: "", enter: true } }),
  },
  wait: {
    label: "Wait",
    defaultKind: () => ({ type: "wait", waitConfig: { mode: "idle" } }),
  },
  capture: {
    label: "Capture",
    defaultKind: () => ({ type: "capture", captureConfig: { all: false, ansi: false } }),
  },
  kill: { label: "Kill", defaultKind: () => ({ type: "kill", killConfig: {} }) },
  subflow: {
    label: "Subflow",
    defaultKind: () => ({ type: "subflow", subflowConfig: { workflowName: "", inputs: [], maxDepth: 10 } }),
  },
  call: {
    label: "Call",
    defaultKind: () => ({ type: "call", subflowConfig: { workflowName: "", inputs: [], maxDepth: 10 } }),
  },
};

export function defaultNodeName(type: WorkflowNodeType, count: number): string {
  return `${NODE_TYPE_META[type].label} ${count}`;
}

export function defaultNodeKind(type: WorkflowNodeType): NodeKind {
  return NODE_TYPE_META[type].defaultKind();
}
