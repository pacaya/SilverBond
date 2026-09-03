# Spec gate round 6 — PRD-260902-0301-01

> One `cold-reader` (`model: opus`, effort xhigh) per `~/.claude/skills/to-spec/SPEC-GATE.md`
> § Cold-reader procedure, run 2026-09-02 against the working tree at branch `feature/tmux-panes`,
> HEAD `70a878b` (artifacts uncommitted). Verdict: **FAIL**.
> Every finding below was independently re-verified by the gate runner before any edit was made.

Owed dimensions derived from the scale matrix at `scale: epic`, `stakes: internal`: 1, 2, 3, 6, 7,
8, 9 always; 4 owed (the epic mints `ADR-260902-0312-01` and adds production surface); 5 owed under
the "introduces" reading (the epic declines `tokio`'s `test-util`). 10 and 11 not owed; checked
anyway. Mechanical conditions all held: zero `[ASSUMED:]`, zero `open` rows, `[OPEN: perf-test-tier]`
anchor↔ledger reciprocity holds in both directions, overlays honored. The failure is on the other
half of the procedure — that each `decided` row resolves to text which is *true*.

## decision-needed

| # | Dim | Finding | Disposition |
|---|---|---|---|
| F1 | 9 | "the capture-tail reader" is in the Logic Tier inventory (§ Testing Decisions) while § Implementation Decisions puts "output-tail capture" in the Integration Tier. `src/proc.rs` has exactly two tests; the only one exercising `read_capture` (`:149`) is `command_timeout_capture_is_bounded_to_output_tail` (`:211`), which builds `sh -c` and asserts `elapsed < Duration::from_secs(2)` (`:243`). No slice and no ADR resolves it. | **open — maintainer** |
| F2 | 4/9 | The HTTP marking exception was justified on the clock conjunct alone and its closure stated as "`-11` removes the clock waits", which the same document falsifies three paragraphs later ("today these tests fail the isolation half outright"). | **applied** — restated on both conjuncts; `-11` closes both |
| F3 | 9/1 | § Solution and § Testing Decisions still carried "the producer is merely descheduled" as the reproduced failure's mechanism, contradicting the corrected § Implementation Decisions text; and the correction's cross-reference named § Problem Statement, which never made the claim. | **applied** — mechanism corrected in both places; cross-reference re-pointed |
| F4 | 9 | The frontend sizing contract ("must exceed the slowest observed full-suite-load duration for the affected test") is satisfiable at ~9s from the PRD's own figures, while `ISSUE-260902-0747-03` binds the implementer to **>10790 ms** from an observation the PRD never states. The PRD also offered two flakes as evidence and disqualified one of them in its next paragraph. | **applied** — 10.79s observation and its source stated; floor ratified upstream; evidence base restated |
| F5 | 9 | `testing/seams` is verdicted `deferred` while the body decides the tier rule, the bound line, the seam ordering, the storage split and the acceptance command — rubric 2(d) on its face. | **open — maintainer** |

## delegable — all applied

- **F13-nit** (upgraded from the reader's `fine`): `stream_run` subscribes at `src/api.rs:892` **before** reading `db.list_events` at `:893`. The PRD said the journal read comes "before it ever touches the live receiver" — backwards, and the subscribe-first ordering is part of what makes post-hoc subscription race-free. Load-bearing for `-11`.
- **F16**: `impl Database` carries 21 methods (18 `pub`, 2 `pub(crate)`, 1 private). A fake owes **nineteen** implementations plus a constructor, not twenty — the private helper is not part of the interface. Corrected in the PRD and in `ADR-260902-0312-01`.
- **F18**: § Further Notes claimed the two residual findings were "first written down" in the PRD. Both are recorded in `ISSUE-260901-0216-03` § "Related defects found during the audit". Corrected; the same sentence had lost its verb across an earlier edit and was repaired.
- **F19**: "Between `-01` and `-11` the gate is honestly red" over-claims against an intermittent failure the reproduction measured at 2 red / 1 green. Restated.
- **F21**: `justfile` is tracked-and-modified, not untracked (`git ls-files --error-unmatch justfile` succeeds). "untracked working-tree artifact" → "uncommitted working-tree change", in the PRD and in `ISSUE-260902-0747-01`.
- **F23a**: tier-term casing drift — 59 occurrences across 8 spellings. Normalized to `Logic Tier`/`Integration Tier` as nouns and `logic-tier`/`integration-tier` adjectivally, in the PRD and the ADR.
- **F29**: the void blockquote claimed "the scan rows below are pre-amendment" while the `data/schema` row cites two of the four amendments by name. Blockquote rewritten.
- **F31**: three citations off — `tests/http_api.rs:117`→`:116`; `src/tmux_exec.rs:1035`→`:1036`; `run_checked_owned` `:2215`→`:2229` (`:2215` is the `new-session` argument). Corrected. `with_runner` additionally noted as private as well as `#[cfg(test)]`.
- **F32**: two figures attributed to "the timing audit" live outside its classification table — the five-extracted-fixtures count is in `ISSUE-260901-0216-03` § "Root cause", the convergence-wait genus in § Mechanism. Re-attributed, with the classification table named as the contractual part.

## fine — verified, no action

F6 (the four abort-path C-sites and the disjunct that covers each), F7 (the supervisor's single abort check; both `select!` blocks carry a 250 ms sleep arm and no abort arm), F8 (`src/api.rs:5220` — the drain signal precedes the await), F9 (`pump_pane_stream`, all three call sites, every conjunct), F10 (the § Testing Decisions citations), F11 (the location argument in both directions; both shims resolve to 600 s outside `cfg(test)`), F12 (no `[[test]]` entries; exactly one `#[ignore]`, at `tests/docs_catalog.rs:2705`), F13 (the observability seams; `clear` removes before cancelling; `resume_run` repeats the register-spawn-return ordering), F14 (all six decision functions; `emit_event`'s append-before-broadcast; the four call sites across two tests), F15 (the four real-tmux tests, the guard's `is_ok()` weakness, the out-of-scope shapes, the hand-written seam), F17 (`secure_database_files` before and after connect; pool `max_size(8)`), F20 (three closed records sighted and declined; 518 tests), F22 (§ Out of Scope; `docs/testing.md` lists 4 cases against 17), F24 (the four minted terms), F25, F26, F27 (no `test-util`, no `time::pause()`), F28.

## Open with the maintainer after this round

1. **F1** — the capture-tail reader's tier.
2. **F5** — whether `testing/seams` stays `deferred`.
3. **"controlled data"** — bolded and load-bearing in the PRD, the ADR and the briefs, defined only inside CONTEXT.md's Logic Tier entry, absent from the PRD's `terms:`. Mint as a term or leave as a clause?
4. **`[OPEN: perf-test-tier]`** — unchanged; the maintainer's to answer, `resolve-by: during-triage`.
