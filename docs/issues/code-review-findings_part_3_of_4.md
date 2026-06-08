# Code Review Findings — Part 3 of 4: Pane Lifecycle, Abort, Streaming & API Security

> Split from the consolidated `code-review-findings.md` backlog into four developer-ready reports
> grouped by domain/source files. This part covers the **pane lifecycle and streaming layer** — abort
> propagation into live tmux work, pane-spawn cleanup, the run-owned pane registry, the pane-stream
> WebSocket surface and its security (CSWSH, ownership, concurrent viewers), and small API-hygiene
> defects. Sibling reports:
> - Part 1 — workflow engine semantics & validation (`src/runtime.rs` + `src/model.rs`)
> - Part 2 — tmux execution path & agent-interaction safety net (`src/tmux_exec.rs`)
> - Part 4 — frontend (Svelte 5 / TypeScript)

## Scope & owning files

- **`src/runtime.rs`** — abort path (`abort` `AtomicBool`, orchestrator-loop abort check,
  `running_tasks.abort_all`/`join_next` drain), the run-owned `active_panes` registry
  (`set_active_pane`/`clear_active_pane`, `resolve_active_pane`, `cleanup_reused_session_panes`),
  `handle_parallel_batch_node` (abort/checkpoint). *Scope/decide/validation parts of runtime.rs are
  Parts 1/2.*
- **`src/tmux_exec.rs`** — pane spawn lifecycle (`spawn_pane`, `names::set`, `ActivePaneRegistration`,
  `clear_key`) and the poll loops that must observe pane death on abort. *The interaction safety-net
  parts of tmux_exec.rs are Part 2.*
- **`src/api.rs`** — `pane_stream_ws`/`pane_stream_socket`, `resolve_run_pane_context`,
  `pane_candidates`/`pane_target_from_value`, `build_attach_command`, `list_sessions`/`session_history`.
- **`tmux-tools/core/src/stream.rs`** — `stream_pane`/`PaneStream::drop` (external crate; R2-1 fix is
  in-SilverBond, no crate change).
- **`ui/src/lib/components/SessionHistory.svelte`**, **`docs/backend.md`**, **`docs/execution-model.md`** — dead consumer + stale docs for F13.

> **Verification (2026-06-08, branch `development`, HEAD `71a96e7`):** all 9 findings re-checked
> against current source — **all STILL-VALID**, anchors accurate to HEAD. Specific confirmations:
> - `ui/src/lib/components/SessionHistory.svelte` **exists and is never imported anywhere** (dead).
> - F11: the `silverbond` socket default matches `tmux_exec.rs`, so the *quoting* defect is the primary
>   issue; the *socket-mismatch* sub-claim applies to the **no-`runAs` (default-socket) path** — verify
>   that path emits `-L` incorrectly when implementing.

**Totals: 9 findings — 1 HIGH, 6 MEDIUM, 2 LOW.**

> ⚠️ **Strong internal coupling.** R2-7 (abort), R1-3 (batch abort), R2-8 (registry keying), and R2-2
> (spawn cleanup) all manipulate the `active_panes` registry and the abort/cleanup paths. Audit
> registry coverage **first** (R2-8 keying) so the abort path (R2-7) can rely on it. R2-5/R2-6/R2-1
> are the WS-streaming security trio and can proceed somewhat independently.

---

## HIGH (1)

