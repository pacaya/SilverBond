import type {
  ExecutionLogDetail,
  InterruptedRun,
  LogListItem,
  NodeTestContext,
  NodeTestPreview,
  RunEvent,
  RunObservability,
  RuntimeCapabilities,
  TemplateItem,
  ValidationResponse,
  WorkflowDocument,
  WorkflowItem,
} from "@/lib/types/workflow";

export type RunActionResponse = {
  success: boolean;
  runId: string;
} & RunObservability;

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
    postJson<RunActionResponse>("/api/runs", {
      workflow, variableOverrides, startNodeId: workflow.entryNodeId || null,
    }),
  approveRun: (runId: string, approved: boolean, userInput: string) =>
    postJson<{ success: boolean }>(
      `/api/runs/${encodeURIComponent(runId)}/approve`, { approved, userInput },
    ),
  respondToInteraction: (runId: string, sessionId: string, response: string) =>
    postJson<{ success: boolean }>(
      `/api/runs/${encodeURIComponent(runId)}/respond-interaction`, { sessionId, response },
    ),
  abortRun: (runId: string) =>
    postJson<{ success: boolean }>(`/api/runs/${encodeURIComponent(runId)}/abort`),
  resumeRun: (runId: string) =>
    postJson<RunActionResponse>(
      `/api/runs/${encodeURIComponent(runId)}/resume`,
    ),
  restartFromNode: (runId: string, nodeId: string) =>
    postJson<RunActionResponse>(
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
const PANE_FAST_CLOSE_MS = 1000;
const PANE_FAST_CLOSE_LIMIT = 3;
// Sockets rejected before `onopen` (origin 403, refused, upgrade rejected) never
// count as "fast closes" — this separate cap bounds the "can't even connect"
// case so the reconnect loop terminates and surfaces the "unavailable" state.
const PANE_FAILED_CONNECT_LIMIT = 5;
const PANE_HEARTBEAT_TIMEOUT_MS = 15000;

export type PaneStreamStatus = "connecting" | "open" | "closed" | "unavailable";

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

function decodeBase64(value: string): Uint8Array | null {
  try {
    const binary = atob(value);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i += 1) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  } catch {
    return null;
  }
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
  let consecutiveFastCloses = 0;
  let consecutiveFailedConnects = 0;
  let lastSeq = -1;
  let resyncPending = false;
  let heartbeatWatchdog: ReturnType<typeof setInterval> | null = null;

  const clearHeartbeatWatchdog = () => {
    if (heartbeatWatchdog) {
      clearInterval(heartbeatWatchdog);
      heartbeatWatchdog = null;
    }
  };

  // Permanently stop reconnecting and surface the terminal "unavailable" state.
  const giveUp = () => {
    closed = true;
    handlers.onStatus?.("unavailable");
    handlers.onError?.("Pane unavailable");
  };

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
      consecutiveFailedConnects += 1;
      if (consecutiveFailedConnects >= PANE_FAILED_CONNECT_LIMIT) {
        giveUp();
        return;
      }
      scheduleReconnect();
      return;
    }
    socket = ws;
    // A new connection always begins with a server snapshot — reset gap tracking.
    lastSeq = -1;
    resyncPending = false;

    let openedAt: number | null = null;
    let firstFrameSeen = false;
    let lastFrameTime = 0;

    ws.onopen = () => {
      if (closed || ws !== socket) return;
      openedAt = Date.now();
      lastFrameTime = Date.now();
      clearHeartbeatWatchdog();
      heartbeatWatchdog = setInterval(() => {
        if (closed || ws !== socket) return;
        if (Date.now() - lastFrameTime > PANE_HEARTBEAT_TIMEOUT_MS) {
          handlers.onStatus?.("connecting");
          ws.close();
        }
      }, PANE_HEARTBEAT_TIMEOUT_MS / 3);
      handlers.onStatus?.("open");
    };

    ws.onmessage = (event) => {
      if (closed || ws !== socket) return;
      if (typeof event.data !== "string") return;
      let frame: { type?: string; seq?: number; data?: string; error?: string };
      try {
        frame = JSON.parse(event.data);
      } catch {
        return;
      }
      if ((frame.type === "snapshot" || frame.type === "data") && !firstFrameSeen) {
        firstFrameSeen = true;
        reconnectDelay = PANE_RECONNECT_BASE_MS;
        consecutiveFastCloses = 0;
        consecutiveFailedConnects = 0;
      }
      const seq = typeof frame.seq === "number" ? frame.seq : null;
      if (
        frame.type === "snapshot" ||
        frame.type === "data" ||
        frame.type === "heartbeat" ||
        frame.type === "error"
      ) {
        lastFrameTime = Date.now();
      }
      switch (frame.type) {
        case "snapshot":
          if (seq !== null) lastSeq = seq;
          if (typeof frame.data === "string") {
            const bytes = decodeBase64(frame.data);
            if (bytes === null) {
              handlers.onError?.("malformed base64 in snapshot frame");
              requestResync();
              break;
            }
            handlers.onSnapshot(bytes);
          }
          resyncPending = false;
          break;
        case "data":
          if (seq !== null) {
            // A gap means we missed frames — request a fresh snapshot to repaint.
            const gapDetected = lastSeq >= 0 && seq > lastSeq + 1;
            lastSeq = seq;
            if (gapDetected && !resyncPending) {
              resyncPending = true;
              requestResync();
            }
          }
          if (resyncPending) return;
          if (typeof frame.data === "string") {
            const bytes = decodeBase64(frame.data);
            if (bytes === null) {
              handlers.onError?.("malformed base64 in data frame");
              requestResync();
              break;
            }
            handlers.onData(bytes);
          }
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
      if (closed || ws !== socket) return;
      clearHeartbeatWatchdog();
      socket = null;
      const openedThisConnection = openedAt !== null;
      const fastClose =
        openedAt !== null &&
        !firstFrameSeen &&
        Date.now() - openedAt <= PANE_FAST_CLOSE_MS;
      // "Connects but unstable" (fast close) and "can't even connect" (closed
      // before onopen) are tracked independently so each terminates the loop.
      consecutiveFastCloses = fastClose ? consecutiveFastCloses + 1 : 0;
      consecutiveFailedConnects = openedThisConnection ? 0 : consecutiveFailedConnects + 1;
      if (
        consecutiveFastCloses >= PANE_FAST_CLOSE_LIMIT ||
        consecutiveFailedConnects >= PANE_FAILED_CONNECT_LIMIT
      ) {
        giveUp();
        return;
      }
      handlers.onStatus?.("closed");
      scheduleReconnect();
    };

    ws.onerror = () => {
      if (closed || ws !== socket) return;
      // `onclose` follows and drives reconnection; nothing extra to do here.
    };
  };

  connect();

  return {
    requestResync,
    close: () => {
      closed = true;
      clearHeartbeatWatchdog();
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
