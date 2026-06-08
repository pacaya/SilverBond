# Code Review Findings — Part 2 of 4: tmux Execution Path & Agent-Interaction Safety Net

> Split from the consolidated `code-review-findings.md` backlog into four developer-ready reports
> grouped by domain/source files. This part covers the **live tmux execution path** (`src/tmux_exec.rs`)
> — the `runAs` sandbox boundary, the 4-tier capture-pane interaction safety net (permission walls,
> destructive blocklist, visual idle, prompt extraction), agent-config rendering, and the
> async/blocking boundary. These are predominantly findings from the most recent
> `flickering-fluttering-iverson` refactor (Part A of the original backlog). Sibling reports:
> - Part 1 — workflow engine semantics & validation (`src/runtime.rs` + `src/model.rs`)
> - Part 3 — pane lifecycle, abort, streaming & API security (`src/api.rs` + pane registry/abort)
> - Part 4 — frontend (Svelte 5 / TypeScript)

## Scope & owning files

- **`src/tmux_exec.rs`** — `run_tmux_oneshot`, `run_agent_interactive`/`poll_agent_interactive`, the
  interaction handler (`escalate_or_fallback`, permission/destructive branches), `CaptureProgress`,
  `capture_delta`, `build_agent_command`, `wrap_keep_open`, `extract_after_prompt`,
  `resolve_tmux_bin`/`build_tmux_invocation`.
- **`src/runtime.rs`** — *touched only at the interaction-escalation and one-shot call sites*
  (`run_decide_node`, `run_orchestrator_*`, `escalate_agent_interaction`, `set_pending_interaction`,
  `resolve_interaction`). The pane-registry/abort parts of runtime.rs are **Part 3**; the
  scope/batch/decide-selection parts are **Part 1**.
- **`src/driver.rs`** — `build_session_args` (the renderer of model/tool/turns/budget flags).
- **`src/model.rs`** — `RunAgentConfig` per-node tool lists (read-only reference for F5).
- **`src/api.rs`** — respond-interaction endpoint (`:1008-1018`), shared with F3/F4.
- **`core/src/agents/builtin.rs`**, **`tmux-tools/core/src/agents/mod.rs`** — builtin agent profiles / `launch_argv`.

> **Verification (2026-06-08, branch `development`, HEAD `71a96e7`):** all 10 findings re-checked
> against current source — **all STILL-VALID** at their stated severity, no line drift, none resolved
> (Phase 5 auto-fix was intentionally skipped for this refactor). Anchors below are accurate to HEAD.

**Totals: 10 findings — 1 CRITICAL, 4 HIGH, 3 MEDIUM, 2 LOW.**

> ⚠️ **Tight clustering.** F2/F6/F7/F8/F9 all live inside the same `poll_agent_interactive` /
> `tmux_exec.rs` interaction machinery; F3/F4 are the runtime-side interaction-concurrency pair and
> **must be applied together** (same functions/lines). F1 introduces an `inv` parameter to
> `run_tmux_oneshot` that several other findings call through. Do these as one focused work-stream by
> a single engineer to avoid churn.

---

## CRITICAL (1)

