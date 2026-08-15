import { store } from "@/lib/stores/workflowStore.svelte";
import type {
  BatchConfig,
  CaptureConfig,
  DecideConfig,
  KillConfig,
  NodeKind,
  SendConfig,
  SpawnConfig,
  SubflowConfig,
  WaitConfig,
  WorkflowNode,
} from "@/lib/types/workflow";

/** Merge a field into a config object; pass undefined to clear it. */
export function mergeConfig<T extends object>(
  defaults: T,
  base: T | undefined,
  field: string,
  value: unknown,
): T {
  const next: Record<string, unknown> = {
    ...(defaults as Record<string, unknown>),
    ...((base ?? {}) as Record<string, unknown>),
  };
  if (value === undefined) delete next[field];
  else next[field] = value;
  return next as T;
}

type NodeKindType = NodeKind["type"];
type KindVariant<K extends NodeKindType> = Extract<NodeKind, { type: K }>;
type ConfigField<K extends NodeKindType> = Exclude<keyof KindVariant<K>, "type"> & string;
type KindConfigValue<
  K extends NodeKindType,
  F extends ConfigField<K>,
> = Extract<NonNullable<KindVariant<K>[F]>, object>;

/** Reject defaults whose keys are not exactly those of the target config type. */
type Exact<D, U> = Exclude<keyof D, keyof U> extends never ? D : never;

export interface ConfigTarget<
  K extends NodeKindType,
  F extends ConfigField<K>,
> {
  readonly kindTypes: readonly K[];
  readonly configKey: F;
  readonly defaults: KindConfigValue<K, F>;
}

/** Define the checked kind → config field → defaults association consumed by accessors and writers. */
export function defineConfigTarget<
  const K extends NodeKindType,
  const F extends ConfigField<K>,
  D extends KindConfigValue<K, F>,
>(
  kindTypes: readonly K[],
  configKey: F,
  defaults: Exact<D, KindConfigValue<K, F>>,
): ConfigTarget<K, F> {
  return { kindTypes, configKey, defaults: defaults as KindConfigValue<K, F> };
}

function matchesTarget<K extends NodeKindType, F extends ConfigField<K>>(
  kind: NodeKind,
  target: ConfigTarget<K, F>,
): kind is Extract<NodeKind, { type: K }> {
  return target.kindTypes.some((kindType) => kindType === kind.type);
}

export const DEFAULT_DECIDE_CONFIG: DecideConfig = { prompt: "", inputs: [], outcomes: [] };
export const MAX_BATCH_CONCURRENT = 32;
export const DEFAULT_BATCH_CONFIG: BatchConfig = {
  itemsBinding: "",
  maxConcurrent: 4,
  itemVar: "item",
  bodyEntry: "",
};
export const DEFAULT_SPAWN_CONFIG: SpawnConfig = {};
export const DEFAULT_SEND_CONFIG: SendConfig = { text: "", enter: true };
export const DEFAULT_WAIT_CONFIG: WaitConfig = { mode: "idle" };
export const DEFAULT_CAPTURE_CONFIG: CaptureConfig = { all: false, ansi: false };
export const DEFAULT_KILL_CONFIG: KillConfig = {};
export const DEFAULT_SUBFLOW_CONFIG: SubflowConfig = { workflowName: "", inputs: [], maxDepth: 10 };

export const DECIDE_CONFIG_TARGET = defineConfigTarget(
  ["decide"],
  "decideConfig",
  DEFAULT_DECIDE_CONFIG,
);
export const BATCH_CONFIG_TARGET = defineConfigTarget(
  ["parallel_batch"],
  "batchConfig",
  DEFAULT_BATCH_CONFIG,
);
export const SPAWN_CONFIG_TARGET = defineConfigTarget(
  ["spawn"],
  "spawnConfig",
  DEFAULT_SPAWN_CONFIG,
);
export const SEND_CONFIG_TARGET = defineConfigTarget(
  ["send"],
  "sendConfig",
  DEFAULT_SEND_CONFIG,
);
export const WAIT_CONFIG_TARGET = defineConfigTarget(
  ["wait"],
  "waitConfig",
  DEFAULT_WAIT_CONFIG,
);
export const CAPTURE_CONFIG_TARGET = defineConfigTarget(
  ["capture"],
  "captureConfig",
  DEFAULT_CAPTURE_CONFIG,
);
export const KILL_CONFIG_TARGET = defineConfigTarget(
  ["kill"],
  "killConfig",
  DEFAULT_KILL_CONFIG,
);
export const SUBFLOW_CONFIG_TARGET = defineConfigTarget(
  ["subflow", "call"],
  "subflowConfig",
  DEFAULT_SUBFLOW_CONFIG,
);

