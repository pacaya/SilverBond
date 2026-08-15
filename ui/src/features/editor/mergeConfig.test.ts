import { afterEach, describe, expect, it } from "vitest";
import { store } from "@/lib/stores/workflowStore.svelte";
import type { NodeKind, WorkflowDocument, WorkflowNode } from "@/lib/types/workflow";
import {
  BATCH_CONFIG_TARGET,
  CAPTURE_CONFIG_TARGET,
  DECIDE_CONFIG_TARGET,
  KILL_CONFIG_TARGET,
  makeConfigWriter,
  SEND_CONFIG_TARGET,
  SPAWN_CONFIG_TARGET,
  SUBFLOW_CONFIG_TARGET,
  WAIT_CONFIG_TARGET,
} from "./mergeConfig";

const CONFIG_KEYS = [
  "decideConfig",
  "batchConfig",
  "spawnConfig",
  "sendConfig",
  "waitConfig",
  "captureConfig",
  "killConfig",
  "subflowConfig",
] as const;

type ConfigKey = (typeof CONFIG_KEYS)[number];

function node(kind: NodeKind): WorkflowNode {
  return {
    id: "node-under-test",
    name: "Node under test",
    kind,
    prompt: "",
  };
}

function workflowWith(workflowNode: WorkflowNode): WorkflowDocument {
  return {
    version: 4,
    name: "Config writer test",
    goal: "",
    cwd: "",
    useOrchestrator: false,
    entryNodeId: workflowNode.id,
    variables: [],
    limits: { maxTotalSteps: 50, maxVisitsPerNode: 10 },
    nodes: [workflowNode],
    edges: [],
  };
}

function storedKind(): NodeKind {
  const kind = store.workflow?.nodes[0]?.kind;
  if (!kind) throw new Error("expected the stored workflow node");
  return kind;
}

function expectNoSiblingConfig(kind: NodeKind, ownKey: ConfigKey): void {
  for (const key of CONFIG_KEYS) {
    if (key === ownKey) continue;
    expect(
      Object.hasOwn(kind, key),
      `${kind.type} writer must not create sibling config field ${key}`,
    ).toBe(false);
  }
}

