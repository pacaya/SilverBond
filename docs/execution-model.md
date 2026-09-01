# Execution Model

The runtime module (`src/runtime.rs`) is the authority for how a workflow Run executes. Storage owns checkpoint persistence and workflow-snapshot migration (`src/storage.rs`). This document explains mechanism in prose — not generated catalogs. For node-kind shapes, see the generated `## Node catalog` in `docs/workflow-schema.md`. Terminology follows `CONTEXT.md`; unmarked entries there describe the engine as it is today.

## Run and cursor lifecycle

A Run is one execution of a workflow against a frozen snapshot. The client submits via `POST /api/runs`; the backend validates at run start, then `build_initial_checkpoint` seeds a `RuntimeCheckpoint` with a single Cursor at the entry node and `execute_workflow` drives the scheduler loop until every Cursor is gone or the Run reaches a terminal Run Status.

A Cursor is an independent execution pointer inside a Run. It carries its own variable scope (`var_map`), call stack (`call_stack`), loop and visit counters, branch-decision markers, split-family membership, and the Cursor Runtime State that the scheduler consults. Finished Cursors are removed from `active_cursors` rather than marked terminal — a checkpoint from a completed or failed Run will never list a Cursor in a failed or succeeded state. Terminal-ness is a separate concept: `CursorTerminalStatus` (`success`, `failure`, `timeout`, `cancelled`) is recorded only on Collector Barrier arrivals, never on the Cursor itself. The `cancelled` tally in collector summaries is always zero because nothing in production constructs that status.

The Cursor Runtime State has exactly four variants — `Runnable`, `Running`, `WaitingCollector`, and `WaitingApproval` in `CursorRuntimeState` (serialized as `runnable`, `running`, `waiting_collector`, `waiting_approval`). There are no terminal variants. The scheduler dispatch filter requires both `Runnable` and `cancel_requested == false` (`execute_workflow`); a Cursor in `running` is already inside the concurrent task set, one in `waiting_collector` is blocked at a Collector Barrier, and one in `waiting_approval` is parked on the Approval Queue. Terminal removal is not a complete event stream: `handle_terminal_cursor_status` removes the Cursor and emits `cursor_cancelled` on the failure/timeout path only. `finish_cursor`, the terminal path through `handle_skipped_task`, and the non-representative-waiter deletion in `release_collectors_if_ready` all remove a Cursor without emitting that event.

**Where Cursors come from.** Counting by sites that mint a fresh cursor id via `new_cursor_id`, there are six production call sites: run start (`build_initial_checkpoint`), restart (`restart_from`), resume rehydration when `active_cursors` is empty (`rehydrate_checkpoint_for_execution`), split fan-out (`handle_split_node`, once per branch edge), parallel-batch item dispatch (`spawn_batch_item_task`, once per concurrent item), and collector release when no live waiter survives (`release_collectors_if_ready`). Counting by sites that construct a `CursorState` and register it on the Run's cursor list, there are six: run start, restart, resume rehydration, split fan-out, collector resurrection pushing a cloned snapshot, and collector continuation reconstructing and re-registering the representative under the same id. Parallel-batch item Cursors are the exception: `spawn_batch_item_task` builds a full `CursorState` and passes it into `run_cursor_task`, but that value never enters `active_cursors` — it is ephemeral for the duration of one batch item.

Split children are created by Copy-on-Split: each child deep-copies the parent's whole scope — `last_output`, counters, `var_map`, `call_stack`, and branch markers — so sibling writes never reconverge. Split membership is sticky: once enrolled in a Split Family, a Cursor keeps in-split failure treatment for the rest of the Run.

Run Status values are `running`, `paused`, `completed`, `failed`, `aborted`, and `restarted`. `paused` is never assigned outside tests — a Run blocked on approval stays `running`. Limit violations (`max_total_steps`, `max_visits_per_node`) abort the whole Run with `aborted`, not `failed`.

## The scheduler loop

