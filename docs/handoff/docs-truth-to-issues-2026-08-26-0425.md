# Handoff: docs-truth epic — /to-issues breakdown, gating, and a glossary audit that outran its scope

> Prepared 2026-08-26 04:25 from a Claude Code session in `/Users/Shared/Data/work/Programming/SilverBond`.
> Audience: a fresh Claude instance with no memory of the prior conversation. Read top to bottom.
> First handoff for this strand.

## TL;DR

`/to-issues` ran over `PRD-260826-0009-01` and produced eight issue records, `ISSUE-260826-0637-01`
through `-08`. Six are stamped `PASS`; **`-05` and `-08` are fixed but need a round-5 re-gate**.
Step 7 (the adversarial breakdown pass) has not run, and **nothing is committed**. The single most
important open item is not the gating: it is an unanswered scope question about `CONTEXT.md`, whose
audit found 3 false and 12 imprecise entries out of 39 — findings that exist **only in this
document**.

## Goal

Slice `docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md` into ready-for-agent issues. The epic
rewrites `docs/workflow-schema.md` and `docs/execution-model.md` from the Rust source, adds a
node-catalog generator, and adds the repo's first Rust CI job. It makes **no engine changes** and
edits nothing under `src/`.

Constraint that shaped everything: the PRD designates `CONTEXT.md` as the terminology authority both
rewritten documents must use, and its header rule states that an unmarked entry's definition is true
of `src/` at HEAD. That rule turned out not to hold — see **Open questions**.

## Current state

| Strand | Status | Artifact |
|---|---|---|
| Task 1 — scoped re-gate on domain terms | **done**, 3 rounds | `PRD-260826-0009-01`, `gate: passed 2026-08-26` unchanged |
| Eight issue records written | **done** | `docs/issues/ISSUE-260826-0637-0{1..8}-*.md` |
| PRD `issues:` reciprocal list | **done** | PRD frontmatter |
| Readiness gate (step 6) | **6 of 8 PASS** | stamps under each record's `## Triage Notes` |
| `-05` round 5 | **pending** — fixed, ungated | `ISSUE-260826-0637-05-node-field-tables.md` |
| `-08` round 5 | **pending** — fixed, ungated | `ISSUE-260826-0637-08-execution-model-rewrite.md` |
| Step 7 adversarial breakdown pass | **not started** | see **Next actions** |
| Commit | **not started** | working tree carries everything |
| Glossary audit | **done, unactioned** | findings in this document only |

Gate rounds per record: `-01` PASS r4 · `-02` PASS r3 · `-03` PASS r2 · `-04` PASS r2 · `-05`
ungated (r4 findings fixed) · `-06` PASS r3 · `-07` PASS r3 · `-08` ungated (r4 findings fixed).

Branch `feature/tmux-panes`. `cargo test` green: 466 tests (449 lib + 17 `tests/http_api.rs`).

## Key decisions and rationale

- **Eight slices, with `04→05→06→07` a chain and `-08` parallel to it.**
  - *Why:* the generator rewrites only between `BEGIN GENERATED`/`END GENERATED` markers and
    `docs/workflow-schema.md` has none, so the scaffold (`-03`) must precede it. `-08` is a
    different file and can run alongside once `-03` lands.
- **`-03` pins eight `## ` headings as a contract.** Every downstream record anchors acceptance
  criteria to them.
  - *Why:* it is what makes the chain falsifiable. Verified in gate round 2 that all siblings
    anchor to the exact strings.
- **CI job (`-01`) is its own unblocked slice, landing first.**
  - *Why:* it does not depend on the generator, and landing it early means the generator's
    freshness check is enforced from the moment it exists. `cargo test` was already green, so no
    swamp behind it.
- **`-08` not split**, despite being the largest authoring task.
  - *Why:* three separate cold readers reached this independently — a half-rewritten
    `docs/execution-model.md` is internally contradictory, which is worse than either endpoint.
    PRD § Further Notes records the one-author-per-document decision.
- **Drift audit corrected in place** rather than frozen as point-in-time evidence.
  - *Why:* it is the epic's cited work order, so an error in it re-infects the next reader. It
    already carried inline `*(Corrected 2026-08-25 …)*` notes, so the precedent existed.
  - *Status:* **flagged to the user, never explicitly confirmed.** Reversible.