### F1. `runAs` sandbox bypass in decide/orchestrator one-shots (FINALIZED)
**Severity:** CRITICAL — sandbox-escape for classifier/orchestrator agents on any `runAs` run; no guard exists and no test covers it.
**Files:** src/runtime.rs:2633 (`run_decide_node`), src/runtime.rs:4906 (`run_orchestrator_refinement`), src/runtime.rs:4938 (`run_orchestrator_branch`); helper src/tmux_exec.rs:800 (`run_tmux_oneshot`); correct contrast src/tmux_exec.rs:136-150.
**Description:** When a workflow sets `runAs`, the decide-node classifier and both orchestrator one-shots call `run_tmux_oneshot` inside `spawn_blocking` with no thread-local invocation installed, so `resolve_invocation()` falls through to `TmuxInvocation::default()` (empty command prefix, no socket — SilverBond never calls `set_global_invocation`, so the fallback is real, not theoretical). The agents therefore run as the privileged backend user instead of the sandboxed `runAs` user, and their panes land on the default tmux socket instead of `-L <socket>` — defeating the sandbox boundary and orphaning panes, since `cleanup_panes` (src/runtime.rs:4620) only targets the run socket. The two orchestrator fns don't currently receive `ctx`/`run_invocation`.
**Fix:** Add an `inv: Option<TmuxInvocation>` parameter to `run_tmux_oneshot` and perform the `with_invocation(inv, || …)` wrapping inside it, so every caller — the three one-shots plus the preview path at src/runtime.rs:1064 — is covered by construction (single choke-point; no future one-shot site can reintroduce the bug). Thread `ctx.run_invocation.clone()` into `run_orchestrator_refinement` and `run_orchestrator_branch` by adding an `inv` param to each, passed from the `select_next_decision(ctx)` callers (`ctx` is already in scope; `run_decide_node` already has it). Add a regression test asserting that a `runAs` workflow with a decide/orchestrator node applies the invocation (non-default prefix + socket reach the spawned command).

---

## HIGH (4)

### F2. Missing escalation channel auto-approves permission walls (fail-open) (FINALIZED)
**Severity:** HIGH — non-destructive permission walls are silently granted on every unattended run; `auto_approve` defaults to false so the vulnerable branch is the default path.
**Files:** src/tmux_exec.rs:1177-1186 (non-destructive `PermissionRequest` branch), src/tmux_exec.rs:1243-1256 (`escalate_or_fallback`, `None` branch at 1251-1255); affected callers src/tmux_exec.rs:815 (`run_tmux_oneshot`), src/tmux_exec.rs:115 (plain `NodeRunner::run`).
**Description:** In the agent interaction handler, a non-destructive `PermissionRequest` with `auto_approve=false` calls `escalate_or_fallback(interaction, …, "y")`. When `interaction` is `None` — every non-interactive execution path — the helper skips the human oneshot wait and returns `"y"` verbatim, which `send_reply_if_present` types into the agent pane. A permission prompt the user explicitly chose not to auto-approve is therefore silently granted on unattended runs (fail-open). Only the destructive sub-branch is safe; it already passes `"n"` (src/tmux_exec.rs:1166-1174).
**Fix:** In the no-channel (`interaction.is_none()`) case, send `"n"` instead of `"y"`, reserving `"y"` strictly for explicit `auto_approve` or a real human response — matching the destructive branch's existing `"n"` convention. Additionally emit a `permission_denied` (or equivalent) observability event so the auto-denial is visible in the UI/log rather than silent; this requires plumbing an event emitter into `poll_agent_interactive`, which currently lacks `ctx`. Add a test asserting a non-destructive permission prompt on a `None`-channel path is denied, not approved.

### F3. Run-level interaction slot race (FINALIZED)
**Severity:** HIGH — reachable today via ParallelBatch and Split fanout; causes lost/hung interactions and misdelivered approvals of security-relevant prompts.
**Files:** src/runtime.rs:529 (`interaction_sender` single-slot field in `ActiveRun`), src/runtime.rs:639 (unconditional overwrite in `set_pending_interaction`), src/runtime.rs:643-663 (`resolve_interaction`, run_id-only take), src/runtime.rs:4854 (event already carries `sessionId`), src/runtime.rs:4865 (`receiver.await`), src/api.rs:1008-1018 (respond endpoint, run_id-only).
**Description:** Each run holds a single `Option<oneshot::Sender<String>>` slot; `set_pending_interaction` overwrites it without checking, and `resolve_interaction`/the respond endpoint key only by `run_id`. When two concurrent branches of one run (ParallelBatch items at src/runtime.rs:3664, Split cursors at src/runtime.rs:1781) hit interaction prompts together, the second escalation drops the first branch's sender — its `receiver.await` fails ("Interaction channel closed") and that branch aborts — and a human response keyed only by run_id is delivered to whichever sender currently occupies the slot, so an approval intended for pane A can land on pane B. Concurrency is reachable today (tests already exercise ParallelBatch + approval templates).
**Fix:** Replace the per-run `Option<Sender>` with a `HashMap<sessionId, oneshot::Sender<String>>`. `sessionId` already flows into `escalate_agent_interaction` (src/runtime.rs:4844) and is already on the `agent_interaction_required` event (src/runtime.rs:4854), so the client only needs to echo it back on `respond-interaction` and the backend resolves by `(run_id, sessionId)`. Clean up the map entry when a branch aborts mid-wait. Add a test with two concurrent escalations in one run asserting each response reaches its own sender.