Execution is not depth-first graph traversal. `execute_workflow` runs a scheduler loop: it drains Immediate Kind nodes synchronously via `process_immediate_cursors`, dispatches every Runner Kind that passes the dispatch filter above into a concurrent `JoinSet`, then `select!`s across task completions, approval resolution, and a 250 ms timer. Runner Kinds (`task`, `decide`, `spawn`, `send`, `wait`, `capture`, `kill`, `run_agent` per `is_runner_node_kind`) run concurrently when dispatched. Immediate Kinds (`approval`, `split`, `collector`, `parallel_batch`, `subflow`, `call` per `process_immediate_cursors`) resolve inside the loop itself. A Parallel Batch node's entire fan-out runs inside that loop and blocks it until every item completes; the batch node succeeds only when every item succeeded (`handle_parallel_batch_node`).

The split between Runner and Immediate Kinds has exactly one crossing. In `process_immediate_cursors`, when a Runner Kind node's skip condition fires (`should_skip_cursor_node`), the node is resolved inline via `handle_skipped_task` rather than being dispatched; only when the skip does not fire does the cursor fall through to the dispatch pass in `execute_workflow` and enter the task set. Presenting the two classes as cleanly separated would be false.

When every active Cursor sits at `waiting_collector` and no tasks are running, the Run fails hard with a workflow error. A second stall backstop (`fail_run_on_idle_unschedulable_cursors`) can also terminate a Run that cannot make progress.

## Call frames and result scoping

Subflows and `call` nodes enter a callee workflow through `handle_subflow_node`, which pushes a Call Frame onto the cursor's `call_stack`. The frame snapshots the caller's variable map, loop and visit counters, and branch markers into `parent_*` fields, carries a separate `subflow_results` namespace for callee Node Results, and records `parent_last_output` — a field written into every checkpoint carrying a call frame but read nowhere in production (a write-only vestige alongside the checkpoint-level `loop_counters` and `visit_counters` maps, which are also permanently empty for any Run started by current code).

Entry wipes the cursor's `last_output`, counters, and branch markers and replaces `var_map` wholesale rather than layering over it. A zero-variable subflow therefore sees an empty scope rather than inheriting the caller's map. Exit (`complete_subflow_if_at_exit`) pops the frame and restores five of the six snapshotted fields — counters, branch markers, and the variable map — but deliberately not `last_output`. Exit overwrites `last_output` with the subflow exit-node output and writes the call node's own Node Result into the caller's scope via `insert_result_for_cursor_index`. That pair is the subflow return mechanism: an author expecting `{{previous_output}}` after a call node to hold the pre-call output gets the subflow's result instead.

Variable resolution is a two-branch selection, not a cascade. `var_map_for_cursor` returns `checkpoint.var_map` only when the cursor's own `var_map` and `call_stack` are both empty; otherwise it returns `cursor.var_map`. There is no fallback from a non-empty cursor map to the checkpoint map. Nested frames hold parent maps that no lookup consults.

Node Results are shared between split siblings at root scope but private inside a subflow — a single fact implemented by two mechanisms. Split children clone the parent's call stack wholesale, and each Call Frame carries its own `subflow_results`. `results_for_cursor` returns the innermost frame's map or the run-global `all_results` and never both; `all_results_for_cursor` is a plain clone with no merge. Two branches of a split at root scope see each other's Node Results; the same two branches inside a subflow do not. Node Results are replaced on loop re-execution (`insert_result_for_cursor_index` is a plain map insert with no occupancy guard).

## Collector barriers

A Collector implements barrier semantics keyed by Barrier Key: `(scope, collector_id, execution_epoch)`, where `scope` is `"root"` or the innermost call frame's `frame_id`, falling back to its `call_node_id` for a legacy frame whose `frame_id` is empty (`collector_barrier_scope`). The same collector reached from two concurrent subflow calls gets two barriers rather than colliding.

