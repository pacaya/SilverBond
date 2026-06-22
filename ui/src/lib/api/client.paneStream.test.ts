/**
 * T15 — PaneStream WebSocket client unit tests
 *
 * Tests the frame-parsing, gap-detection, resync-request, and reconnect logic
 * in `streamPane` (ui/src/lib/api/client.ts) without a real WebSocket server.
 *
 * We provide a deterministic MockWebSocket class that captures handlers
 * assigned by the code under test (onopen / onmessage / onclose / onerror).
 *
 * Coverage:
 *  1. snapshot frame decoded and forwarded to onSnapshot
 *  2. data frame decoded and forwarded to onData
 *  3. gap in seq triggers a "resync" message to the server
 *  4. contiguous seq does NOT trigger resync
 *  5. requestResync sends "resync" while the socket is OPEN
 *  6. requestResync is a no-op while the socket is not OPEN
 *  7. unexpected close schedules a reconnect
 *  8. close() stops reconnection permanently
 *  9. error frame is forwarded to onError
 * 10. heartbeat frame is silently ignored
 * 11. first seq after reconnect resets gap tracking (no spurious resync)
 * 12. accept-then-close reconnects use growing backoff
 * 13. repeated accept-then-close cycles give up as unavailable
 * 14. first snapshot/data frame resets reconnect backoff
 * 15. late frames from a closed socket are ignored
 * 16. gap frames and following data are suppressed until snapshot repaint
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { streamPane } from "@/lib/api/client";

// ---------------------------------------------------------------------------
// WebSocket state constants (jsdom may not define them on the class)
// ---------------------------------------------------------------------------
const WS_CONNECTING = 0;
const WS_OPEN = 1;
const WS_CLOSED = 3;

// ---------------------------------------------------------------------------
// MockWebSocket
// ---------------------------------------------------------------------------

/** One instance created per `new WebSocket(url)` call in client code. */
class MockWebSocket {
  static CONNECTING = WS_CONNECTING;
  static OPEN = WS_OPEN;
  static CLOSED = WS_CLOSED;

  readonly url: string;
  readonly protocols?: string | string[];
  readyState: number = WS_CONNECTING;
  readonly sent: string[] = [];

