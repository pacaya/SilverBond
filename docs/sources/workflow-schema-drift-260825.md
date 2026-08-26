# Documentation drift audit — 2026-08-25

Point-in-time audit of `docs/workflow-schema.md` and `docs/execution-model.md` against the
Rust source, taken at commit `5f2f2c7`. Produced for the `docs-truth` epic of
RDMP-260815-2009-01 and cited by PRD-260826-0009-01 as its work order.

**Method.** Three read-only agents compared each doc against its authority — `src/model.rs`
for the schema, `src/runtime.rs` + `src/storage.rs` for the execution model — and inventoried
every discrepancy with citations on both sides. Findings are grouped below by the remediation
tier the PRD assigns them.

**Headline.** `docs/workflow-schema.md` does not describe a stale version of the schema; it
describes a *different* schema (the v2/v3 flat-node shape). `docs/execution-model.md` is
roughly two feature epochs behind: no subflows, no call frames, no parallel batch, no decide
nodes, no pane node family. Separately, `CONTEXT.md` has drifted in the **opposite** direction
— it describes the v5 target state that does not exist yet. Code sits between two documents
that are both wrong, in opposite directions.

**Tier legend** (PRD-260826-0009-01 § Implementation Decisions — this legend is subordinate to the
PRD; where they differ, the PRD wins):
- **G** — generated. Emitted into marker-delimited blocks from constructed Rust values. Never
  hand-edited. Scope: the node-kind catalog only.
- **P** — hand-written, source cross-referenced. Config field tables, the template-token list, the
  validation catalog, the event vocabulary, edge and top-level field tables. Not machine-pinned.
- **C** — conceptual prose about mechanism. Hand-written, the epic's highest-value content.
- **D** — delete. Neither generated, nor source-cross-referenced, nor narrated.

*(Revised 2026-08-25 during the PRD's round-2 gate: an earlier revision used a T1/T2/T3 legend in
which "T1" meant "pinned by a doc-example test". That scheme was superseded when generation was
adopted; the group headings below carry the current tiers.)*

---

## Part 1 — `docs/workflow-schema.md` vs `src/model.rs`

### 1.1 Grammar-level falsehoods (G — every one is load-bearing for a copied example)

1. **Every node example is the v2 flat shape.** Doc `:106`, `:132`, `:150-166`, `:175`, `:194`,
   `:216` write `"type": "task"` with config objects flat on the node. `WorkflowNode` has **no
   `type` field**; kind lives in a required nested internally-tagged object
   `kind: { "type": "…", "<x>Config": {…} }` (`src/model.rs:864-878`; the struct's own
   doc-comment at `:867-877` spells out the exact shape the doc contradicts). Every example in
   the Node Types section (doc `:103-221`) is un-deserializable as canonical v4.
2. **`agentConfig` placement is wrong.** Doc shows it as a node-root key (`:132`, `:164`). It
   lives inside `kind`, and only for `Task` (`src/model.rs:696-702`) and `RunAgent` (`:741-747`).
   On every other kind the v2 migration silently drops it (`src/model.rs:1281-1293`).
3. **Edge fields `source`/`target` do not exist.** Truth is
   `WorkflowEdge { id, from, to, outcome, label, branchId, condition }` (`src/model.rs:922-937`),
   `rename_all = "camelCase"`, **no serde aliases** — a doc-shaped edge fails deserialization.
4. **Edge `id` is required and undocumented** (`src/model.rs:925`, no default); duplicate ids
   are a validation error (`:2140-2145`). The doc's edge table (`:236-241`) omits it.
5. **Edge `branchId` undocumented** (`src/model.rs:931-932`).
6. **Edge `condition` undocumented** (`src/model.rs:933-934`) — the entire deterministic
   branch-routing mechanism, absent from the doc.
7. **`StructuredCondition` never documented at all.** `{field, operator, value}`, all three
   required (`src/model.rs:128-134`). Operator set exists only in `evaluate_condition`
   (`:3182-3234`): `==`, `!=`, `contains`, `matches`, `>`, `<`, `>=`, `<=`; unknown operators
   return `"unknown operator: …"`; `matches` patterns over 256 chars are rejected at runtime
   (`:3206-3211`); `field` is a dot-path via `get_nested_field` (`:3174-3180`).
8. **Top-level `subflows` catalog undocumented** — `BTreeMap<String, Box<WorkflowV3>>`
   (`src/model.rs:965-969`). No row in the doc's top-level table (`:32-46`).

### 1.2 Node-kind coverage (G — catalog; P — per-kind field tables)

9. **4 of 14 kinds documented.** Present: `task` (doc `:99`), `approval` (`:168`), `split`
   (`:187`), `collector` (`:209`). Absent: `decide`, `parallel_batch`, `subflow`, `call`,
   `spawn`, `send`, `wait`, `capture`, `kill`, `run_agent` — all in `NodeKind`
   (`src/model.rs:695-753`) and all advertised by `GET /api/capabilities` (`src/api.rs:216`).
10. `DecideConfig` (`src/model.rs:441-452`) — `inputs`, `prompt`, `model`, `outcomes`.
    `default_decide_model()` = `"claude-haiku-4-5"` (`:437-439`), an undocumented real default.
11. `BatchConfig` (`src/model.rs:461-471`) — `itemsBinding` (**required**), `maxConcurrent`
    (default 4, `:454-456`), `itemVar` (**required**), `bodyEntry` (**required**),
    `collectorVar`. Serde alias: `batchConfig` **or** legacy `parallelBatchConfig` (`:709-711`).
    Runtime clamp `MAX_PARALLEL_BATCH_CONCURRENT = 32` (`:459`, applied `src/runtime.rs:5113`).
12. `SubflowConfig` (`src/model.rs:628-639`) — `workflowName` with **two aliases** `workflow`
    and `subflowName` (`:631`), `exitNodeId`, `inputs`, `maxDepth` (default 10, `:247-249`).
    Shared verbatim by `Subflow` and `Call` under the same `subflowConfig` key (`:712-719`).
