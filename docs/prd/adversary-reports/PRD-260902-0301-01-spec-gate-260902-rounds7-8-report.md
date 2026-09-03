# Spec gate rounds 7 and 8 — PRD-260902-0301-01

> One `cold-reader` per round (`model: opus`, effort xhigh), 2026-09-02, per `SPEC-GATE.md`
> § Cold-reader procedure. Both returned **FAIL**. Every finding acted on was re-derived by the gate
> runner first. Round 6's report is beside this file; the twelve-brief round is in
> `PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`.

## Round 7 — FAIL. All findings applied.

| # | Finding | Status |
|---|---|---|
| 9A | `src/host.rs:129` `starts_host_on_ephemeral_port_and_serves_health` binds a real `TcpListener` (`:33`) and gates on `wait_for_health` (`:80-95`), a convergence wait that `bail!`s at a deadline — so it fails **both** halves of the Logic Tier rule, sits in the gate, is in the contractual audit table (`ISSUE-260901-0216-03:132`), and appeared in **no** tier inventory and **no** brief through seven rounds. Same silence covered the self-exec pair (`src/model.rs:5221`, `:5301`). | Applied — both named in the Integration Tier inventory; assigned to `-01` by maintainer decision; `--exact` selector hazard stated |
| 4A | The clock-identity membership rule was anchored on **one of four** clock-derived identity mints (`src/runtime.rs` mints `run_`, `cursor_`, `frame_`, `family_`, all `Uuid::now_v7`) | Applied — rewritten to quantify over every clock-derived identifier, with both derivation hazards named |
| 4B | The abort-promptness set of four omits a fifth elapsed assertion at `src/runtime.rs:11070`, inside `abort_kills_active_panes_before_draining_running_tasks` — the test the diagnostic record names as the epic's **second reproduction** (20.05s / 0.84s / 0.67s) | Applied — the bullet now states no count and requires deriving from the audit table *and* an independent `.elapsed()` sweep, asserting the two disagree |
| 9B | Amendment residue from round 6's decision A: "the target stays unmarked" while a test inside it was being marked | Applied — rephrased, with a rider that "the HTTP target carries no Integration Tier marker" is a wrong criterion phrasing (which is how `-01` AC2 and `-11` currently read) |
| 9C | No brief states its baseline | Discounted — artifact of ordering; the repair pass had not run |
| 9D | `testing/seams` `deferred` under-reports the dimension | Not acted on; recurs as round 8 F7 |

## Round 8 — FAIL. **No findings applied. This is the open work.**

Round 8's priority was to test whether round 7's *structural* remedy held. It held where applied — the
abort-path derivation and the inventory disclaimer both survived falsification — **but it was applied
to two passages, not to the class.**

| # | Dim | Finding | Verified |
|---|---|---|---|
| **F2** | 9 | **The fifth instance of the recurring pattern, inside round 7's own fix.** The rephrase to "**the run-stream tests** stay unmarked" is too small: the reproduction's failing set is `run_control_routes_return_typed_client_errors`, `streaming_routes_allow_same_origin_sse_and_require_ws_origin`, `run_stream_requires_matching_stream_token` — the first is a run-**control** test (`tests/http_api.rs:434`). Under `-01`'s marking rule it leaves the gate, which is what the exception exists to prevent. | Yes — audit `:29-33` and the three `async fn` sites |
| **F1** | 9 | "Two tests are unmarked while failing the Logic Tier rule" is false; the population is **five**, because the paragraph counts "the HTTP target" as one test while the document twice says membership is per test. The fifth is `creates_and_approves_runs`, whose two unconditional 50ms sleeps (`tests/http_api.rs:1222`, `:1258`) fail the rule's first clause. **The wrong count has already propagated to `-01`.** | Yes |
| F3 | 9 | The vacuous-skip constraint is scoped to tmux; the tree has three guard families with the identical defect (`tmux_available` `src/tmux_exec.rs:3932`; `zsh_available` `:3924`; the root/sudo/daemon guard `src/api.rs:4827-4836`). `-04` already generalises, so the brief's scope exceeds what the PRD authorizes | Yes |
| F4 | 9 | "The remaining wall-clock users are the scripted-delay fixtures" is a closed set the tree falsifies (11 `tokio::time::sleep` in `src/api.rs` tests, 4 non-`ScriptedRunner` in `src/runtime.rs`, `thread::sleep` in two modules, plus audit C-sites). `delegable` — the virtual-clock decision does not turn on it | Not independently re-derived |
| F5 | 3 | Three different quantifiers for the clock-identity rule: CONTEXT.md unqualified, ADR "state or event identity", PRD "**checkpoint** state". Mints exist outside the PRD's quantifier (`src/api.rs:681`, `src/tmux_exec.rs:1000`, `:2597`) | Yes |
| F6 | 4 | The "*What the fixture actually does*" paragraph names `test_router_with_security` but makes claims true only of `create_terminal_echo_run` (`tests/http_api.rs:185-215`), which it never names. A slice converting "the shared fixture" as cited converts the wrong function | Yes |
| F7 | scan | `testing/seams` `deferred` cites only the ledger slug while three sections decide the dimension | Open |
| F8 | — | The void blockquote says "recurred four times" above a six-item list, and "seven rounds" where the directory holds nine numbered adversary rounds plus the codex breakdown. Transient text | Open |

### Round 8's closed-set sweep

Mechanical extraction of every set-asserting sentence, after masking artifact IDs and inline code:
234 sentences carried a quantifier or numeral, **98** survived the filter, **2** more found by reading.
**Ledger size 100. Six defective.** Everything the PRD *decides* survived falsification — the tier rule,
the isolation rule, the liveness line, the seam constraints, the storage split, the `:memory:` grounds,
the acceptance construction, the virtual-clock rejection.

## The pattern, stated once

Five rounds have found the same defect genus: **a closed set asserted where a derivation was owed.**

1. Rounds 1–5 — the clock half taken for the whole tier rule (found by Codex)
2. Briefs round — the fixture taken for the only isolation violation (`-11`)
3. Briefs round — two functions where the reachability closure was five (`-07`)
4. Round 7 — one clock-derived mint where the runtime has four; four abort assertions where a sweep finds five; a tier inventory read as a roster
5. Round 8 — **inside the round-7 fix**: "the run-stream tests" excludes one of three reproduced failures

The per-instance remedy has a demonstrated failure rate, including when applied deliberately by an
author who knows the pattern.
