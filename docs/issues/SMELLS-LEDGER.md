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
| src/api.rs:1441-1610 Divergent Change | 1 | 2026-07-19 | open |
| src/model.rs:1205-1235 Speculative Generality | 1 | 2026-07-19 | open |
| ui/src/lib/stores/workflowStore.svelte.ts:104-108 Duplicated Code | 1 | 2026-07-19 | open |
| src/driver.rs:648-674 Duplicated Code | 1 | 2026-07-19 | retired: not independently actionable - duplicate branch collapses as a byproduct of the read-only-enforcement fix in the same if/else chain |
| src/api.rs:1544-1547 Mysterious Name | 1 | 2026-07-19 | retired: duplicate key of src/api.rs:1498 (same identifier `termination`, same rename) |
| ui/src/app/AppShell.svelte:201 Duplicated Code | 1 | 2026-07-19 | fixed: S8 (promoted LOW -> shared runLifecycle(label, kickoff) helper; collapses startRun/resumeRun/restartFromNode triplication) |
| ui/src/lib/stores/workflowStore.svelte.ts:110 Repeated Switches | 1 | 2026-07-19 | fixed: S9 (promoted MEDIUM -> OutsideNodePatch carries prompts[]; deletes two switches + parallel interface, rider on M17) |
| `src/model.rs:990` + Speculative Generality | 1 | 2026-07-19 | open |
| `ui/src/lib/stores/workflowStore.svelte.ts:401` + Repeated Switches | 1 | 2026-07-19 | retired: duplicate key of workflowStore.svelte.ts:110 (same CompoundPromptField smell, filed at one of its switch sites) |
| `ui/src/lib/stores/workflowStore.svelte.ts:909` + Speculative Generality | 1 | 2026-07-19 | open |
| `src/api.rs:1498` + Mysterious Name | 1 | 2026-07-19 | open |
