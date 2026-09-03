---
id: ISSUE-260901-0216-03
kind: issue
category: bug
status: done
summary: Intermittent test failures reproduced under CPU load and traced to fixed wall-clock deadlines plus a test seam placed below the process boundary; remediation is epic-scale and moved to its own PRD
---

## Triage Notes

**Reading this record.** It is a diagnostic record, not a specification: its job is to hold evidence
that `PRD-260902-0301-01` builds on. Line-number citations throughout are **as of 2026-09-02** and
against an unmodified `src/`; they locate evidence and will drift. The one part meant to be consumed
as an input rather than read as evidence is the classification table under
`### Timing-site audit`, which is keyed by owning function precisely so that it does not drift.

Filed 2026-09-01 during a completeness audit of the `docs-truth` epic, to preserve an observation
three closed records had each sighted and each declined to file. Triaged 2026-09-02. The original
body asserted a mechanism (contention on a shared real-tmux server namespace) that the readiness
gate falsified; a second body then recorded the falsification but left the real mechanism unknown.
Both are superseded by the reproduction below. The record is rewritten rather than appended to
because neither earlier description reflected what is actually happening.

### The symptom is real and still live

Reproduced 2026-09-02 on an 8-core machine. `cargo test --locked` run three times under 16
concurrent CPU-bound processes:

| Run | Result | Failing set |
|-----|--------|-------------|
| 1 | FAILED | `run_control_routes_return_typed_client_errors`, `streaming_routes_allow_same_origin_sse_and_require_ws_origin`, `run_stream_requires_matching_stream_token` |
| 2 | FAILED | `run_stream_requires_matching_stream_token` |
| 3 | green | — |

The failing set changes between runs with no code change, which is exactly what
`ISSUE-260830-1925-01` reported and what the earlier bodies preserved only as a second-hand
mention. Run 3's load may have been partially lifted near its tail; treat it as the weakest of the
three data points.

A second, independent reproduction was measured separately:
`runtime::tests::abort_kills_active_panes_before_draining_running_tasks` completed once in
**20.05s** — the full `AbortBlockingRunner` deadline at `src/runtime.rs:7949` — and in 0.84s and
0.67s on two reruns.

### Why earlier probes missed it

The readiness gate that falsified the first diagnosis ran `npm test` ten consecutive times and got
ten green. Load is the independent variable here, so idle repetition cannot falsify a
load-dependent flake however many times it runs. This is a method note, not a criticism of that
gate: its falsification of the namespace theory was correct and independently re-confirmed during
this triage (the two named runtime tests are hermetic; the real-tmux tests are guarded by
`tmux_available()` at `src/tmux_exec.rs:3932` and use per-test UUID session names at `:3945`,
`:3964`).

### Mechanism

Every reproduced failure panics at the same site:

```
panicked at tests/http_api.rs:154:9:
timed out waiting for run run_...
```

That is inside `wait_for_run`, whose deadline is declared at `tests/http_api.rs:147`:

```rust
let deadline = Instant::now() + Duration::from_secs(5);
```

A fixed wall-clock deadline sized for an idle machine. The three failing tests consume **~3.0s of
that 5s budget even on a quiet machine**, so the margin is roughly 1.7x and any contention closes
it. The `http_api` target runs 5.04s in isolation at 28% CPU — it is not CPU-bound, it is waiting.

This is a repo-wide pattern rather than one site, and it has two distinct causes that need
different fixes:

1. **Clock-bound waits** — the test waits out a production timer. Production poll intervals at
   `src/tmux_exec.rs:1612`, `:1752`, `:1812` and `src/runtime.rs:3007`, `:3022`, `:3772` (all
   250ms), plus backoff and timeout enforcement.
2. **Convergence waits** — the test polls a real resource until it changes. `wait_for_run` polls
   real SQLite every 20ms; `wait_for_path` polls the real filesystem for a sentinel touched by a
   real spawned shell script.

A virtual clock fixes the first class and does nothing for the second. The second needs a seam.

### Timing-site audit

This section is the **authority the remediation sweep consumes**. It has two parts with different
standing: a *classification*, which is a judgment keyed by function name and does not decay, and a
*scale snapshot*, which is decaying repo state recorded with the command that re-derives it.

#### Classification (contractual — the sweep's input)

Every timing construct in Rust test code was classified into one of three kinds:

| Category | Meaning | Treatment |
|---|---|---|
| **A** — liveness guard | Arbitrary headroom; expiry means a hang | Safe to raise; better replaced by a happens-before edge |
| **B** — poll tick / yield | A sleep interval inside a loop, not a deadline | Not a timing site; ignore |
| **C** — timing under test | The duration is load-bearing for an assertion | **Raising it deletes the property.** Never sweep |

