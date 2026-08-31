# Smells ledger

Mutable work artifact -- advisory code smells accrued across review runs. Owned by the review cluster; not an issue record.

## Row grammar

One row per unique key (`file:line` + smell name). Columns:

| column | meaning |
|---|---|
| key | `` `file:line` + smell name `` (unique row key) |
| count | recurrence count -- increment on re-observation of the same key |
| last-seen | UTC date (`YYYY-MM-DD`) of the most recent observation |
| verdict | `open`, `retired: <reason>`, or `fixed: <ref>`; absent in older ledgers reads as `open` |

## Entries

| key | count | last-seen | verdict |
|---|---|---|---|
| `src/tmux_exec.rs:813` + Long Function | 0 | 2026-07-14 | retired: reviewer fix fails deletion test; legit lifecycle extraction owned by S2 |
| `src/tmux_exec.rs:812` + Duplicated Code | 0 | 2026-07-14 | fixed: S2 (promoted MEDIUM -> PaneGuard RAII) |
| `src/tmux_exec.rs:1347` + Duplicated Code | 0 | 2026-07-14 | fixed: S3 (promoted MEDIUM -> shared dispatch_pattern_match) |
| `src/tmux_exec.rs:1543` + dedup key embeds byte offsets | 0 | 2026-07-14 | fixed: S4 (promoted MEDIUM -> consumed-cursor delta-scan) |
| `src/tmux_exec.rs:2405-2413` + Primitive Obsession | 0 | 2026-07-14 | retired: category error - shell-fragment cannot be argv; cwd+inner already shell_quoted, single site |
| `src/api.rs:1455` + Primitive Obsession | 0 | 2026-07-14 | fixed: S7 (promoted LOW -> PANE_ALIAS_KEYS const + SessionName newtype, after H1+M3) |
| `src/app.rs:117` + Duplicated Code | 0 | 2026-07-14 | fixed: S8 (promoted MEDIUM -> crate::util::constant_time_eq) |
| `src/app.rs:157` + unsubscribe missing same_channel guard | 0 | 2026-07-14 | fixed: S9 (promoted LOW -> sender-threaded same_channel guard) |
| `src/driver.rs:44-45` + event_type expect panic risk | 0 | 2026-07-14 | fixed: S10 (promoted LOW -> exhaustive match returning &static str) |
| `src/driver.rs:743-752` + build_session_args silent drop | 0 | 2026-07-14 | fixed: S11 (promoted LOW -> fail-loud typed error, co-schedule with M7) |
| `src/runtime.rs:2228` + invalid skip regex silently swallowed | 0 | 2026-07-14 | fixed: S14 (promoted MEDIUM -> validate-at-submission + compile-once fail-loud) |
| `ui/src/lib/stores/workflowStore.svelte.ts:660` + setError per-call timer clears later messages | 0 | 2026-07-14 | fixed: S16 (promoted LOW -> single tracked timer handle, clear+reset per call) |
| `ui/src/features/runtime/RunPanel.svelte:30` + auto-scroll no at-bottom check | 0 | 2026-07-14 | fixed: S17 (promoted MEDIUM -> stickToBottom boolean + onscroll, co-impl with M14) |
| `ui/src/lib/api/client.ts:344` + seq-gap drops frames silent freeze | 0 | 2026-07-14 | fixed: S18 (promoted MEDIUM -> bounded gap-wait deadline + M4 reconnect escalation, after M4) |
| `ui/src/features/editor/flowNodes.ts:55` + node.kind.type no legacy guard | 0 | 2026-07-14 | fixed: S19 (promoted MEDIUM -> client-side normalizer at ingress, mirrors migrate_v2_node_to_v3_kind) |
| `src/app.rs:82` + Primitive Obsession | 1 | 2026-07-15 | fixed: S1 (promoted LOW -> documented threat-model assumption + cfg(test) reject-branch coverage) |
| `src/model.rs:985` + Speculative Generality | 1 | 2026-07-15 | fixed: S2 (promoted LOW -> reject nested catalogs in validate_workflow_input_bounds, with M8 root-vs-subflow distinction) |
| `ui/src/features/editor/InspectorPanel.svelte:66` + Duplicated Code | 1 | 2026-07-15 | fixed: S3 (promoted LOW -> delegate both runAs mutators to existing mergeConfig helper) |
| `ui/src/lib/types/workflow.ts:1071` + Duplicated Code | 1 | 2026-07-15 | fixed: S4 (promoted LOW -> recurse into subflows[*].nodes; actual site is workflow.ts:613, key line stale) |
| `ui/src/lib/stores/workflowStore.svelte.ts:754` + Positional coupling | 0 | 2026-07-19 | fixed: S5 (promoted LOW -> match-by-id with fail-safe no-op; branch unreachable from current emitter) |
| `ui/src/lib/stores/workflowStore.svelte.ts:455-457` + entry-guard OR-vs-AND | 0 | 2026-07-19 | fixed: S6 (promoted LOW -> REFUTED: shipped OR is correct; added entry-selection tests + comment, corrected M4/R3-4 record) |
| `playwright.config.ts:3-5` + committed test unlock hash / reuseExistingServer | 0 | 2026-07-19 | fixed: S7 (promoted LOW -> reuseExistingServer:false + per-run random secret via globalSetup; harness hygiene, not security) |
| src/api.rs:1441-1610 Divergent Change | 1 | 2026-07-19 | fixed: S1 |
| src/model.rs:1205-1235 Speculative Generality | 1 | 2026-07-19 | open |
| ui/src/lib/stores/workflowStore.svelte.ts:104-108 Duplicated Code | 1 | 2026-07-19 | open |
| src/driver.rs:648-674 Duplicated Code | 1 | 2026-07-19 | retired: not independently actionable - duplicate branch collapses as a byproduct of the read-only-enforcement fix in the same if/else chain |
| src/api.rs:1544-1547 Mysterious Name | 1 | 2026-07-19 | retired: duplicate key of src/api.rs:1498 (same identifier `termination`, same rename) |
| ui/src/app/AppShell.svelte:201 Duplicated Code | 1 | 2026-07-19 | fixed: S8 (promoted LOW -> shared runLifecycle(label, kickoff) helper; collapses startRun/resumeRun/restartFromNode triplication) |
| ui/src/lib/stores/workflowStore.svelte.ts:110 Repeated Switches | 1 | 2026-07-19 | fixed: S9 (promoted MEDIUM -> OutsideNodePatch carries prompts[]; deletes two switches + parallel interface, rider on M17) |
| `src/model.rs:990` + Speculative Generality | 1 | 2026-07-19 | open |
| `ui/src/lib/stores/workflowStore.svelte.ts:401` + Repeated Switches | 1 | 2026-07-19 | retired: duplicate key of workflowStore.svelte.ts:110 (same CompoundPromptField smell, filed at one of its switch sites) |
| `ui/src/lib/stores/workflowStore.svelte.ts:909` + Speculative Generality | 1 | 2026-07-19 | open |
| `src/api.rs:1498` + Mysterious Name | 2 | 2026-07-20 | open |
| `ui/src/lib/stores/workflowStore.svelte.ts:325` + Speculative Generality | 1 | 2026-07-19 | open |
| `ui/src/lib/stores/workflowStore.svelte.ts:147` + Duplicated Code | 1 | 2026-07-19 | retired: duplicate key of `ui/src/lib/stores/workflowStore.svelte.ts:104-108` (S7) — retarget-side of the same compound token grammar; S7 is the single owner |
| `src/api.rs:1375` + Divergent Change | 2 | 2026-07-20 | fixed: S1 |
| `src/runtime.rs:1-12785` + Divergent Change | 1 | 2026-07-20 | fixed: S2 |
| `src/model.rs:2061` + Feature Envy | 1 | 2026-07-20 | open |
| `src/driver.rs:291-305` + Primitive Obsession | 1 | 2026-07-20 | open |
| `ui/src/features/editor/InspectorPanel.svelte:1` + Divergent Change | 1 | 2026-07-20 | fixed: ISSUE-260723-0823-01 (four axes separated across ISSUE-260808-2000-07, -2022-04, -2022-11 and this record; capability/unlock hoist deliberately out of scope per M25/L20) |
| `ui/src/features/editor/InspectorPanel.svelte:789` + Duplicated Code | 1 | 2026-07-21 | fixed: ISSUE-260808-2000-07 (one component per control; run_agent/spawn/per-node consumers converted) |
| `src/model.rs:2247` + Shotgun Surgery | 1 | 2026-07-21 | open |
| `src/driver.rs:291` + Primitive Obsession | 1 | 2026-07-21 | open |
| `src/driver.rs:301` + Repeated Switches | 1 | 2026-07-21 | open |
| `src/api.rs:1601` + Divergent Change | 1 | 2026-07-21 | fixed: S1 |
| `src/api.rs:2205` + Mysterious Name | 1 | 2026-07-21 | open |
| `ui/src/lib/types/workflow.ts:511` + Shotgun Surgery | 1 | 2026-07-21 | open |
| `src/runtime.rs:1352` + Divergent Change | 1 | 2026-07-21 | fixed: S2 |
| `src/proc.rs:37` + Speculative Generality | 1 | 2026-07-22 | open |
| `src/proc.rs:14` + Speculative Generality | 1 | 2026-07-22 | open |
| `src/model.rs:110` + Duplicated Code | 1 | 2026-07-22 | open |
| `src/api.rs:5381` + Duplicated Code | 1 | 2026-07-22 | fixed: S4 |
| `src/runtime.rs:1376` + Speculative Generality | 1 | 2026-07-23 | open |
| `ui/src/features/editor/PaneNameField.svelte:7` + Mysterious Name | 1 | 2026-08-09 | open |
| `ui/src/features/editor/AccessProfileField.svelte:20` + Speculative Generality | 1 | 2026-08-09 | open |
| `ui/src/features/editor/EdgeInspector.svelte:14` + Middle Man | 1 | 2026-08-09 | open |
| `ui/src/features/editor/mergeConfig.ts:2` + Primitive Obsession | 1 | 2026-08-09 | open |
| `ui/src/features/editor/InspectorPanel.svelte:669` + Primitive Obsession | 1 | 2026-08-09 | open |
| `ui/src/features/editor/InspectorPanel.test.ts:363` + Duplicated Code | 1 | 2026-08-09 | open |
| `ui/src/features/editor/WorkflowInspector.svelte:17` + Speculative Generality | 1 | 2026-08-09 | open |
| `ui/src/features/editor/AgentConfigFields.svelte:20` + Data Clumps | 1 | 2026-08-09 | open |
| `ui/src/features/editor/InspectorPanel.svelte:250` + Middle Man | 1 | 2026-08-15 | open |
| `ui/src/features/editor/mergeConfig.ts:72` + Divergent Change | 1 | 2026-08-15 | open |
| `ui/src/features/editor/mergeConfig.ts:175` + Divergent Change | 1 | 2026-08-15 | open |
| `ui/src/features/editor/mergeConfig.ts:190` + Primitive Obsession | 1 | 2026-08-15 | open |
| `ui/src/features/editor/mergeConfig.ts:1` + Divergent Change | 1 | 2026-08-15 | open |
| `ui/src/features/editor/AgentConfigFields.svelte:56` + Mysterious Name | 1 | 2026-08-15 | open |
| `ui/src/features/editor/AgentConfigFields.svelte:48` + Duplicated Code | 1 | 2026-08-15 | open |
| `ui/src/features/editor/AgentConfigFields.svelte:58` + Primitive Obsession | 1 | 2026-08-15 | open |
| `ui/src/lib/utils/sectionUtils.ts:42` + Shotgun Surgery | 1 | 2026-08-15 | open |
| `.github/workflows/rust-tests.yml:3-13` + Duplicated Code | 1 | 2026-08-30 | open |
| `.github/workflows/rust-tests.yml:32` + Duplicated Code | 1 | 2026-08-30 | open |
| `.github/workflows/rust-tests.yml:15-18` + Speculative Generality | 1 | 2026-08-30 | open |
| docs/api-reference.md:24 Duplicated Code | 1 | 2026-08-30 | open |
| `tests/docs_catalog.rs:310` + Mysterious Name | 1 | 2026-08-30 | retired: names at the site read fine (example_id, yes_node, a_collect); the mild wart is a bare positional "split" passed as entry_node_id, which is positional coupling and cosmetic, not a naming defect |
| `tests/docs_catalog.rs:477` + Duplicated Code | 1 | 2026-08-30 | retired: real but cosmetic -- two near-identical panic arms in the catalog length assertion, no correctness stake; the larger four-way duplication in the adjacent set-equality assertion is the same class and is likewise not actioned |
| `tests/docs_catalog.rs:556` + Speculative Generality | 1 | 2026-08-30 | open (promoted -> ISSUE-260831-0626-01; mislabelled -- the site is a verbatim copy of a private validator predicate, not unused generality) |
| `tests/docs_catalog.rs:117` + Repeated Switches | 1 | 2026-08-30 | retired: load-bearing -- example_for and example_workflow match the same enum at different levels (kind data vs workflow scaffolding) and the second calls the first; two exhaustive matches are the compiler-enforced mechanism that makes adding a node type fail loudly in both places, which is this file's purpose |
| `tests/docs_catalog.rs:89` + Data Clumps | 1 | 2026-08-30 | retired: category error -- base_workflow's four params are the natural workflow constructor signature, not a clump; the nearby real nit (a repeated "node-catalog-example" literal across match arms) is Duplicated Code and cosmetic |
| docs/workflow-schema.md:1292 — Duplicated Code | 1 | 2026-08-30 | open |
| docs/workflow-schema.md:1346-1348 — Duplicated Code | 1 | 2026-08-30 | open |
| docs/workflow-schema.md:1451-1453 — Duplicated Code | 1 | 2026-08-30 | open |
| docs/workflow-schema.md:1496 — Primitive Obsession | 1 | 2026-08-30 | open |
| docs/workflow-schema.md:1603 Duplicated Code | 1 | 2026-08-30 | open |
| docs/execution-model.md:61 — Divergent Change | 1 | 2026-08-30 | open |
| `src/runtime.rs:14442` + Long Function | 1 | 2026-08-30 | open |
| `src/runtime.rs:14493` + Duplicated Code | 1 | 2026-08-30 | open |
| `src/runtime.rs:6357` + Speculative Generality | 1 | 2026-08-30 | open |
| `ARCHITECTURE.md:293-306` + Duplicated Code | 1 | 2026-08-31 | open |
| `ARCHITECTURE.md:291` + Mysterious Name | 1 | 2026-08-31 | open |
| `tests/docs_catalog.rs:382` + Duplicated Code | 1 | 2026-08-31 | retired: mechanism difference is essential -- WorkflowNodeType is a plain enum probed via a custom Deserializer intercepting deserialize_enum, NodeKind is internally tagged and must capture unknown_variant's expected list; the two harvests cannot be unified |
| `ui/src/features/editor/GraphEditor.svelte:270` + Duplicated Code | 1 | 2026-08-31 | open |
| `src/serde_wire_tags.rs:71` + Middle Man | 1 | 2026-08-31 | open |
| `ui/src/features/editor/InspectorPanel.test.ts:569` + Duplicated Code | 1 | 2026-08-31 | open |
| src/model.rs:3055 — Speculative Generality | 1 | 2026-08-31 | open |
| src/model.rs:2969 — Duplicated Code | 1 | 2026-08-31 | open |
| src/model.rs:6140 — Mysterious Name | 1 | 2026-08-31 | open |
| `templates/research-and-summarize.json:77` + Mysterious Name | 1 | 2026-08-31 | open |
| `templates/research-and-summarize.json:30` + Mysterious Name | 1 | 2026-08-31 | open |
| `tests/bundled_templates.rs:53` + Primitive Obsession | 1 | 2026-08-31 | open |
