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

    await streamRun("run/1", "stream-token-1", vi.fn());

    expect(fetchMock).toHaveBeenCalledWith("/api/runs/run%2F1/stream", {
      headers: { "X-Stream-Token": "stream-token-1" },
    });
  });
});
