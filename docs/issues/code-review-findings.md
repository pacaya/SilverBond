# Code Review Findings — Backlog

Consolidated, living backlog of adjudicated code-review findings produced by the multi-agent
plan-implementation workflow (Phase 4: two independent reviewers — Claude + OpenAI Codex — per
batch, with mutual cross-verification). **Auto-fix (Phase 5) was intentionally skipped** for both
refactorings below; these are handed back for manual triage and will be addressed in a separate
workflow.

This file has two parts:

- **Part A** — findings from the *tmux-only execution + run-as* refactoring (`flickering-fluttering-iverson`, 2026-06-08).
- **Part B** — findings carried over from the prior *mutable-kettle* refactoring (`we-need-to-sreer-mutable-kettle`, 2026-06-07).

> ⚠️ **Staleness note:** Part B's findings were written against the code *before* the Part A
> refactoring, which rewrote `src/tmux_exec.rs`, replaced the runtime's node runner (deleting the
> expectrl `SessionManager`/`src/session.rs`, `--print`/`call_llm`, and `PtyNodeRunner`), and reworked
> the `/api/sessions` surface. **Re-validate each Part B item against current `HEAD` before acting** —
> some are likely resolved or relocated. Known overlaps/changes are flagged inline. Notably Part B
> #25 (R2-3, `extract_after_prompt`) is the same defect as Part A **F9**, and Part B #26 (R2-4, the
> `registry_all_drivers` exact-count test) was failing on the prior consolidation but the current
> suite reports **0 failing** — verify it is genuinely resolved.

---

# Part A — `flickering-fluttering-iverson` (tmux-only execution + run-as)

Reviewed the post-merge staging tree (T1–T14: tmux-only agent execution, the `runAs` sandbox switch,
the 4-tier capture-pane interaction safety net, session reuse, observability, and the expectrl/PTY
removal). Two review batches (RB1 core execution & security; RB2 API/model/cleanup/frontend), each
dual-reviewed and fully cross-verified. **All 14 findings were confirmed by both reviewer families**
(zero disagreements; F13 is the only ambiguous one). No findings dropped.

**Totals: 14 unique findings — 1 CRITICAL, 3 HIGH, 7 MEDIUM, 3 LOW.**

## CRITICAL (1)

1. **F1 · src/runtime.rs:2633 (also 4906, 4938) · `runAs` sandbox bypass in decide/orchestrator one-shots** — `run_decide_node`, `run_orchestrator_refinement`, and `run_orchestrator_branch` call `run_tmux_oneshot` inside `spawn_blocking` **without** wrapping in `tmux_tools_core::with_invocation(ctx.run_invocation, …)`. Because the invocation override is thread-local, `resolve_invocation()` falls through to `TmuxInvocation::default()` (empty prefix, no socket). With `runAs` configured, classifier/orchestrator agents therefore run as the **privileged SilverBond backend user** (not the sandboxed `runAs` user) and on the **default tmux socket** instead of `-L <socket>` — breaking the core sandbox boundary the plan requires for *all* agents, and putting those panes on a different server than worker panes (breaking observability/attach/cleanup). `run_orchestrator_branch` doesn't even receive `ctx`, so the invocation can't currently be installed there. *Fix:* thread `ctx.run_invocation.clone()` into every one-shot path and wrap the blocking body in `with_invocation(inv, || run_tmux_oneshot(...))` exactly as `TmuxNodeRunner::run_node_with_interaction` does; add a regression test that asserts a `runAs` workflow with a decide/orchestrator node applies the invocation.

## HIGH (3)