The mapping is keyed by owning function rather than line number, so it survives edits above it. A
mechanical duration sweep **cannot** reconstruct it: `TMUX_CLEANUP_TEST_TIMEOUT` is used as an A-site
bound and a C-site assertion in the same test, and the `ScriptedStep::with_delay` fixtures encode
concurrency orderings as bare integers that no duration pattern matches.

Counts are per construct, so a function may own several.

| File | Function | A | B | C |
|---|---|---:|---:|---:|
| `src/api.rs` | `abandoned_pane_stream_setup_task_unlinks_the_fifos_it_creates` | 2 |  |  |
| `src/api.rs` | `pane_death_before_live_bytes_sends_pane_stream_unavailable_frame` | 2 |  |  |
| `src/api.rs` | `pane_stream_enable_descendant_pipe_cannot_outlive_setup_deadline` | 2 |  |  |
| `src/api.rs` | `pane_stream_fast_reconnect_reuses_setup_in_progress` | 1 |  |  |
| `src/api.rs` | `pane_stream_fifo_accepts_a_writer_from_a_different_uid` | 1 |  |  |
| `src/api.rs` | `pane_stream_fifo_is_cross_user_accessible_after_umask` | 2 |  |  |
| `src/api.rs` | `pane_stream_fifo_is_owner_only_for_same_user_writer` | 2 |  |  |
| `src/api.rs` | `pane_stream_owner_supervisor_clears_terminating_entry_on_panic` | 2 |  |  |
| `src/api.rs` | `pane_stream_probe_timeout_cannot_late_enable_pipe_or_leave_fifos` | 2 |  |  |
| `src/api.rs` | `pane_stream_pump_honors_explicit_drain_with_receiver_alive` | 2 |  |  |
| `src/api.rs` | `pane_stream_slow_enable_fails_setup_without_leaking` | 1 | 1 |  |
| `src/api.rs` | `pane_stream_subscribe_force_clears_when_owner_never_completes` | 1 |  | 1 |
| `src/api.rs` | `pane_stream_subscribe_waits_for_terminal_owner_exit` | 1 |  |  |
| `src/api.rs` | `pane_stream_writer_termination_closes_the_broadcast_stream` | 1 |  |  |
| `src/api.rs` | `pane_writer_that_never_starts_sends_pane_stream_unavailable_frame` | 2 |  |  |
| `src/api.rs` | `timed_out_pane_stream_enable_cannot_replace_a_newer_pipe` |  |  | 1 |
| `src/api.rs` | `timed_out_pane_stream_stop_cannot_run_after_owner_exits` |  |  | 1 |
| `src/api.rs` | `tmux_command_failure_surfaces_stderr` |  |  | 1 |
| `src/api.rs` | `tmux_commands_can_be_bounded_below_the_default_timeout` |  |  | 1 |
| `src/api.rs` | `wait_for_pending_approval` | 1 | 1 |  |
| `src/driver.rs` | `registry_cache_reloads_after_equal_length_rewrite_with_restored_mtime` |  | 1 |  |
| `src/host.rs` | `starts_host_on_ephemeral_port_and_serves_health` | 1 |  |  |
| `src/model.rs` | `validate_workflow_many_calls_against_large_subflow_within_budget` |  |  | 1 |
| `src/proc.rs` | `command_timeout_capture_is_bounded_to_output_tail` |  |  | 2 |
| `src/proc.rs` | `output_timeout_does_not_wait_for_descendants_holding_inherited_fds` |  |  | 3 |
| `src/runtime.rs` | `abort_and_wait_force_clears_when_drain_never_arrives` |  |  | 1 |
| `src/runtime.rs` | `abort_and_wait_returns_when_the_run_drains_concurrently` | 2 |  |  |
| `src/runtime.rs` | `abort_delivered_as_agent_enters_interaction_wait_terminates_the_node` | 1 |  | 1 |
| `src/runtime.rs` | `abort_kills_active_panes_before_draining_running_tasks` | 2 |  |  |
| `src/runtime.rs` | `abort_signal_is_latched_before_the_waiter_is_polled` | 1 |  |  |
| `src/runtime.rs` | `checkpoint_persist_hash_retired_when_run_registry_cleared` |  |  | 1 |
| `src/runtime.rs` | `concurrent_interactions_resolve_by_session_id` | 2 |  |  |
| `src/runtime.rs` | `decide_abort_returns_promptly_and_kills_pane` | 1 |  | 1 |
| `src/runtime.rs` | `fail_workflow_backstop_cleans_registry_on_prelude_persistence_error` |  |  | 1 |
| `src/runtime.rs` | `failed_backstop_run_cleans_non_persistent_active_panes` |  |  | 1 |
| `src/runtime.rs` | `finalize_run_cleans_registry_on_persistence_error` |  |  | 1 |
| `src/runtime.rs` | `finalize_run_distinct_logs_for_same_workflow_same_second` |  |  | 1 |
| `src/runtime.rs` | `interaction_can_be_resolved_immediately_after_required_event` | 1 |  |  |
| `src/runtime.rs` | `next_interaction_required` | 1 |  |  |
| `src/runtime.rs` | `parallel_batch_abort_cancels_pending_items_after_next_completion` |  |  | 1 |
| `src/runtime.rs` | `parallel_batch_active_panes_are_registered_per_child_cursor` | 2 |  |  |
| `src/runtime.rs` | `run` |  | 1 |  |
| `src/runtime.rs` | `run_node_with_interaction` | 1 | 1 |  |
| `src/runtime.rs` | `terminal_cleanup_kills_consumed_continue_session_source_pane` |  |  | 1 |
| `src/runtime.rs` | `wait_for_event` | 1 | 1 |  |
| `src/runtime.rs` | `wait_for_path` | 1 | 1 |  |
| `src/runtime.rs` | `wait_for_registry_empty` | 1 | 1 |  |
| `src/runtime.rs` | `wait_for_run` | 1 | 1 |  |
| `src/storage.rs` | `writer_progresses_while_a_read_connection_is_checked_out` | 1 |  | 1 |
| `src/tmux_exec.rs` | `continuous_subagent_markers_are_bounded_by_absolute_deadline` | 1 | 1 | 3 |
| `src/tmux_exec.rs` | `idle_does_not_auto_complete_and_requires_escalation` |  |  | 1 |
| `src/tmux_exec.rs` | `poll_loop_ignores_interaction_literals_in_echoed_prompt` |  |  | 2 |
| `src/tmux_exec.rs` | `poll_loop_ignores_pre_send_literals_before_prompt_echo` |  |  | 2 |
| `src/tmux_exec.rs` | `poll_loop_permission_prompt_answered_once_after_redraw` |  |  | 2 |
| `src/tmux_exec.rs` | `resolve_tmux_bin_timeout_falls_back_to_tmux` |  |  | 2 |
| `src/tmux_exec.rs` | `wait_for_child_output` |  | 1 |  |
| `src/tmux_exec.rs` | `wrap_keep_open_runs_follow_up_shell_after_short_lived_command` | 1 |  |  |
| `tests/http_api.rs` | `creates_and_approves_runs` |  | 2 |  |
| `tests/http_api.rs` | `wait_for_run` | 1 | 1 |  |