> **Apply jointly with F4** (same function/lines) — F4 is the register-before-emit ordering fix that
> depends on this `HashMap`.

### F5. Agent safety config silently dropped on the tmux path (FINALIZED)
**Severity:** HIGH — per-node tool allow/deny lists (and model, system prompt, max turns/budget, web-search, resume) silently fail to apply on the live execution path, leaving only the coarse access profile; an over-permissioning security gap. Caveat: author-constructible only — no shipped template sets these fields.
**Files:** src/tmux_exec.rs:1475-1507 (`build_agent_command` — argv = `launch_argv` + `extra_args` only), src/tmux_exec.rs:582 (`run_agent_interactive` receives resolved `AgentConfig`), src/tmux_exec.rs:625 (driver fetched but used only for capabilities/patterns/blocklist), src/tmux_exec.rs:652 (`SpawnConfig` carries no config fields), src/tmux_exec.rs:1509-1519 (`access_profile_from_config` — drops fine-grained tool lists), src/driver.rs:352-424 (`build_session_args` — the renderer of `--model`/`--allowedTools`/`--disallowedTools`/`--max-turns`/`--max-budget-usd`/system-prompt/web-search/resume, called only in tests), src/model.rs:259-260 (per-node `allowedTools`/`disallowedTools`); external `tmux-tools/core/src/agents/mod.rs:134-173` (`launch_argv` returns bare binary + static access-profile args only).
**Description:** The live agent executor `run_agent_interactive` (src/tmux_exec.rs:582) receives a fully-resolved `AgentConfig` but routes it through `spawn_pane` → `build_agent_command` (src/tmux_exec.rs:1475-1507), which ignores it except for the access-mode shorthand (`access_profile_from_config`, src/tmux_exec.rs:1509-1519). The argv is just `launch_argv` (binary + static access-profile args, per external `tmux-tools/core/src/agents/mod.rs:134-173`) plus `extra_args`. The driver's `build_session_args` (src/driver.rs:352-424), which renders model/tool-allow/deny/turns/budget/system-prompt/web-search/resume flags, is invoked only in tests — never on the spawn path. A workflow author who restricts an agent to `allowedTools: [Read, Grep]` (or sets `disallowedTools`, `model`, `maxTurns`, etc.) gets an agent running with whatever the coarse access profile permits, so security-relevant tool restrictions silently fail to apply.
**Fix:** In `build_agent_command` (src/tmux_exec.rs:1475), fetch the driver via `driver::get_driver(agent)` and append `build_session_args(agent_config).args` (src/driver.rs:352) before `extra_args`, making `build_session_args` the single source of all dynamic config/safety flags. To avoid emitting access flags twice, pass `access=None` (or the neutral profile) to `launch_argv` so access-mode args come solely from `build_session_args` — first verify `build_session_args` renders the same access-mode args `launch_argv` previously supplied, or access mode regresses. Thread any env vars `build_session_args` produces through the tmux `new-session` invocation. Add tests asserting `allowedTools`/`disallowedTools` (and model) propagate to the constructed agent argv.

