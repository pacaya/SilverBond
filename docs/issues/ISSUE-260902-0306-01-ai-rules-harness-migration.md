---
id: ISSUE-260902-0306-01
kind: issue
category: enhancement
status: needs-triage
summary: The repo has no AGENTS.md and no .claude/rules/ tree, so path-scoped agent rules have nowhere to live and every convention competes for the same always-loaded budget
---

## Triage Notes

Filed 2026-09-02, split out of the grilling session that produced `PRD-260902-0301-01` at the
maintainer's direction, as work that is genuinely separate from that PRD and deliberately **not** a
blocker for it.

**What's missing.** The AI-rules harness described in
`/Users/Shared/Data/work/Programming/ai/claude/docs/ai-rules-migration-prompt.md` has never been
initialized here. Confirmed by inspection:

- No `AGENTS.md` anywhere in the tree (root or nested).
- No `.claude/rules/` directory. `.claude/` holds only `launch.json`, `settings.json`,
  `settings.local.json`, an empty `skills/`, and `worktrees/`.
- `CLAUDE.md` is 37 lines and carries every convention as always-loaded prose.

The target layout that prompt describes is a root `AGENTS.md`, a thin `CLAUDE.md` shim, path-scoped
`.claude/rules/*.md` for Claude Code, and nested local `AGENTS.md` files in subsystem folders for
Codex's closest-wins discovery. Its governing constraint is a budget: joint instruction compliance
degrades with rule count, so the prompt targets roughly 40 or fewer always-loaded rules across all
always-on files combined, and everything else gets scoped or split.

**Why it matters now.** `PRD-260902-0301-01` produces testing rules that are inherently
path-scoped — they apply when touching test code and nowhere else. With no `.claude/rules/` tree
they would either land in `CLAUDE.md` and consume always-loaded budget they don't need, or not be
written at all.

**Why it is not a blocker.** The decision taken during grilling was to decouple the two. The PRD's
*reasoning* goes into an ADR, which is a reference document rather than an always-loaded rule, so it
costs nothing against the budget and the migration would treat it as source material rather than
something to restructure. Only the two or three *operative* lines want path-scoping, and moving
those into `.claude/rules/tests.md` once this record lands is trivial. Coupling a well-scoped test
fix to a repo-wide rules migration was rejected as the larger risk.

**Next step for triage.** Decide scale before writing a brief. The migration prompt runs discovery,
classification of every existing rule (derivable / deterministically-enforceable /
prose-load-bearing / stale / workflow), and a restructure — that is plausibly epic-shaped rather
than issue-shaped, in which case this should route through `/to-spec` rather than grow an
`## Agent Brief` here. The prompt itself recommends running in plan-first mode and reviewing the
migration plan before any file is written, which is a strong hint that the planning half wants its
own artifact.

**Related.** `PRD-260902-0301-01` (testing ADR + operative rules are this record's first consumer).
