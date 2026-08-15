import {
  DEFAULT_BATCH_CONFIG,
  DEFAULT_CAPTURE_CONFIG,
  DEFAULT_DECIDE_CONFIG,
  DEFAULT_KILL_CONFIG,
  DEFAULT_SEND_CONFIG,
  DEFAULT_SPAWN_CONFIG,
  DEFAULT_SUBFLOW_CONFIG,
  DEFAULT_WAIT_CONFIG,
  defineConfigTarget,
} from "./mergeConfig";

// Positive: decide maps to its compiler-checked config field and defaults.
defineConfigTarget(["decide"], "decideConfig", DEFAULT_DECIDE_CONFIG);
// Positive: parallel_batch maps to its compiler-checked config field and defaults.
defineConfigTarget(["parallel_batch"], "batchConfig", DEFAULT_BATCH_CONFIG);
// Positive: spawn maps to its compiler-checked config field and defaults.
defineConfigTarget(["spawn"], "spawnConfig", DEFAULT_SPAWN_CONFIG);
// Positive: send maps to its compiler-checked config field and defaults.
defineConfigTarget(["send"], "sendConfig", DEFAULT_SEND_CONFIG);
// Positive: wait maps to its compiler-checked config field and defaults.
defineConfigTarget(["wait"], "waitConfig", DEFAULT_WAIT_CONFIG);
// Positive: capture maps to its compiler-checked config field and defaults.
defineConfigTarget(["capture"], "captureConfig", DEFAULT_CAPTURE_CONFIG);
// Positive: kill maps to its compiler-checked config field and defaults.
defineConfigTarget(["kill"], "killConfig", DEFAULT_KILL_CONFIG);
// Positive: subflow maps to its compiler-checked shared config field and defaults.
defineConfigTarget(["subflow"], "subflowConfig", DEFAULT_SUBFLOW_CONFIG);
// Positive: call maps to the same compiler-checked config field and defaults as subflow.
defineConfigTarget(["call"], "subflowConfig", DEFAULT_SUBFLOW_CONFIG);
// Positive: the shared subflow/call target has a field valid on both variants.
defineConfigTarget(["subflow", "call"], "subflowConfig", DEFAULT_SUBFLOW_CONFIG);

// P2(c): a kind name outside NodeKind["type"] must be rejected.
// @ts-expect-error "not_a_node_kind" is not a workflow node kind.
defineConfigTarget(["not_a_node_kind"], "captureConfig", DEFAULT_CAPTURE_CONFIG);

// P2(a): a config-field key absent from the named kind variant must be rejected.
// @ts-expect-error capture nodes have captureConfig, not capureConfig.
defineConfigTarget(["capture"], "capureConfig", DEFAULT_CAPTURE_CONFIG);

// P2(a): a real sibling config field is still absent from the named kind variant.
// @ts-expect-error capture nodes cannot be associated with sendConfig.
defineConfigTarget(["capture"], "sendConfig", DEFAULT_SEND_CONFIG);

// P2(b): defaults must have the named kind's config-field type.
// @ts-expect-error SendConfig is not the CaptureConfig required by captureConfig.
defineConfigTarget(["capture"], "captureConfig", DEFAULT_SEND_CONFIG);

// P2(b): mutually-assignable weak configs — KillConfig must not back spawnConfig.
// @ts-expect-error KillConfig is not the SpawnConfig required by spawnConfig.
defineConfigTarget(["spawn"], "spawnConfig", DEFAULT_KILL_CONFIG);

// P2(b): mutually-assignable weak configs — SpawnConfig must not back killConfig.
// @ts-expect-error SpawnConfig is not the KillConfig required by killConfig.
defineConfigTarget(["kill"], "killConfig", DEFAULT_SPAWN_CONFIG);

// Empty defaults must not satisfy a config with required properties.
// @ts-expect-error CaptureConfig requires all and ansi.
defineConfigTarget(["capture"], "captureConfig", {});

// A subset missing a required property must be rejected.
// @ts-expect-error WaitConfig requires mode.
defineConfigTarget(["wait"], "waitConfig", { target: "x" });

// Same on a required-config kind: missing prompt must be rejected.
// @ts-expect-error DecideConfig requires prompt.
defineConfigTarget(["decide"], "decideConfig", { outcomes: [] });

// Right keys, wrong property type must be rejected.
// @ts-expect-error CaptureConfig.all must be boolean, not string.
defineConfigTarget(["capture"], "captureConfig", { all: "yes", ansi: false });
