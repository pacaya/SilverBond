---
id: ISSUE-260901-0216-06
kind: issue
category: bug
status: done
prd: PRD-260826-0009-01
summary: The backend module reference's core-enums list names the plain node-type discriminant but not the tagged node-kind enum that carries each kind's configuration
claimed_by: implement-issue@macmini
claimed_at: 2026-09-01T11:27:26Z
---

## Agent Brief

**Category:** bug
**Summary:** List the tagged node-kind enum among the workflow model's core enums in the backend
module reference.

**Current behavior:**
The backend module reference has a core-enums list for the workflow model module. It names the
plain node-type enum with all fourteen of its variants, alongside the response-format, edge-outcome
and split-failure-policy enums.

It does not name the tagged node-kind enum. That type is the one that actually defines the
canonical document shape: the plain enum is the bare discriminant, while the tagged enum carries
each kind's configuration payload and is what a workflow document serializes as a nested object
with a type tag. A reader using this document to orient in the model module is pointed at the
discriminant and never told the type it discriminates exists.

**Desired behavior:**
The core-enums list names the tagged node-kind enum, and distinguishes it from the plain node-type
enum it sits beside — the plain one being the discriminant, the tagged one carrying per-kind
configuration.

The entry matches the list's existing presentation rather than introducing a new one.

**Key interfaces:**
- The tagged node-kind enum and the plain node-type enum — two distinct types in the workflow model
  module. The reference currently names only the second.

**Acceptance criteria:**
- [ ] The core-enums list names the tagged node-kind enum.
      `rg -n '^- .NodeKind.' docs/backend.md` returns the list entry; no matches before this change.
- [ ] The entry states what distinguishes it from the plain node-type enum, so a reader can tell
      which of the two to reach for.
- [ ] The entry sits inside the workflow model module's core-enums list, not elsewhere in the
      document. `rg -n -A 8 '^\*\*Core enums:\*\*' docs/backend.md` shows the entry within that
      list's extent.

**Out of scope:**
- The per-node canvas state gap in the schema document, which is ISSUE-260901-0216-04.
- Enumerating the tagged enum's variants or their configuration payloads. The schema document owns
  that; this entry is a pointer, matching how the list treats its other entries.
- Any other section of the backend module reference.
- Adding test coverage for `docs/backend.md`. No test guards this document today, and establishing
  one is a separate decision.

## Context Pack — generated at claim (2026-09-01T11:27:26Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01):
- Content is tiered by who can maintain it correctly: Tier G generated (node-kind catalog), Tier P hand-written but source-cross-referenced, Tier C conceptual prose, Tier D deleted. A hand-written list in `docs/backend.md` is Tier P by nature — one-liner entries with an owning source, never a re-enumeration.
- The generator emits the node-kind catalog into marker-delimited blocks in `docs/workflow-schema.md` **and nothing else** — it does not touch any block in another file, so `docs/backend.md` is edited by hand.
- The two enums are distinct on purpose: `WorkflowNodeType` is the plain discriminant harvested from its `Deserialize` derive; `NodeKind` is internally tagged (`#[serde(tag = "type")]`) and carries each kind's configuration payload. `From<WorkflowNodeType> for NodeKind` maps one to the other, and `NodeKind::node_type()` maps back. This distinction is exactly what the issue's second acceptance criterion asks the entry to state.
- The nested `kind` object is the canonical document shape; the flat `type` shape is accepted only by the legacy migration path. That is why `NodeKind` — not `WorkflowNodeType` — is the type that defines what a workflow document serializes as.
- `src/model.rs` is the named authority for schema shape; docs point at it rather than restating it. Enumerating variants or config payloads belongs to the schema document's catalog, matching this issue's out-of-scope line.
- Post-epic correction at epic close: the PRD's old `## Out of Scope` exclusion of `docs/backend.md` ("left standing deliberately" at ten node kinds) was **overturned** by ISSUE-260826-0240-01, which brought it to fourteen. `docs/backend.md` is in-scope territory for the docs-truth family; this record is the residual that record deferred.
- Residual work the epic did not close is filed as ISSUE-260901-0216-01 through -06 and tracked on the roadmap's `docs-truth` entry.

**Test seam & Testing Decisions:** observable at the rendered text of `docs/backend.md` — specifically the workflow model module's `**Core enums:**` list — read with `rg` per the acceptance criteria. No automated seam guards this document: the PRD's four checks (compile stop, freshness/coverage/mapping, `NodeKind` wire-tag harvest, structural validity) all bind `tests/docs_catalog.rs` to the generated blocks in `docs/workflow-schema.md` only, and the freshness check "gates the generated blocks only" and "is not a general documentation linter". The brief's out-of-scope line agrees: adding test coverage for `docs/backend.md` is a separate decision. Verification is therefore the two `rg` commands plus reading their output — and the triage gate's carried note applies: `-A 8` runs past the core-enums list into the neighbouring agent-configuration list, so the exit status alone does not prove placement.

