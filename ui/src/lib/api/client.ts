import type {
  ExecutionLogDetail,
  InterruptedRun,
  LogListItem,
  NodeTestContext,
  NodeTestPreview,
  RunEvent,
  RuntimeCapabilities,
  TemplateItem,
  ValidationResponse,
  WorkflowDocument,
  WorkflowItem,
} from "@/lib/types/workflow";

async function apiFetch<T>(input: string, init?: RequestInit): Promise<T> {
  const response = await fetch(input, init);
  if (!response.ok) {
    const text = await response.text().catch(() => response.statusText);
    throw new Error(text || `${response.status} ${response.statusText}`);
  }
  return (await response.json()) as T;
}

export const api = {
  capabilities: () => apiFetch<RuntimeCapabilities>("/api/capabilities"),
  workflows: () => apiFetch<WorkflowItem[]>("/api/workflows"),
  workflow: (name: string) =>
    apiFetch<WorkflowItem>(`/api/workflows/${encodeURIComponent(name)}`),
  saveWorkflow: (name: string, workflow: WorkflowDocument) =>
    apiFetch<{ success: boolean; name: string }>("/api/workflows", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, workflow }),
    }),
  deleteWorkflow: (name: string) =>
    apiFetch<{ success: boolean }>(`/api/workflows/${encodeURIComponent(name)}`, {
      method: "DELETE",
    }),
  templates: () => apiFetch<TemplateItem[]>("/api/templates"),
  validateWorkflow: (workflow: WorkflowDocument) =>
    apiFetch<ValidationResponse>("/api/validate-workflow", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ workflow }),
    }),
  testNode: (node: WorkflowDocument["nodes"][number], cwd: string, mockContext: NodeTestContext) =>
    apiFetch<NodeTestPreview>("/api/test-node", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ node, cwd, mockContext }),
    }),
  createRun: (workflow: WorkflowDocument, variableOverrides: Record<string, string>) =>
    apiFetch<{ success: boolean; runId: string }>("/api/runs", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ workflow, variableOverrides, startNodeId: workflow.entryNodeId || null }),
    }),
  approveRun: (runId: string, approved: boolean, userInput: string) =>
    apiFetch<{ success: boolean }>(`/api/runs/${encodeURIComponent(runId)}/approve`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ approved, userInput }),
    }),
  respondToInteraction: (runId: string, response: string) =>
    apiFetch<{ success: boolean }>(`/api/runs/${encodeURIComponent(runId)}/respond-interaction`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ response }),
    }),
  abortRun: (runId: string) =>
    apiFetch<{ success: boolean }>(`/api/runs/${encodeURIComponent(runId)}/abort`, {
      method: "POST",
    }),
  resumeRun: (runId: string) =>
    apiFetch<{ success: boolean; runId: string }>(
      `/api/runs/${encodeURIComponent(runId)}/resume`,
      { method: "POST" },
    ),
  restartFromNode: (runId: string, nodeId: string) =>
    apiFetch<{ success: boolean; runId: string }>(
      `/api/runs/${encodeURIComponent(runId)}/restart-from/${encodeURIComponent(nodeId)}`,
      { method: "POST" },
    ),
  dismissRun: (runId: string) =>
    apiFetch<{ success: boolean }>(`/api/runs/${encodeURIComponent(runId)}/dismiss`, {
      method: "POST",
    }),
  interruptedRuns: () => apiFetch<InterruptedRun[]>("/api/interrupted-runs"),
  logs: () => apiFetch<LogListItem[]>("/api/logs"),
  log: (id: string) => apiFetch<ExecutionLogDetail>(`/api/logs/${encodeURIComponent(id)}`),
  deleteLog: (id: string) =>
    apiFetch<{ success: boolean }>(`/api/logs/${encodeURIComponent(id)}`, {
      method: "DELETE",
    }),
  runEvents: (runId: string) =>
    apiFetch<RunEvent[]>(`/api/runs/${encodeURIComponent(runId)}/events`),
};

export async function streamRun(
  runId: string,
  onEvent: (event: RunEvent) => void,
): Promise<void> {
  const response = await fetch(`/api/runs/${encodeURIComponent(runId)}/stream`);
  if (!response.ok || !response.body) {
    throw new Error(`Unable to open stream for run ${runId}`);
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const chunks = buffer.split("\n");
    buffer = chunks.pop() ?? "";
    for (const chunk of chunks) {
      if (!chunk.startsWith("data: ")) continue;
      try {
        onEvent(JSON.parse(chunk.slice(6)) as RunEvent);
      } catch {
        // Ignore malformed chunks; the stream continues.
      }
    }
  }
}