2. **F2 · src/tmux_exec.rs:1184 (`escalate_or_fallback` 1243-1256) · Missing escalation channel auto-approves permission walls (fail-open)** — in the `PermissionRequest` branch, when not destructive and `auto_approve` is false, `escalate_or_fallback(..., "y")` returns the fallback `"y"` verbatim whenever `interaction` is `None` (the `run_tmux_oneshot` path and the plain `NodeRunner::run` path). So a permission prompt on any non-interactive path is **silently approved** even though auto-approve was off — the opposite of fail-safe. *Fix:* default the no-channel fallback to deny (`"n"`/empty); only send `"y"` on explicit `auto_approve` or a human response.
3. **F3 · src/runtime.rs:639 · Run-level interaction slot race** — `set_pending_interaction` stores a single `Option<oneshot::Sender<String>>` per run and overwrites any existing sender; `respond_interaction` is keyed only by `run_id`. Concurrent branches (ParallelBatch / parallel cursors) requesting permission at once collide — the later overwrites the earlier (first blocks/errors), and a response intended for one pane can be misdelivered to another. *Fix:* store pending interactions in a map keyed by a generated interaction id (include pane/session id in the event; the response endpoint resolves that id).
4. **F5 · src/tmux_exec.rs:1487 · Agent safety config silently dropped on the tmux path** — `build_agent_command` builds argv from `tmux_tools_core::agents::Registry::launch_argv` + only `extra_args`; it never calls the driver's `build_session_args`, so resolved `AgentConfig` fields — model, system prompt, max turns, max budget, **tool allowlists/disallowlists**, web-search toggles, resume args — do not reach the agent CLI. Fine-grained tool restrictions configured in a workflow can silently fail to apply. *Fix:* build argv from the driver using the resolved `AgentConfig` (or extend the registry launch path to render all dynamic safety/config fields); add tests for allow/disallow tool propagation.

## MEDIUM (7)

5. **F4 · src/runtime.rs:4850 · Interaction-event TOCTOU** — `escalate_agent_interaction` emits `agent_interaction_required` *before* it creates/registers the oneshot sender (~4862). A fast client can respond before `set_pending_interaction` installs the sender → response lost, agent thread waits forever. *Fix:* register the pending interaction before emitting the event; clear on emit failure/cancellation.
6. **F6 · src/tmux_exec.rs:1130 · Destructive blocklist only scans the capture delta** — the blocklist runs on `new_text` from `capture_delta` only; if tmux redraws a line or a destructive command spans captures, no single delta contains the whole expression, so the first-tier check can miss it (the cumulative `output_so_far` check runs only inside the `PermissionRequest` branch). *Fix:* scan a cumulative rolling window with deduped match keys before permission handling.
7. **F7 · src/tmux_exec.rs:1205 · Visual idle can end active work** — `poll_agent_interactive` treats any `CaptureProgress::observe` result, including plain `Idle` (default idle ~2s), as completion. An agent without a reliable `ready_regex` (e.g. the built-in Codex profile) that pauses mid-response is declared complete prematurely, before later permission/destructive prompts appear. *Fix:* require an explicit ready/until/sentinel signal for interactive agents; don't treat visual idle alone as success when no ready marker is configured.
8. **F8 · src/tmux_exec.rs:1759 · `wrap_keep_open` cannot keep the pane open** — emits `cd … && exec <cmd>; exec zsh -li`. The first `exec` replaces the shell, so the trailing `exec zsh -li` is unreachable after the command exits; short-lived commands close the pane before capture/name setup. *Fix:* drop the first `exec` for keep-open panes, e.g. `cd <cwd> && { <cmd>; }; exec zsh -li`; reserve `exec` for the close-with-command variant.
9. **F9 · src/tmux_exec.rs:1768 (impact src/runtime.rs:2786) · `extract_after_prompt` corrupts decide outcomes for multi-line/TUI prompts** — it matches the prompt per captured line with `line.contains(prompt_text)`; a multi-line decide prompt matches no single line, so it returns the entire `after` capture (echoed prompt + TUI chrome). `select_decide_outcome` then does `response.contains(outcome)` over that blob, and because decide prompts enumerate the outcome names, it matches the first listed outcome regardless of the agent's actual answer → silent branch misclassification. *(Same defect as Part B #25 / R2-3.)* *Fix:* anchor extraction on a runner-controlled sentinel/end marker (or line-set subtraction); make `select_decide_outcome` prefer an exact trimmed-line match and ignore the echoed prompt region.
10. **F11 · src/api.rs:125 · `build_attach_command`: no shell-escaping + socket mismatch** — interpolates `command.join(" ")`, `user`, `socket`, and `session_name` (all user-controlled) into a copyable shell command without quoting; and it always renders `tmux -L silverbond` even though runtime only installs a socket when `workflow.run_as` is present (`runtime.rs:1641`), so **no-`runAs` runs are created on the default socket while the advertised attach command points at `silverbond`** (attach fails). *Fix:* build the attach hint from the actual invocation used for the run; shell-quote every token and the session/socket; omit `-L` when there is no socket (or make runtime always use the `silverbond` default the API advertises).
11. **F12 · ui/src/features/editor/InspectorPanel.svelte:70-88 · Command-prefix editor corrupts the argv round-trip** — `runAs.command` is a verbatim `string[]` argv, but the inspector renders it with `.join(" ")` and writes it back with `value.trim().split(/\s+/)`. Any argument containing spaces/quotes/empties is silently rewritten (e.g. `["sudo","-u","Agent User"]` → `["sudo","-u","Agent","User"]`), so the backend no longer runs the configured prefix. The attach hint (`:83`) also ignores `command` precedence and always shows the `sudo -u <user>` form. *Fix:* use an argv-preserving editor (one arg per line / JSON array / quote-aware lexer); render the attach hint from the same parsed argv, preferring `command` when set.