- **CI actions provenance bounded as: prefer first-party `actions/*`; third-party only at a full
  commit SHA.**
  - *Why:* the repo has no Rust CI precedent to analogize to, so the implementer would otherwise
    mint a supply-chain decision.
  - *Status:* **flagged to the user, never explicitly confirmed.** Reversible.

## Files touched

| File | Change |
|---|---|
| `docs/issues/ISSUE-260826-0637-01-rust-ci-job.md` | new; PASS r4 |
| `…-0637-02-adjacent-doc-corrections.md` | new; PASS r3 |
| `…-0637-03-schema-doc-scaffold.md` | new; PASS r2; pins the eight headings |
| `…-0637-04-node-catalog-generator.md` | new; PASS r2 |
| `…-0637-05-node-field-tables.md` | new; **ungated**, r4 fixes applied to AC5/AC6 |
| `…-0637-06-document-level-tables.md` | new; PASS r3 |
| `…-0637-07-validation-catalog.md` | new; PASS r3 |
| `…-0637-08-execution-model-rewrite.md` | new; **ungated**, r4 fix applied to the call-frame paragraph |
| `CONTEXT.md` | 43→46 entries. Minted **Access Profile**, **Runtime Event**, **Validation Issue**. Corrected **Template Token** (×2), **Validation Issue**, **Active Pane Registry**, **Merge Key**, **Access Profile**, **Call Frame**. Extended **Profile**'s `_Avoid_`. |
| `docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md` | `issues:` list; `terms:` +5; counts 43/27→46/30; Tier P inventory gained the common `WorkflowNode` field table in 3 places; user stories 13/14/15 re-framed off a save gate that does not exist; 6 `_Avoid_`-banned labels replaced; two stale glossary-session passages rewritten; the "14 tables" figure corrected in 2 places |
| `docs/sources/workflow-schema-drift-260825.md` | corrected **D5**, **D17**, **D19**, **D35**, **D36**, **item 21** — each with a dated inline note |
| `docs/issues/ISSUE-260826-0004-01-…md` | corrected the "no event or warning is emitted" claim |

## Dead ends — things tried that did not work

**The dominant failure mode of this session was my own remedies introducing the next defect.**
`-01` ran four rounds on one genus:

1. r1: an acceptance criterion asserted a filesystem count ("the two existing workflows").
2. r2: the remedy replaced it with a manifest count ("its one third-party Rust dependency") — also
   wrong on the merits: that crate shares the repo's GitHub owner, and most deps carry semver
   ranges, so the cited precedent argued *for* the floating tags the rule forbids.
3. r3: the remedy replaced *that* with an environment inventory ("assume no YAML parser … none is")
   — false; Ruby/Psych is installed.
4. r4: passed only once the instruction became **conditional on discovery** instead of asserting
   state at all.

*Insight:* no discovery command can pin installed-interpreter state, because it is not a repo
property. The fix for that genus is never a better assertion; it is removing the assertion.

**Acceptance-criterion observables that looked sound and were not:**

- `rg -c '"type": "task"'` as a check for "no flat-shape node example survives." Wrong: `NodeKind`
  is internally tagged, so that string is what a **correct** v4 node carries inside `kind`, and the
  generated catalog emits it ~14 times. Replaced with `^## Node Types$` removal + `"kind"` presence.
- Anchoring an observable to `## Edges and conditions` — a section that does not exist at baseline
  and stays a placeholder after the issue lands. Green at both ends, verifies nothing.
- Document-wide absence checks in downstream records. Their real baseline is the tree **after their
  blockers land**, and `-03` deletes the sections those tokens live in, so they are already green
  when the issue starts.
- `rg --pcre2 --multiline '(?s)^## X\n(?:(?!^## ).)*?^### '` to prove "at least two subsections."
  The lazy quantifier stops at the first `###`; it can only ever prove one.
- "Confirm the suite goes red" as a mutation check. The freshness assertion reddens on the same
  mutations, so an implementer who built only freshness would pass all three coverage criteria.