export function decideConfig(node: WorkflowNode): DecideConfig {
  return matchesTarget(node.kind, DECIDE_CONFIG_TARGET)
    ? node.kind[DECIDE_CONFIG_TARGET.configKey]
    : DECIDE_CONFIG_TARGET.defaults;
}

export function batchConfig(node: WorkflowNode): BatchConfig {
  return matchesTarget(node.kind, BATCH_CONFIG_TARGET)
    ? node.kind[BATCH_CONFIG_TARGET.configKey]
    : BATCH_CONFIG_TARGET.defaults;
}

export function spawnConfig(node: WorkflowNode): SpawnConfig {
  return matchesTarget(node.kind, SPAWN_CONFIG_TARGET)
    ? node.kind[SPAWN_CONFIG_TARGET.configKey] ?? SPAWN_CONFIG_TARGET.defaults
    : SPAWN_CONFIG_TARGET.defaults;
}

export function sendConfig(node: WorkflowNode): SendConfig {
  return matchesTarget(node.kind, SEND_CONFIG_TARGET)
    ? node.kind[SEND_CONFIG_TARGET.configKey] ?? SEND_CONFIG_TARGET.defaults
    : SEND_CONFIG_TARGET.defaults;
}

export function waitConfig(node: WorkflowNode): WaitConfig {
  return matchesTarget(node.kind, WAIT_CONFIG_TARGET)
    ? node.kind[WAIT_CONFIG_TARGET.configKey] ?? WAIT_CONFIG_TARGET.defaults
    : WAIT_CONFIG_TARGET.defaults;
}

export function captureConfig(node: WorkflowNode): CaptureConfig {
  return matchesTarget(node.kind, CAPTURE_CONFIG_TARGET)
    ? node.kind[CAPTURE_CONFIG_TARGET.configKey] ?? CAPTURE_CONFIG_TARGET.defaults
    : CAPTURE_CONFIG_TARGET.defaults;
}

export function killConfig(node: WorkflowNode): KillConfig {
  return matchesTarget(node.kind, KILL_CONFIG_TARGET)
    ? node.kind[KILL_CONFIG_TARGET.configKey] ?? KILL_CONFIG_TARGET.defaults
    : KILL_CONFIG_TARGET.defaults;
}

export function subflowConfig(node: WorkflowNode): SubflowConfig {
  return matchesTarget(node.kind, SUBFLOW_CONFIG_TARGET)
    ? node.kind[SUBFLOW_CONFIG_TARGET.configKey]
    : SUBFLOW_CONFIG_TARGET.defaults;
}

export function numOrUndef(v: string): number | undefined {
  const n = Number(v);
  // Guard against non-finite values (e.g. "1e999" → Infinity): return
  // undefined so the field is omitted from the JSON rather than serialized
  // as null, preserving the backend's number-or-absent contract.
  return v !== "" && Number.isFinite(n) ? n : undefined;
}

/** Build a panel-local config writer from a checked config target. */
export function makeConfigWriter<
  K extends NodeKindType,
  F extends ConfigField<K>,
>(
  nodeId: string,
  target: ConfigTarget<K, F>,
): (field: string, value: unknown) => void {
  return (field: string, value: unknown) => {
    store.updateWorkflow((wf) => {
      const n = wf.nodes.find((x) => x.id === nodeId);
      if (!n || !matchesTarget(n.kind, target)) return;
      const kind = n.kind as Record<string, KindConfigValue<K, F> | undefined>;
      kind[target.configKey] = mergeConfig(
        target.defaults,
        kind[target.configKey],
        field,
        value,
      );
    });
  };
}
