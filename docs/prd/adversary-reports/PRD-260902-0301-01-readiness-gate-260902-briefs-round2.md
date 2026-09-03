# Readiness gate over the twelve briefs — 2026-09-02

> Twelve `cold-reader` agents (`model: opus`, effort xhigh), one per record, per
> `~/.claude/skills/triage/READINESS-GATE.md` § Protocol. Round 2 for `-01`…`-10`; round 1 for `-11`
> and `-12`, neither of which had been gated before. Working tree at branch `feature/tmux-panes`,
> HEAD `70a878b`, artifacts uncommitted.
> **Every finding recorded here was independently re-derived by the gate runner before being acted on.**

## Outcome

| Verdict | Records |
|---|---|
| PASS | `-03`, `-08` |
| FAIL | `-01`, `-02`, `-04`, `-05`, `-06`, `-07`, `-09`, `-10`, `-11`, `-12` |

`-08`'s PASS turned on the baseline question now settled in the PRD (§ Testing Decisions, "A slice's
baseline is the tree after its blockers land"); under the rubric's literal default baseline its AC4
would have fired class 9 arm A.

## The finding that changed the epic

**`-11` / PRD — a fourth process-spawn site in the HTTP target, unreachable by the approval-only fix.**

```
tests/http_api.rs:889  test_node_accepts_v3_task_node  → asserts OK and preview["success"] == true
src/api.rs:680         spawn_blocking(build_tmux_invocation(…))     ← unconditional
src/tmux_exec.rs:121   Command::new("zsh").args(["-lic", …])        ← real interactive login shell
src/runtime.rs:1749    run_node_preview → run_tmux_oneshot          ← no NodeRunner port on this path
```

The run path is shell-free by contrast (`resolve_workflow_invocation_with` returns
`run_scoped_tmux_invocation` when `run_as` is `None`), so the preview path is a genuinely separate
genus. Two corollaries: `create_terminal_echo_run` is called by **two** tests while the reproduction
names **three** — `run_control_routes_return_typed_client_errors` carries its own inline echo workflow
— and `-11`'s positive control is red today because of `test_node`, not the fixture, so it would stay
red after the work as scoped.

This is the third occurrence of one failure mode: a rule with two conjuncts, verified only on the
conjunct the prose foregrounds. Round 1–5 checked the clock half. Codex found the isolation half.
This round found that the isolation half had been checked at one site and generalised.

## Findings that routed upstream to the PRD

| Source | Finding | Verified by |
|---|---|---|
| `-07` | § Solution named **two** functions on the clock-identity path; the closure is **five of six** (only `select_next_decision` is clean). The mint is `.unwrap_or_else(new_cursor_id)` — a function *reference*, so call-syntax searches miss it | reachability closure over `src/runtime.rs` |
| `-05` | "terminal waits … a minority of the sites" is false: **42** terminal call sites against **36** for the other four helpers combined | `rg -F -c` per helper, minus definitions |
| `-02` | fixtures come in **three** forms, not two — the third is module-local helpers that write the body *and* set the mode (`src/runtime.rs:8920`, `:8948`, `:11293`; `src/tmux_exec.rs:3406`, `:3518`), which are the "only 5 behind extracted helpers" the diagnostic record counts | enclosing-function attribution over the three modules |
| `-10` | neither PRD nor ADR said what makes the tier boundary "mechanically obvious" or who decides, so the advisory fallback discharged every criterion by declaration | resolution of both cited clauses |
| `-01` | "exactly one temporary exception" was unreconciled with `[OPEN: perf-test-tier]`; the perf test asserts elapsed time (`src/model.rs:6644-6652`) and the brief forbids marking it, so two unmarked rule-failing sets exist | read both passages against AC1's forbidden-mechanism list |
| `-08`, `-05`, `-07`, `-12` | sequenced slices' pre-change criteria are false against the tree at authorship and true only at the post-`-01` baseline | executed each observable |

All six are applied in the PRD; the `-10` criterion is also applied in `ADR-260902-0312-01`
§ Consequences.

## Per-record blocking findings