### F9. `extract_after_prompt` corrupts decide outcomes for multi-line/TUI prompts (FINALIZED)
**Severity:** HIGH — raised from MEDIUM. Reachable by default: all 6 shipped decide prompts (`dual-review`, `epic-dev`, `multi-agent-plan-implementation`) are multi-line and enumerate every outcome name, so the whole-blob fallback — and thus silent misrouting — is the default path for shipped decide nodes, not an edge case. Zero direct test coverage. *(Same upstream defect as the dismissed Part B R2-3; complementary to the finalized R1-2 outcome-matching fix in Part 1.)*
**Files:** src/tmux_exec.rs:1768 (`extract_after_prompt`), src/tmux_exec.rs:1775 (per-line `line.contains(prompt_text)` match), src/tmux_exec.rs:1789 (whole-capture fallback when no line matches), src/tmux_exec.rs:1608 (`send-keys -l` sends the prompt literally → multi-line render), src/runtime.rs:2638 (`result.output` → `response`), src/runtime.rs:2640 (`select_decide_outcome` consumes the blob); templates/dual-review.json, templates/epic-dev.json, templates/multi-agent-plan-implementation.json (multi-line decide prompts).
**Description:** After an agent answers a decide prompt, the runner captures the pane and calls `extract_after_prompt` (src/tmux_exec.rs:1768) to isolate the answer. It scans captured lines for one containing the full prompt (src/tmux_exec.rs:1775), but prompts are sent literally via `send-keys -l` (src/tmux_exec.rs:1608) and render across multiple pane lines, so no single line `contains` the whole prompt; the fallback at src/tmux_exec.rs:1789 then returns the *entire* capture — the echoed prompt (which enumerates all outcome names) plus TUI chrome. That blob flows as `result.output` → `response` (src/runtime.rs:2638) into `select_decide_outcome` (src/runtime.rs:2640), which matches an outcome from the echoed prompt regardless of the agent's actual answer → silent branch misclassification. This corrupts the *input*; Part 1's R1-2 hardens the *matching* — both are needed because the echoed prompt contains exact, word-boundary-clean occurrences of every label.
**Fix:** Anchor extraction on a runner-controlled sentinel / end-marker instead of re-finding the prompt text in the capture. Have the runner append a unique sentinel token to the prompt send (or reuse the existing `until`-marker plumbing), then extract the text *between* the end of the sent prompt and the sentinel — making extraction independent of how tmux wraps/renders the prompt. Take care that the sentinel cannot appear in the answer region (inject it so the agent does not echo it). Add tests for `extract_after_prompt` with multi-line and TUI-wrapped prompts asserting only the agent's answer (not the echoed prompt) is returned, plus a decide-routing test that a multi-line prompt routes to the agent's actual outcome.

> **Cross-part dependency:** pairs with **Part 1 R1-2** (decide outcome matching). F9 fixes the input
> extraction; R1-2 fixes the matching. Coordinate so both land together for correct decide routing.

---

## MEDIUM (3)

