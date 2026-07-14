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