13. `InputBinding` (`src/model.rs:430-435`) — `{name, source}`, both required.
14. `SpawnConfig` (`src/model.rs:485-504`) — `agent`, `command`, `access`, `extraArgs`, `cwd`,
    `name`, `sessionName`.
15. `SendConfig` (`src/model.rs:504-511`) — `target`, `text`, `enter`. **`enter` defaults to
    `true`**, not `false` (`Default` impl `:513-521`).
16. `WaitConfig` + `WaitMode` (`src/model.rs:532-547`) — `target`, `mode` (default `idle`),
    `marker`, `timeout`, `idleSeconds`, `readyStableSeconds`. `WaitMode` is snake_case
    `idle | ready | until` (`:523-531`).
17. `CaptureConfig` (`src/model.rs:562-575`) — `target`, `lines`, `all`, `ansi`.
18. `KillConfig` (`src/model.rs:575-583`) — `target`, `sessionName`.
19. `RunAgentConfig` (`src/model.rs:584-608`) — `agent`, `prompt`, `cwd`, `access`, `extraArgs`,
    `name`, `timeout`, `idleSeconds`, `readyStableSeconds`, `until`, `killAfter`. **`killAfter`
    defaults to `true`** (`:610-626`). `run_agent` is the only kind carrying **both**
    `runAgentConfig` and `agentConfig` inside `kind` (`:740-753`).
20. **Emission asymmetry.** `spawnConfig`/`sendConfig`/`waitConfig`/`captureConfig`/`killConfig`/
    `runAgentConfig` use `skip_serializing_if = "is_default"` (`src/model.rs:721-745`) and vanish
    from emitted JSON when default; `decideConfig`/`batchConfig`/`subflowConfig` do not (`:704-719`).

### 1.3 Task-node field drift (P)