- **`-01`** — the exception/perf-test contradiction above; and `writer_progresses_while_a_read_connection_is_checked_out`, which the PRD twice assigns to "the slice that draws the boundary" (`:522`, `:554`), is never mentioned in the brief while AC1's rule and the brief's storage paragraph classify it oppositely.
- **`-02`** — the third fixture form unnamed; AC1 and AC2 contradict each other on the shebang literal; AC2 as written forbids two directory-permission tests (`src/api.rs:4237`, `:4842`) that no exclusion covers and the discovery command cannot see; the genus command misses 16 `from_mode(0o755)` sites.
- **`-04`** — class 9 arm B req. 2 on AC6: its failure signal is one the *passing* path already emits (`src/api.rs:4834`). AC5 carries "a signal the passing path never emits" and an acted-on complement; AC6 has neither. Seven class-7 count rows. Class 6 prong (b) fires.
- **`-05`** — "every API test that calls the pending-approval helper also writes an executable fixture" is false: **9 of 14** callers write none, so the brief leaves nine Logic-Tier-clean tests marked. Class 6 prong (b) fires; the split's counter-argument was the false minority claim.
- **`-06`** — excludes `abort_kills_active_panes_before_draining_running_tasks` (an abort-path elapsed assertion at `src/runtime.rs:11070`) on the grounds that the audit scores it **A** — the instrument the ADR forbids for that purpose — and does so in `## Triage Notes`, which no implementer reads. The audit at `:41` names that same test as the epic's **second independent reproduction**. AC3's observable (`rg abort_signal`) is green at baseline. `TMUX_CLEANUP_TEST_TIMEOUT` has a third role at `:7950`.
- **`-07`** — the two-name clock-identity enumeration, with a tier consequence attached.
- **`-09`** — the premise "no current test reaches it" rests on `retry_count`, but the only discovery command searches `retry_delay`, whose hits are mostly `retry_delay: None` — the trigger condition, rewarding the inverted reading. Separately: the prescribed `Duration`-returning shim moves `from_secs` out of `run_cursor_task`'s extent, and `check_citation` (`tests/docs_catalog.rs:1820-1831`) then reports `DiscriminantOutsideSymbol`, turning the live out-of-crate test `workflow_schema_citations_resolve` red. The guarded citation is `docs/workflow-schema.md:1073`.
- **`-10`** — class 9 arm B req. 1 on AC5 and AC6: both are leave-alone assertions satisfiable by a check that reports nothing, with no acted-on witness. AC7 omits the acceptance's intermittency limit entirely (`-11` carries it twice). AC9's control needs an exception-entry form `-01` does not pin.
- **`-11`** — the fourth spawn site above; the inline duplicate; AC5's control is an ambient `PATH` manipulation rather than a mechanical copy (arm B req. 1); AC9's four-term deterministic pairing is one-quarter false.
- **`-12`** — all 14 `with_delay` owners end at `wait_for_terminal_run` → `wait_for_run`'s 5s deadline (`src/runtime.rs:9862`), so removing the sleep is necessary and not sufficient; `-05` and `-10` state this correctly and `-12` does not. The excluded test carries **four** `with_delay` calls, not one. The discovery command needs a required pin.

## Batch-level authoring defect

Class-7 kind (a) — a count or inventory asserted as fact with no command deriving it — fired on
**8 of 12** records: `-02`, `-04`, `-05`, `-06`, `-07`, `-09`, `-11`, `-12`. `-01`, `-03`, `-08` and
`-10` were clean. Remedy is substitution by discovery command, never a refreshed number.

## Maintainer decisions taken on these findings

1. The preview endpoint's task test is **Integration Tier**, by the rule the PRD already stated for endpoint tests requiring task execution. No new production surface; nothing relocates, since it is not in the reproduction.
2. **`-05` splits** at the terminal/intermediate line; **`-04` does not** — its batches share the same four test functions, so the merge seam costs more than the split buys.
3. A slice's **baseline is the tree after its blockers land**, stated per brief.
4. `-01` cannot be gated before `[OPEN: perf-test-tier]` is answered; "one exception" counts decided exceptions, not unmarked rule-failing tests.