**ADRs:**
- No `adrs:` on this record — tier skipped. (Parent PRD carries ADR-260815-2009-01, the v5 bump, which this v4-documenting slice only forward-references.)

**Terms:**
- No `terms:` on this record — tier skipped. Parent PRD's glossary rule still governs word choice: an unmarked `CONTEXT.md` entry is true of `src/` at HEAD; `_(planned — ADR-…)_` marks a term the engine does not accept yet.

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md · CONTEXT.md · docs/backend.md · docs/workflow-schema.md

## Code Review

Review file: `issue-260901-0216-06-code-review-20260901-114140.md`

Outcome: ACCEPTED after 1 fix round (cap 4). All findings on the single added line at `docs/backend.md:61`.

- M1 (MEDIUM): "carrying each kind's configuration payload" is factually overbroad against `src/model.rs` — FIXED
- L1 (LOW): entry's presentation diverges from the list convention the brief asked it to match — FIXED
- L2 (LOW): "canonical workflow `kind` object" is a verbless fragment that can be misparsed — FIXED
- L3 (LOW): entry names the distinction but not the bridge between the two types — dismissed (out of scope per the brief — the entry is a pointer, not a mechanism reference)
- L4 (LOW): merging the discriminant sentence into a trailing adjunct slightly re-aims it at the wire shape — dismissed (raised at fix-verification, uncorroborated by the second reviewer; finder recommended no change)

Smells: 1 raised (borderline Duplicated Code), 0 recorded — suppressed under the baseline's "documented standard overrides" rule; `docs/issues/SMELLS-LEDGER.md` unchanged.

## Resolution

**Commit:** `fix: list the tagged node-kind enum in the backend module reference (ISSUE-260901-0216-06)`

**Route:** cursor (`/cursor-developer`) — route-picker classified it as a well-scoped single-file
documentation edit with no cross-module reasoning, security surface, or infra concern. The fix round
routed to cursor again on the same rationale.

**TDD:** n/a (linear) — docs-only, mechanical. The brief's Out of scope explicitly declines test
coverage for `docs/backend.md`, and no test guards that document: `tests/docs_catalog.rs` binds only
the generated marker-delimited blocks in `docs/workflow-schema.md`.

**Review telemetry:** dual review (Claude + Codex), cross-verified, adjudicated inline. 5 findings —
1 MEDIUM, 4 LOW. Outcome: 3 FIXED (M1, L1, L2), 0 deferred, 2 dismissed (L3, L4). 1 fix round used of
a cap of 4. Smells: 1 raised (borderline Duplicated Code), 0 recorded — suppressed under the smell
baseline's "documented standard overrides" rule, since the parent PRD's Tier-P convention prescribes
exactly the pointer one-liner that overlaps the owning schema document; `docs/issues/SMELLS-LEDGER.md`
is unchanged by this issue.

The one substantive finding was M1: the first implementation described `NodeKind` as "carrying each
kind's configuration payload", which is false for the three bare unit variants `Approval`, `Split`,
and `Collector` (`src/model.rs:704-706`) — a factual overstatement inherited verbatim from this
record's own Context Pack. Codex's first pass had cleared the wording; on cross-verify it re-read
`src/model.rs:693-752` and reversed to agree. The shipped line qualifies the claim as "carrying
per-kind configuration where the kind has one".

**Suite:** `just test` — PASS. 659 passed, 0 failed, 1 skipped (Rust 517 across 6 binaries; vitest 142
in 15 files). No pre-existing failures to record.

**Acceptance criteria:** all three verified against the shipped line, including AC3's placement read
from the command output rather than its exit status, per the carried triage-gate caveat — the entry
sits at `docs/backend.md:61`, inside the `**Core enums:**` list (extent 59-64), and the `-A 8`
window's overrun into `**Agent configuration types:**` (66-69) does not contain it.

**Closed:** 2026-09-01 (UTC)

## Triage Notes

Filed 2026-09-01, split out of ISSUE-260901-0216-04 at the readiness gate. The gate found the two
gaps had no shared seam — different documents, different types, no ordering dependency — and that
no test guards `docs/backend.md` at all, so the suite could not bind them into one slice.

Originally deferred out of `ISSUE-260826-0240-01`, whose resolution says the gap belonged in "a
separate record in the PRD-260826-0009-01 docs-truth family". No such record was minted at the
time. Confirmed still open at audit time by reading the enum declarations against the document.

**Readiness gate (cold-reader): PASS** (round 2, 2026-09-01)

Gate note, carried for the implementer (non-binding): the third criterion's `-A 8` window runs past
the core-enums list into the neighbouring agent-configuration list, so the command alone does not
prove placement. The prose clause carries that requirement — read the output, do not just check the
exit status.