## LOW (3)

12. **F10 · src/tmux_exec.rs:59 (called src/runtime.rs:1641) · Blocking login-shell subprocess runs on the async executor** — `build_tmux_invocation → resolve_tmux_bin` runs `[sudo -u <user> -H --] zsh -lic 'command -v tmux'` via synchronous `Command::output()`, invoked directly from async `execute_workflow` (not `spawn_blocking`), stalling a tokio worker for its duration (and `sudo` may block). *Fix:* run it inside `tokio::task::spawn_blocking`; cache the resolved path per `(user, socket)`.
13. **F13 · src/api.rs:1101 · `/api/sessions` is now an interrupted-run alias (ambiguous — deprecate/document)** — the reimplemented `list_sessions` only maps `db.list_interrupted_runs()` (`runId`→`id`); it does not enumerate tmux sessions/panes, and `/api/sessions/{id}/history` hard-404s. This is likely an intentional contract change (the expectrl `SessionManager` was deliberately deleted and no live frontend consumer of `GET /api/sessions` exists — `SessionHistory.svelte` is unmounted), but the route name implies a session inventory. *Resolution:* either deprecate/rename the route and update docs/callers to use `/api/interrupted-runs`, or implement real tmux-backed enumeration (with the per-run invocation, `list-sessions`/`list-panes`, mapping back to run/node/session ids). Reviewers split MEDIUM (Codex) / UNCERTAIN (Claude) — recorded as document-only, not a behavioral fix.
14. **F14 · src/model.rs:1146-1162 · `runAs` validation gaps** — `validate_run_as_config` rejects only a zero-length `command` vec and shell-metachars in `user`. A `command` of `[""]` (or blank tokens) passes → `build_tmux_invocation` sets `prefix=[""]` → `Command::new("")` spawn failure; `user: Some("")` passes → `sudo -u "" -H --`. Reachable only via the direct API (the inspector trims/clears empties). NB: argv-based execution means the metachar check **is** adequate against actual sudo-prefix shell injection — there is no shell interpolation of `user`. *Fix:* reject empty/blank `user`; reject `command` containing any empty/whitespace-only token.

### Part A — suggested remediation order

1. **F1** (CRITICAL sandbox bypass) — and add the `runAs` one-shot regression test.
2. **tmux_exec.rs interaction cluster** (do together — same file/functions): F2, F5, F6, F7, F8, F9, F10.
3. **Interaction concurrency** (runtime.rs): F3, F4.
4. **API/observability**: F11, F13.
5. **Frontend + validation**: F12, F14.

### Part A — manual e2e still required (not run by the unattended test wave)

- Live `sudo -u agent` sandbox run; server-up multi-step workflow with panes under the agent user.
- Permission-wall trigger to exercise the 4-tier safety net end-to-end.
- Interactive `attachCommand` attach mid-execution (observability).
- Two-node `continueSessionFrom` against a live agent (pane reuse, no respawn).

---

# Part B — `we-need-to-sreer-mutable-kettle` (carried over; re-validate against current HEAD)

> The text below is the prior refactoring's adjudicated findings, preserved verbatim. It was
> produced before the Part A refactoring and adjudicated by a third-family judge (Google Gemini 3.1
> Pro High via Antigravity CLI). Several items reference code paths that Part A rewrote or removed —
> re-validate before acting.

Produced by Phase 4 of multi-agent-plan-implementation: two independent reviewers (Claude + OpenAI Codex) per batch, mutual cross-verification, adjudicated by a third-family judge (Google Gemini 3.1 Pro High via Antigravity CLI). Reviewed the **post-merge staging tree** (all of T1–T16).