Expected inputs are Merge Keys derived from inbound edge labels falling back to the source node id (`merge_key_for_edge`, `collector_required_inputs`). Two inbound edges sharing a key are a validation error (`src/model.rs` collector input validation), so a runnable workflow never has them; the barrier builder's dedupe into a set behind a log warning is reachable only if validation is bypassed. Duplicate arrivals at an already-recorded key are dropped (`insert_collector_arrival`).

Failed and timed-out Cursors also arrive at a barrier (`handle_terminal_cursor_status` registers arrivals before removing the cursor). That is what lets the default `best_effort_continue` Split Family policy release a barrier after a branch died.

The aggregate shape is a keyed `inputs` object beside a `summary` carrying the required count and per-status tallies (`release_collectors_if_ready`). A reader addresses a branch by its merge key, never by position. A barrier releases only when every required key has arrived; every arrival is keyed by an inbound edge of that collector, so `summary.total` equals the number of inputs whenever the aggregate is built. Barriers re-arm after release (`reset_released_collector_barrier`), which is what makes a loop through a collector work.

Representative selection is first-still-live-in-arrival-order, not "whichever branch won." `release_collectors_if_ready` picks the first waiter still present in `active_cursors`; every other waiter is deleted and its variable writes discarded silently. With no live survivor, the engine resurrects the first terminal arrival's snapshot (`representative_snapshot`), whose struct comment flags that the snapshot may be stale if another branch mutated globals after capture. Release is gated on arrival count alone and never checks liveness, so under the default split failure policy the all-dead path is reachable.

## Checkpoint, resume, and restart

The Checkpoint is the serialized full state of a Run — the sole authority for resume and restart (`RuntimeCheckpoint`). Beyond the cursor list and run-global `all_results`, it carries mid-batch item results (`batch_item_results`), stagnation-detector output hashes (`output_hashes`), the Approval Queue (`queued_approvals` plus the single active `pending_approval`), limits frozen at start (`max_total_steps`, `max_visits_per_node`) and the consumed global-step counter (`total_executed`), split families, collector barriers, and the Execution Log accumulated as the Run proceeds. Checkpoint-level `loop_counters` and `visit_counters` are vestigial: `prepare_cursor_visit` increments per-cursor counters only.

Persistence uses a content-hash dedup guard (`checkpoint_content_hash`, `persist_checkpoint`): a djb2 hash with `updated_at` blanked skips the write entirely when nothing changed. There is no checkpoint versioning — forward compatibility rests on serde defaults.

Resume is reconciliation, not merely reading the last row. `resume_run` rejects terminal or already-active Runs, backfills tmux invocation for legacy rows, then `rehydrate_checkpoint_for_execution` synthesizes a cursor when needed, normalizes `running` cursors back to `runnable`, and runs inconsistency passes that drop orphaned approval state (`drop_inconsistent_pending_approval`, `drop_inconsistent_waiting_approval_cursors`). Approval is re-bound through `restore_pending_approval` / `activate_next_approval`. Execution Epoch 0 checkpoints migrate to epoch 1 on resume without clearing barriers, which can strand arrivals keyed at 0.

Restart (`restart_from`) drains the live executor, increments Execution Epoch, reseeds exactly one cursor from the global `var_map`, clears all split families and collector barriers, deletes Node Results for all graph descendants, marks survivors `stale`, mints a new `run_id`, and marks the old Run `restarted`. It preserves `total_executed`, so the new Run inherits the consumed portion of the frozen `max_total_steps` budget. Stale predecessors emit `sys_warn` and are prefixed `[preserved from prior run]` in `{{all_predecessors}}`. Restart resets Execution Log header fields — run id, start and end time, duration, terminal reason, aborted flag — but leaves accumulated node executions, decisions, and transitions in place, so a restarted Run's persisted log still carries the prior Run's entries.

The Execution Log is accumulated on the checkpoint throughout the Run (`execution_log` on `RuntimeCheckpoint`) and separately persisted at finalize (`finalize_run` calls `save_execution_log` and emits `log_saved`). It is written as the Run proceeds via `node_executions`, `decisions`, and `transitions` pushes throughout execution.

