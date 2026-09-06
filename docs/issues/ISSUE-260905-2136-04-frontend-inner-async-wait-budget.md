---
id: ISSUE-260905-2136-04
kind: issue
category: bug
status: needs-triage
summary: Raising the Vitest per-test timeout does not raise Testing Library's separate 1000ms asyncUtilTimeout, so findBy/waitFor calls in the InspectorPanel test keep their own unchanged deadline
---

## Triage Notes

Filed 2026-09-05, from the adversarial review of `PRD-260902-0301-01` (Codex, `gpt-6-astra`).
Scoped out of `ISSUE-260902-0747-03`, which stays a small configuration patch.

**The claim, not yet verified by me.** `ISSUE-260902-0747-03` raises the outer Vitest test timeout for
the frontend. The test used to justify it also calls `findByLabelText` and `waitFor`
(`ui/src/features/editor/InspectorPanel.test.ts:145`, `:150`) without their own wait budget. Testing
Library defaults `asyncUtilTimeout` to 1000ms
(`node_modules/@testing-library/dom/dist/config.js:15`, consumed at `dist/wait-for.js:16`), and
`ui/src/test/setup.ts` does not override it. So the outer timeout and the convergence-wait budget are
two different deadlines, and `-03` moves only the first.

Explicitly **not** claimed: that the historical 6160ms failure was caused by the inner timeout. This
records a second surviving failure path, not a diagnosis of the observed one.

**Why it is separate from `-03`.** `-03` is a one-line configuration change with a known
justification. Reconciling the nested async wait policy is a judgement about how the frontend tests
should observe completion at all — the durable answer is observable UI completion and controlled async
inputs, not a larger polling budget. Raising either number is headroom, not determinism.

**Also worth recording.** The frontend suite is not uniformly under fake timers. The pane-stream
timing-policy tests use them; this InspectorPanel test uses real Testing Library waits. Any statement
that frontend waits are already deterministic is therefore false as a blanket claim, and
`ISSUE-260902-0747-03:100` should not be read that way.

**Discovery.**

```
rg -n 'asyncUtilTimeout|waitFor|findBy' ui/src --glob '*.test.ts'
rg -n 'configure|testTimeout' ui/src/test/setup.ts ui/vite.config.ts
```

**Related.** `ISSUE-260902-0747-03` (the outer-timeout patch this was split from).