**Phase 5 (auto-fix) was intentionally skipped** per user decision (2026-06-07) — these are handed back for manual triage. The implementation was consolidated to the base branch `development` as-is.

**Totals: 28 unique findings — 14 HIGH, 10 MEDIUM, 4 LOW. Zero dropped** (reviewers were in near-total agreement; signal is high).

> ⚠️ **Known-failing test on the consolidated branch:** `src/driver.rs` `registry_all_drivers` (finding **#23 / R2-4**) — `cargo test` showed **154 passed, 1 failed**. This is a stale `assert_eq!(len, 3)` from before T9 added registry-keyed profiles. One-line fix below; everything else compiles and the rest of the suite + frontend/e2e are green. *(NB: the current `flickering-fluttering-iverson` suite reports 0 failing — verify whether this was since resolved.)*

---

## HIGH (14)

### Backend engine — `src/runtime.rs` (concurrency / scope / completion; 7, interdependent)

1. **R1-1 · runtime.rs:3513 · ParallelBatch `collectorVar` written to global scope** — written to `checkpoint.var_map`, but nodes read cursor-scoped `var_map_for_cursor()` (1562-1566) first; collected value is invisible downstream when the cursor has variables, and inside a subflow it leaks the batch output to the caller/root scope (isolation breach). *Fix:* write `collectorVar` into the active cursor's scoped map (call-frame scope when `call_stack` non-empty); mirror to global only at top level.
2. **R1-5 · runtime.rs:4000 (also 4267) · Collector barriers not scoped to subflow call instance** — barrier key `"{node_id}:{execution_epoch}"`; subflow node ids are workflow-local and reused per call, so two concurrent calls to the same subflow share one barrier and cross-contaminate. *Fix:* add a stable call-instance id to the barrier key in `handle_collector_entry` + `handle_terminal_cursor_status` (or store barriers in the active call frame).
3. **R1-6 · runtime.rs:3626 · Terminal ParallelBatch exit drops parent call result** — when a batch node with no success edge is a subflow's exit, the cursor is removed without `complete_subflow_if_at_exit`, so the call frame is discarded and the parent call never gets output (same gap for collector release / nested call-as-exit). *Fix:* centralize subflow-exit completion across all node handlers that produce a `NodeResult`.
4. **R1-7 · runtime.rs:3463 (also 816, 838) · ParallelBatch task `Err` leaves run stuck in Running forever** — per-item task `Err` returns immediately, propagates out of `execute_workflow` whose `JoinHandle` is discarded (`let _ = …`), so the checkpoint stays Running, no terminal event. *Fix:* convert per-item `Err` to a failed `NodeResult` / mark checkpoint failed; have the top-level `execute_workflow` wrapper persist a failed terminal checkpoint on unexpected error.
5. **R2-7 · runtime.rs:1717 · Abort does not stop running tmux work** — `abort_all()` can't cancel `spawn_blocking` tmux tasks once started; a running wait/agent sequence continues to timeout and the pane keeps executing. *Fix:* cooperative cancellation token threaded through the `NodeRunner` seam; poll it in wait loops; kill the active pane/session on cancel; track run-owned sessions centrally for abort/finalize cleanup. *(Part A added run-end pane cleanup + session reuse — re-scope against the new tmux runner.)*

### Backend API/security — `src/api.rs` (2)

6. **R2-5 · api.rs:339 · Pane WebSocket accepts unauthenticated cross-origin upgrades (CSWSH)** — no auth / Origin allowlist / stream token before `on_upgrade`; 127.0.0.1 binding doesn't stop a malicious web page opening `ws://127.0.0.1`. (uuid-v7 run_id lowers practical risk, but the gap is real; panes can hold credentials/file contents.) *Fix:* validate `Origin` + require a short-lived per-`{run,pane}` token (or app auth) before upgrade; tests for denial.
7. **R2-6 · api.rs:545 · Pane ownership bypass via arbitrary node JSON output** — `pane_candidates()` treats any node output JSON containing `paneId`/`pane_id`/`target` as an owned pane; a node emitting `{"target":"%1"}` can stream an unowned/stale pane. *Fix:* resolve targets only from a run-owned pane registry / `AgentExecutionMetadata.agent_session_id`; stamp panes with run/node metadata and verify ownership before capture/pipe.

### Backend validation — `src/model.rs` (1)

8. **R1-8 (incl R3-12) · model.rs:1240 · Subflow catalog bodies not fully validated** — `validate_subflow_catalog` only checks entry existence + nested call configs; invalid Decide/Batch configs, dangling edges, collector violations, duplicate ids inside saved subflows pass parent validation and fail only at runtime. (Raised independently in both the engine and frontend review batches → upgraded to HIGH.) *Fix:* recursively validate each catalog subflow with full node/edge/graph rules; scope issue ids; visited-set guard.

### Frontend (4)

9. **R3-1 · PaneTerminal.svelte:29-34 · Pane selector omits the node types that own panes** — filters `task`/`split` only; the tmux runner registers panes under `run_agent`/`spawn`/etc and `split` owns none. All 3 templates use `run_agent`, so only "Active pane" is ever offered; multi-pane (dual-review) can't pick a reviewer pane. *Fix:* build options from `run_agent`/`spawn` (+`task` back-compat); drop `split`.
10. **R3-2 · client.ts:212-215 · WS reconnect backoff reset on `onopen` → reconnect storm** — backend completes the upgrade then error-frames+closes unresolvable panes; `onopen` resets delay to 500ms each cycle → tight 500ms reconnect loop hammering the server. *Fix:* reset backoff only after the connection is stably open / first data frame; slow or stop after repeated immediate close-after-open.
11. **R3-4 · workflowStore.svelte.ts:430-433 · `saveSelectionAsCompound` doesn't enforce single-entry/exit** — copies all selected nodes but infers one exit; a ≥2-terminal selection yields a backend-invalid subflow with no warning. *Fix:* detect exactly one entry/exit, else abort+prompt or synthesize a collector and rewire terminals.
12. **R3-5 · workflowStore.svelte.ts:221-264 · Nested subflow catalog resolution inconsistent/broken** — `drillIntoSubflow` checks only the root catalog, `resolveActive` walks nested catalogs, and the backend treats the catalog as flat/root; saving-as-compound while drilled writes where runtime/root-drill can't find it. *Fix:* one catalog model (store reusable subflows at root; resolve every breadcrumb from root) applied across drill/runtime/validation.
13. **R3-8 · InspectorPanel.svelte:429 · RunAgent inspector edits shadowed fields the backend ignores** — binds agent/prompt to `selectedNode.agent`/`.prompt`, but runtime/validator prefer `runAgentConfig.agent`/`.prompt`; editing template-loaded `run_agent` nodes silently no-ops at runtime. *Fix:* bind controls to `runAgentConfig.*` (or migrate to one canonical field + clear shadows). *(NB: Part A added a Run As section to this same inspector — re-check line numbers.)*

### Templates (1)

14. **R3-11 · templates/multi-agent-plan-implementation.json:32 · ParallelBatch item bindings use unsupported `node:…output.path` syntax** — `read_batch_items` only accepts a bare node id / var name / JSON array, not the binding-resolver path syntax; impl/test/fix batches fail before spawning items. *Fix:* bind batches to supported variables/node outputs, or extend `read_batch_items` to share the `node:`/parsedOutput resolver used by Decide and subflow inputs.

---

## MEDIUM (10)

15. **R1-2 · runtime.rs:2803-2816 · Decide outcome matching: order-dependent substring fallback misroutes** — overlapping labels (`approve` vs `approve_with_changes`) and reasoning-bleed match the wrong outcome. *Fix:* exact match; fallback needs word-boundary + earliest/longest-label; fail on ambiguous multi-match. *(Related to Part A F9.)*
16. **R1-3 · runtime.rs:1990,3460 · ParallelBatch runs synchronously in the dispatch loop** — ignores abort until all items finish and never checkpoints mid-batch (crash re-runs the whole batch, re-spawning agents). *Fix:* poll `is_aborted` + cancel the JoinSet between items; persist partial checkpoint.
17. **R1-4 · runtime.rs:1562-1566,2204,2239 · Subflow with zero declared variables leaks global scope** — empty `subflow_vars` makes `var_map_for_cursor` fall back to the global map for nodes inside the subflow. *Fix:* treat a cursor inside a call frame as scoped even when its var_map is empty.
18. **R2-1 · core/src/stream.rs:31,122-127 + api.rs:379 · Multiple pane viewers race on the single tmux pipe** — one `pipe-pane` per pane; a 2nd viewer clobbers the 1st, and the 1st's `Drop` tears down the survivor. *Fix:* one backend stream task per pane fanned to N WS clients via a broadcast channel + refcount; teardown only when the last subscriber leaves. *(touches external tmux-tools repo)*
19. **R2-2 · tmux_exec.rs:588-604 · Spawned tmux session leaks if metadata registration fails** — `new-session` succeeds then `names::set(...)?` can return early without killing the session. *Fix:* cleanup guard after `new-session`; kill on any post-spawn error. *(tmux_exec.rs rewritten in Part A — re-locate.)*
20. **R2-8 · tmux_exec.rs:72 · Active pane registry keyed by `node.id` only → collides across concurrent cursors** — ParallelBatch / reused subflow bodies run the same node id concurrently; entries overwrite and one completion's `clear_key()` drops another's mapping. *Fix:* scope registrations by cursor id + call-stack/item context (or a multimap with cursor metadata). *(Related to Part A F3; tmux_exec.rs rewritten — re-locate.)*
21. **R3-3 · workflowStore.svelte.ts:226-234 · `activeWorkflow` getter mutates `$state` during `$derived`** — `resolveActive()` writes `doc.ui` lazily; drilling into a subflow without `ui.canvas` triggers `state_unsafe_mutation`. *Fix:* ensureCanvas all subflows up front, or return a default without writing back.
22. **R3-7 · AppShell.svelte:73 · Debounced validation doesn't track nested workflow edits** — the effect reads only `store.workflow` synchronously; in-place nested mutations don't re-fire it, so validation goes stale. *Fix:* snapshot synchronously inside the effect, or a reactive revision counter incremented by mutators.
23. **R3-9 · client.ts:217 · Closed pane sockets can still deliver stale frames** — old WS handlers aren't guarded/cleared on close; a queued frame can hit the new terminal. *Fix:* ignore events when `closed` or `ws !== socket`; null handlers before close.
24. **R3-10 · client.ts:234 · Gap detection appends data before snapshot repaint** — on a seq gap it requests resync but still writes the gap frame to the stale buffer. (Cosmetic per plan, but valid.) *Fix:* resync-pending state — drop incremental frames until the snapshot arrives.

---

## LOW (4)

25. **R2-3 · tmux_exec.rs:909-931 · `extract_after_prompt` degrades to whole-pane for multi-line/wrapped prompts** — pollutes `NodeResult.output` that Decide routes on. *Fix:* anchor on a runner-controlled sentinel/end marker instead of echo-matching the prompt text. *(Same defect as Part A **F9**, where it was re-confirmed at the rewritten `src/tmux_exec.rs:1768`.)*
26. **R2-4 · driver.rs:1352-1360 · `registry_all_drivers` asserts exactly 3 drivers** — stale after T9's registry profiles; **this was the currently-failing test on the prior consolidation.** *Fix:* assert the three built-ins are present (the `names.contains` checks already do) and drop the exact-length assertion (or `len() >= 3`). *(Current suite reports 0 failing — verify resolution.)*
27. **R2-9 · model.rs:1158 · Wait config validation misses invalid regex + timing bounds** — `until` marker regex isn't compiled at validation; negative `idleSeconds`/`readyStableSeconds` accepted. *Fix:* compile the marker in `until` mode; non-negative finite checks on `WaitConfig`/`RunAgentConfig`.
28. **R3-6 · workflowStore.svelte.ts:144 · `selectedPane` never reset across runs/workflows** — a stale node-id selection carries into the next run and feeds the reconnect storm (#10). *Fix:* reset `selectedPane = "active"` in `resetRun()`/`setWorkflow()`.

---

## Part B — suggested remediation order (if/when tackled)

1. **Quick win first:** #26 (driver.rs stale test) — makes the suite green (verify still needed).
2. **`runtime.rs` cluster (sequential, one engineer):** #1, #2, #3, #4, #5, #15, #16, #17, #20 — interdependent; do together to avoid churn.
3. **API security:** #6, #7.
4. **Streaming UX:** #18 (broadcast fan-out, also fixes #10/#23/#24 ergonomics), then #9, #10, #23, #24, #28.
5. **Validation + templates:** #8, #14, #27.
6. **Compound-node UX:** #11, #12, #13, #21, #22.

Dropped findings: none (either part).