### F4. Interaction-event TOCTOU (emit before register) (FINALIZED)
**Severity:** MEDIUM — reachable on the default interaction path and fails silently (agent hangs until node timeout), but the emit→register window is narrow. Must be applied together with F3 (same lines).
**Files:** src/runtime.rs:4841 (`escalate_agent_interaction`), src/runtime.rs:4850-4859 (`emit_event("agent_interaction_required", …)` — has internal await points), src/runtime.rs:4862-4863 (`set_pending_interaction` — sender registered *after* the emit), src/runtime.rs:643 + 656-657 (`resolve_interaction` bails when no sender registered), src/runtime.rs:4865 (`receiver.await` — no inner timeout), src/runtime.rs:2554 (outer `node_timeout`), src/api.rs:1008 (respond endpoint).
**Description:** `escalate_agent_interaction` (src/runtime.rs:4841) emits `agent_interaction_required` at src/runtime.rs:4850-4859 *before* registering the reply sender via `set_pending_interaction` at src/runtime.rs:4862-4863. `emit_event` awaits (`db.append_event`, `registry.send_event`), so the task can yield with the event already delivered to the client and no sender installed. A `respond-interaction` that lands in that gap reaches `resolve_interaction` (src/runtime.rs:643), finds no sender, and bails at src/runtime.rs:656-657 — the response is rejected to the client and never delivered. The agent thread blocked at `receiver.await` (src/runtime.rs:4865) then hangs until the outer `node_timeout` (src/runtime.rs:2554) fires; no inner timeout wraps the receiver. No test covers respond-before-register.
**Fix:** Register the pending interaction *before* emitting the event: create the oneshot and insert the sender into the F3 `HashMap` keyed by `(run_id, sessionId)` first, then call `emit_event`. On emit failure, remove/`take` the just-installed sender so no stale entry lingers. This closes the race window entirely (the only remaining window has the sender installed before the client is told, which is harmless — no respond can arrive yet). Apply jointly with F3 (same function/lines). Add a test that delivers a `respond-interaction` immediately after the event is observed and asserts the response reaches the agent's receiver rather than being dropped.

### F6. Destructive blocklist only scans the capture delta (FINALIZED)
**Severity:** MEDIUM — reachable on the default path and silent (a miss just `continue`s, no log), but a miss doesn't auto-execute on its own: the destructive command must still clear a `PermissionRequest`, which re-scans `output_so_far` cumulatively. The genuine gap is destructive execution that surfaces no recognized permission prompt (auto-approve mode, or a TUI whose prompt regex doesn't match), which bypasses both tiers.
**Files:** src/tmux_exec.rs:1130-1134 (general-path blocklist matches only `new_text`), src/tmux_exec.rs:1384 (`capture_delta` — common-prefix char diff), src/tmux_exec.rs:1648 (`capture_visible_stripped` — visible-pane-only snapshot), src/tmux_exec.rs:1162-1165 (cumulative `output_so_far` destructive re-check, only inside the `PermissionRequest` arm).
**Description:** On the general output-scanning path (src/tmux_exec.rs:1130-1134), the destructive blocklist runs against `new_text` from `capture_delta` (src/tmux_exec.rs:1384), which returns only the suffix after the longest common prefix between successive *visible* pane snapshots (src/tmux_exec.rs:1648). Two real miss vectors: (a) a TUI repaint diverges the prefix early, so a destructive line may not appear as a clean appended suffix; (b) a destructive command streamed incrementally splits across two 250ms polls (`rm -r` then `f /foo`), so no single `new_text` matches the regex. The cumulative `output_so_far` re-scan that would catch these runs only inside the `PermissionRequest` branch (src/tmux_exec.rs:1162-1165), so any destructive action that doesn't surface a recognized prompt slips both tiers silently.
**Fix:** At the top of the poll loop — before the `PermissionRequest` branch — scan the accumulated `output_so_far` buffer against `destructive_regexes`, keying each match on its span/line so a given destructive line escalates exactly once (reuse the existing handled-match dedup pattern). This closes both the redraw and split vectors and covers the no-prompt path. Document the residual limitation that `output_so_far` is visible-pane-bounded, so a destructive line that scrolls fully off-screen can still escape (a later unbounded-transcript follow-up would close that). Add tests asserting the blocklist fires when a destructive command is split across two captures and when a TUI repaint reorders it, and that it escalates only once per match.