**Frontend.** One A-site (a per-test timeout override on a heavy render test in `GraphEditor`), two
B-sites (fake-timer plumbing in `AppShell.runActions`), and the C-sites in
`AppShell.validation` and `client.paneStream` — the latter are fake-timer boundary assertions pinned
to production backoff constants, deterministic already, and must not be touched.

#### Scale snapshot (non-contractual), 2026-09-02

Decaying repo state, recorded with its derivation per `AGENT-BRIEF.md` § *Assert commands, not
state*. Re-run rather than trust:

- `rg -uu -o '#\[(tokio::)?test[\](]' src/ tests/ | wc -l` → 518 test attributes (461 in-crate,
  57 across the out-of-crate targets).
- `for h in wait_for_run wait_for_event wait_for_path wait_for_registry_empty \
  wait_for_pending_approval wait_for_terminal_run wait_for_child_output; do \
  rg -F -c "$h(" src/ tests/; done` → 96 call sites across seven wall-clock wait helpers,
  `wait_for_terminal_run` much the largest at 43. (Note the `-F`: without it the parenthesis is a
  regex metacharacter and the command errors.)
- The classification table above totals 48 A, 14 B, 34 C across Rust test code.

**Raising the deadlines cannot close this class**, which is why no stopgap was taken. Only the
A-sites are raisable (48 Rust, 1 frontend), and several C-sites are themselves load-fragile — notably the
`elapsed() < Duration::from_millis(500)` promptness bounds at `src/runtime.rs:8528`, `:11360` and
`:11472`, the second of which sits inside `decide_abort_returns_promptly_and_kills_pane`, one of
the two tests `ISSUE-260826-0637-04` originally named.