### R2-7 (#5). Abort does not stop running tmux work (FINALIZED) (FIXED)
**Severity:** HIGH — reachable by aborting any run with a live agent or long wait; abort is delayed by up to the node timeout while the agent keeps executing in tmux. The recent refactor added a run-owned pane registry but did not wire it to the abort path.
**Files:** src/runtime.rs:570 + 919 (abort `AtomicBool` set), src/runtime.rs:1680 (abort checked only at top of orchestrator loop), src/runtime.rs:1689 (`running_tasks.abort_all()` — no-op for running blocking thread), src/runtime.rs:1690 (`join_next().await` drain blocks abort path), src/runtime.rs:530 + 669-724 (`active_panes` run-owned registry, added by `flickering-fluttering-iverson`), src/runtime.rs:4601 (`cleanup_reused_session_panes` in `finalize_run` — runs only after drain, only session-persistence panes); poll loops src/tmux_exec.rs:1120-1228 (`poll_agent_interactive`), src/tmux_exec.rs:1053-1093 (`wait_for_agent_ready_interactive`), `wait_for_mode`/`wait_for_idle`; blocking entry points src/tmux_exec.rs:105, 136, 186 (`spawn_blocking`).
**Description:** A user abort sets an `AtomicBool` (src/runtime.rs:570, 919) checked only at the top of the orchestrator loop (src/runtime.rs:1680). When a node is mid-flight in `spawn_blocking` (src/tmux_exec.rs:105/136/186), abort cannot interrupt it: `abort_all()` (src/runtime.rs:1689) is a no-op for a running blocking OS thread, and the immediately-following `join_next().await` drain loop (src/runtime.rs:1690) stalls the abort until the task finishes or hits its full node timeout. The tmux poll loops (`poll_agent_interactive` at src/tmux_exec.rs:1120-1228, the `wait_for_*` helpers) loop on capture→sleep(250ms) and never check any cancel signal; `tmux_exec.rs` has zero abort references. Nothing kills the active pane on the abort path — `cleanup_reused_session_panes` (src/runtime.rs:4601) runs only after the drain already waited the task out, and only for session-persistence panes. The user sees "aborted" delayed by up to the node timeout while the agent keeps running.
**Fix:** On the abort branch, before the `join_next` drain loop (src/runtime.rs:1690), look up the run's panes via the existing `active_panes` registry (src/runtime.rs:530, 669-724) and issue `kill-pane`/`kill-session` for each. Killing the pane makes the blocked `capture_visible_stripped` call inside each poll loop error out, unwinding the loop naturally so the blocking task returns promptly and the drain completes quickly. No change to the external tmux-tools crate is required. Audit registration coverage first: ensure every active pane (not just session-persistence panes) is registered in `active_panes` before this path can rely on it. Add a test that aborts a run with a live agent pane and asserts the run reaches a terminal aborted state promptly (well under the node timeout) with the pane killed.

> **Depends on R2-8 (this part).** The "audit registration coverage first" step *is* R2-8's keying fix —
> with the `node.id`-only key, the registry can't reliably enumerate concurrent panes to kill. Do R2-8
> before relying on the registry here.

---

## MEDIUM (6)

### R1-3 (#16). ParallelBatch runs synchronously, ignores abort + no mid-batch checkpoint (FINALIZED) (FIXED)
**Severity:** MEDIUM (leaning MEDIUM-high) — confirmed on current HEAD, both sub-claims. The flagship template re-spawns multiple long/paid agents on any crash, and abort latency equals the full batch duration. Distinct from R2-7 (which is per-item in-flight kill); this is the batch-loop failing to poll abort + orthogonal checkpointing.
**Files:** src/runtime.rs:3365 (`handle_parallel_batch_node`, synchronous), src/runtime.rs:1962 (batch dispatched as "immediate"), src/runtime.rs:1698 (awaited via `process_immediate_cursors`), src/runtime.rs:3433-3460 (collect loop `while let Some(joined) = running.join_next().await` — no abort poll, re-spawns pending at 3442-3459), src/runtime.rs:1680 (only abort check — unreachable until batch returns), src/runtime.rs:3675 (`record_batch_item_result` — in-memory only), src/runtime.rs:3440 (per-item record site), src/runtime.rs:1851 (first persist, post-batch), src/runtime.rs:3666 (`run_cursor_task` spawns agent items).
**Description:** A ParallelBatch is an "immediate" node executed entirely within one synchronous `handle_parallel_batch_node` call (src/runtime.rs:3365, dispatched at 1962, awaited via `process_immediate_cursors` at 1698). Its collect loop (src/runtime.rs:3433-3460) re-checks neither abort nor persists partial state — it keeps draining the JoinSet and re-spawning pending items (3442-3459) until every item finishes; the only abort check (src/runtime.rs:1680) is unreachable until the batch returns. So clicking Stop has no effect until the slowest item finishes (abort latency = full batch duration, potentially many minutes), and any crash forces a full re-run because the first checkpoint persist (src/runtime.rs:1851) only happens post-batch — `record_batch_item_result` (src/runtime.rs:3675) only mutates the in-memory checkpoint. The flagship `templates/multi-agent-plan-implementation.json` drives agent items through `run_cursor_task` (src/runtime.rs:3666), so both the latency and re-spawn costs are real and expensive. No tests cover batch abort or batch resume.
**Fix:** Apply both mechanisms. (1) In the collect loop (src/runtime.rs:3433), check `ctx.registry.is_aborted(&run_id)` each iteration; on abort, stop pulling from `pending_items`, call `running.abort_all()`, drain, and return early so the main loop transitions to Aborted — bounding abort latency to ~1 item (independent of R2-7). Define partial-result-on-abort semantics (record completed items, mark the rest cancelled). (2) After each `record_batch_item_result` (src/runtime.rs:3440), call `persist_checkpoint(...)`, and add per-item resume granularity (track completed item indices in the checkpoint) so a resumed run skips already-recorded items rather than re-spawning all of them. Add tests: aborting a running batch transitions to Aborted within ~1 item; a crash after k/N items resumes from k without re-spawning the completed agents.

