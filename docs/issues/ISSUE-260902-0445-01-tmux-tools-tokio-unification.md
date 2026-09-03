---
id: ISSUE-260902-0445-01
kind: issue
category: enhancement
status: needs-triage
summary: tmux-tools exposes a blocking process API that forces SilverBond's sync/async split; unifying it on tokio is the root fix for the process boundary, sequenced after the test-seam work
blocked_by: [PRD-260902-0301-01]
---

## Triage Notes

Filed 2026-09-02, split out of `PRD-260902-0301-01` during that PRD's round-3 plan-adversary gate.
It was originally an implementation decision inside that PRD and was moved out because the PRD's
acceptance criterion — the logic tier producing identical results under CPU contention — is
satisfied by the seam work alone and never observes this migration. Keeping it in meant the epic
could not close on work its own acceptance could not see.

**The problem.** SilverBond's split between synchronous and asynchronous code is not a style choice;
it is downstream of one dependency's API shape. `src/tmux_exec.rs` is 168 synchronous functions to
3 async, importing `std::process::Command` and `std::thread::sleep` at `:1-6`, and is bridged back
to async by 8 `spawn_blocking` sites — because `tmux_tools_core` exposes a blocking API that builds
a `std::process::Command` and busy-polls `try_wait` with `thread::sleep(10ms)`.

The async side pays for this visibly. `src/api.rs` is otherwise fully async (15 `tokio::time::sleep`,
zero thread sleeps), yet `run_tmux_status_until` at `:1620` is an `async fn` that hand-rolls a raw
OS thread and bridges back over a `oneshot` purely to reach that blocking code.

Meanwhile `tokio`'s `process` feature is already enabled (`Cargo.toml:29`) and `tokio::process` is
used **zero** times — the async process API is paid for and unused.

**Why it is not a mechanical substitution.** `with_invocation` resolves the tmux invocation through a
**thread-local** whose RAII scope is a *synchronous* closure. An async future may be polled after
that scope restores the override, and may migrate threads between polls. SilverBond leans on that
scope broadly (34 call sites under `src/`), and its active-pane and interaction adapters
deliberately use `spawn_blocking` so synchronous code can block on a handle. **Defining the
cross-`await` invocation-scope contract is a precondition of the migration, not a consequence of
it.** A plan that assumes the bridges simply disappear is wrong.

**What it does not fix.** Unifying on tokio makes the *sleeps* virtualizable; it does not make the
*execs* cheap or deterministic. `tokio::process` still spawns real processes, and a task blocked on
process I/O is not a timer, so no virtual clock collapses it. The seam work in
`PRD-260902-0301-01` remains necessary regardless, which is why this is sequenced second.

**Sequencing.** Blocked by `PRD-260902-0301-01`. Doing this first would mean migrating tests that
that epic is about to rewrite.

**Repository.** `tmux-tools-core = { git = "https://github.com/pacaya/tmux-tools.git", rev =
"69173d1bf31ca790831f08cd980271ef92af02c8" }` (`Cargo.toml:28`) — a separate repository under the
same owner, pinned to a revision. This work spans two repositories and moves that pin.

**Next step for triage.** Decide scale before writing a brief. A cross-repository API change with a
precondition contract to define, its own test suite to bring along, and a pinned revision to move is
plausibly epic-shaped rather than issue-shaped; if so it routes through `/to-spec` rather than
growing an `## Agent Brief` here. It also carries a principle worth stating explicitly when it is
planned: tmux-tools should own the tests for tmux invocation behavior, which SilverBond currently
carries on its behalf.