  // Handlers set by the code under test
  onopen: (() => void) | null = null;
  onmessage: ((evt: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(url: string, protocols?: string | string[]) {
    this.url = url;
    this.protocols = protocols;
    MockWebSocket.instances.push(this);
  }

  send(msg: string) {
    this.sent.push(msg);
  }

  close() {
    this.readyState = WS_CLOSED;
    this.onclose?.();
  }

  // --- Test-side helpers ---

  /** Simulate the TCP handshake completing. */
  simulateOpen() {
    this.readyState = WS_OPEN;
    this.onopen?.();
  }

  /** Simulate the server sending a JSON text frame. */
  simulateMessage(data: string) {
    this.onmessage?.({ data });
  }

  /** Simulate a remote-initiated close (e.g. network drop). */
  simulateClose() {
    this.readyState = WS_CLOSED;
    this.onclose?.();
  }

  // --- Class-level registry ---
  static instances: MockWebSocket[] = [];

  static reset() {
    MockWebSocket.instances = [];
  }

  static last(): MockWebSocket {
    const inst = MockWebSocket.instances.at(-1);
    if (!inst) throw new Error("No MockWebSocket has been created yet");
    return inst;
  }
}

// ---------------------------------------------------------------------------
// Frame helpers
// ---------------------------------------------------------------------------

function b64(text: string): string {
  return btoa(text);
}

function frame(
  type: string,
  seq: number,
  data?: string,
  error?: string,
): string {
  const obj: Record<string, unknown> = { type, seq };
  if (data !== undefined) obj.data = data;
  if (error !== undefined) obj.error = error;
  return JSON.stringify(obj);
}

// ---------------------------------------------------------------------------
// Setup / teardown
// ---------------------------------------------------------------------------

beforeEach(() => {
  MockWebSocket.reset();
  vi.useFakeTimers();

  // Replace the global WebSocket with our mock class.
  vi.stubGlobal("WebSocket", MockWebSocket);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("streamPane client", () => {
  it("passes the stream token as a WebSocket subprotocol", () => {
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
    });

    expect(MockWebSocket.last().protocols).toEqual(["stream-token-1"]);

    handle.close();
  });

  it("1. snapshot frame decoded and forwarded to onSnapshot", () => {
    const onSnapshot = vi.fn();
    const handle = streamPane("run-1", "pane-0", "stream-token-1", { onSnapshot, onData: vi.fn() });

    const ws = MockWebSocket.last();
    ws.simulateOpen();
    ws.simulateMessage(frame("snapshot", 0, b64("hello world")));

    expect(onSnapshot).toHaveBeenCalledOnce();
    const bytes = onSnapshot.mock.calls[0][0] as Uint8Array;
    expect(new TextDecoder().decode(bytes)).toBe("hello world");

    handle.close();
  });

  it("2. data frame decoded and forwarded to onData", () => {
    const onData = vi.fn();
    const handle = streamPane("run-1", "pane-0", "stream-token-1", { onSnapshot: vi.fn(), onData });

    const ws = MockWebSocket.last();
    ws.simulateOpen();
    ws.simulateMessage(frame("snapshot", 0, b64("snap")));
    ws.simulateMessage(frame("data", 1, b64("chunk")));

    expect(onData).toHaveBeenCalledOnce();
    const bytes = onData.mock.calls[0][0] as Uint8Array;
    expect(new TextDecoder().decode(bytes)).toBe("chunk");

    handle.close();
  });

  it("3. gap in seq triggers a 'resync' message to the server", () => {
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
    });

    const ws = MockWebSocket.last();
    ws.simulateOpen();
    ws.simulateMessage(frame("snapshot", 0, b64("snap"))); // lastSeq = 0
    ws.simulateMessage(frame("data", 2, b64("x")));        // skipped seq=1 → gap!

    expect(ws.sent).toContain("resync");

    handle.close();
  });

  it("3b. gap data is not forwarded again until a snapshot repaints", () => {
    const onSnapshot = vi.fn();
    const onData = vi.fn();
    const handle = streamPane("run-1", "pane-0", "stream-token-1", { onSnapshot, onData });

    const ws = MockWebSocket.last();
    ws.simulateOpen();
    ws.simulateMessage(frame("snapshot", 0, b64("snap")));
    ws.simulateMessage(frame("data", 2, b64("gap")));
    ws.simulateMessage(frame("data", 3, b64("pending")));

    expect(ws.sent).toContain("resync");
    expect(onData).not.toHaveBeenCalled();

    ws.simulateMessage(frame("snapshot", 4, b64("fresh")));
    ws.simulateMessage(frame("data", 5, b64("resume")));

    expect(onSnapshot).toHaveBeenCalledTimes(2);
    expect(onData).toHaveBeenCalledOnce();
    const bytes = onData.mock.calls[0][0] as Uint8Array;
    expect(new TextDecoder().decode(bytes)).toBe("resume");

    handle.close();
  });

  it("4. contiguous seq does NOT trigger resync", () => {
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
    });

    const ws = MockWebSocket.last();
    ws.simulateOpen();
    ws.simulateMessage(frame("snapshot", 0, b64("snap")));
    ws.simulateMessage(frame("data", 1, b64("a")));
    ws.simulateMessage(frame("data", 2, b64("b")));
    ws.simulateMessage(frame("data", 3, b64("c")));

    expect(ws.sent).not.toContain("resync");

    handle.close();
  });

  it("5. requestResync sends 'resync' while the socket is OPEN", () => {
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
    });

    const ws = MockWebSocket.last();
    ws.simulateOpen();
    handle.requestResync();

    expect(ws.sent).toContain("resync");

    handle.close();
  });

  it("6. requestResync is a no-op when the socket is not OPEN", () => {
    // Socket stays in CONNECTING state (simulateOpen not called).
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
    });

    const ws = MockWebSocket.last();
    handle.requestResync();

    expect(ws.sent).toHaveLength(0);

    handle.close();
  });

  it("7. unexpected close schedules a reconnect", () => {
    const onStatus = vi.fn();
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
      onStatus,
    });

    const firstWs = MockWebSocket.last();
    firstWs.simulateOpen();
    firstWs.simulateClose(); // remote drop

    expect(onStatus).toHaveBeenCalledWith("closed");

    // Advance past PANE_RECONNECT_BASE_MS (500 ms)
    vi.advanceTimersByTime(600);

    expect(onStatus).toHaveBeenCalledWith("connecting");
    expect(MockWebSocket.instances).toHaveLength(2); // new socket created

    handle.close();
  });

  it("7b. accept-then-close reconnects use growing backoff until a frame arrives", () => {
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
    });

    const firstWs = MockWebSocket.last();
    firstWs.simulateOpen();
    firstWs.simulateClose();

    vi.advanceTimersByTime(500);
    expect(MockWebSocket.instances).toHaveLength(2);

    const secondWs = MockWebSocket.last();
    secondWs.simulateOpen();
    secondWs.simulateClose();

    vi.advanceTimersByTime(999);
    expect(MockWebSocket.instances).toHaveLength(2);

    vi.advanceTimersByTime(1);
    expect(MockWebSocket.instances).toHaveLength(3);

    handle.close();
  });

  it("7c. repeated accept-then-close cycles stop reconnecting as unavailable", () => {
    const onStatus = vi.fn();
    const onError = vi.fn();
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
      onError,
      onStatus,
    });

    const firstWs = MockWebSocket.last();
    firstWs.simulateOpen();
    firstWs.simulateClose();
    vi.advanceTimersByTime(500);

    const secondWs = MockWebSocket.last();
    secondWs.simulateOpen();
    secondWs.simulateClose();
    vi.advanceTimersByTime(1000);

    const thirdWs = MockWebSocket.last();
    thirdWs.simulateOpen();
    thirdWs.simulateClose();

    expect(onStatus).toHaveBeenLastCalledWith("unavailable");
    expect(onError).toHaveBeenCalledWith("Pane unavailable");

    const countAfterGiveUp = MockWebSocket.instances.length;
    vi.advanceTimersByTime(10_000);
    expect(MockWebSocket.instances).toHaveLength(countAfterGiveUp);

    handle.close();
  });

  it("7d. a healthy frame resets reconnect backoff to the base delay", () => {
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
    });

    const firstWs = MockWebSocket.last();
    firstWs.simulateOpen();
    firstWs.simulateClose();

    vi.advanceTimersByTime(500);
    expect(MockWebSocket.instances).toHaveLength(2);

    const secondWs = MockWebSocket.last();
    secondWs.simulateOpen();
    secondWs.simulateMessage(frame("snapshot", 0, b64("healthy")));
    secondWs.simulateClose();

    vi.advanceTimersByTime(499);
    expect(MockWebSocket.instances).toHaveLength(2);

    vi.advanceTimersByTime(1);
    expect(MockWebSocket.instances).toHaveLength(3);

    handle.close();
  });

  it("8. close() stops reconnection permanently", () => {
    const onStatus = vi.fn();
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
      onStatus,
    });

    const firstWs = MockWebSocket.last();
    firstWs.simulateOpen();
    // Close the handle *before* the socket drops
    handle.close();
    firstWs.simulateClose();

    const countBefore = MockWebSocket.instances.length;

    vi.advanceTimersByTime(10_000);

    // No new socket should have been created
    expect(MockWebSocket.instances.length).toBe(countBefore);
  });

  it("8b. data frames queued on a closed socket are ignored", () => {
    const onData = vi.fn();
    const handle = streamPane("run-1", "pane-0", "stream-token-1", { onSnapshot: vi.fn(), onData });

    const ws = MockWebSocket.last();
    ws.simulateOpen();
    handle.close();
    ws.simulateMessage(frame("data", 0, b64("stale")));

    expect(onData).not.toHaveBeenCalled();
  });

  it("9. error frame forwarded to onError with the error message", () => {
    const onError = vi.fn();
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
      onError,
    });

    const ws = MockWebSocket.last();
    ws.simulateOpen();
    ws.simulateMessage(frame("error", 1, undefined, "pane not found"));

    expect(onError).toHaveBeenCalledWith("pane not found");

    handle.close();
  });

  it("10. heartbeat frame is silently ignored (no callbacks invoked)", () => {
    const onSnapshot = vi.fn();
    const onData = vi.fn();
    const onError = vi.fn();
    const handle = streamPane("run-1", "pane-0", "stream-token-1", { onSnapshot, onData, onError });

    const ws = MockWebSocket.last();
    ws.simulateOpen();
    ws.simulateMessage(JSON.stringify({ type: "heartbeat", seq: 0 }));

    expect(onSnapshot).not.toHaveBeenCalled();
    expect(onData).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();

    handle.close();
  });

  it("7e. closes rejected before onopen give up as unavailable (pre-open path)", () => {
    const onStatus = vi.fn();
    const onError = vi.fn();
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
      onError,
      onStatus,
    });

    // Reject every socket *before* it opens (openedAt stays null), e.g. an
    // origin 403 / refused upgrade. PANE_FAILED_CONNECT_LIMIT is 5.
    for (let i = 0; i < 4; i += 1) {
      MockWebSocket.last().simulateClose(); // never simulateOpen()
      vi.advanceTimersByTime(6000); // drain any backoff
      expect(onStatus).not.toHaveBeenLastCalledWith("unavailable");
    }

    // 5th failed connect trips the cap and surfaces the terminal state.
    MockWebSocket.last().simulateClose();

    expect(onStatus).toHaveBeenLastCalledWith("unavailable");
    expect(onError).toHaveBeenCalledWith("Pane unavailable");

    const countAfterGiveUp = MockWebSocket.instances.length;
    vi.advanceTimersByTime(10_000);
    expect(MockWebSocket.instances).toHaveLength(countAfterGiveUp);

    handle.close();
  });

  it("7f. a successful connection resets the failed-connect counter", () => {
    const onStatus = vi.fn();
    const onError = vi.fn();
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
      onError,
      onStatus,
    });

    // Four pre-open rejections — one short of the cap.
    for (let i = 0; i < 4; i += 1) {
      MockWebSocket.last().simulateClose();
      vi.advanceTimersByTime(6000);
    }

    // A healthy connection (open + first frame) resets the counter.
    const healthy = MockWebSocket.last();
    healthy.simulateOpen();
    healthy.simulateMessage(frame("snapshot", 0, b64("ok")));
    healthy.simulateClose();
    vi.advanceTimersByTime(6000);

    // Four more pre-open rejections must NOT trip the cap (counter was reset).
    for (let i = 0; i < 4; i += 1) {
      MockWebSocket.last().simulateClose();
      vi.advanceTimersByTime(6000);
    }

    expect(onStatus).not.toHaveBeenCalledWith("unavailable");
    expect(onError).not.toHaveBeenCalledWith("Pane unavailable");

    handle.close();
  });

  it("11. first seq after reconnect resets gap tracking (no spurious resync)", () => {
    const handle = streamPane("run-1", "pane-0", "stream-token-1", {
      onSnapshot: vi.fn(),
      onData: vi.fn(),
    });

    const firstWs = MockWebSocket.last();
    firstWs.simulateOpen();
    firstWs.simulateMessage(frame("snapshot", 0, b64("s")));
    firstWs.simulateMessage(frame("data", 1, b64("a")));
    firstWs.simulateMessage(frame("data", 2, b64("b")));
    // Drop the connection mid-stream
    firstWs.simulateClose();

    // Allow the reconnect timer to fire
    vi.advanceTimersByTime(600);

    const secondWs = MockWebSocket.last();
    expect(secondWs).not.toBe(firstWs);

    secondWs.simulateOpen();
    // Server leads fresh connection with snapshot at seq=0 (NOT continuing from seq=2)
    secondWs.simulateMessage(frame("snapshot", 0, b64("fresh")));
    // seq=1 follows seq=0 → no gap
    secondWs.simulateMessage(frame("data", 1, b64("d")));

    expect(secondWs.sent).not.toContain("resync");

    handle.close();
  });
});
