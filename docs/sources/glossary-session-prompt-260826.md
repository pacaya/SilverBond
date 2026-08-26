# Glossary session — prompt for a fresh Claude Code session

Paste everything below the line into a new session in this repo.

---

I need to settle SilverBond's domain glossary before the `docs-truth` epic can be sliced into
issues. This is a decision session: I approve each term individually, and nothing gets written
until I do.

**Read first, in this order:**

1. `CONTEXT.md` — the current glossary (16 entries, seeded by RDMP-260815-2009-01).
2. `docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md` — specifically `## Open Questions` →
   `### glossary-minting`, which is the blocking question, and the "Terminology follows an approved
   glossary" decision in `## Implementation Decisions`.
3. `docs/sources/workflow-schema-drift-260825.md` — the 160-finding drift audit, which is where
   every candidate term below comes from. Sections 2.1–2.7 carry the execution-model concepts.
4. `/Users/agent/.claude/skills/domain-modeling/CONTEXT-FORMAT.md` — the entry format.

**Why this session exists.** `docs-truth` rewrites `docs/workflow-schema.md` and
`docs/execution-model.md` from the Rust source. Both documents must use one name per concept, and
the glossary is not currently safe to follow. Two problems:

## Half (a) — terms the glossary lacks

The rewrite needs roughly twenty concepts `CONTEXT.md` does not carry, all describing engine
behavior that exists today. Candidates, with the audit finding that motivates each:

| Candidate | What it names | Audit ref |
|---|---|---|
| Node Kind | the 14-variant tagged union a node's `kind` field holds | 1.2 |
| Edge Outcome | `Success \| Reject \| Branch \| LoopContinue \| LoopExit` | D45 |
| Split Family | the group a split's child cursors belong to, carrying the failure policy | D47 |
| Copy-on-Split | children deep-copy the parent scope; sibling writes never reconverge | D4 |
| Call Frame | the saved caller scope + separate result namespace pushed on subflow entry | D5 |
| Dispatch Tier | runner kinds go to a `JoinSet`; the rest resolve inline in the scheduler loop | D32 |
| Collector Barrier | the keyed join point where parallel cursors wait | D16 |
| Barrier Key | `(scope, collector_id, execution_epoch)` | D16 |
| Merge Key | `edge.label` falling back to `edge.from`; duplicates collapse a slot | D17 |
| Representative Cursor | the surviving cursor a barrier release picks; the others are deleted | D21 |
| Parallel Batch / Batch Item | the fan-out node and its ephemeral per-item cursors | D3, D55 |
| Decide Node / Outcome | the LLM-routed branch node and its declared labels | D33, D34 |
| Pane Node Family | `spawn`/`send`/`wait`/`capture`/`kill` | D35 |
| Active Pane Registry | per-run pane resolution incl. the `active`/`current` aliases | D35 |
| Execution Epoch | the counter a restart bumps | D13 |
| Node Result | the write-once per-node output record | D6 |
| Template Token | the ten `{{…}}` substitution forms | D25 |
| Skip Condition | the pre-execution `contains`/`not_contains`/`regex` form | D30 |
| Approval Queue | approvals serialized one at a time | D43 |
| Stagnation Detection | three identical outputs abort the run | D48 |
| Execution Log | the persisted per-run record written at finalize | D61 |
| Run Status vs Cursor Terminal Status | two different enums people conflate | D2, D46 |

**Three of these are genuinely contentious and I want to argue them:**

- **"Barrier" vs "Join"** — the code says barrier; "join" may read better to newcomers.
- **"Dispatch Tier"** — there is no such symbol in `src/`. `is_runner_node_kind`
  (`src/runtime.rs:2086-2098`) is the nearest thing. Is this a real concept or one we are inventing?
- **"Representative Cursor"** — the symbol exists (`representative_cursor_id`,
  `src/runtime.rs:6041`), but the mechanism is lossy and may not survive the `cursor-state` epic.
  Should we name something we expect to remove?

## Half (b) — four existing terms that are false of the engine

This is the sharper half. `CONTEXT.md` was seeded as the **v5 target-state** glossary, so four of
its entries describe behavior that does not exist yet and contradict the v4 engine `docs-truth`
must document:

| Term | Glossary says | Code says |
|---|---|---|
| **Workflow** (`CONTEXT.md:7`) | schema `version: 5` | `WORKFLOW_SCHEMA_VERSION = 4` (`src/model.rs:10`) |
| **Variable** (`:13`) | "JSON-valued" | `BTreeMap<String, String>` (`src/runtime.rs:223`) |
| **Condition** (`:17`) | nested `all`/`any`/`not` AST with `onMissing` | flat `{field, operator, value}` (`src/model.rs:128-134`) |
| **Collector** (`:16`) | branch results aggregate into "parsed JSON values" | strings today |

An agent told to "use the glossary's vocabulary" would write *"a Condition is a nested
`all`/`any`/`not` AST"* into a **v4** reference — reintroducing exactly the falsehood class the
epic exists to delete. `Assignment` (`:15`), `Profile`, `Script Node`, `onMissing` and
`Failure Outcome` are in the same category: named in the glossary, absent from `src/`.

**The decision I need from you is a rule, not just a list.** Options I can see, and I want your
critique before I pick:

1. **Dual definitions** — each affected entry carries a present-state line and a target-state line,
   with the ADR that will make the target true.
2. **Present-state glossary + forward pointers** — entries describe v4, with `→ ADR-…` for the
   change coming.
3. **Docs define locally** — the glossary stays target-state, and the v4 documents define present
   behavior in their own text, linking the glossary for where it is heading.
4. Something better.

Whichever we pick has to survive six schema-changing epics, each of which will flip some entries
from target to present.

## How I want the session to run

- Explore the code to ground every term — cite `file:line` for what each concept actually is today.
- Work through half (b) **first**: the rule governs how half (a) entries get written.
- Then half (a), a few terms at a time. For each: proposed name, one-line definition, the code it
  points at, and any alternative name worth considering. Give me genuine pushback where a name is
  bad or a concept is invented.
- **Do not write to `CONTEXT.md` until I approve terms explicitly.** Batch the approved ones and
  write them in `CONTEXT-FORMAT.md` shape when we are done.

## On completion

1. Update `CONTEXT.md` with the approved entries.
2. In `docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md`: delete the `[OPEN: glossary-minting]`
   anchor and its `## Open Questions` ledger entry, and flip the `domain terms` row in
   `## Dimension Scan` from `deferred` to `decided` with a citation to `CONTEXT.md`. Resolving the
   question must clear **both** anchor and entry — that is the marker law in
   `/Users/agent/.claude/skills/to-spec/SPEC-GATE.md`.
3. Tell me whether the PRD needs a re-gate on the `domain terms` dimension before `/to-issues`
   runs, and if the rule we picked changes any body text, make those edits too.
4. Then `/to-issues` is unblocked.

## Context you should know

The PRD went through eight adversarial gate rounds; its evidence is in
`docs/prd/adversary-reports/`. Two issues were filed during that pass and are untriaged:
`ISSUE-260826-0004-01` (unreachable orchestrator branch fallback, plus the silent first-branch-edge
default) and `ISSUE-260826-0240-01` (`ARCHITECTURE.md` claims four "current first-class node types"
where the engine has fourteen). Nothing from this work is committed yet.
