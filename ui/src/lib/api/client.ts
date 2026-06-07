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

/* ── Live pane terminal stream (WebSocket) ────────────────────────────────
 *
 * Mirrors the backend protocol in `src/api.rs::pane_stream_socket`:
 *   server → client : JSON text frames
 *     { type: "snapshot",  seq, data }  full colored capture (base64) — clear + write
 *     { type: "data",      seq, data }  incremental pane bytes (base64) — write
 *     { type: "heartbeat", seq }        keepalive (seq repeats the last data seq)
 *     { type: "error",     seq, error } error notice, may be followed by close
 *   client → server : text "resync" (or {"type":"resync"}) requests a fresh snapshot.
 *
 * `seq` is monotonic across snapshot/data/error frames on a single connection, so a
 * jump signals dropped frames → we ask the server to resync (clear + repaint snapshot).
 * On socket close we auto-reconnect with backoff; the server always leads a fresh
 * connection with a snapshot, so the view repaints itself after a mid-run WS kill.
 */

const PANE_RECONNECT_BASE_MS = 500;
const PANE_RECONNECT_MAX_MS = 5000;

export type PaneStreamStatus = "connecting" | "open" | "closed";

export interface PaneStreamHandlers {
  /** Full repaint: callers should clear the terminal before writing these bytes. */
  onSnapshot: (bytes: Uint8Array) => void;
  /** Incremental bytes appended to the live view. */
  onData: (bytes: Uint8Array) => void;
  onError?: (message: string) => void;
  onStatus?: (status: PaneStreamStatus) => void;
}

export interface PaneStreamHandle {
  /** Ask the server to resend a snapshot (clear + repaint). */
  requestResync: () => void;
  /** Permanently close the stream and stop reconnecting. */
  close: () => void;
}

function decodeBase64(value: string): Uint8Array {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

function paneStreamUrl(runId: string, pane: string): string {
  const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
  return (
    `${scheme}//${window.location.host}` +
    `/api/runs/${encodeURIComponent(runId)}/panes/${encodeURIComponent(pane)}/stream`
  );
}

export function streamPane(
  runId: string,
  pane: string,
  handlers: PaneStreamHandlers,
): PaneStreamHandle {
  const url = paneStreamUrl(runId, pane);
  let socket: WebSocket | null = null;
  let closed = false;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let reconnectDelay = PANE_RECONNECT_BASE_MS;
  let lastSeq = -1;

  const requestResync = () => {
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send("resync");
    }
  };

  const scheduleReconnect = () => {
    if (closed || reconnectTimer) return;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      connect();
    }, reconnectDelay);
    reconnectDelay = Math.min(reconnectDelay * 2, PANE_RECONNECT_MAX_MS);
  };

  const connect = () => {
    if (closed) return;
    handlers.onStatus?.("connecting");

    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch (error) {
      handlers.onError?.(error instanceof Error ? error.message : "Unable to open pane stream");
      scheduleReconnect();
      return;
    }
    socket = ws;
    // A new connection always begins with a server snapshot — reset gap tracking.
    lastSeq = -1;

    ws.onopen = () => {
      reconnectDelay = PANE_RECONNECT_BASE_MS;
      handlers.onStatus?.("open");
    };

    ws.onmessage = (event) => {
      if (typeof event.data !== "string") return;
      let frame: { type?: string; seq?: number; data?: string; error?: string };
      try {
        frame = JSON.parse(event.data);
      } catch {
        return;
      }
      const seq = typeof frame.seq === "number" ? frame.seq : null;
      switch (frame.type) {
        case "snapshot":
          if (seq !== null) lastSeq = seq;
          if (frame.data) handlers.onSnapshot(decodeBase64(frame.data));
          break;
        case "data":
          if (seq !== null) {
            // A gap means we missed frames — request a fresh snapshot to repaint.
            if (lastSeq >= 0 && seq > lastSeq + 1) requestResync();
            lastSeq = seq;
          }
          if (frame.data) handlers.onData(decodeBase64(frame.data));
          break;
        case "error":
          if (seq !== null) lastSeq = seq;
          handlers.onError?.(frame.error ?? "pane stream error");
          break;
        case "heartbeat":
        default:
          break;
      }
    };

    ws.onclose = () => {
      socket = null;
      if (!closed) {
        handlers.onStatus?.("closed");
        scheduleReconnect();
      }
    };

    ws.onerror = () => {
      // `onclose` follows and drives reconnection; nothing extra to do here.
    };
  };

  connect();

  return {
    requestResync,
    close: () => {
      closed = true;
      if (reconnectTimer) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
      const open = socket;
      socket = null;
      if (open) {
        try {
          open.close();
        } catch {
          // ignore — already closing/closed
        }
      }
    },
  };
}
