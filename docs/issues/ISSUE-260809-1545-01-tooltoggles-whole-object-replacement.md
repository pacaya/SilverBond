---
id: ISSUE-260809-1545-01
kind: issue
category: bug
status: needs-triage
summary: Web search toggle replaces the whole toolToggles object instead of clearing one key, discarding any future sibling toggle
---

## Triage Notes

Filed 2026-08-09 from finding **L5** of the `ISSUE-260808-2022-11` dual review
(`docs/issues/code-reviews/issue-260808-2022-11-code-review-20260809-141744.md`),
which deferred it rather than fixing it in place. Not yet triaged — the account
below is the review's, carried over so the work is not lost.

**What the reviewers established:**

- The Web search checkbox writes `update("toolToggles", checked ? { webSearch: true } : undefined)`.
  That is a whole-object replacement in **both** directions, so any sibling key the
  `toolToggles` shape later grows is discarded on either transition — checking or
  unchecking Web search silently wipes it.
- **Latent, not live.** Codex independently confirmed that `ui/src/lib/types/workflow.ts:45-47`
  currently defines no sibling toggle, so nothing is lost today. The defect needs a
  second key to bite.
- **Pre-existing.** `ISSUE-260808-2022-11` moved the code verbatim; it did not
  introduce the behavior. It was deferred there as squarely inside that brief's
  Out of scope ("any change to agent-defaults semantics").
- **The blast radius doubled.** That extraction promoted the control into the shared
  `AgentConfigFields.svelte`, now rendered by both the `run_agent` node block and the
  workflow-level agent-defaults block. Whoever adds the second `toolToggles` key will
  hit this at two call sites rather than one, and the failure is silent data loss
  rather than an error.

**Fix the reviewers proposed** (not yet evaluated — triage should confirm it against
the current tree rather than adopt it): merge the field instead of replacing it —
`update("toolToggles", { ...values.toolToggles, webSearch: checked || undefined })`
— with an empty-object-to-`undefined` collapse mirroring the prune logic that
`updateAgentDefault` already applies.

**Open questions for triage:**

- Is this worth fixing pre-emptively while `toolToggles` is still single-key, or
  parked until a second key is actually proposed? Fixing it now is cheap and the
  fix is already written; deferring risks it being rediscovered as a data-loss bug.
- Does the same whole-object-replacement pattern appear on other optional config
  objects in the shared field components, or is `toolToggles` the only site? The
  review looked only at the one control it was reviewing.
- Category is provisionally `bug` — it is a real defect in written code, but it is
  unreachable today. Reclassify to `enhancement` (hardening) if triage prefers.
