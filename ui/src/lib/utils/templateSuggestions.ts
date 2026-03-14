import type { WorkflowDocument } from "@/lib/types/workflow";

export interface AutocompleteSuggestion {
  label: string;
  insertText: string;
  description: string;
  category: "output" | "context" | "variable" | "branch";
}

export function buildSuggestions(
  workflow: WorkflowDocument,
  currentNodeId: string,
): AutocompleteSuggestion[] {
  const suggestions: AutocompleteSuggestion[] = [];

  // Static context variables
  suggestions.push({
    label: "previous_output",
    insertText: "{{previous_output}}",
    description: "Output from the previous node in the execution chain",
    category: "output",
  });
  suggestions.push({
    label: "all_predecessors",
    insertText: "{{all_predecessors}}",
    description: "Combined outputs from all predecessor nodes",
    category: "output",
  });
  suggestions.push({
    label: "branch_origin",
    insertText: "{{branch_origin}}",
    description: "Output from the node that initiated the current branch",
    category: "branch",
  });
  suggestions.push({
    label: "branch_choice",
    insertText: "{{branch_choice}}",
    description: "The branch label or ID chosen at the branch point",
    category: "branch",
  });

  // Per-node output references
  for (const node of workflow.nodes) {
    if (node.id === currentNodeId) continue;
    const displayName = node.name || node.id.slice(0, 12);
    suggestions.push({
      label: `${displayName} → output`,
      insertText: `{{node:${node.id}.output}}`,
      description: `Output from "${displayName}"`,
      category: "output",
    });

    // If node has outputSchema, add parsed field references
    if (node.outputSchema) {
      // Extract field names from JSON Schema properties
      const propsObj = node.outputSchema.properties;
      if (!propsObj || typeof propsObj !== "object") continue;
      const props = Object.keys(propsObj as Record<string, unknown>);
      for (const field of props) {
        suggestions.push({
          label: `${displayName} → ${field}`,
          insertText: `{{node:${node.id}.parsedOutput.${field}}}`,
          description: `Parsed field "${field}" from "${displayName}"`,
          category: "output",
        });
      }
    }
  }

  // Context sources on the current node
  const currentNode = workflow.nodes.find((n) => n.id === currentNodeId);
  if (currentNode?.contextSources) {
    for (const ctx of currentNode.contextSources) {
      if (!ctx.name) continue;
      suggestions.push({
        label: `context: ${ctx.name}`,
        insertText: `{{context:${ctx.name}}}`,
        description: `Context source "${ctx.name}"`,
        category: "context",
      });
    }
  }

  // Workflow variables
  for (const variable of workflow.variables) {
    if (!variable.name) continue;
    suggestions.push({
      label: `var: ${variable.name}`,
      insertText: `{{var:${variable.name}}}`,
      description: variable.default
        ? `Variable "${variable.name}" (default: ${variable.default})`
        : `Variable "${variable.name}"`,
      category: "variable",
    });
  }

  return suggestions;
}
