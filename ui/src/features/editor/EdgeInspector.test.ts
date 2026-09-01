import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { store } from "@/lib/stores/workflowStore.svelte";
import type { RuntimeCapabilities } from "@/lib/types/workflow";
import {
  branchEdge,
  buildDecideBranchWorkflow,
} from "@/test/fixtures/decideBranchWorkflow";
import EdgeInspector from "./EdgeInspector.svelte";

const capabilities: RuntimeCapabilities = {
  workflowVersion: 4,
  supportedNodeTypes: ["task", "decide"],
  supportedEdgeOutcomes: ["success", "branch"],
  features: { split: false, collector: false },
  agents: {},
};

function renderBranchInspector(edgePatch: Parameters<typeof branchEdge>[0] = {}) {
  store.setWorkflow(buildDecideBranchWorkflow({ edge: edgePatch }));

  const storeEdge = store.workflow!.edges.find((item) => item.id === "branch_yes")!;

  render(EdgeInspector, {
    props: {
      edge: storeEdge,
      workflow: store.workflow!,
      capabilities,
    },
  });

  return {
    labelInput: screen.getByRole("textbox", { name: "Label" }) as HTMLInputElement,
    storedEdge: () => store.workflow!.edges.find((item) => item.id === "branch_yes")!,
  };
}

describe("EdgeInspector branch label", () => {
  afterEach(() => {
    cleanup();
    store.createWorkflow();
  });

  it("P1: writes normalized label to the store on each keystroke without blur", async () => {
    const { labelInput, storedEdge } = renderBranchInspector();

    await fireEvent.focus(labelInput);
    await fireEvent.input(labelInput, { target: { value: "yes " } });

    expect(storedEdge().label).toBe("yes");
  });

  it("P2: keeps the visible text and caret aligned with raw input while focused", async () => {
    const { labelInput } = renderBranchInspector();

    await fireEvent.focus(labelInput);
    labelInput.setSelectionRange(3, 3);
    await fireEvent.input(labelInput, { target: { value: "yes " } });

    expect(labelInput.value).toBe("yes ");
    expect(labelInput.selectionStart).toBe(4);
    expect(labelInput.selectionEnd).toBe(4);
  });

  it("P3: shows the stored normalized label after blur", async () => {
    const { labelInput } = renderBranchInspector();

    await fireEvent.focus(labelInput);
    await fireEvent.input(labelInput, { target: { value: "yes " } });
    await fireEvent.blur(labelInput);

    expect(labelInput.value).toBe("yes");
  });

  it("P4: stores padded non-branch labels unchanged", async () => {
    const successEdge = branchEdge({
      id: "success_edge",
      outcome: "success",
      label: null,
    });
    store.setWorkflow(buildDecideBranchWorkflow({ edges: [successEdge] }));

    const storeEdge = store.workflow!.edges.find((item) => item.id === "success_edge")!;

    render(EdgeInspector, {
      props: {
        edge: storeEdge,
        workflow: store.workflow!,
        capabilities,
      },
    });

    const labelInput = screen.getByRole("textbox", { name: "Label" });
    await fireEvent.focus(labelInput);
    await fireEvent.input(labelInput, { target: { value: " merge key " } });
    await fireEvent.blur(labelInput);

    const stored = store.workflow!.edges.find((item) => item.id === "success_edge")!;
    expect(stored.label).toBe(" merge key ");
  });

  it("P5: stores an absent label for whitespace-only branch input", async () => {
    const { labelInput, storedEdge } = renderBranchInspector({ label: "yes" });

    await fireEvent.focus(labelInput);
    await fireEvent.input(labelInput, { target: { value: "   " } });

    expect(storedEdge().label).toBeNull();
    expect(storedEdge().label).not.toBe("");
  });

  it("trims Unicode White_Space characters that ECMAScript trim ignores", async () => {
    const { labelInput, storedEdge } = renderBranchInspector();

    await fireEvent.focus(labelInput);
    await fireEvent.input(labelInput, { target: { value: "yes\u0085" } });
    await fireEvent.blur(labelInput);

    expect(storedEdge().label).toBe("yes");
  });

  it("reconciles draft with store on undo, redo, and edge selection while focused", async () => {
    store.setWorkflow(
      buildDecideBranchWorkflow({
        edges: [
          branchEdge({ id: "branch_yes", label: null }),
          branchEdge({ id: "branch_no", label: "no" }),
        ],
      }),
    );

    const yesEdge = store.workflow!.edges.find((item) => item.id === "branch_yes")!;
    const noEdge = store.workflow!.edges.find((item) => item.id === "branch_no")!;

    const { rerender } = render(EdgeInspector, {
      props: {
        edge: yesEdge,
        workflow: store.workflow!,
        capabilities,
      },
    });

    const labelInput = screen.getByRole("textbox", { name: "Label" }) as HTMLInputElement;

    await fireEvent.focus(labelInput);
    await fireEvent.input(labelInput, { target: { value: "yes " } });

    expect(store.workflow!.edges.find((item) => item.id === "branch_yes")!.label).toBe("yes");
    expect(labelInput.value).toBe("yes ");

    store.undo();
    await tick();

    expect(labelInput.value).toBe("");
    expect(store.workflow!.edges.find((item) => item.id === "branch_yes")!.label).toBeNull();

    store.redo();
    await tick();

    expect(labelInput.value).toBe("yes");
    expect(store.workflow!.edges.find((item) => item.id === "branch_yes")!.label).toBe("yes");

    await fireEvent.focus(labelInput);
    await fireEvent.input(labelInput, { target: { value: "yes " } });
    expect(labelInput.value).toBe("yes ");

    await rerender({
      edge: noEdge,
      workflow: store.workflow!,
      capabilities,
    });
    await tick();

    expect((screen.getByRole("textbox", { name: "Label" }) as HTMLInputElement).value).toBe("no");
  });

  it("renormalizes an existing padded label when outcome changes to branch", async () => {
    const successEdge = branchEdge({
      id: "success_edge",
      outcome: "success",
      label: " yes ",
    });
    store.setWorkflow(buildDecideBranchWorkflow({ edges: [successEdge] }));

    const storeEdge = store.workflow!.edges.find((item) => item.id === "success_edge")!;

    render(EdgeInspector, {
      props: {
        edge: storeEdge,
        workflow: store.workflow!,
        capabilities,
      },
    });

    const outcomeSelect = screen.getByRole("combobox", { name: "Outcome" });
    await fireEvent.change(outcomeSelect, { target: { value: "branch" } });

    const stored = store.workflow!.edges.find((item) => item.id === "success_edge")!;
    expect(stored.outcome).toBe("branch");
    expect(stored.label).toBe("yes");
  });
});
