import { afterEach, describe, expect, it, vi } from "vitest";
import { api, streamRun } from "@/lib/api/client";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("run stream token transport", () => {
  it("sends the run stream token when fetching recorded events", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => [],
    });
    vi.stubGlobal("fetch", fetchMock);

    await api.runEvents("run/1", "stream-token-1");

    expect(fetchMock).toHaveBeenCalledWith("/api/runs/run%2F1/events", {
      headers: { "X-Stream-Token": "stream-token-1" },
    });
  });

  it("sends the run stream token in a header when opening the event stream", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      body: new ReadableStream({
        start(controller) {
          controller.close();
        },
      }),
    });
    vi.stubGlobal("fetch", fetchMock);

    const stream = streamRun("run/1", "stream-token-1", vi.fn());
    await stream.finished;

    expect(fetchMock).toHaveBeenCalledWith("/api/runs/run%2F1/stream", {
      headers: { "X-Stream-Token": "stream-token-1" },
      signal: expect.any(AbortSignal),
    });
  });

  it("stops reading and cancels the reader when close() is called", async () => {
    const cancel = vi.fn().mockResolvedValue(undefined);
    let readCount = 0;
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      body: {
        getReader: () => ({
          cancel,
          read: async () => {
            readCount += 1;
            if (readCount === 1) {
              return { done: false, value: new TextEncoder().encode("data: {}\n") };
            }
            return new Promise(() => {
              // hang until aborted
            });
          },
        }),
      },
    });
    vi.stubGlobal("fetch", fetchMock);

    const onEvent = vi.fn();
    const stream = streamRun("run/1", "stream-token-1", onEvent);
    await Promise.resolve();
    stream.close();
    await stream.finished;

    expect(cancel).toHaveBeenCalled();
    expect(onEvent).toHaveBeenCalledTimes(1);
  });

  it("honors an external AbortSignal and stops delivering events", async () => {
    const controller = new AbortController();
    let readCount = 0;
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      body: {
        getReader: () => ({
          cancel: vi.fn().mockResolvedValue(undefined),
          read: async () => {
            readCount += 1;
            if (readCount === 1) {
              return { done: false, value: new TextEncoder().encode('data: {"type":"node_start"}\n') };
            }
            return new Promise(() => {});
          },
        }),
      },
    });
    vi.stubGlobal("fetch", fetchMock);

    const onEvent = vi.fn();
    const stream = streamRun("run/1", "stream-token-1", onEvent, { signal: controller.signal });
    await Promise.resolve();
    controller.abort();
    await stream.finished;

    expect(fetchMock).toHaveBeenCalledWith("/api/runs/run%2F1/stream", {
      headers: { "X-Stream-Token": "stream-token-1" },
      signal: controller.signal,
    });
    expect(onEvent).toHaveBeenCalledTimes(1);
  });
});
