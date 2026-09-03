## Plan adversary report

- scale: epic
- source-decision: author-supplied
- artifacts: docs/prd/PRD-260902-0301-01-deterministic-test-suite.md

### Findings

#### Wrong problem

none.

#### Codebase-reality collision

none.

#### Missed simpler alternative

none.

#### Hidden coupling

- **class:** hidden coupling
- **impact:** The epic can remove polling for newly started runs while leaving resumed and restarted in-process runs dependent on the same five-second database polls, so the default Logic-tier gate can remain load-dependent despite satisfying the stated `start_run` seam.
- **evidence:** The artifact says the runtime event stream is the “general seam” for intermediate states, but makes its race-free contract specific to `start_run`: because that method returns the id after spawning, “`start_run` hands back the receiver, or accepts a pre-registered run id,” with journal-plus-replay only a fallback (`PRD`, `## Implementation Decisions`, lines 155-171). The repository has two sibling lifecycle entries with the same ordering that the artifact does not cover. `RuntimeContext::resume_run` registers and calls `spawn_supervised_run` before returning `Result<()>` (`src/runtime.rs:1443-1477`), while `restart_from` generates a new run id, registers and spawns it, and only then returns that id (`src/runtime.rs:1480-1626`). Existing in-process tests immediately follow those calls with the clock-bound helpers the epic intends to eliminate: `orphan_waiting_approval_cursor_is_reconciled_on_resume` calls `resume_run` and then `wait_for_event` (`src/runtime.rs:8260-8318`), `restore_pending_approval_fallback_binds_rehydrated_cursor_once` does the same for `approval_required` and then also calls `wait_for_run` (`src/runtime.rs:10825-10905`), and `restart_from_advances_epoch_and_drops_stale_collector_arrivals` calls `restart_from` before polling the new run (`src/runtime.rs:12982-13045`). Both helpers race a 20ms database poll against a five-second wall-clock deadline (`src/runtime.rs:9858-9904`). What race-free observability contract covers `resume_run` and `restart_from`, or are all tests of those lifecycle paths intentionally outside the Logic tier?

#### Sequencing errors

none.

#### Unjustified stack/dependency assumptions

none.
