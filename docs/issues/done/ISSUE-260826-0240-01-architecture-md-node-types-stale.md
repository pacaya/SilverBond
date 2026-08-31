---
id: ISSUE-260826-0240-01
kind: issue
category: bug
status: done
origin: docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md
summary: ARCHITECTURE.md states four "current first-class node types" where the engine has fourteen, and docs/backend.md names ten
claimed_by: implement-issue@Mac-mini-4
claimed_at: 2026-08-31T00:07:03Z
---

## Agent Brief

**Category:** bug
**Summary:** Correct the node-kind lists in `ARCHITECTURE.md` and `docs/backend.md` so neither
document presents a strict subset of its enum as the complete set.

**Current behavior:**
`ARCHITECTURE.md` carries a `### Node types today` section whose bullet list is introduced by the
sentence "Current first-class node types:". That list omits node kinds the engine executes. Because
the introducing sentence asserts completeness, this is a false statement about the present rather
than merely an incomplete list, and the same document contradicts itself elsewhere by naming
omitted control kinds in its description of the tmux layer.

`docs/backend.md` has the same defect in a different spelling. Its `**Core enums:**` list carries a
one-line entry presenting `WorkflowNodeType` as a short list of Rust variant names. That entry omits
variants the type actually declares, in the section that purports to define it.

Derive both enums' real variant sets yourself, from the tree as it stands when you do the work —
these commands return each enum's declaration block for you to read the variants off:

```
rg -n --pcre2 --multiline '(?s)^pub enum NodeKind \{.*?^\}' src/model.rs
rg -n --pcre2 --multiline '(?s)^pub enum WorkflowNodeType \{.*?^\}' src/model.rs
```

Neither command is pinned to a commit, deliberately: this brief wants each set as it stands at
implementation time, not a set frozen at authoring time. Do not take any membership claim in this
record's `## Triage Notes` as current — that is filing narrative, and the commands above are the
authority.

**Desired behavior:**
Each of those two lists enumerates the complete variant set of the enum it describes, derived from
the Rust source rather than copied from another document or from this brief. Each keeps its own
existing spelling convention: `ARCHITECTURE.md` lists serialized wire tags (the lower-case
`snake_case` form the workflow document and `/api/capabilities` use), `docs/backend.md`'s Core-enums
entry lists Rust variant identifiers, all on one physical line as that list's other entries are.
Nothing else in either document changes.

**Key interfaces:**
- `NodeKind` — the serde-tagged workflow-model enum. Its `tag = "type"` with
  `rename_all = "snake_case"` is what produces the wire tags a workflow document and the
  capabilities endpoint carry, so the **variant declarations themselves**, mechanically lower-cased,
  are the authority for `ARCHITECTURE.md`'s list. A hand-written string projection over the node
  kinds also exists in the same module, but read it only as a cross-check: it delegates to the other
  enum below, and hand-written exhaustive projections over these enums are exactly the drift-prone
  shape that `ISSUE-260826-0520-01` exists to remove. The declaration is the safer source.
- `WorkflowNodeType` — a separate bare enum in the same model module carrying one unit variant per
  node kind, with a `From` conversion bridging it to `NodeKind`. Its variant identifiers are the
  authority for the `docs/backend.md` Core-enums entry. It is a distinct type, not an alias; read
  its own declaration rather than assuming the two enums agree.

**Acceptance criteria:**
- [ ] The bullet list under `ARCHITECTURE.md`'s `### Node types today` heading contains exactly one
      entry per wire tag `NodeKind` serializes, and no entry that is not one. (The prose sentence in
      that section about planned `join` nodes is not a list entry; it stays — see Out of scope.)
      Observable, both bounded to that section so a mention elsewhere in the file cannot satisfy
      them — each returns a match after this change, and neither matches before it:

      ```
      rg --pcre2 --multiline -n '(?s)^### Node types today\n(?:(?!^### ).)*?\bparallel_batch\b' ARCHITECTURE.md
      rg --pcre2 --multiline -n '(?s)^### Node types today\n(?:(?!^### ).)*?\bspawn\b' ARCHITECTURE.md
      ```

      The second is the one that proves the bound: the file names `spawn` elsewhere today, so an
      unbounded search for it would return a match before this change.