Neither the Active Pane Registry nor the owned-target set appears in the checkpoint — both live on the in-memory `RunRegistry` run object. A resumed Run starts with both empty. For pane nodes the consequence runs through the ownership gate: the production tmux path always constructs `ActivePaneRegistration` (`tmux_exec.rs` runner), and `resolve_pane_target` gates on `owns_pane` — a membership test against the owned-target set. With an empty set after resume, a `send` or `capture` against a still-live pane is refused with a not-owned-by-this-run error rather than silently retargeting. Session reuse and the HTTP pane-context path both depend on the Active Pane Registry as described in §Pane kinds; with the rebuilt registry empty after resume, continuation fails outright and the HTTP path survives only through its persisted-checkpoint fallback.

## Pane kinds

Pane Kinds (`spawn`, `send`, `wait`, `capture`, `kill`) interact with tmux panes rather than LLM prompts. Their configured target (or a pane alias carried in the previous node's output) is parsed through the tmux-tools target parser, then gated on the Run's owned-target set: a Run may only address panes it owns (`resolve_pane_target`). `spawn` alone establishes that membership through `ActivePaneRegistration::register_owned_target`; control kinds only check it through `owns_pane` or `owns_session` before their respective update or clear of the Active Pane Registry.

The Active Pane Registry is a separate per-run map. `resolve_active_pane` recognizes registered keys, resolves `active`/`current` only when exactly one pane is registered, and picks the highest-sequence entry whose `cursor:node` key **ends with** the requested node id (`active_pane_key_matches_node`, `src/runtime.rs:911-916`) — the node id is the key's suffix, not its prefix. Session-continuing nodes (`continueSessionFrom` via `resolve_reused_pane`) consult only this registry. The HTTP pane-context endpoints (`resolve_run_pane_context`) try the registry first; when that lookup misses, they recover candidates reconstructed from persisted checkpoint Node Results and Execution Log entries and apply a distinct `active`/`current` rule via `match_pane_candidate` — current-node candidate when present, otherwise the last candidate — returning `PaneUnavailable` only when candidate recovery also fails. The registry does not resolve a pane node's target. `send`, `wait`, and `capture` each register their resolved target in the registry under their own node key after resolving it by other means. A pane `kill` clears registry entries for both its resolved target and its node key; a session `kill` clears only its node key. Neither path removes owned-target membership: that set has no removal path and persists until the Run's registry entry is torn down. The separation is about resolution, not writes: pane kinds write or clear the registry even though none of them reads it to find its target.

`spawn` registers in both the Active Pane Registry and the owned-target set. PTY interaction during task or `run_agent` execution blocks only the calling task's receiver; sibling cursors keep executing. It does not pause the Run.

## Failure and termination

There is no failure Edge Outcome — `WorkflowEdgeOutcome` is exactly `success`, `reject`, `branch`, `loop_continue`, and `loop_exit`. ADR-260815-2009-02 describes the forward addition of a sixth failure channel; it is not present behavior. A node failure always ends its cursor (`handle_terminal_cursor_status`). Outside any Split Family, that failure fails the whole Run and sets `cancel_requested` on every other cursor; the dispatch guard described above then suppresses any further dispatch for those siblings. Split-family membership is sticky as described above.

A failed task result is classified three ways: aborted, timeout, or failure. The aborted branch — a Run abort flag or `error_type == "aborted"` — short-circuits the whole Run to `aborted` and clears every Cursor before a `CursorTerminalStatus` is chosen; only timeout and failure become a `CursorTerminalStatus` on a Collector Barrier arrival. Whether the Run dies for those latter outcomes depends on split-family membership and policy. Split failure policies (`best_effort_continue`, `drain_then_fail`, `fail_fast_cancel`) apply across every family a cursor belongs to, so nested splits stack: `best_effort_continue` no-ops on sibling failure; `drain_then_fail` sets `force_failed`; `fail_fast_cancel` additionally cancels siblings and fails the Run.

Run-killers beyond per-cursor failure include limit violations (abort whole Run, `aborted` status), stagnation detection (three consecutive identical outputs on a run-global key of call-frame path plus node id aborts every cursor), the all-cursors-at-collector stall, and the `anyhow` backstop: when `execute_workflow` returns an error, `execute_workflow_to_terminal` calls `fail_workflow_after_error`, which fails and finalizes the Run. A panic or cancellation in the supervised task follows a separate `spawn_supervised_run` path that only clears the in-memory registry entry; the persisted Run can remain `running`.

Approval rejection with no `reject` edge fails the cursor; outside a split family that fails the Run (`handle_approval_resolution`). A dropped approval channel is treated as rejection.

## Decide nodes

Decide nodes bypass the task pipeline entirely on entry — `run_cursor_task` short-circuits to `run_decide_node` with no orchestrator refinement, no retries, no session continuation, and no agent-defaults merge. Routing is two-stage. Outcome selection (`select_decide_outcome`): a structured `{"outcome": …}` label outside the declared set fails without falling back to prose; otherwise exact trimmed match, then word-boundary scan. Edge selection (`select_next_decision`): the outcome is matched exactly against a branch edge label; an outcome with no matching branch edge is a validation error, so the emit-`workflow_error`-and-degrade-to-success arm in `select_next_decision` is unreachable from a validated document.

Branch routing's real default for non-decide nodes: when branch edges exist and parsed output is present, the first edge whose condition matches wins; if none match, the first branch edge is taken silently. There is no "no condition matched" outcome and nothing marks the route as defaulted — a `branch_decision` event fires either way with the same `chosenBranch` and `chosenLabel` fields. The `branchChoice` agent capability is advertised but never read on the routing path.

Loop decisions are deterministic only (`loop_decision` with `deterministic: true`). `loopMaxIterations` defaults to 5; the loop condition is consulted only when both `loop_condition` and `parsed_output` are present.

## Event vocabulary

`RuntimeEvent` carries an unconstrained `kind` string (wire name `type`), a flattened data map, and a monotonic `seq` stamped by `emit_event` when the event is appended to the database — the `seq` is what makes the SSE stream resumable. Event kinds are literals at scattered emission sites; there is no enum. The production runtime module sweep found 55 construction sites yielding 29 distinct kinds (test modules elsewhere use throwaway strings and are excluded).

| Event kind | Description |
|------------|-------------|
| `run_start` | Workflow execution begins |
| `run_resumed` | Run resumed from checkpoint |
| `node_start` | Node execution begins |
| `node_done` | Node execution completed |
| `node_retry` | Node execution being retried |
| `node_skipped` | Node skipped due to skip condition |
| `branch_decision` | Branch edge chosen (matched condition or silent first-edge default) |
| `loop_decision` | Loop continue/exit from deterministic condition |
| `loop_max_reached` | Loop node hit `loopMaxIterations` |
| `cursor_spawned` | New cursor created (split fan-out or parallel-batch item) |
| `collector_waiting` | Cursor waiting at collector barrier |
| `aggregate_merged` | Collector barrier released; carries the full keyed inputs object |
| `collector_released` | All required inputs arrived; collector proceeding |
| `approval_queued` | Approval node reached; queued |
| `approval_required` | Active approval waiting for user response |
| `cursor_cancelled` | Cursor removed on the failure/timeout terminal path |
| `transition` | Cursor moving to next node |
| `subflow_start` | Subflow or call entry |
| `subflow_done` | Subflow or call exit |
| `orchestrator_start` | Orchestrator prompt refinement begins |
| `orchestrator_done` | Orchestrator refinement completed |
| `orchestrator_warn` | Orchestrator refinement failed; using original prompt |
| `agent_interaction_required` | PTY prompt detected; waiting for human response |
| `agent_interaction_resolved` | Human responded to PTY interaction |
| `workflow_error` | Runtime error |
| `workflow_warn` | Recoverable workflow warning |
| `sys_warn` | System warning (e.g. stale predecessor outputs) |
| `log_saved` | Execution log persisted at finalize |
| `done` | Run completed (success, failed, or aborted) |
