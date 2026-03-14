export interface ReferenceEntry {
  syntax: string;
  description: string;
  example?: string;
}

export interface ReferenceGroup {
  title: string;
  entries: ReferenceEntry[];
}

export const templateVariables: ReferenceGroup = {
  title: "Template Variables",
  entries: [
    {
      syntax: "{{previous_output}}",
      description: "The output from the immediately preceding node in the execution chain.",
      example: "Summarize this: {{previous_output}}",
    },
    {
      syntax: "{{all_predecessors}}",
      description: "Combined outputs from all predecessor nodes that have executed before this one.",
    },
    {
      syntax: "{{node:<id>.output}}",
      description: "Direct reference to a specific node's output by its ID.",
      example: "{{node:node_a5854541.output}}",
    },
    {
      syntax: "{{node:<id>.parsedOutput.<field>}}",
      description: "Access a specific parsed JSON field from a node with an output schema.",
      example: "{{node:node_a5854541.parsedOutput.summary}}",
    },
    {
      syntax: "{{context:<alias>}}",
      description: "Reference a named context source configured on the current node.",
      example: "{{context:user_input}}",
    },
    {
      syntax: "{{var:<name>}}",
      description: "Reference a workflow-level variable by name.",
      example: "{{var:MAX_RETRIES}}",
    },
    {
      syntax: "{{branch_origin}}",
      description: "The output from the node that initiated the current branch.",
    },
    {
      syntax: "{{branch_choice}}",
      description: "The branch label or ID chosen at the branch decision point.",
    },
  ],
};

export const nodeTypes: ReferenceGroup = {
  title: "Node Types",
  entries: [
    {
      syntax: "task",
      description: "Executes a prompt with an AI agent and produces output. Supports retry, timeout, loop, skip conditions, and structured output schemas.",
    },
    {
      syntax: "approval",
      description: "Pauses execution and asks the user for approval before continuing. Displays a prompt and accepts approve/reject with optional feedback.",
    },
    {
      syntax: "split",
      description: "Fans out execution to multiple downstream branches in parallel. Configurable failure policy: best_effort_continue, fail_fast_cancel, or drain_then_fail.",
    },
    {
      syntax: "collector",
      description: "Waits for all inbound success paths in the current execution epoch, merges their inputs, and continues through a single success edge.",
    },
  ],
};

export const edgeOutcomes: ReferenceGroup = {
  title: "Edge Outcomes",
  entries: [
    {
      syntax: "success",
      description: "The default path taken when a node completes successfully.",
    },
    {
      syntax: "reject",
      description: "Taken when an approval node is rejected by the user.",
    },
    {
      syntax: "branch",
      description: "A conditional branch chosen by the AI agent based on output analysis. Requires a branchId.",
    },
    {
      syntax: "loop_continue",
      description: "Loops back to re-execute the node when the loop condition is met.",
    },
    {
      syntax: "loop_exit",
      description: "Exits the loop when the loop condition is no longer met.",
    },
  ],
};

export const workflowConcepts: ReferenceGroup = {
  title: "Workflow Concepts",
  entries: [
    {
      syntax: "Entry node",
      description: "The first node to execute when the workflow starts. Exactly one node must be marked as the entry node.",
    },
    {
      syntax: "Orchestrator",
      description: "When enabled, an AI orchestrator refines each node's prompt before execution, adding context and improving clarity.",
    },
    {
      syntax: "Limits",
      description: "Safety bounds: maxTotalSteps caps the total number of node executions, maxVisitsPerNode prevents infinite loops on a single node.",
    },
    {
      syntax: "Context sources",
      description: "Named references to other nodes' outputs, allowing a node to access specific upstream results by alias in its prompt.",
    },
    {
      syntax: "Loops",
      description: "A node can loop by having a loop_continue edge back to itself. Loop condition (JSON) determines when to exit. loopMaxIterations sets a hard limit.",
    },
    {
      syntax: "Skip conditions",
      description: "A JSON condition that, when met, causes the node to be skipped entirely during execution.",
    },
    {
      syntax: "Variables",
      description: "Workflow-level key-value pairs with defaults. Referenced in prompts as {{var:<name>}}. Values can be overridden at run time.",
    },
  ],
};

export const allReferenceGroups: ReferenceGroup[] = [
  templateVariables,
  nodeTypes,
  edgeOutcomes,
  workflowConcepts,
];