- [ ] The `WorkflowNodeType` entry in `docs/backend.md`'s `**Core enums:**` list names every variant
      of that enum, and remains a single physical line. Observable, both anchored to that one entry
      — each returns a match after this change, and neither matches before it:

      ```
      rg -n '^- `WorkflowNodeType`.*\bParallelBatch\b' docs/backend.md
      rg -n '^- `WorkflowNodeType`.*\bRunAgent\b' docs/backend.md
      ```

      The second is the one that proves the anchor: the file names `RunAgent` on a different line
      today, so an unanchored search would return a match before this change.

- [ ] Each list's membership equals its enum's variant set, and the two lists agree with each other
      kind for kind, allowing for the two spelling conventions. Verify by re-running the two
      discovery commands above against the tree you are working in and comparing, not by reading
      membership off this brief.

- [ ] `cargo test --locked` and `npm test` pass unchanged. (Preservation criterion: this change
      touches only prose, and nothing should move.)

**Out of scope:**
- **`ARCHITECTURE.md`'s `### Edge outcomes today` section.** Its list was checked during triage
  against `WorkflowEdgeOutcome` and was complete and correct. Confirm that still holds before
  leaving it alone — `rg -n --pcre2 --multiline '(?s)^pub enum WorkflowEdgeOutcome \{.*?^\}' src/model.rs`
  returns that enum's declaration — and if it no longer does, stop and report rather than folding a
  second correction into this record.
- **`ARCHITECTURE.md`'s sentence that explicit `join` nodes are still planned.** Verified during
  triage: neither enum declares a join variant, and the two discovery commands above let you
  re-confirm it. The sentence is accurate and is not part of the bullet list. Keep it.
- **`docs/backend.md`'s description of which node kinds the tmux node-routing helper dispatches.**
  That is a claim about one function's dispatch table, not about enum membership. It is an exact
  match for the kinds that helper actually routes — the remaining kinds hit an arm that refuses them
  by name — so it is correct as written. Leave it alone.
- **Building any mechanism that keeps these lists in sync.** A generated node-kind catalog already
  exists for `docs/workflow-schema.md`, driven by an in-repo test that regenerates fenced blocks
  from constructed Rust values and fails when they are stale. Extending it to cover narrative prose
  in `ARCHITECTURE.md` and `docs/backend.md` is a design decision about marker placement and format
  that this issue does not own. Correct the prose by hand.
- **The canonical schema-version statement in `ARCHITECTURE.md`,** which the `typed-contracts` epic
  already pins for editing at the version bump. Do not touch it here, and do not attempt any wider
  version sweep.
- **Any change to Rust source.** Both enums are correct as they stand; only the two documents are
  wrong.

## Context Pack — generated at claim (2026-08-31T00:07:03Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01, reached via this record's `origin:`):
- Docs are written from the Rust source, not copied between documents; `src/model.rs` is the stated authority for node kinds.
- Hand-maintained enumerations are the named failure mode — the PRD attributes the "4 of 14 kinds" defect to exactly this shape.
- `ARCHITECTURE.md` and `docs/backend.md` are explicitly **out of scope** for that epic, and their disposal is delegated to this record (ISSUE-260826-0240-01) so the exclusion points at a real record.
- Enumeration correctness is defined against serde's own derives: `NodeKind` is internally tagged (`tag = "type"`, `rename_all = "snake_case"`), so the wire tags are the variant identifiers mechanically lower-cased; `WorkflowNodeType` is a separate bare enum bridged by a `From` conversion.
- No engine code changes; this class of work reads `src/` and writes docs only.
- v4 is canonical and the v5 bump (ADR-260815-2009-01) is a forward reference only — do not touch the canonical-version statement here.

**Test seam & Testing Decisions:** the seam this slice's AC echoes is the committed markdown itself, observed by section- and line-anchored `rg` searches (the `### Node types today` bullet list in `ARCHITECTURE.md`; the single-physical-line `WorkflowNodeType` entry under `**Core enums:**` in `docs/backend.md`). Membership is re-derived at implementation time from the two enum declaration blocks in the model module — deliberately unpinned, since a variant landing before the work does must be reflected. Related PRD Testing Decisions: the generated node catalog for `docs/workflow-schema.md` is guarded by a freshness + set-equality test against the serde-harvested variant lists; that machinery covers only the generated blocks and does **not** reach narrative prose in these two documents, which is why this correction is by hand. Preservation seam: `cargo test --locked` and `npm test` must pass unchanged (prose-only change).