21. **"`agent` defaults to `claude`" is misleading** (doc `:153`). The field carries
    `#[serde(default)]` but no default *value* — an omitted `agent` deserializes to `None`, not to
    `"claude"` (`src/model.rs:874-875`); the runtime falls back to `DEFAULT_AGENT` (`:57`,
    `:85-95`), but validation emits a hard **error** for a Task node with no agent (`:1668-1677`),
    so the fallback never rescues a document at run start.
    *(Corrected 2026-08-26 during the to-issues readiness gate: an earlier revision said "No serde
    default" and cited `:871-872`. The attribute is present; what is absent is a default value.)*
22. **"`prompt` Required: Yes" is wrong** (doc `:154`). `#[serde(default)] String`
    (`src/model.rs:873-874`); an empty prompt is only a **warning** (`:1679-1686`).
23. **`loopCondition` type is wrong.** Doc calls it a string prompt for the orchestrator
    (`:131`, `:163`). Truth: `Option<StructuredCondition>` (`src/model.rs:898-899`), evaluated
    deterministically, never by an LLM (`src/runtime.rs:6485-6500`). The doc's example value
    would fail deserialization.
24. **`splitFailurePolicy` is a universal node field**, not split-only as the doc implies
    (`:197`, `:201-205`). It sits on every `WorkflowNode` (`src/model.rs:900-905`) with a custom
    `deserialize_with` mapping explicit `null` → `best_effort_continue` (`:364-373`).
25. Doc's Task table (`:148-166`) omits `kind` and `splitFailurePolicy`.
26. **`retryCount` cap undocumented** — `MAX_NODE_RETRY_COUNT = 10` (`src/model.rs:17`);
    exceeding it is a hard error that short-circuits the whole validation (`:1553-1571`).
27. **`loopMaxIterations` default undocumented** — 5 when unset (`src/runtime.rs:6447`);
    reaching the cap with no `loop_exit` edge fails the run (`:6459-6480`).
28. **`continueSessionFrom` constraints undocumented** — five distinct validation checks
    (`src/model.rs:2022-2130`): source must be `Task`/`RunAgent`, same resolved agent, source
    must not have a broader access profile, same resolved cwd, node must exist.

### 1.4 Approval / Split / Collector semantics (G + C)

29. Approval is a unit variant (`src/model.rs:703`), so the node still carries the full task
    field set — the doc's minimal shape (`:172-178`) misleads.
30. **"Split nodes spawn one cursor per outgoing `branch` edge" (doc `:207`) is backwards.**
    Validation *errors* on any branch/loop/reject edge from a split (`src/model.rs:1943-1955`);
    fan-out is over **success** edges. Zero success edges is an error (`:1956-1963`); exactly
    one is a warning (`:1964-1970`).
31. The doc's split example includes `prompt` (`:196`), which triggers the
    "task execution fields are ignored" warning (`src/model.rs:1923-1941`, predicate `:3116-3130`).
32. The doc's collector example includes `prompt` and `responseFormat: "json"` (`:218-219`) —
    same warning, and it contradicts the doc's own `:223`.
33. **Collector rules undocumented** — ≥1 inbound edge (error, `src/model.rs:1972-1980`);
    exactly one outbound success edge (error, `:1981-1990`); no branch/loop/reject edges
    (error, `:1991-2003`); duplicate input keys, where the key is `edge.label` falling back to
    `edge.from`, are an error (`:2005-2020`).

### 1.5 Top-level grammar (P)

34. **"`goal` Required: Yes" is wrong** — `#[serde(default)]` (`src/model.rs:942-943`).
35. **"`nodes`/`edges` Required: Yes" is wrong** — both `#[serde(default)]` (`:955-958`).
36. **`entryNodeId` and `version` are the only genuinely required top-level keys**
    (`src/model.rs:940`, `:951`).
37. `nodes`/`edges`/`variables`/`limits` are always emitted; `agentDefaults`/`subflows`/`runAs`/
    `name`/`ui` are skipped when empty (`src/model.rs:952-971`).
38. `agentDefaults` is a `BTreeMap` — sorted, deterministic on serialize (`:959-960`).
39. **Unknown keys are silently accepted** — no `deny_unknown_fields` anywhere in `src/`.
40. **The `limits` `0` sentinel is undocumented.** `ensure_defaults` rewrites any `0` back to
    50/10 (`src/model.rs:1116-1132`), and `limits_are_canonical` treats `{0,0}` as
    absent-equivalent (`:241-245`). `maxTotalSteps: 0` means 50, not "unlimited".
41. `ui.viewport` sub-fields have **no individual serde defaults** (`src/model.rs:652-658`,
    `:670-677`) — a partial `viewport` object fails deserialization.

### 1.6 Variables (P + C)

42. **`WorkflowVariable.default` is a `String`**, not arbitrary JSON (`src/model.rs:113-120`).
43. **Undocumented second role:** an empty `default` on a subflow variable makes it a
    **required input binding** — an unbound one is a hard error (`src/model.rs:2688-2703`).
44. Root variable names are not validated for uniqueness; only subflow *bindings* are deduped
    (`src/model.rs:2661-2672`).

### 1.7 Prompt templates (P — the doc lists 4 of 10 and invents one)

45. **`{{node_name:field}}` does not exist** (doc `:261`).
46. Complete set in `resolve_template_vars` (`src/runtime.rs:7062-7153`): `{{var:<name>}}`,
    `{{<nodeId>}}`, `{{node:<nodeId>.output}}`, `{{node:<nodeId>.output.<path>}}`,
    `{{node:<nodeId>.parsedOutput.<path>}}`, `{{context:<name>}}`, `{{previous_output}}`,
    `{{branch_origin}}`, `{{branch_choice}}`, `{{all_predecessors}}`.
47. **`contextSources` is not injected into the prompt** (doc `:276`). It only registers the
    `{{context:<name>}}` substitution, which does nothing unless the prompt contains that token
    (`src/runtime.rs:7124-7128`).
48. **`{{all_predecessors}}` is direct inbound predecessors only**, empty outputs dropped,
    joined by `\n---\n`, stale results prefixed `[preserved from prior run]`
    (`src/runtime.rs:7135-7152`).

### 1.8 `skipCondition` (P)

49. Shape never specified. `source` defaults to `"previous_output"` (`src/model.rs:136-148`);
    `kind` is **renamed to `"type"`** on the wire (`:140-141`).
50. **Allowed `type` values undocumented and the fall-through is silent** — exactly `contains`,
    `not_contains`, `regex`; anything else evaluates `false` with no validation error
    (`src/runtime.rs:2702-2732`). Only `regex` gets a compile check (`src/model.rs:1628-1642`).
51. A non-`"previous_output"` `source` is treated as a node id, defaulting to `""` if not found
    (`src/runtime.rs:2711-2718`).

### 1.9 Validation (P — doc claims 4 errors + 2 warnings + a nonexistent severity)

52. **Truth: 70 `error` and 16 `warning` issues, and zero `info`.** The doc's "Info: Graph
    metadata" (`:388`) is not a severity — `GraphMetadata` is a separate `graph` field on
    `ValidationResult` (`src/model.rs:982-993`, `:1001-1011`).
53. `ValidationIssue` is `{severity, nodeId?, scope?, message}` (`src/model.rs:971-981`). There
    is no `location`; subflow scoping uses `scope` (`:2168-2177`).
54. `ValidationResult` also returns `workflow` and `notices` (`:1003-1011`).
55. Input-bound hard rejections: `MAX_WORKFLOW_SUBFLOWS = 1024`, `MAX_SUBFLOW_CALL_EDGES = 4096`,
    `MAX_WORKFLOW_NODES = 50_000`, `MAX_WORKFLOW_EDGES = 100_000` (`src/model.rs:13-16`,
    enforced `:1490-1580`), counted across root + subflow catalog.
56. Nested subflow catalogs are rejected outright (`src/model.rs:1391-1420`).
57. Subflow bodies warn on root-only fields — `runAs` (`:1352-1364`), non-canonical `limits`
    (`:1366-1375`).
58. **Absolute-cwd enforcement** on workflow `cwd`, node `cwd`, `spawnConfig.cwd`,
    `runAgentConfig.cwd` (`validate_absolute_cwd`, `src/model.rs:1321-1340`).
59. `runAs` validation: empty command/user → error; blank tokens → error; a 22-character
    shell-metachar set in `user` → error (`src/model.rs:2179-2250`).
60. `extraArgs` allowlist (`validate_agent_launch_config`, `src/model.rs:2252-2360`): option-syntax
    args rejected unless allowlisted; `--model` for `claude`/`codex`; `--search` for `codex`;
    `-c`/`--config` for `codex` with `KEY=VALUE` validated (`:2366-2426`); any key containing
    `sandbox` or `approval` rejected (`:2446-2450`).
61. `access` profile validation (`src/model.rs:2258-2285`) — a **different concept** from the
    `accessMode` enum; the doc documents neither.
62. Wait/run-agent timing validation (`src/model.rs:2452-2502`).
63. Decide-node validation — nine distinct checks (`src/model.rs:2917-3043`).
64. Parallel-batch validation — six checks (`src/model.rs:3045-3114`).
65. Subflow/call validation — thirteen checks (`src/model.rs:2537-2707`).
66. SCC-based subflow call-cycle detection, emitted as a **warning** (`src/model.rs:2727-2772`).
67. Edge-topology rules (`src/model.rs:1830-1886`).
68. Consistency warnings for `outputSchema`/`loopCondition`/branch-conditions without
    `responseFormat: json` (`src/model.rs:1608-1618`, `:1887-1918`).
69. **"Dead-end nodes" is mislabeled** (doc `:387`) — the warning is `"…" is a terminal node.`
    (`src/model.rs:1913-1922`); `deadEndNodeIds` is separate `GraphMetadata`.
70. **"Task nodes without prompts" is listed as an Error** (doc `:386`) — it is a warning.
71. Both edge source and target existence are checked (`src/model.rs:2146-2161`).

### 1.10 Version handling (C — this is a contract, not an enumeration)

72. Acceptance/rejection is centralized in `migrate_workflow_value_to_v4`
    (`src/model.rs:1134-1172`); the rejection is an `anyhow::bail!` — an ingest failure, **not**
    a `ValidationIssue`, so it never reaches the documented "issues with severity" channel.
73. **A missing `version` key is a hard ingest error**, not a default (`src/model.rs:1136-1139`).
74. Non-integer/negative/float versions fail with the same "version is required" message.
75. **Version is force-rewritten to 4 in two places** (`src/model.rs:1170`, `:1117`), so
    `validate_workflow` can never reject a version.
    *(Corrected 2026-08-26 during the to-issues readiness gate: the first citation read `:1169`,
    one line above the `workflow.insert("version", …)` it names. Verified against commit `5f2f2c7`,
    the commit this audit is pinned to, so the off-by-one was present at the pin rather than
    introduced by later drift. The count itself is accurate; the derived issue briefs and the PRD
    deliberately do not restate it.)*
76. Subflows are version-migrated recursively (`src/model.rs:1157-1166`) — a v4 root with a v1
    subflow is rejected.
77. Legacy `outputSchema` shorthand migration accepts both `outputSchema` and `output_schema`
    keys and is narrowly defined (`src/model.rs:1174-1198`, `:382-404`).
78. **The v2→v3 `kind` migration fires on *any* version**, not just 2/3 — it is called
    unconditionally (`src/model.rs:1152`) and triggers per-node whenever `kind` is absent and
    `type` is present (`:1206-1215`). *(This is why the generated examples are
    constructed Rust values checked without the migration path; see PRD § Testing Decisions.)*
79. The v2 migration silently discards config fields that don't match the node type
    (`src/model.rs:1281-1293`); unknown node types produce a `kind` with only `type`, which then
    fails `NodeKind` deserialization (`:1279`).
80. `capture` and `kill` get an injected empty config object during migration (`:1264-1276`).
81. Storage re-normalizes any stored row whose version is not 4 (`src/storage.rs:1333`).

### 1.11 Agent config (P)

82. `agentConfig` is `AgentNodeConfig`, which `#[serde(flatten)]`s `AgentDefaults`
    (`src/model.rs:256-299`). `allowedTools`/`disallowedTools` are the only node-exclusive
    fields; `autoApprove` and `orchestrator` exist at both levels.
83. **`accessMode` defaults to `execute`**, not `read_only` (`src/driver.rs:503-511`).
84. `reasoningLevel` is `Option` at both levels with no default (`src/model.rs:261-262`).
85. `toolToggles` is exactly `{webSearch?: boolean}` (`src/driver.rs:513-519`).
86. `autoApprove` defaults to `false` at resolve time (`src/model.rs:352`).
87. `OrchestratorConfig` emission behavior undocumented (`src/model.rs:40-55`).
88. `OrchestratorActivation` wire values are correct in the doc (`:330-335`) — no drift.
89. Resolution order is correct (doc `:311`), but the doc omits that `access` on
    `spawnConfig`/`runAgentConfig` bypasses the merge chain (`src/model.rs:325-332`).

### 1.12 Narrative claims (D — delete or fold into C prose)

90. **Doc `:95` is half wrong** — `features.runAs` is on `/api/capabilities` (`src/api.rs:224`),
    but `attachCommand` is emitted by the run-panes endpoint (`:433`, `:451`, `:470`).
91. `/api/capabilities` also publishes `workflowVersion`, `supportedNodeTypes` (all 14), and
    `supportedEdgeOutcomes` (`src/api.rs:215-217`) — the machine-readable counterpart of the
    table the doc is missing.
92. **Doc `:38` overstates `useOrchestrator`** — it gates one call site
    (`src/runtime.rs:3837`); loop verdicts are deterministic and decide-node routing runs through
    `DecideConfig.model`.

---

## Part 2 — `docs/execution-model.md` vs `src/runtime.rs` / `src/storage.rs`

Doc last touched 2026-06-08; `runtime.rs` last touched 2026-07-23.

### 2.1 Cursor lifecycle (C — the epic's highest-value content)

- **D1.** `CursorState` has 16 fields (`src/runtime.rs:201-234`); the doc lists 13 and omits the
  three that matter: `var_map` (`:223`, the per-cursor variable scope — the doc never says
  cursors carry variables), `call_stack` (`:225`), and the two concrete `last_branch_*` fields
  (`:227-229`).
- **D2.** **The cursor state enum is entirely wrong.** Doc claims
  `Running | WaitingAtCollector | WaitingInteraction | Done | Cancelled | Failed` plus a
  "Released" pseudo-state (`:37`, `:43-58`). `CursorRuntimeState` is exactly four variants:
  `Runnable`, `Running`, `WaitingCollector`, `WaitingApproval` (`src/runtime.rs:157-165`).
  `Runnable` — the state the whole scheduler filters on (`:2874`, `:3129`) — is absent from the
  doc. `WaitingInteraction` does not exist. `Done`/`Cancelled`/`Failed` are not cursor states:
  terminated cursors are *removed from the vector* (`:4912`, `:6314`, `:3372`). Terminal-ness is
  a separate enum, `CursorTerminalStatus` (`:167-174`), used only for collector arrival records.
- **D3.** **Five cursor-creation sites, four undocumented:** initial (`:2205-2222`), split
  fan-out (`:5851-5868`), **ephemeral parallel-batch item cursors** that never enter
  `active_cursors` (`:5509-5526`), rehydration synthesis on resume (`:2266-2291`), and collector
  release resurrection (`:6045-6051`).
- **D4.** Split children **clone the full parent scope** — `last_output`, `loop_counters`,
  `visit_counters`, `var_map`, `call_stack`, `last_branch_*` are deep-copied per child
  (`:5859-5865`). Copy-on-split; divergent sibling writes never reconverge.
- **D5.** **Call frames are absent from the doc entirely.** `CallFrameState` (`:176-199`) snapshots
  the caller's whole scope plus a separate `subflow_results` namespace. Push wipes the cursor's
  `last_output`/counters/`last_branch_*` and *replaces* `var_map` (`:3502-3543`); pop restores five
  of the six snapshotted fields (`:4718-4726`) — **not** `last_output`, which is overwritten with the
  subflow's exit-node output and, together with `insert_result_for_cursor_index` on the call node
  (`:4740`), constitutes the subflow return mechanism. `parent_last_output` (`:186`) is written at
  `:3523` and read nowhere in production. Depth cap default 10 (`:3465`, `:3481-3484`).
  *(Corrected 2026-08-26 during the to-issues readiness gate: an earlier revision said "pop restores"
  unqualified, which inverts how a subflow returns a value.)*
- **D6.** **Cursor scope selects which result map is read/written** — `results_for_cursor`
  (`:2553-2562`), `insert_result_for_cursor_index` (`:2639-2655`), `var_map_for_cursor`
  (`:2628-2637`). The doc's flat `nodeResults{}`/`variables{}` model (`:140-141`) is wrong for
  any run containing a subflow.
- **D7.** **"Traverses the graph depth-first" (doc `:11`) is false.** `execute_workflow`
  (`:2823-3029`) drains immediately-resolvable cursors synchronously (`:2843-2856`), dispatches
  every `Runnable` runner-kind cursor concurrently into a `JoinSet` (`:2879-2944`), then
  `select!`s on completion / approval / a 250 ms tick (`:2993-3028`). There is no DFS anywhere.

### 2.2 Checkpoint semantics (P field list + C mechanism)

- **D8.** `RuntimeCheckpoint` has **29 fields** (`src/runtime.rs:558-608`); the doc lists 8.
  Behaviourally significant omissions: `batch_item_results` (`:571`, mid-batch resume state,
  persisted after every item at `:5225`), `output_hashes` (`:591`, the stagnation detector),
  `queued_approvals` (`:583` — approvals are a **queue** serialized one at a time, `:3631-3662`,
  not the doc's single `pendingApproval`), and `total_executed`/`max_total_steps`/
  `max_visits_per_node` (`:589`, `:601-602` — limits are frozen into the checkpoint at start).
- **D9.** `split_families` is a `BTreeMap` (`:579`); `collector_barriers` is a `BTreeMap` with a
  **custom serde impl** (`:344-437`) flattening the composite key, plus a legacy string-key
  parser (`:332-342`).
- **D10.** `checkpoint.loop_counters` / `visit_counters` are **vestigial** — nothing in the
  execution path writes them (`prepare_cursor_visit` increments the cursor's, `:3288-3325`).
  Permanently empty for any run started by current code.
- **D11.** **Checkpoint persistence has a dedup guard** the doc omits: a `djb2` content hash with
  `updated_at` blanked, skipping the write entirely on a match (`:7020-7061`).
- **D12.** Resume reads far more than "the last checkpoint" (doc `:147`): terminal-state and
  already-active rejections, tmux-invocation backfill for legacy rows (`:1443-1478`,
  `:3393-3421`), then three reconciliation passes (`:2259-2313`, `:2315-2344`, `:2366-2443`) and
  an ambiguity-resolving approval restore (`:3664-3757`).
- **D13.** `restart_from` (`:1480-1618`) does far more than "new run with incremented epoch":
  drains the live executor, resets to exactly one cursor seeded from the global `var_map`,
  **clears all split families and collector barriers**, deletes results for all graph
  descendants, marks survivors `stale = true`, mints a new `run_id`, and marks the old run
  `Restarted`. Stale predecessors emit `sys_warn` (`:3822-3833`) and are prefixed
  `[preserved from prior run]` in `{{all_predecessors}}` (`:7145`).
- **D14.** **There is no checkpoint versioning at all** — no `version` field on
  `RuntimeCheckpoint`; forward-compat rests entirely on `#[serde(default)]`.
- **D15.** `upgrade_run_workflow_json` migrates the **workflow snapshot**, not the checkpoint
  (`src/storage.rs:1328-1372`). Related invariants the doc omits: `workflow_json` and
  `stream_token` are **frozen at first INSERT** (`:252-266`) — this is what makes a Run's
  snapshot immutable; new rows are born canonical (`:242-244`); `get_run` migrates on read in
  memory only (`:388-393`).

### 2.3 Collector / join semantics (C)

- **D16.** Barrier keying is `(scope, collector_id, execution_epoch)` (`src/runtime.rs:267-294`),
  not one barrier per collector. `scope` is `"root"` or the innermost call frame's `frame_id`
  (`:2047-2057`) — the same collector in two concurrent subflow calls gets two barriers.
- **D17.** **"Expected inputs" are merge keys, not cursor arrivals** — `edge.label` falling back
  to `edge.from` (`:1925-1943`). The barrier builder dedupes duplicate merge keys into a set behind
  a `tracing::warn!` (`:1933-1940`), but that path is unreachable from a validated document:
  duplicate collector input keys are an **error**-severity validation issue computed by the same
  rule (`src/model.rs:2005-2020`, and item 33), and run start rejects any error-severity issue
  (`src/api.rs:751-766`). Duplicate arrivals are dropped (`:2023-2045`).
  *(Corrected 2026-08-26 during the to-issues readiness gate: an earlier revision said duplicates
  "silently collapse the required count" with no mention of the validation gate, contradicting this
  audit's own item 33.)*
- **D18.** **Failed and timed-out cursors also arrive at the barrier** (`:6251-6284`) — this is
  what lets `best_effort_continue` release a barrier where a branch died. The doc gives no hint.
- **D19.** The `{inputs, summary}` shape is undocumented (`:6009-6018`): `inputs` is a **keyed
  object**, not an array; `summary.total` is the *required* count, alongside per-status tallies.
  *(Corrected 2026-08-26 during the to-issues readiness gate: an earlier revision said `total` can
  differ from `inputs.len()` when merge keys collide. It cannot. Release requires every required key
  to have arrived (`:5981-5987`) and every arrival is keyed by an inbound edge of that collector, so
  the two sets are equal at construction; a collision collapses both the `BTreeSet` and the
  `BTreeMap` by the same key, and is in any case an error-severity validation issue — see item 33.)*
- **D20.** **"Aggregated and available via `{{all_predecessors}}`" (doc `:101`) is misleading** —
  for the node after a collector that resolves to the single collector JSON blob, not the branch
  outputs. The natural accessors are `{{previous_output}}`, `{{<collectorId>}}`, or
  `{{node:<collectorId>.parsedOutput.inputs.<key>.output}}`.
- **D21.** **Representative selection is lossy and undocumented.** The first still-live waiter
  wins (`:6041-6045`); all other waiters are **deleted** (`:6091-6098`). With no survivor it
  resurrects the *first terminal arrival's* snapshot, whose own struct comment flags the hazard:
  "Its variable snapshot may be stale if another branch mutates globals after capture"
  (`:446-449`). Losing branches' variable writes are silently discarded — stated nowhere.
- **D22.** Barriers are **re-entrant** (this is what makes a loop through a collector work,
  `:1958-1971`) and garbage-collected (`:1973-2021`).
- **D23.** **A run where every cursor sits at a collector fails hard** (`:2963-2979`), plus a
  second stall backstop (`:2444-2482`). Neither is in the doc.
- **D24.** **`parallel_batch.collectorVar` ordered-list contract** (`:5295-5312`): ordering holds
  because results accumulate in a `BTreeMap<usize, Value>` read via `.values()` (`:5132`, `:5219`,
  `:5276`) — ascending index, independent of completion order. The written value is a
  **JSON-encoded string**, not a JSON array (`:5300`). Scoping: always the parent cursor's
  `var_map`, and additionally the global map **only** when the call stack is empty (`:5301-5311`).

### 2.4 Variables and template substitution (P list + C scoping)

- **D25.** `{{node_name:field}}` (doc `:67`) does not exist; the doc lists 4 of 10 real tokens
  and invents one. Full set: `src/runtime.rs:7063-7153`.
- **D26.** `{{branch_origin}}`/`{{branch_choice}}` are set only on a `"branch"` transition
  (`:4651-4654`, `:4875-4878`), collapse to `""` when unresolved, and are wiped on subflow entry
  and restored on exit (`:3541-3542`, `:4723-4724`).
- **D27.** **Stringification is lossy and every variable is a string at rest.** `var_map` is
  `BTreeMap<String,String>` (`:223`); `value_to_template_string` (`:7156-7162`) maps `Null` → `""`,
  `String(s)` → unquoted `s`, everything else → compact JSON. Missing lookups silently substitute
  `""` (`:7092`, `:7115`). There is no typed variable store — contrary to what `CONTEXT.md`
  implies. *(This is the gap `typed-contracts` closes.)*
- **D28.** Templates are substituted in **six** places, not one: task prompt (`:3798-3810`),
  decide prompt via a shadow `var_map` plus a second raw pass (`:4200-4223`), subflow input
  bindings (`:3491-3515`), batch `itemsBinding` (`:5099-5109`), batch item body (`:5480-5497`),
  and node preview (`:1770-1782`).
- **D29.** **Two-map variable resolution with a selection rule**, hidden by the doc's flat
  `variables{}` (`:141`). `var_map_for_cursor` (`:2628-2637`) has exactly two branches: it returns
  `checkpoint.var_map` when the cursor map **and** the call stack are both empty, and
  `cursor.var_map` otherwise. It does **not** walk a cascade — there is no fallback from a
  non-empty cursor map to the checkpoint map. Subflow entry saves the parent map into the call
  frame and *replaces* `cursor.var_map` wholesale (`:3487-3542`); exit restores it (`:4691-4724`),
  so nested frames hold several latent parent maps that no lookup consults. A zero-variable
  subflow therefore sees an empty scope rather than inheriting one. There is no mid-run variable
  write path other than `collectorVar`.
  *(Corrected 2026-08-25: an earlier revision of this entry described a "three-level scope",
  which the two-branch helper does not implement.)*
- **D30.** `previous_output` has a second consumer the doc never mentions: `skipCondition`
  (`:2702-2733`). Regexes are pre-compiled per subflow scope at run start, and an invalid one
  fails the whole run before any node executes (`:2593-2606`, `:2797`).

### 2.5 Node execution paths (C)

- **D31.** The doc knows 4 node kinds; there are 14.
- **D32.** **Two dispatch tiers, neither documented.** `is_runner_node_kind` (`:2086-2098`) —
  `{Task, Decide, Spawn, Send, Wait, Capture, Kill, RunAgent}` — goes into the concurrent
  `JoinSet` (`:2936-2942`). Everything else is handled **synchronously inside the scheduler
  loop** (`:3161-3249`), including `handle_parallel_batch_node`, which runs its entire fan-out
  to completion while blocking that loop.
- **D33.** **Decide nodes bypass the whole task pipeline** — `run_cursor_task` short-circuits on
  entry (`:3785-3795`), so decide gets no orchestrator refinement, no retries, no
  `continueSessionFrom`, no `agent_defaults` merge; a hardcoded `DEFAULT_AGENT` (`:4027`) with
  only `decideConfig.model` (`:4029-4034`), always `attempts: 1` (`:4103`).
- **D34.** **Decide routing is two-stage, both stages undocumented.** Outcome selection
  (`:4366-4431`): structured `{"outcome": …}` wins; a structured label not in `outcomes` fails
  **without** falling back to prose (`:4379-4381`); otherwise exact trimmed match, then a
  `\bword\b` scan preferring earliest offset then longest label, with same-offset ties rejected
  as ambiguous. Edge selection (`:6383-6444`): on no matching edge it emits `workflow_error` but
  **degrades to the plain success edge** rather than failing (`:6436-6443`).
- **D35.** **The pane/tmux node family is absent from the doc.** Dispatch at
  `src/tmux_exec.rs:292-373`; spawn (`:749-802`), send (`:804-836`), wait (`:838-879`, timeout
  sets `exit_code = -2`), capture (`:881-916`), kill (`:918-…`, **enforces run ownership**,
  `:937-939`). Target resolution goes through `resolve_pane_target` (`src/tmux_exec.rs:2454-2468`),
  which parses the configured target (or a pane alias carried in `previous_output`) via
  `tmux_tools_core::target` and then gates it on `owned_tmux_targets` — a run may only address panes
  it owns.
  *(Corrected 2026-08-26 during the to-issues readiness gate: an earlier revision said target
  resolution goes through the per-run `active_panes` registry. It does not. `resolve_active_pane`
  (`src/runtime.rs:1221-1245`) — which supports pane keys, raw targets, the `active`/`current`
  aliases only when exactly one pane is registered, and node-id prefix matching by highest sequence
  — is reached only by `resolve_reused_pane` (`src/tmux_exec.rs:1494-1512`) for `continueSessionFrom`
  pane reuse, and by the HTTP pane-context endpoints (`src/api.rs:356`, `:362`, `:1395`). The two
  maps are distinct: `active_panes` at `src/runtime.rs:874`, `owned_tmux_targets` at `:876`.)
- **D36.** **The orchestrator branch fallback is dead code** — `chosen` is unconditionally seeded
  from `branch_edges.first()` (`:6528`) before the `chosen.is_none()` guard (`:6541`), so
  `run_orchestrator_branch` (`:7444`) is unreachable. Real behavior: the first branch edge whose
  condition matches wins; **if none match, the first branch edge is silently taken.** There is no
  "no condition matched" outcome, and no signal distinguishes a defaulted route from a matched one —
  a `branch_decision` event is emitted either way (`:6570`), carrying the same `chosenBranch` and
  `chosenLabel` fields.
  *(Corrected 2026-08-26 during the to-issues readiness gate: an earlier revision said no event is
  emitted on this path. One is; what is absent is any field that marks the route as defaulted.)* *Filed as ISSUE-260826-0004-01; docs document the real behavior.*
- **D37.** **The `branchChoice` capability exists but has no runtime effect.** `AgentCapabilities`
  declares `branch_choice: bool` (`src/driver.rs:96`) under `rename_all = "camelCase"`, registry
  capabilities copy it (`:118`), the Claude driver advertises it `true` (`:733`) and two others
  `false` (`:866`, `:1047`), and `/api/capabilities` serializes it (`src/api.rs:199-216`). What is
  false is the doc's claim at `:118` and `:233` that it *gates* orchestrator branch selection: no
  code reads the flag on the routing path, and the path it would gate is unreachable anyway (D36).
  The docs must record an advertised-but-unconsumed capability, not an absent one.
  *(Corrected 2026-08-25: an earlier revision of this entry claimed no such symbol existed. It was
  produced by a camelCase grep that could not match the snake_case field.)*
- **D38.** **Loop decisions are deterministic-only** (`:6485-6524`); the `loop_decision` event is
  emitted with `deterministic: true` hardcoded (`:6496`). `loopMaxIterations` defaults to 5
  (`:6447`); the loop condition is consulted only when **both** `loop_condition` and
  `parsed_output` are present, so a loop node with no JSON output silently never continues.
- **D39.** **Splits fan out over `Success` edges** (`:5738-5743`), never use the orchestrator, and
  "execute" only as a synthetic system result (`:5756-5774`).
- **D40.** Orchestrator prompt refinement is gated on `use_orchestrator && !last_output.is_empty()`
  (`:3837`) — the **first node of every run is never refined** — and it is *workflow*-level, not
  per-node as the doc claims at `:70`/`:226`.
- **D41.** **Budgets are agent-CLI flags, not runtime guards.** `maxBudgetUsd`/`maxTurns` become
  `--max-budget-usd`/`--max-turns` (`src/driver.rs:767-773`); the runtime never measures spend or
  turns, and `ErrorMaxBudget`/`ErrorMaxTurns` are *parsed back* from agent output.
- **D42.** Doc's task-node steps 4/5/8 are stale: the driver method is `build_session_args`
  (`src/driver.rs:611-612`); there is **no schema validation step** — `outputSchema` only builds a
  prompt hint (`src/runtime.rs:7179-7201`), and native schema passing is hardcoded off
  (`:3910-3911`).
- **D43.** Approvals are queued, serialized one at a time, and re-bound on resume
  (`:3608-3662`). Rejection with no `Reject` edge is a **terminal cursor failure** (`:5065-5077`);
  a dropped channel is treated as rejection (`:4934-4937`).
- **D44.** Node preview / dry-run is undocumented (`:1749-1843`).

### 2.6 Failure and termination (C)

- **D45.** **There is no failure edge outcome.** `WorkflowEdgeOutcome` is exactly
  `{Success, Reject, Branch, LoopContinue, LoopExit}` (`src/model.rs:215-221`). Node failure is
  **always terminal for the cursor** (`src/runtime.rs:6228-6327`). *(This is what `condition-ast`
  adds; `CONTEXT.md`'s "Failure Outcome" term describes the target state.)*
- **D46.** Failure is classified into three outcomes — aborted, timeout, failure (`:4555-4593`);
  whether the run dies depends on split-family membership (`:6286`).
- **D47.** Split failure policies (`:6287-6313`): `BestEffortContinue` no-ops; `DrainThenFail`
  sets `force_failed`; `FailFastCancel` additionally cancels every sibling. Policy applies to
  **all** families a cursor belongs to — nested splits stack.
- **D48.** Two undocumented run-killers: **stagnation detection** (three consecutive identical
  outputs abort the run, `:2670-2681`, `:4595-4619`) and the `anyhow` backstop (`:3034-3112`).
- **D49.** **Limit violations produce `aborted`, not `failed`** (doc `:198`, `:244` both wrong).
  Exceeding `max_total_steps`/`max_visits_per_node` aborts the run whole and clears all cursors
  (`:3262-3316`). Defaults are 50 / 10, which the doc never states.
- **D50.** **`paused` is a dead run status** — `RuntimeStatus::Paused` is never assigned in
  production code (only in `#[cfg(test)]`); it is read only as a resume-eligibility predicate
  (`:1452`, `src/api.rs:1412`). A run blocked on approval stays `running`.
- **D51.** `cursor_cancelled` fires for **every** terminal cursor removal (`:6315-6323`), not just
  failure-policy cancellation as doc `:184` says.
- **D52.** **PTY interaction does not pause the run** (doc `:220`) — it blocks only the calling
  task's receiver; sibling cursors keep executing (`:7276-7383`).
- **D53.** Destructive-blocklist matches do **not** always escalate — `escalate_or_fallback`
  auto-denies when no interaction channel is attached (`src/tmux_exec.rs:1822-1849`).

### 2.7 Runtime surface the doc never mentions (C)

- **D54.** Subflows / `Call` nodes — `handle_subflow_node` (`:3445-3567`), exit-node resolution
  requiring exactly one terminal node when `exitNodeId` is omitted (`:3569-3596`),
  `complete_subflow_if_at_exit` (`:4672-4898`), events `subflow_start`/`subflow_done`.
- **D55.** Parallel batch — `handle_parallel_batch_node` (`:5081-5448`): concurrency clamped to
  `[1, 32]`, mid-batch resume by re-validating checkpointed items by value (`:5137-5142`), item
  panics caught via `catch_unwind` (`:5531-5557`), and the batch succeeds only when **every**
  item succeeded (`:5315`).
- **D56.** Run security / `run_as` / tmux invocation, including the privileged-unlock gate on
  `POST /api/runs` (`src/api.rs:743-749`, `:778-796`).
- **D57.** `RunRegistry` in-memory state (`:902-1350`) — abort tokens with a drain timeout, the
  `active_panes` registry, `owned_tmux_targets` for kill authorization, the persist-hash memo.
- **D58.** Session persistence is recomputed **per dispatch** over the active (possibly subflow)
  graph and cursor-scoped results (`:2061-2077`, `:2919-2920`), not pre-scanned once as doc
  `:156-161` says.
- **D59.** Terminal pane cleanup and stale-session reaping (`:6695-7011`), including
  `pane_candidates`' trusted-target guard against pane-id injection from node output (`:618-728`).
- **D60.** Interrupted-run recovery (`:745-760`, `src/storage.rs:656`, `src/api.rs:166`).
- **D61.** Execution-log persistence and the `log_saved` event (`:535-556`, `:6640-6643`).
- **D62.** **Event vocabulary is incomplete.** Missing from the doc's table (`:168-189`):
  `subflow_start`, `subflow_done`, `orchestrator_start`, `orchestrator_done`, `orchestrator_warn`,
  `loop_max_reached`, `sys_warn`, `workflow_warn`, `log_saved`. `RuntimeEvent` also carries a
  monotonic `seq` assigned by the DB append (`:57-58`, `:7014-7015`) — what makes the SSE stream
  resumable, and unmentioned.

---

## Part 3 — Cross-cutting

- **X1. `CONTEXT.md` drifts in the opposite direction.** It asserts schema `version: 5` (`:7`)
  against `WORKFLOW_SCHEMA_VERSION = 4` (`src/model.rs:10`); a nested `all`/`any`/`not` condition
  AST with `onMissing` against the flat `{field, operator, value}` that exists; and
  `Assignment`/`assignVars`, `Profile`/`profiles.json`, `Script Node` and `Failure Outcome`, none
  of which exist in `src/`. **This is by design** — the glossary is the initiative's target state,
  seeded by RDMP-260815-2009-01, and `typed-contracts` makes it true. Recorded here so the
  contradiction is not re-discovered as a defect.
- **X2. `docs/api-reference.md:24`** advertises `"supportedNodeTypes": ["task","approval","split",
  "collector"]`; `src/api.rs:216` returns all 14. A false claim about a live endpoint.
  *(Corrected 2026-08-25: an earlier revision cited `:23`, which is the `workflowVersion` line.)*
- **X3. `docs/README.md:20`** links to `agent-improvements-plan.md`, which does not exist.
- **X4. Node-kind coverage across all docs**, for scope calibration: `docs/workflow-schema.md` 4,
  `ARCHITECTURE.md` 9, `docs/backend.md` 10, `docs/execution-model.md` 7, `README.md` 4,
  `docs/architecture-overview.md` 1, `CLAUDE.md` 0. Only `/api/capabilities` reports all 14.
  `ARCHITECTURE.md:289-296` and `docs/backend.md:60` both present a four-kind list as complete,
  which makes them false rather than merely partial; tracked as ISSUE-260826-0240-01.
  *(Corrected 2026-08-25: an earlier revision counted ARCHITECTURE.md as 10 and backend.md as 9.
  Recount by exhaustive token sweep gives 9 and 10 respectively.)*
- **X5. The existing guard is a blacklist, not a verifier.** `scripts/check-canonical-v4-docs.sh:8`
  greps five hardcoded v3 phrases. It never reads `WORKFLOW_SCHEMA_VERSION` and would pass green
  if every doc said v2, v5, or nothing. Wired into CI at `.github/workflows/canonical-v4-docs.yml`
  and `justfile:71-73`.
- **X6. Only two of six bundled templates are pinned to the schema by tests** —
  `src/model.rs:6074` (`epic-dev.json`) and `:6194` (`multi-agent-plan-implementation.json`).
  The other four and `workflows/Test Workflow.json` are unpinned. Node kinds appearing in **no**
  template: `subflow`, `spawn`, `send`, `wait`, `capture`, `kill`.