**A false-clean verification of my own:** I grepped `CONTEXT.md` for save-time framing while
filtering out lines containing `_Avoid_` — and the faulty line contained one. It reported clean. I
only caught it because an unrelated Python arity error had prevented the write, making the "clean"
result impossible on its face. **Do not filter the verification grep by a pattern the target text
may itself contain.**

## Conventions and gotchas observed

- Gate rounds ≥4 must be **full-enumeration rounds** (READINESS-GATE § Round escalation D4):
  mechanical sweep over classes 1–5 with a per-gap ledger, plus a re-run class-6 scale check. Round
  ≥4 is also a maintainer-attention signal.
- Editing a brief after a `PASS` stamp requires a `REOPENED` stamp first (P3 atomicity). **Fix
  before stamping**, not after. Where I did edit post-verdict (`-01`), the stamp note says so.
- `#[cfg(test)]` boundaries matter for every source sweep: `src/runtime.rs:7477`,
  `src/model.rs:3248`, `src/api.rs:2749`, `src/driver.rs:1152`, `src/tmux_exec.rs:2725`,
  `src/storage.rs:1397`. A whole-`src/` event-kind sweep wrongly picks up `"first"`, `"second"`,
  `"concurrent_write"` from test modules.
- `rg` **does** descend into `.github/` when the path is named explicitly — verified, not assumed.
- The repo is per-repo mode for `/to-issues` (no `COORDINATION.md`); `docs/issues/` is flat, and
  existing records carry no `context:` frontmatter field.
- Engine facts that repeatedly surprised: **saving a workflow does not validate it** (`save_workflow`
  at `src/api.rs:535-543` normalizes and writes; enforcement is at run start, `:751-766`).
  Duplicate collector merge keys are an **error**, not a silent collapse. `branch_decision` fires on
  the defaulted branch path. Pane nodes never touch the pane registry.

## User preferences and working style

- Wants decisions surfaced as prose with rationale and honest alternatives, then a recommendation —
  not a multiple-choice prompt. `AskUserQuestion` was never used and should not be.
- Terse approvals ("agreed", "go on"). Interpret as approval of the batch just presented, not as
  blanket authority for new scope.
- Values critical pushback, including on their own framing. Two of their handoff facts turned out to
  be wrong (see **Reference snippets**) and saying so plainly was the right move.
- Long autonomous runs are acceptable; interim reports should be substantive, not status pings.

## Open questions

1. **`CONTEXT.md` audit scope — the blocking one.** 39 unmarked entries audited: 24 true, 12
   imprecise, 3 false, plus 7 of 76 `↔` citations off by a line or two. Options put to the user,
   unanswered: (a) fix all 15 now and re-gate `-08`; (b) fix the 3 false plus the entries `-08`
   depends on, file the rest; (c) file the whole audit as an issue and let the epic proceed. I
   recommended (b).
2. Should the drift audit stay corrected in place, or be frozen with corrections living only
   downstream?
3. Is the CI actions-provenance bound (first-party preferred, third-party at full SHA) what the user
   wants, or should third-party actions be forbidden outright?
4. When should the planning commit happen, and should it be one commit or split?

## Next actions

1. Get the user's answer on **Open question 1** before touching `CONTEXT.md` further.
2. Run round-5 readiness gates on `ISSUE-260826-0637-05` and `-08` (cold-reader, opus, xhigh;
   full-enumeration shape, since both are past authoritative round 3). Stamp on pass.
3. Run step 7: `/codex-researcher` per `/Users/agent/.claude/skills/to-spec/PLAN-ADVERSARY.md`
   with `scale: breakdown`, artifacts = all eight briefs + the PRD. Hunt the six wrongness classes
   plus B1–B4 (wrong dependency order · smuggled horizontal slice · missing slice · plan-vs-code
   collision). Persist the report/prompt pair as
   `docs/prd/adversary-reports/PRD-260826-0009-01-adversary-260826-round9-{report,prompt}.md`
   (rounds 1–8 already exist).
4. Commit the planning artifacts. Every brief cites glossary terms and sibling records that exist
   only in the working tree, so an agent from a fresh clone would find half its references missing.
5. Consider filing an issue for `docs/sources/prior-art-*`-style follow-ups the audit surfaced but
   the epic excludes.

## Reference snippets

**Glossary audit — the 3 false entries** (full detail exists nowhere else):