describe("makeConfigWriter", () => {
  afterEach(() => {
    store.createWorkflow();
  });

  it("writes decide values only to decideConfig", () => {
    store.setWorkflow(workflowWith(node({
      type: "decide",
      decideConfig: { prompt: "old", inputs: [], outcomes: [] },
    })));

    makeConfigWriter("node-under-test", DECIDE_CONFIG_TARGET)("prompt", "Choose a path");

    const kind = storedKind();
    expect(kind.type, "the decide writer must preserve the node kind").toBe("decide");
    if (kind.type !== "decide") throw new Error("expected a decide node");
    expect(kind.decideConfig, "the decide writer must merge into decideConfig").toEqual({
      prompt: "Choose a path",
      inputs: [],
      outcomes: [],
    });
    expectNoSiblingConfig(kind, "decideConfig");
  });

  it("writes parallel_batch values only to batchConfig", () => {
    store.setWorkflow(workflowWith(node({
      type: "parallel_batch",
      batchConfig: {
        itemsBinding: "items",
        maxConcurrent: 4,
        itemVar: "item",
        bodyEntry: "start",
      },
    })));

    makeConfigWriter("node-under-test", BATCH_CONFIG_TARGET)("maxConcurrent", 7);

    const kind = storedKind();
    expect(kind.type, "the batch writer must preserve the node kind").toBe("parallel_batch");
    if (kind.type !== "parallel_batch") throw new Error("expected a parallel_batch node");
    expect(kind.batchConfig, "the batch writer must merge into batchConfig").toEqual({
      itemsBinding: "items",
      maxConcurrent: 7,
      itemVar: "item",
      bodyEntry: "start",
    });
    expectNoSiblingConfig(kind, "batchConfig");
  });

  it("writes spawn values only to spawnConfig", () => {
    store.setWorkflow(workflowWith(node({ type: "spawn" })));

    makeConfigWriter("node-under-test", SPAWN_CONFIG_TARGET)("command", "printf ok");

    const kind = storedKind();
    expect(kind.type, "the spawn writer must preserve the node kind").toBe("spawn");
    if (kind.type !== "spawn") throw new Error("expected a spawn node");
    expect(kind.spawnConfig, "the spawn writer must merge into spawnConfig").toEqual({
      command: "printf ok",
    });
    expectNoSiblingConfig(kind, "spawnConfig");
  });

  it("writes send values only to sendConfig", () => {
    store.setWorkflow(workflowWith(node({ type: "send" })));

    makeConfigWriter("node-under-test", SEND_CONFIG_TARGET)("text", "hello");

    const kind = storedKind();
    expect(kind.type, "the send writer must preserve the node kind").toBe("send");
    if (kind.type !== "send") throw new Error("expected a send node");
    expect(kind.sendConfig, "the send writer must merge defaults into sendConfig").toEqual({
      text: "hello",
      enter: true,
    });
    expectNoSiblingConfig(kind, "sendConfig");
  });

  it("writes wait values only to waitConfig", () => {
    store.setWorkflow(workflowWith(node({ type: "wait" })));

    makeConfigWriter("node-under-test", WAIT_CONFIG_TARGET)("marker", "READY");

    const kind = storedKind();
    expect(kind.type, "the wait writer must preserve the node kind").toBe("wait");
    if (kind.type !== "wait") throw new Error("expected a wait node");
    expect(kind.waitConfig, "the wait writer must merge defaults into waitConfig").toEqual({
      mode: "idle",
      marker: "READY",
    });
    expectNoSiblingConfig(kind, "waitConfig");
  });

  it("writes capture values only to captureConfig", () => {
    store.setWorkflow(workflowWith(node({ type: "capture" })));

    makeConfigWriter("node-under-test", CAPTURE_CONFIG_TARGET)("lines", 20);

    const kind = storedKind();
    expect(kind.type, "the capture writer must preserve the node kind").toBe("capture");
    if (kind.type !== "capture") throw new Error("expected a capture node");
    expect(kind.captureConfig, "the capture writer must merge defaults into captureConfig").toEqual({
      all: false,
      ansi: false,
      lines: 20,
    });
    expectNoSiblingConfig(kind, "captureConfig");
  });

  it("writes kill values only to killConfig", () => {
    store.setWorkflow(workflowWith(node({ type: "kill" })));

    makeConfigWriter("node-under-test", KILL_CONFIG_TARGET)("target", "pane-1");

    const kind = storedKind();
    expect(kind.type, "the kill writer must preserve the node kind").toBe("kill");
    if (kind.type !== "kill") throw new Error("expected a kill node");
    expect(kind.killConfig, "the kill writer must merge defaults into killConfig").toEqual({
      target: "pane-1",
    });
    expectNoSiblingConfig(kind, "killConfig");
  });

  it("writes subflow values only to subflowConfig", () => {
    store.setWorkflow(workflowWith(node({
      type: "subflow",
      subflowConfig: { workflowName: "Old", inputs: [], maxDepth: 10 },
    })));

    makeConfigWriter("node-under-test", SUBFLOW_CONFIG_TARGET)("workflowName", "Reusable");

    const kind = storedKind();
    expect(kind.type, "the shared writer must preserve the subflow node kind").toBe("subflow");
    if (kind.type !== "subflow") throw new Error("expected a subflow node");
    expect(kind.subflowConfig, "the shared writer must merge into subflowConfig").toEqual({
      workflowName: "Reusable",
      inputs: [],
      maxDepth: 10,
    });
    expectNoSiblingConfig(kind, "subflowConfig");
  });

  it("uses the shared subflow writer to write call values only to subflowConfig", () => {
    store.setWorkflow(workflowWith(node({
      type: "call",
      subflowConfig: { workflowName: "Reusable", inputs: [], maxDepth: 10 },
    })));

    makeConfigWriter("node-under-test", SUBFLOW_CONFIG_TARGET)("maxDepth", 6);

    const kind = storedKind();
    expect(kind.type, "the shared writer must preserve the call node kind").toBe("call");
    if (kind.type !== "call") throw new Error("expected a call node");
    expect(kind.subflowConfig, "the shared writer must merge call values into subflowConfig").toEqual({
      workflowName: "Reusable",
      inputs: [],
      maxDepth: 6,
    });
    expectNoSiblingConfig(kind, "subflowConfig");
  });

  it("leaves a different node kind untouched", () => {
    store.setWorkflow(workflowWith(node({
      type: "send",
      sendConfig: { target: "pane-1", text: "unchanged", enter: false },
    })));

    makeConfigWriter("node-under-test", CAPTURE_CONFIG_TARGET)("lines", 20);

    const kind = storedKind();
    expect(kind, "the capture kind guard must leave a send node unchanged").toEqual({
      type: "send",
      sendConfig: { target: "pane-1", text: "unchanged", enter: false },
    });
    expect(
      Object.hasOwn(kind, "captureConfig"),
      "the rejected capture write must not create captureConfig on a send node",
    ).toBe(false);
  });
});