### F7. Visual idle can end active work prematurely (FINALIZED)
**Severity:** MEDIUM — real and on a shipped default (Codex is the only built-in lacking a ready marker), but mitigated: authors can set a per-node `until:` or a `ready_regex`, and the idle window is configurable. Failure is silent (node finishes `success: true`).
**Files:** src/tmux_exec.rs:1205-1218 (`poll_agent_interactive` — `if let Some(_) = progress.observe(...)` discards the `IdleReason` variant → returns `Completed`), src/tmux_exec.rs:1302-1345 (`CaptureProgress::observe` returns `Some(IdleReason::Idle)` after `idle_seconds` with no ready/until pending, :1336-1342), src/tmux_exec.rs:757-758 (records `success: true`), src/tmux_exec.rs:618 (default `idle_seconds = 2.0`), core/src/agents/builtin.rs:18-26 (Codex profile ships `ready_regex: None`; `claude`/`cursor`/`agy` set one).
**Description:** `poll_agent_interactive` (src/tmux_exec.rs:1205) treats *any* `CaptureProgress::observe` result — including plain `IdleReason::Idle` — as completion, because the `Some(_)` binding discards the variant, so visual idle is indistinguishable from a positive `ReadyMatched`/`UntilMatched` signal. `observe` returns `Some(Idle)` after just `idle_seconds` (default 2.0, src/tmux_exec.rs:618) of no visual change when no ready/until match is pending (src/tmux_exec.rs:1336-1342). The built-in Codex profile ships `ready_regex: None` (core/src/agents/builtin.rs:26), so a Codex node with no `until:` configured completes on any ~2s mid-response streaming/thinking pause, recording `success: true` (src/tmux_exec.rs:757-758) — before later output, permission prompts, or destructive-command prompts render. Because the poll loop has already exited, those safety prompts are never seen. No test covers idle-as-completion.
**Fix:** In `poll_agent_interactive` (src/tmux_exec.rs:1205), bind the `IdleReason` instead of discarding it. When both `ready_signal.regex` and `until_regex` are `None`, ignore `IdleReason::Idle` and keep polling until `timeout_duration` (or a configured sentinel) rather than declaring completion — i.e. interactive completion requires a real ready/until/sentinel signal, and bare visual idle is never accepted as success for marker-less agents. Idle-as-success remains honored only when a marker is actually configured. Document the behavior change: marker-less Codex nodes will run to timeout unless the author adds `until:`/`ready_regex`, so audit shipped Codex-using templates and add markers where needed. Add tests asserting a marker-less agent that goes idle mid-response is NOT marked complete, and that a configured ready/until marker still completes on match.

---

## LOW (2)

### F8. `wrap_keep_open` cannot keep the pane open (FINALIZED)
**Severity:** LOW — lowered from MEDIUM. The dead branch never fires for the dominant long-lived-agent path (all shipped callers spawn agents that never exit); it only misbehaves for short-lived `command` spawns, which aren't the primary path and have no test relying on the behavior.
**Files:** src/tmux_exec.rs:1759-1762 (`wrap_keep_open`); callers src/tmux_exec.rs:392, 670, 870 (long-lived agents); short-lived spawn path src/tmux_exec.rs:1416-1417.
**Description:** `wrap_keep_open` (src/tmux_exec.rs:1759-1762) builds `cd <cwd> && exec <cmd>; exec zsh -li`. The leading `exec` replaces the wrapper shell with `<cmd>`, so once `<cmd>` exits the process is gone and the trailing `exec zsh -li` is unreachable — the pane cannot stay open as the function name promises. In practice it is latent: all three callers (src/tmux_exec.rs:392, 670, 870) launch long-lived agents that never exit, so the keep-open shell is never reached anyway. The defect surfaces only for short-lived `command` spawns (src/tmux_exec.rs:1416-1417), where the pane dies immediately instead of staying open, and the branch is misleading dead code for everyone else.
**Fix:** In `wrap_keep_open` (src/tmux_exec.rs:1760), drop the leading `exec` and group the command so the wrapper shell survives its exit: emit `cd <cwd> && { <cmd>; }; exec zsh -li`. `<cmd>` then runs as a child of the wrapper; on exit, control returns and `exec zsh -li` runs, holding the pane open. Keeps the existing single-string `zsh -lic` invocation contract; reserve a bare `exec <cmd>` (no keep-open shell) for any close-with-command variant. Add a test that a short-lived command wrapped via `wrap_keep_open` leaves an interactive shell (pane stays open) rather than terminating.

