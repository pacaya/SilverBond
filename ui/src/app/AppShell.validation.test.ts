import { cleanup, render } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { store } from "@/lib/stores/workflowStore.svelte";
import ValidationEffectHarness from "@/test/validationEffectHarness.svelte";

describe("AppShell debounced validation", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    store.createWorkflow();
    store.clearLines();
    store.resetRun();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("re-triggers debounced validation after an in-place nested workflow edit", async () => {
    store.addNode("task", { x: 0, y: 0 });
    const onValidate = vi.fn();

    render(ValidationEffectHarness, {
      props: { onValidate, debounceMs: 500 },
    });

    await vi.advanceTimersByTimeAsync(500);
    expect(onValidate).toHaveBeenCalledTimes(1);
    expect(onValidate.mock.calls[0][0].nodes[0].prompt).toBe("");

    store.updateWorkflow((wf) => {
      wf.nodes[0].prompt = "updated prompt";
    });
    await tick();

    await vi.advanceTimersByTimeAsync(500);
    expect(onValidate).toHaveBeenCalledTimes(2);
    expect(onValidate.mock.calls[1][0].nodes[0].prompt).toBe("updated prompt");
  });
});
