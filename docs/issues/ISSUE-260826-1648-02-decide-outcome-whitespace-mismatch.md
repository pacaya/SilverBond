---
id: ISSUE-260826-1648-02
kind: issue
category: bug
status: needs-triage
origin: docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md
terms: [Decide Outcome]
summary: Decide-node validation compares outcome labels to branch-edge labels untrimmed while the runtime trims, so a whitespace-padded label validates clean and then misses at runtime, reaching an arm that is otherwise unreachable
---

## Triage Notes

Found on 2026-08-26 while grounding `CONTEXT.md`'s **Decide Outcome** entry against `src/` for the
`docs-truth` epic, and independently rediscovered by the round-5 readiness gate on
`ISSUE-260826-0637-08`. Filed rather than folded into that epic: `docs-truth` is scoped to make no
engine changes and edit nothing under `src/`, and this needs a code change.

**The defect.** Validation and the runtime disagree about whitespace.

- Validation builds the branch-label set from raw `edge.label` values and tests membership with the
  raw `outcome` string. Its only emptiness guard trims, but the comparison itself does not.
- The runtime trims the model's output before matching it against the raw `edge.label`.

So a decide node declaring an outcome `"approve "` with a branch edge labelled `"approve "`
validates clean — the raw strings match — and then fails to match at runtime, because the trimmed
`"approve"` is compared against the untrimmed `"approve "`.

**Why it matters beyond the padding itself.** The miss lands in the arm that emits a
`workflow_error` and degrades to the plain success edge. Both `CONTEXT.md` and
`ISSUE-260826-0637-08` document that arm as **unreachable from a validated document**, which is
correct for every document an author would plausibly write — validation makes an outcome with no
matching branch edge an error, and an outcome the model never names fails the node instead. This
whitespace path is the sole exception found, and it means a run can silently route down the success
edge where the documentation says routing cannot happen.

**Not urgent, and the framing should not change.** The trigger is a pathological label and the
consequence is a mis-route rather than corruption. The documentation framing is a recorded
maintainer decision, and `CONTEXT.md`'s **Decide Outcome** `_Avoid_` clause bans describing the
fallthrough as behaviour an author can meet — that stands. Fixing the engine removes the exception
rather than the framing.

**Fix shape, not yet decided.** The obvious candidates are trimming on both sides of the validation
comparison, or trimming neither and rejecting padded labels at validation. Whoever picks it up
should check whether any other validator/runtime pair in the decide path has the same asymmetry
before choosing, rather than patching the one comparison.