> **Coordinate with Part 1 R1-7** — both edit `handle_parallel_batch_node` (src/runtime.rs:3365-3460).
> R1-7 hardens per-item `Err`; R1-3 adds abort-poll + checkpointing. Sequence to avoid collision.

### R2-5 (#6). Pane WebSocket accepts unauthenticated cross-origin upgrades (CSWSH) (FINALIZED) (FIXED)
**Severity:** MEDIUM — lowered from HIGH. Real CSWSH gap, but loopback-only bind + uuid-v7 run_id + Tauri desktop context make it defense-in-depth, not broad impact. *(Code relocated by `flickering-fluttering-iverson`; finding still valid.)*
**Files:** src/api.rs:525-545 (`pane_stream_ws`; `on_upgrade` at 530), src/api.rs:60-63 (route `/api/runs/{run_id}/panes/{pane}/stream`), src/api.rs:677 (`resolve_run_pane_context` — DB lookup, not auth); bind at src/main.rs:21 (127.0.0.1:3333).
**Description:** `pane_stream_ws` completes the WS upgrade with zero pre-upgrade authorization — no `Origin` allowlist, no stream token, no auth layer (`tower-http` isn't a dependency). The only post-upgrade gate is a `run_id`/`pane` registry lookup, which is not an auth control. A malicious web page open in the user's browser can attempt `ws://127.0.0.1:3333/api/runs/{run_id}/panes/{pane}/stream` (CSWSH) and, if it guesses or leaks a live uuid-v7 run id, stream live terminal pane output that may contain secrets. Loopback bind, uuid run id, and the desktop context bound the practical risk.
**Fix:** In `pane_stream_ws`, read the `Origin` header before `on_upgrade` and reject any request whose origin is not the app's own webview origin (permit absent/native origins for same-origin/Tauri). Browsers attach a page-non-forgeable `Origin` to cross-site WS handshakes, so this is the precise CSWSH control. Take care to allowlist the actual Tauri webview origin (which may be `tauri://` or opaque) so the real frontend isn't broken. Add tests asserting a foreign-origin upgrade is denied and the app origin is accepted. (A per-run stream token can be layered on later if defense against run_id leakage is wanted, but is not required at MEDIUM.)

### R2-6 (#7). Pane ownership bypass via arbitrary node JSON output (FINALIZED) (FIXED)
**Severity:** MEDIUM — lowered from HIGH. Real injection, but loopback single-user desktop + same-`runAs` sandbox bound it to a same-user cross-run leak, not a privilege/tenant boundary crossing. *(Code relocated by `flickering-fluttering-iverson`; finding still valid.)*
**Files:** src/api.rs:738 (`pane_candidates`), src/api.rs:781-785 (`pane_target_from_value` key harvesting), src/api.rs:677 (`resolve_run_pane_context` — registry-first, candidate fallback), src/api.rs:717-730 (`start_pane_stream`/`stream_pane` capture); agent-JSON source at src/runtime.rs:4789 (`parse_structured_output`).
**Description:** `resolve_run_pane_context` resolves a stream target from the authoritative run-owned registry first, but on a miss (finished/cleared/restarted run) falls back to `pane_candidates`, which harvests any `paneId`/`pane_id`/`target` string from the run's checkpoint output and pipes it straight into `capture-pane -t <target>` with no ownership verification. Because `parse_structured_output` turns an agent's free-text response into `parsed_output` when `response_format == Json`, a malicious or confused agent can emit `{"target":"%1"}` and redirect the stream to another concurrent run's pane on the same tmux server — a same-user cross-run terminal-output leak.
**Fix:** Keep harvesting candidates but accept a target only if it also appears in this run's owned-pane set — the registry plus a run-owned pane list persisted into the checkpoint so it survives registry clear/restart. Reject any harvested `target` not in that set before capture/pipe. Consider tmux-layer pane stamping (`set-option -p @sb_run <run_id>`, verified before capture) as later defense-in-depth. Add a test asserting an injected foreign `%N` in node/agent output is rejected while a legitimately owned pane streams.

### R2-1 (#18). Multiple pane viewers race on the single tmux pipe (FINALIZED) (FIXED)
**Severity:** MEDIUM — confirmed on current HEAD: silent stream corruption + cross-teardown are real, but it's a single-user loopback desktop app where two concurrent viewers of the *same* pane is uncommon, and a `stream.rs:30-31` comment notes live-stream-to-UI is deprioritized in favor of terminal-attach.
**Files:** src/api.rs:566 (`pane_stream_socket` → `start_pane_stream` per WS connection), tmux-tools/core/src/stream.rs:36 (`stream_pane` runs `pipe-pane -t <pane>` afresh each time), tmux-tools/core/src/stream.rs:127-131 (`PaneStream::drop` — bare `pipe-pane -t <pane>` toggle-off, no owner guard), src/api.rs:606 (EOF/heartbeat), src/api.rs:677 (`resolve_run_pane_context` — selector + "active pane" can resolve to one target).
**Description:** tmux permits one active pipe per pane. Each WS viewer sets up its own `pipe-pane` with no refcount or shared per-pane task: `pane_stream_socket` (src/api.rs:566) calls `start_pane_stream` per connection → `stream_pane` (tmux-tools/core/src/stream.rs:36) runs `pipe-pane` afresh. When two viewers open the same pane, the second's pipe replaces the first's, so the first's fifo silently stalls (only broken by the unrelated EOF/heartbeat at src/api.rs:606). When *either* viewer disconnects, `PaneStream::drop` (tmux-tools/core/src/stream.rs:129) issues `pipe-pane -t <pane>`, disabling the pane's current pipe regardless of owner and killing the surviving viewer. Two browser tabs on one pane — or the pane-selector and "active pane" both resolving via `resolve_run_pane_context` (src/api.rs:677) to the same target — trigger it. No tests cover concurrent viewers.
**Fix:** Run one backend stream task per pane, fanned out to N WS clients, entirely within SilverBond (no external-crate change). In `AppState`, keep a `HashMap<pane_target, broadcast::Sender<Bytes>>` plus a subscriber refcount. The first subscriber spawns a single task that owns the `PaneStream` and forwards reads into the broadcast; later subscribers attach to the existing sender. Drop the task and `PaneStream` (running `pipe-pane` off exactly once) only when the refcount reaches zero. Handle per-client snapshot/resync on attach so a late subscriber gets current state. Add a test with two concurrent WS subscribers on one pane asserting both receive frames and the pipe survives until the last leaves.

> **Note for Part 4:** the broadcast fan-out here also fixes the ergonomics behind several frontend
> streaming findings (R3-9/R3-10 stale/gap frames). Coordinate so the client-side guards (Part 4) and
> the server-side fan-out (this finding) compose.

### R2-2 (#19). Spawned tmux session leaks if metadata registration fails (FINALIZED) (FIXED)
**Severity:** MEDIUM — confirmed on current HEAD as a true orphan (no socket-scoped reaper) and silent, but the failing step is an immediate same-socket `set-option` on a just-created pane, so failure is rare (requires a tmux server death/race in the millisecond window).
**Files:** src/tmux_exec.rs:1404 (`spawn_pane`), src/tmux_exec.rs:1449 (`tmux new-session` — live session created), src/tmux_exec.rs:1455-1464 (`names::set(...)?` metadata writes, fallible `tmux set-option -p`, no kill-on-error), src/tmux_exec.rs:1467 (`Ok(SpawnedPane{…})` construction), src/tmux_exec.rs:678-679 + 878 (caller registers into `active_panes` only after Ok), src/tmux_exec.rs:392 (`cfg=None` CLI caller with no registry), src/runtime.rs:4606 (`cleanup_reused_session_panes` — reaps only registered panes).
**Description:** `spawn_pane` (src/tmux_exec.rs:1404) creates a detached tmux session at src/tmux_exec.rs:1449, then performs up to four `names::set(...)?` metadata writes (src/tmux_exec.rs:1455-1464) — each a fallible `tmux set-option -p` — with no kill-on-error guard. On any `Err` the `?` returns early with the session still alive and **unregistered**, because the caller only inserts into `active_panes` after a successful return (src/tmux_exec.rs:678-679, :878). Run teardown does not reap it: `cleanup_reused_session_panes` (src/runtime.rs:4606) kills only already-registered, persistence-keyed panes, so the orphan survives run completion as a silent leak. No test covers spawn-failure cleanup.
**Fix:** Immediately after the successful `new-session` (src/tmux_exec.rs:1449), wrap the new `session_name`/`pane_id` in a scope-guard (drop-bomb) struct whose `Drop` runs `tmux kill-session -t <session_name>`; call `.disarm()` on it right before constructing `Ok(SpawnedPane{…})` at src/tmux_exec.rs:1467. Any error — or panic — between spawn and success then kills the just-created session automatically, covering all current and future fallible post-spawn steps with panic-safe RAII and no caller changes. Add a test that forces a post-`new-session` metadata write to fail and asserts no orphaned tmux session/pane remains on the socket.

### R2-8 (#20). Active pane registry keyed by `node.id` only → collides across concurrent cursors (FINALIZED) (FIXED)
**Severity:** MEDIUM — confirmed on current HEAD (NOT fixed by `flickering-fluttering-iverson`). Reachable on the flagship ParallelBatch template and any reused-subflow/Split; failure is silent (wrong-pane stream or leaked pane), but requires concurrency on an identical node id.
**Files:** src/runtime.rs:530 (`active_panes: Arc<Mutex<BTreeMap<String, String>>>` — `node.id → pane_id`), src/tmux_exec.rs:183 (`ActivePaneRegistration::new(&ctx, run_id, node.id.clone())` — node.id-only key), src/runtime.rs:2548-2559 (`run_node_with_interaction` — no `cursor_id` threaded in), src/runtime.rs:669-676 (`set_active_pane` insert), src/runtime.rs:679-682 (`clear_active_pane`) + src/tmux_exec.rs:303-310 (`clear_key`), src/runtime.rs:3620 (distinct `child_cursor_id` per batch item), src/runtime.rs:3665 (shared `body_node`), src/runtime.rs:704 (single-pane heuristic), src/api.rs:208/693 (`resolve_active_pane` callers), src/runtime.rs:4612 (`cleanup_reused_session_panes`, the R2-7 abort path).
**Description:** The run-owned `active_panes` map (src/runtime.rs:530) keys panes by `node.id` only; registration inserts via `set_active_pane` (src/runtime.rs:669-676) and cleanup removes by key via `clear_active_pane`/`clear_key` (src/runtime.rs:679-682, src/tmux_exec.rs:303-310). `cursor_id` is never threaded into the runner (src/runtime.rs:2548-2559), so concurrent ParallelBatch items — distinct `child_cursor_id` (src/runtime.rs:3620) but the same `body_node.id` (src/runtime.rs:3665) — all insert under one key: later registrations clobber earlier ones, and the first item to finish `clear_key()`s the mapping now owned by a still-running sibling. The same occurs for a reused subflow body across Split branches. Streaming resolution (src/api.rs:208/693 → `resolve_active_pane`) then returns the wrong/missing pane, and abort cleanup (src/runtime.rs:4612) reads the clobbered map and can skip a live pane → leak. No tests cover concurrent pane registration.
**Fix:** Thread `cursor.cursor_id` through `run_node_with_interaction` (src/runtime.rs:2548) into `ActivePaneRegistration::new` (src/tmux_exec.rs:183) and build the registry key as `"{cursor_id}:{node_id}"`, so concurrent same-node-id panes no longer collide. Update `resolve_active_pane` and the `src/api.rs` callers (which pass a bare `node_id`, src/api.rs:208/693) to scan for the matching `node_id` suffix rather than an exact-key lookup, and define which pane "stream this node" resolves to when several share a node id (e.g. most-recent, or require a cursor/session disambiguator). Reassess the single-pane heuristic at src/runtime.rs:704 under the new key. Add a test with a ParallelBatch (or Split into one reused subflow body) running the same node id in two concurrent cursors, asserting each pane is independently registered, resolvable, and cleaned up without dropping a sibling's mapping.

> **Do this before R2-7.** The abort path (R2-7) enumerates `active_panes` to kill live panes; with the
> colliding `node.id`-only key it can miss a sibling pane. R2-8's keying fix is the "audit registration
> coverage" prerequisite R2-7 calls out.

---

## LOW (2)

### F11. `build_attach_command`: no shell-escaping + socket mismatch (FINALIZED) (FIXED)
**Severity:** LOW — lowered from MEDIUM. The string is display-only (a copyable UI hint, never executed by the backend), so worst case is a non-working copy-paste command, not code execution; the socket mismatch is a real but cosmetic/UX failure.
**Files:** src/api.rs:125-150 (`build_attach_command` — unquoted interpolation at :136-150, hard-coded `-L silverbond` default at :130/:150), response field src/api.rs:266-272 (`attachCommand`), render ui/src/features/editor/InspectorPanel.svelte (clipboard `<code>` hint); socket source src/runtime.rs:1641-1646 (`TmuxInvocation::default()` → `socket: None` when no `runAs`); origins src/model.rs:199-203 (`RunAsConfig` — author-controlled `command`/`user`/`socket`).
**Description:** `build_attach_command` (src/api.rs:125) advertises `tmux -L silverbond attach -t <name>` for every run, hard-coding the `silverbond` socket (src/api.rs:130,150). But a run without `runAs` is created on tmux's default socket — `TmuxInvocation::default()` has `socket: None`, so no `-L` flag is emitted (src/runtime.rs:1641-1646) — so the copied command targets the wrong server and fails to attach. Separately, `command.join(" ")`, `user`, `socket`, and `session_name` are interpolated unquoted (src/api.rs:136-150), all originating from author-controlled `RunAsConfig` (src/model.rs:199-203) or tmux output, so any value containing whitespace or shell metacharacters produces a malformed paste. The string is display-only (stored as `attachCommand`, rendered as a clipboard hint in `InspectorPanel.svelte`) and never executed by the backend, so this is a correctness/UX defect, not an injection vulnerability. No Rust tests cover this function.
**Fix:** Derive the attach hint from the same `TmuxInvocation` the run actually uses (the run's `run_invocation`) rather than re-deriving it in `build_attach_command`, making the hint a faithful rendering of reality and a single source of truth. Shell-quote every interpolated token (e.g. via `shlex::try_quote`) — `command` tokens, `user`, `socket`, and `session_name` — and emit `-L <socket>` only when `invocation.socket` is `Some` (omit it entirely for default-socket runs). This fixes both the socket mismatch and the quoting in one place with no change to how runs are created. Add a test asserting (a) a no-`runAs` run's hint omits `-L`, (b) a `runAs` run's hint includes `-L <socket>`, and (c) tokens with spaces are quoted.

> **Frontend pair:** the inspector's inline attach-hint `$derived` (Part 4 **F12**) hard-codes a
> `sudo -u <user>` prefix and contradicts this backend hint. Align the two so the UI mirrors
> `build_attach_command`.

### F13. `/api/sessions` is a misleading interrupted-run alias with a dead consumer (FINALIZED) (FIXED)
**Severity:** LOW — the route returns valid data and the `runId`→`id` rename is additive (non-breaking); the only consumer is unmounted dead code. The defect is contract/naming ambiguity plus stale docs, not a runtime bug.
**Files:** src/api.rs:1101-1113 (`list_sessions` — maps `db.list_interrupted_runs()`, copies `runId`→`id`), src/api.rs:1116-1124 (`session_history` — unconditional `404`), src/api.rs:1067 (`interrupted_runs` — the canonical surface, served at `/api/interrupted-runs`), ui/src/lib/api/client.ts:74 (frontend uses `/api/interrupted-runs`), ui/src/lib/components/SessionHistory.svelte (only `/api/sessions` consumer — never imported anywhere, i.e. unmounted; **confirmed dead on HEAD**); stale docs docs/backend.md:100-114, docs/execution-model.md:204 (still describe the deleted `src/session.rs` expectrl `SessionManager` with `list_sessions()`/`get_history()`).
**Description:** `GET /api/sessions` (`list_sessions`, src/api.rs:1101-1113) returns the interrupted-runs list with `runId` duplicated as `id` — the same payload as the canonical `GET /api/interrupted-runs` (src/api.rs:1067) that the frontend actually calls (ui/src/lib/api/client.ts:74) — despite a name implying a tmux session/pane inventory it never produces. Its companion `GET /api/sessions/{id}/history` (src/api.rs:1116-1124) always returns `404`. The sole consumer, `SessionHistory.svelte`, is never imported and thus dead/unmounted. Compounding the confusion, docs/backend.md:100-114 and docs/execution-model.md:204 still document the long-deleted `src/session.rs` expectrl `SessionManager`, reinforcing the misleading route name.
**Fix:** Remove the alias and its dead consumer rather than maintaining a redundant, misleading surface. Delete the `/api/sessions` and `/api/sessions/{id}/history` routes and their handlers `list_sessions`/`session_history` (src/api.rs:1101-1124), and delete the unmounted `ui/src/lib/components/SessionHistory.svelte`. Correct the stale `src/session.rs` references in docs/backend.md:100-114 and docs/execution-model.md:204 to reflect the current tmux-based model. Before deleting, grep deploy/proxy configs for any external `/api/sessions` caller; `/api/interrupted-runs` (src/api.rs:1067) already serves the data for in-tree consumers.

---

## Suggested remediation order (Part 3)

1. **Registry keying first:** R2-8 (`"{cursor_id}:{node_id}"` key) — prerequisite for reliable pane enumeration.
2. **Abort propagation:** R2-7 (kill panes on abort, relies on R2-8) + R1-3 (batch abort-poll + checkpoint — *coordinate with Part 1 R1-7 on the batch handler*).
3. **Spawn cleanup:** R2-2 (RAII drop-bomb around `spawn_pane`).
4. **WS streaming security trio:** R2-5 (Origin check), R2-6 (owned-pane allowlist), R2-1 (broadcast fan-out — also improves Part 4 streaming ergonomics).
5. **API hygiene:** F11 (attach-hint), F13 (delete dead `/api/sessions` + docs).

## Cross-part coordination notes

- **`src/runtime.rs` is shared with Parts 1 & 2.** This part edits the abort/registry/`finalize_run`
  regions and `handle_parallel_batch_node` (R1-3) — the latter overlaps **Part 1 R1-7**. Sequence the
  batch work.
- **`src/tmux_exec.rs` is shared with Part 2.** This part edits the pane-spawn lifecycle
  (`spawn_pane`, `ActivePaneRegistration`); Part 2 edits the interaction safety net. The poll loops
  (`poll_agent_interactive`) are read by R2-7 (they must observe pane death) but *edited* by Part 2 —
  coordinate on the cancel-on-pane-death behavior.
- **R2-1 (this part) + R3-9/R3-10 (Part 4)** together fix the pane-streaming corruption story (server
  fan-out + client guards).
- **F11 (this part) + F12 (Part 4)** are the attach-hint pair — align backend and inspector rendering.