**ADRs:**
- ADR-260815-2009-01 — Typed workflow contracts over a JSON-valued variable store (accepted); relevant only as the forward reference that pins the v4→v5 statement for a later epic — not to be acted on here.
- (Record declares no `adrs:` frontmatter; the line above comes from the parent PRD's `adrs:` via `origin:`.)

**Terms:** (record declares no `terms:` frontmatter; glossary lines below are the entries this slice's vocabulary touches)
- `Workflow` — a versioned node/edge graph definition (schema `version: 4`; v2–v3 normalize forward at ingest), including its subflow catalog; bumps to v5 under ADR-260815-2009-01.
- `Subflow` — a workflow invoked as a callable block from another workflow via a call frame.
- `Collector` — the barrier node where parallel cursors join and branch results aggregate into a keyed `{inputs, summary}` object.
- `Collector Barrier` — glossary `_Avoid_` list bans "join" as a noun; relevant because the `join`-nodes-are-planned sentence in the target section stays untouched.

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md · docs/adr/INDEX.md · CONTEXT.md · docs/sources/workflow-schema-drift-260825.md (item X4)

## Triage Notes

Found on 2026-08-25 during the gate pass on PRD-260826-0009-01 (`docs-truth` epic of
RDMP-260815-2009-01). Filed rather than folded into that PRD: the epic's named deliverable is
`docs/workflow-schema.md` and `docs/execution-model.md`, and these are different documents. It is
filed rather than merely excluded so that the PRD's exclusion points at a real record.

**The defect.** `ARCHITECTURE.md:289-296` reads:

> ### Node types today
>
> Current first-class node types:
>
> - `task`
> - `approval`
> - `split`
> - `collector`

`NodeKind` (`src/model.rs:695-753`) has fourteen variants: `task`, `approval`, `split`,
`collector`, `decide`, `parallel_batch`, `subflow`, `call`, `spawn`, `send`, `wait`, `capture`,
`kill`, `run_agent`. All fourteen are returned by `GET /api/capabilities` (`src/api.rs:216`).

The phrase "Current first-class node types:" makes this a **false statement about the present**,
not an incomplete one — the same class as the `docs/api-reference.md:24` claim that
PRD-260826-0009-01 does fix. `ARCHITECTURE.md:202` separately names five of the missing kinds
("execute spawn, send, wait, capture, and kill control nodes"), so the document contradicts itself:
nine distinct kinds appear in total, four of them under a heading asserting the list is complete.

**Also in scope for whoever takes this:** `docs/backend.md` has the same shape. `:60` reads
"`WorkflowNodeType` — `Task`, `Approval`, `Split`, `Collector`"; `:105` names `Task`, `RunAgent`,
`Spawn`, `Send`, `Wait`, `Capture`, `Kill`. Ten distinct kinds, and `:60` presents its four as the
type's definition.

**Not urgent, and deliberately deferred once.** `typed-contracts` (RDMP-260815-2009-01) already
pins `ARCHITECTURE.md`'s canonical-version statement for editing at the v5 bump, so there is a
natural moment to fix the node-type list in the same pass rather than touching the file twice.
Triage may reasonably route this to that epic instead of scheduling it standalone.

**Evidence base.** The full documentation drift inventory is
`docs/sources/workflow-schema-drift-260825.md` (item X4 covers cross-document node-kind coverage).

---

**Readiness gate (cold-reader): FAIL** (round 1, 2026-08-30)

Independent cold reader, no planning context. Classes 1–6, 8 and 9 do not fire; class 9 arm A was
executed and all four acceptance observables were confirmed red at baseline, with positive and bound
controls proving both regexes can go green and that neither can be satisfied by an incidental match
elsewhere in its file. Forty-one class-7 surfaces swept.

Blocked on class 7 kind (a), **rule-violation figure, brief text under edit** — the unconditional row
of the class-7 decision table. Six bare cardinalities in the `## Agent Brief` (the completeness of
each of the two node-kind lists, each enum's variant count, and the edge-outcome section's variant
count) were asserted as facts with no discovery command deriving them. Every figure was verified
correct at the time of writing; correctness is not the escape, and the remedy is substitution, never
a refreshed numeral.

The reader grounded the call in this repo rather than in abstract strictness: sibling
`ISSUE-260826-0520-01` passed its gate carrying no cardinality anywhere, stating magnitudes
qualitatively ("in more than one place") beside `rg` commands. It also showed the rule doing real
work on this very record — see the correction below.

**Remedy applied, 2026-08-30 (no `REOPENED` stamp: the record had never been stamped `PASS` or
`WAIVED`, so the brief was not yet immutable).** All six figures replaced with `rg` discovery
commands over the two enum declaration blocks, with polarity stated qualitatively and an explicit
note that the commands are deliberately unpinned because the brief wants each set as of
implementation time. No numeral was refreshed. Also fixed in the same edit, from the round's
non-blocking notes:

- The Key-interfaces claim that the node-kind enum's string projection is "the safest single place"
  to read the tags from was imprecise — that projection delegates to the other enum, and the literals
  live there. The brief now names the serde representation on the declarations as the authority and
  demotes the projection to a cross-check, noting it is the drift-prone shape `ISSUE-260826-0520-01`
  exists to remove.
- The first acceptance criterion said "the section", which a literal reader could take as licence to
  delete the `join` sentence that lives inside it. Now scoped to the bullet list, with the sentence
  named as excluded.
- The `docs/backend.md` observable requires the Core-enums entry to stay on one physical line. The
  desired behavior now states that constraint, so a correct-but-wrapped edit cannot produce a red
  observable.
- The tmux dispatch-table exclusion is now stated as exact rather than as a tolerable subset, which
  makes it self-checking.
- The edge-outcome and `join` exclusions now carry the discovery command that re-confirms them,
  rather than resting on an unrepeatable "checked during triage".

**Correction to the filing narrative above (appended, not edited — this section is append-only).**
The paragraph beginning "The phrase 'Current first-class node types:'" cites the
`docs/api-reference.md:24` claim as one that `PRD-260826-0009-01` "does fix", present tense. That
fix has since landed (`ISSUE-260826-0637-02`); that line now carries the complete tag list. The
sentence reads correctly only as a statement about the PRD's scope, not as a pointer to a live
defect.

Relatedly: the membership figures in the filing narrative above are historical evidence from
2026-08-25, not contract. The Agent Brief deliberately carries no membership claim of its own and
directs the implementer to the discovery commands instead.

**Deliberate decision — frontmatter `summary:` keeps its figures.** It is outside the class-7
partition (which covers `## Agent Brief`, `## Triage Notes`, and `## Context Pack`), it is written
once and feeds the generated digest, and rewriting it would trade a durable identifier for a
marginal consistency gain. Recorded here so it reads as a decision rather than an oversight.

---

**Readiness gate (cold-reader): PASS** (round 2, 2026-08-30, full-enumeration)

Independent cold reader, no planning context. Full round, re-judged from scratch rather than as a
diff against round 1 — the acceptance observables were re-executed rather than inherited. Classes
1-7 and 9 all `fine`; class 6 does not fire in either prong; class 8 inert. Seventy class-7 surfaces
swept against an extraction list of the same size.

- **The remedy self-check passed on all three items.** A mechanical sweep for digits across the
  whole brief returns only the `rg` command text and one issue ID — no numeric repo-state figure
  survives, so no round-1 cardinality was refreshed rather than replaced. The number-words that
  remain (`two lists`, `one physical line`, `exactly one entry per wire tag`) are scope and
  structural statements, not occurrence counts.
- **Anchoring was proved rather than accepted.** The reader built its own positive and bound
  controls for all four observables: each goes green when the tag is written into the target
  section or list entry, and each stays red when the same tag is planted just outside it. It also
  ran a control the brief did not claim — a correct-but-wrapped `docs/backend.md` entry leaves both
  observables red — confirming the observables really do enforce the single-physical-line constraint
  the Desired behavior states.
- **The no-pin claim was checked against the "required pin" definition and upheld.** This brief's
  use of the derived set depends on it being the set at implementation time, not the same set later;
  pinning would reintroduce the defect if a variant lands before the work does.
- **Every out-of-scope premise was independently re-derived**, and one is stronger than the brief
  claims: `docs/backend.md`'s tmux dispatch description is an exact match for the kinds that helper
  routes, with the remaining kinds refused by name in the complement arm, so the complement is
  complete over the whole enum.
- **The append-only correction leaves no actionable contradiction.** The uncorrected present-tense
  sentence in the filing narrative points at an out-of-scope file and carries no instruction, and
  the appended correction resolves it accurately.

Seven non-blocking notes. Three worth carrying for whoever claims this:

- The section-bounding regex stops at `### ` only, not at `## ` or `# `. The window is tight today
  because the next heading is `### Edge outcomes today`, and the bound control confirms it — but if
  that heading were ever promoted or the section moved to the end of a `##` block, the bound would
  leak downward silently. A durability consideration for a record that may sit in the queue.
- The observables are necessary, not sufficient: the `spawn` observable would also go green if the
  tag were written into the `join` prose sentence rather than the bullet list. AC1's prose and AC3
  carry the real contract; close-time review catches that, not the gate.
- The reader flagged a systemic risk rather than a defect here: the brief's disclaimer that its
  Triage Notes are not current is doing real work in this round's class-7 outcome. It is reinforced
  structurally on this record — the brief carries no membership figure, AC3 forces re-derivation, and
  every disclaimed figure was independently verified still true — but if that disclaimer became
  boilerplate across records it would hollow out the "the Triage Notes the brief relies on"
  qualifier. Worth watching across siblings.

Brief is immutable from this stamp. Promoting to `ready-for-agent`.


## Code Review

Review file: `issue-260826-0240-01-code-review-20260831-002900.md`

- L1 (LOW): `NodeKind` is missing from the `**Core enums:**` list it belongs in — deferred (pre-existing, out of scope per "Nothing else in either document changes"; belongs to a separate record in the PRD-260826-0009-01 docs-truth family)

Smells: 2 advisory (both borderline, both `ARCHITECTURE.md`, neither promoted — the Duplication smell's other cited sites are pre-existing code outside the issue diff). No graduation rows emitted for the diff's files.

## Resolution

**Commit:** `fix: correct the node-kind lists in ARCHITECTURE.md and docs/backend.md (ISSUE-260826-0240-01)`

**Route:** `cursor` (`/cursor-developer`) — route-picker classified this as a well-scoped documentation-accuracy task with exact discovery commands supplied and mechanical edits to two markdown files; no cross-module reasoning, no ambiguity, no security surface.

**TDD:** n/a (linear) — prose-only documentation correction; the acceptance seam is the committed markdown itself, observed by the brief's four section- and line-anchored `rg` observables, not a code seam.

**Review telemetry:** dual review (Claude + Codex, cross-verified, adjudicated inline). Findings by severity: 0 CRITICAL, 0 HIGH, 0 MEDIUM, 1 LOW. Outcomes: 0 FIXED, 1 deferred, 0 dismissed. Fix rounds used: **0** of 4. Codex reported zero findings and zero smells; Claude's sole finding (L1 — `NodeKind` absent from `docs/backend.md`'s `**Core enums:**` list) was self-scoped as out of bounds, Codex returned DISAGREE on its defect framing, and the orchestrator adjudicated the split verdict by reading the code: the gap is real but pre-existing and outside "Nothing else in either document changes", so it is deferred to a separate record in the PRD-260826-0009-01 docs-truth family. 2 advisory smells recorded in `docs/issues/SMELLS-LEDGER.md`, neither promoted (the Duplication smell's other cited sites are pre-existing code outside the issue diff). Review file: `issue-260826-0240-01-code-review-20260831-002900.md`.

**Suite:** `just test` — Rust `cargo test --locked` 473 passed / 0 failed / 1 ignored, fully green. Vitest 128 passed / 1 failed: `InspectorPanel.test.ts` → "prompts for an unlock secret and retries a privileged node preview" timed out at 5000ms (reported 6160ms) under full-suite load. **Pre-existing / environmental, not caused by this change:** the diff touches only two markdown files, the failing test is a Svelte component test that reads neither, and a file-scoped re-run passed all 17 tests with that test completing in 1064ms — well under the cap. Both reviewers independently observed the same file behave this way (one saw the full suite green in an unrestricted run; the other saw the same timeout in a constrained sandbox and the same isolation pass). The preservation criterion is satisfied on the Rust side outright and on the frontend side modulo this load-dependent flake, which is a candidate for its own record.

**Closed:** 2026-08-31 (UTC)
