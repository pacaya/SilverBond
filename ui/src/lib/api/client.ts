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

function postJson<T>(url: string, body?: unknown): Promise<T> {
  return apiFetch<T>(url, {
    method: "POST",
    ...(body !== undefined && {
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    }),
  });
}

export const api = {
  capabilities: () => apiFetch<RuntimeCapabilities>("/api/capabilities"),
  workflows: () => apiFetch<WorkflowItem[]>("/api/workflows"),
  workflow: (name: string) =>
    apiFetch<WorkflowItem>(`/api/workflows/${encodeURIComponent(name)}`),
  saveWorkflow: (name: string, workflow: WorkflowDocument) =>
    postJson<{ success: boolean; name: string }>("/api/workflows", { name, workflow }),
  deleteWorkflow: (name: string) =>
    apiFetch<{ success: boolean }>(`/api/workflows/${encodeURIComponent(name)}`, {
      method: "DELETE",
    }),
  templates: () => apiFetch<TemplateItem[]>("/api/templates"),
  validateWorkflow: (workflow: WorkflowDocument) =>
    postJson<ValidationResponse>("/api/validate-workflow", { workflow }),
  testNode: (node: WorkflowDocument["nodes"][number], cwd: string, mockContext: NodeTestContext) =>
    postJson<NodeTestPreview>("/api/test-node", { node, cwd, mockContext }),
  createRun: (workflow: WorkflowDocument, variableOverrides: Record<string, string>) =>
    postJson<{ success: boolean; runId: string }>("/api/runs", {
      workflow, variableOverrides, startNodeId: workflow.entryNodeId || null,
    }),
  approveRun: (runId: string, approved: boolean, userInput: string) =>
    postJson<{ success: boolean }>(
      `/api/runs/${encodeURIComponent(runId)}/approve`, { approved, userInput },
    ),
  respondToInteraction: (runId: string, response: string) =>
    postJson<{ success: boolean }>(
      `/api/runs/${encodeURIComponent(runId)}/respond-interaction`, { response },
    ),
  abortRun: (runId: string) =>
    postJson<{ success: boolean }>(`/api/runs/${encodeURIComponent(runId)}/abort`),
  resumeRun: (runId: string) =>
    postJson<{ success: boolean; runId: string }>(
      `/api/runs/${encodeURIComponent(runId)}/resume`,
    ),
  restartFromNode: (runId: string, nodeId: string) =>
    postJson<{ success: boolean; runId: string }>(
      `/api/runs/${encodeURIComponent(runId)}/restart-from/${encodeURIComponent(nodeId)}`,
    ),
  dismissRun: (runId: string) =>
    postJson<{ success: boolean }>(`/api/runs/${encodeURIComponent(runId)}/dismiss`),
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