### F10. Blocking login-shell subprocess runs on the async executor (FINALIZED)
**Severity:** LOW — one once-per-run stall of sub-second to ~2s on a single tokio worker when `run_as` is set; degrades concurrency under load but never deadlocks (no tty means `sudo` usually fails fast rather than hanging).
**Files:** src/tmux_exec.rs:68-78 (`resolve_tmux_bin` — synchronous `std::process::Command::output()`, import at :3), src/tmux_exec.rs:40-59 (`build_tmux_invocation`, calls `resolve_tmux_bin` at :59), src/runtime.rs:1635-1646 (`async fn execute_workflow`, unwrapped call at :1645); already-safe callers src/api.rs:117/243/688.
**Description:** `resolve_tmux_bin` (src/tmux_exec.rs:68-78) resolves the tmux binary by running `[sudo -u <user> -H --] zsh -lic 'command -v tmux'` via synchronous `std::process::Command::output()`. It is invoked through `build_tmux_invocation` (:59) directly in the body of `async fn execute_workflow` at src/runtime.rs:1645 with no `spawn_blocking`, so it blocks a tokio runtime worker for the subprocess's duration (being a `tokio::spawn`'d task does not help — a sync `.output()` still pins a worker). A login+interactive shell sources rc files (tens of ms to ~1-2s with heavy configs), and the un-`-n` `sudo` prefix can add PAM/auth latency. It runs once per workflow start, only when `run_as` is set (the `.or_else` branch maps `workflow.run_as`), with no memoization. Existing tests (src/tmux_exec.rs:1792-1875) cover invocation synthesis but never exercise `resolve_tmux_bin` or the blocking concern.
**Fix:** At src/runtime.rs:1645, move the `build_tmux_invocation` call into `tokio::task::spawn_blocking(move || …).await` so the synchronous shell-out runs on the blocking pool instead of a runtime worker. Keep `build_tmux_invocation`/`resolve_tmux_bin` synchronous so the other (already non-hot or already-blocking) callers in src/api.rs need no change. Add a test asserting `execute_workflow`'s invocation-resolution path does not run the blocking resolver inline on the async worker (e.g. via the spawn_blocking wrapper), and that a `run_as` workflow still resolves the correct invocation.

---

## Suggested remediation order (Part 2)

1. **F1** (CRITICAL sandbox bypass) — adds the `inv` param to `run_tmux_oneshot`; do first since other one-shot call sites route through it. Add the `runAs` one-shot regression test.
2. **Interaction concurrency pair (runtime.rs, do together):** F3 (per-session `HashMap`) + F4 (register-before-emit). Same functions/lines.
3. **Safety-net cluster (`poll_agent_interactive` / tmux_exec.rs interaction machinery — one engineer):** F2 (fail-open denial), F6 (cumulative blocklist scan), F7 (idle-as-completion), F9 (sentinel-anchored extraction).
4. **Config rendering:** F5 (route `build_session_args` onto the spawn path).
5. **Cleanup:** F8 (`wrap_keep_open`), F10 (`spawn_blocking` wrap).

## Cross-part coordination notes

- **`src/runtime.rs` is shared with Parts 1 & 3.** This part touches it only at the one-shot call sites
  (F1) and the interaction-escalation functions (F3/F4). The pane-registry/abort code (Part 3) and the
  scope/batch/decide-selection code (Part 1) are distinct regions, but `execute_workflow` (F1, F10) and
  `escalate_agent_interaction` are hot edit zones — sync with Part 3 (R2-7 abort, also edits
  `execute_workflow`) and Part 1 before large refactors.
- **F9 (this part) + R1-2 (Part 1)** are the decide-routing pair — land together.
- **F3/F4 (this part)** edit the respond-interaction path that also appears in `src/api.rs:1008-1018`;
  the `sessionId` echo-back is a frontend contract change — coordinate the client side with Part 4.