Two hazards would corrupt any mechanical sweep. `TMUX_CLEANUP_TEST_TIMEOUT` (`src/runtime.rs:7444`)
is used with **opposite polarity** — a liveness bound at `:11061` and a promptness assertion at
`:11070` — so raising it silently weakens an assertion; the two uses must be decoupled first. And
the 32 `ScriptedStep::with_delay(N)` fixtures are bare `u64` millis whose relative magnitudes
encode the concurrency orderings several tests assert (`with_delay(2000)` at `:11432` is calibrated
against the 500ms bound at `:11472`), so they cannot be scaled as a group.

### Root cause: the seam is below the process boundary

`tmux_tools_core::with_invocation` is not a test double. It swaps a thread-local
`TmuxInvocation { prefix, socket, tmux_bin }`, and the consumption path builds a real
`std::process::Command` unconditionally. The only seam at the tmux boundary is *which binary gets
exec'd*. There are **63 distinct fake shell-script fixtures** across three files serving roughly 70
tests, only 5 of them behind extracted helpers; `src/api.rs` has 26 inline with no helper at all.
Every one pays a real fork, exec, pipe drain and reaper poll.

The sync/async split follows from one dependency's API shape. `src/tmux_exec.rs` is 168 sync
functions to 3 async, importing `std::process::Command` and `std::thread::sleep` at `:1-6`, bridged
back to async by 8 `spawn_blocking` sites — because `tmux_tools_core` exposes a blocking API that
busy-polls `try_wait` with `thread::sleep(10ms)`. `src/api.rs` is otherwise fully async (15
`tokio::time::sleep`, zero thread sleeps) yet hand-rolls a raw OS thread at `:1632` to reach that
blocking code. `tokio`'s `process` feature is enabled in `Cargo.toml:29` and `tokio::process` is
used **zero** times.

### Related defects found during the audit

- Four tests (`src/tmux_exec.rs:3941`, `:3960`, `:3980`, `:4013`) drive the **real tmux binary on
  the shared default socket**, creating real `sleep 600` sessions, and `return` silently when
  `tmux_available()` is false. That guard is `tmux::run(&["list-sessions"]).is_ok()`, and `is_ok()`
  holds for any process that ran regardless of exit code — the sibling `tmux_session_exists` adds an
  explicit `exit_code == 0` check, which is the tell — so it amounts to "is the tmux binary
  spawnable". Only an absent binary skips, and absence is reported as a pass. (Corrected 2026-09-02
  across two drafts: the first said these tests pass vacuously on a developer machine and would first
  execute on CI — tmux 3.7b is installed here and they do run; the second said the guard reports
  unavailable when no server is running — it does not.)
- `build_tmux_invocation` (`src/tmux_exec.rs:80`) spawns a **real interactive login shell**
  (`zsh -lic`), costing 729–998ms and sourcing the developer's shell config, for five tests that
  assert only pure struct fields. `build_tmux_invocation_without_resolving` (`:86`) already exists.
- `creates_and_approves_runs` (`tests/http_api.rs:1184`) uses two unconditional 50ms sleeps at
  `:1222` and `:1258` because `test_router()` (`:90`) discards the `Database` handle, leaving it
  nothing to poll.
- `run_cursor_task` has a retry sleep at `src/runtime.rs:3970` —
  `tokio::time::sleep(Duration::from_secs(node.retry_delay.unwrap_or(2)))` — that no test reaches:
  the attempt loop is sized by `max_node_retry_attempts(node.retry_count)`, which is 1 when
  `retry_count` is unset, so the loop breaks first. The latent 2s cost appears when a test sets
  `retry_count` and leaves `retry_delay` unset.
- `docs/testing.md` states the `tests/http_api.rs` suite "start[s] a real `ApplicationHost` on an
  ephemeral port". It does not — requests go through `tower::ServiceExt::oneshot` in-process. The
  document also lists 4 test cases where there are 17. This belongs to the `docs-truth` family
  (`PRD-260826-0009-01`), not here.

## Resolution

**Closed as `done` 2026-09-02.** This record's open question — *do these flakes still occur, and by
what mechanism* — is answered above with two independent reproductions and a traced cause. That
discharges what the record was waiting for.

Remediation is deliberately **not** carried here. The fix is epic-scale: it spans a test-tier
split, a seam extraction across three modules, two production observability changes, a CI
restructure, a written testing ADR, and a `tokio` unification in the separate `tmux-tools`
repository. Carrying that in one issue record would be the epic-in-issue-clothing failure the
readiness gate exists to catch, so it was routed to its own PRD after a grilling session that
settled the design. This record stands as the diagnostic evidence that PRD is built on.

No code changed under this record. The reproduction was run against the working tree as-is;
`git status --porcelain` was unchanged before and after.