- **Agents Registry** — "record of which CLIs are installed" is false. `src/driver.rs:224` →
  `agents::Registry::load()` returns a compiled-in catalog of four builtins deep-merged with an
  optional `~/.config/tmux-tools/agents.toml`. Nothing touches PATH. Installation is probed
  separately by `check_cli(&spec.binary)` at `src/api.rs:201`.
- **Stagnation Detection** — "tracked per cursor-scoped node key" is false. `scoped_output_hash_key`
  (`src/runtime.rs:2657-2668`) keys on the **call-frame path**; the cursor id never enters it. At
  root scope the key is the bare node id, so two split siblings running the same node share one
  3-slot window and can abort the run between them. Contrast `parallel_batch_checkpoint_key`
  (`:2683-2692`), which *does* splice in `cursor_id`.
- **Decide Outcome** — "falls through to the plain success edge rather than failing" is unreachable
  from a validated document. Validation makes an unmatched outcome an error
  (`src/model.rs:2996-3005`, `:3009-3021`); the runtime only reaches that arm with a declared
  outcome (`select_decide_outcome`, `src/runtime.rs:4366-4425`).

**Glossary audit — the imprecise entries that most affect the rewrite:** Edge Outcome and Approval
Queue (failure outside a split fails the **whole run** and cancels every cursor,
`src/runtime.rs:6286`, `:6305-6316` — not just "terminal for its cursor") · Node Result ("its
cursor's result map" is run-global at root scope) · Active Pane Registry (node-id **suffix**, not
prefix, `src/runtime.rs:911-917`) · Representative Cursor (a barrier whose branches all died still
releases, with a fresh cursor from a possibly-stale snapshot, `:6041-6060`) · Cursor Terminal Status
(`cancelled` is never constructed in production) · Template Token (decide prompts resolve an
eleventh bare-name namespace, `:4222`; `sendConfig.text` is also templated) · Validation Issue
(`scope` carries `subflow:<name>`, not the bare name) · Pane Kind (`send`/`wait`/`capture` also
register panes) · Access Profile (a `spawn` node with a `command` has `access` unvalidated;
unregistered agent yields a warning, not an error) · Execution Epoch (the stated rationale describes
a guard the restart path never needs).

**Two facts from the user's own session prompt that turned out wrong**, both corrected in-repo:

- "Node results are shared between split siblings at root but private inside a subflow" — correct,
  but the prompt's framing of the `active_panes` resume question presupposed a false premise: pane
  nodes never call `resolve_active_pane`. They go through `resolve_pane_target`
  (`src/tmux_exec.rs:2454-2468`) and gate on `owned_tmux_targets`. Post-resume the node is **refused**
  with `"tmux pane target … is not owned by this run"` — traced and closed.
- "ExecutionLog … reset by restart" — `restart_from` resets only six header fields
  (`src/runtime.rs:1566-1571`); `node_executions`, `decisions`, `transitions` survive, so a restarted
  run's persisted log still carries the prior run's entries.

**The best engine discovery of the session** (from `-08` round 4, verified directly): the call-frame
pop restores **five** of six snapshotted fields. `cursor.last_output = call_result.output`
(`src/runtime.rs:4719`) — the subflow's exit-node output — not `frame.parent_last_output`, which is
written at `:3523` and **read nowhere in production**. That overwrite, plus
`insert_result_for_cursor_index` on the call node (`:4740`), *is* the subflow return mechanism. An
author expecting `{{previous_output}}` after a call node to hold the pre-call output gets the
subflow's result.

**Commands worth reusing:**

```bash
# gate status across the batch
for f in docs/issues/ISSUE-260826-0637-*.md; do
  printf '%s %s\n' "$(basename "$f" | sed 's/ISSUE-260826-0637-\([0-9]*\).*/\1/')" \
    "$(rg -o 'PASS\*\* \(round \d+' "$f" | sed 's/\*\*//' || echo ungated)"
done

# glossary shape
echo "entries: $(( $(grep -c '^\*\*' CONTEXT.md) - 1 ))  planned: $(( $(grep -c '(planned' CONTEXT.md) - 1 ))"

# outstanding planned terms (the header's own handle)
grep '(planned' CONTEXT.md
```
