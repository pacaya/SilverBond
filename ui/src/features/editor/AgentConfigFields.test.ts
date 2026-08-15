import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AgentCapabilities, AgentDefaults, ToolToggles } from "@/lib/types/workflow";
import AgentConfigFields from "./AgentConfigFields.svelte";

const caps: AgentCapabilities = {
  workerExecution: true,
  promptRefinement: true,
  branchChoice: true,
  loopVerdict: true,
  structuredOutput: true,
  sessionReuse: true,
  nativeJsonSchema: true,
  modelSelection: true,
  reasoningConfig: true,
  systemPrompt: true,
  budgetLimit: true,
  turnLimit: true,
  costReporting: true,
  toolAllowlist: true,
  webSearch: true,
};

function setup(values: AgentDefaults = {}) {
  const update = vi.fn<(key: keyof AgentDefaults, value: AgentDefaults[keyof AgentDefaults]) => void>();
  render(AgentConfigFields, {
    props: {
      caps,
      accessProfiles: undefined,
      values,
      update,
      placeholders: { model: "agent default", systemPrompt: "none" },
    },
  });
  return { update };
}

function webSearchSelect(): HTMLSelectElement {
  return screen.getByLabelText("Web search") as HTMLSelectElement;
}

describe("AgentConfigFields web search", () => {
  afterEach(() => {
    cleanup();
  });

  it("renders three distinct control states for inherit, on, and off", () => {
    setup();
    const inheritValue = webSearchSelect().value;
    expect(inheritValue).toBe("");

    cleanup();
    setup({ toolToggles: { webSearch: true } });
    const onValue = webSearchSelect().value;
    expect(onValue).toBe("true");

    cleanup();
    setup({ toolToggles: { webSearch: false } });
    const offValue = webSearchSelect().value;
    expect(offValue).toBe("false");

    expect(inheritValue).not.toBe(offValue);
    expect(inheritValue).not.toBe(onValue);
    expect(onValue).not.toBe(offValue);
  });

  it("emits on, off, and inherit through the update callback", async () => {
    const { update } = setup();

    await fireEvent.change(webSearchSelect(), { target: { value: "false" } });
    expect(update).toHaveBeenLastCalledWith("toolToggles", { webSearch: false });

    await fireEvent.change(webSearchSelect(), { target: { value: "true" } });
    expect(update).toHaveBeenLastCalledWith("toolToggles", { webSearch: true });

    await fireEvent.change(webSearchSelect(), { target: { value: "" } });
    expect(update).toHaveBeenLastCalledWith("toolToggles", undefined);
  });

  it("collapses stored toolToggles to undefined when selecting inherit", async () => {
    const { update } = setup({ toolToggles: { webSearch: false } });

    await fireEvent.change(webSearchSelect(), { target: { value: "" } });
    expect(update).toHaveBeenLastCalledWith("toolToggles", undefined);
  });

  it("merges web search changes into the stored toolToggles object", async () => {
    const stored = { webSearch: true, siblingKey: true } as ToolToggles & { siblingKey: boolean };
    const { update } = setup({ toolToggles: stored });

    await fireEvent.change(webSearchSelect(), { target: { value: "false" } });
    expect(update).toHaveBeenLastCalledWith("toolToggles", { webSearch: false, siblingKey: true });

    await fireEvent.change(webSearchSelect(), { target: { value: "true" } });
    expect(update).toHaveBeenLastCalledWith("toolToggles", { webSearch: true, siblingKey: true });

    await fireEvent.change(webSearchSelect(), { target: { value: "" } });
    expect(update).toHaveBeenLastCalledWith("toolToggles", { siblingKey: true });
  });
});
