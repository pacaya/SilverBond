---
id: ISSUE-260901-0216-05
kind: issue
category: bug
status: ready-for-human
prd: PRD-260826-0009-01
summary: ISSUE-260826-0637-01's acceptance criterion is unobserved — the branch has since been pushed and the workflow has almost certainly run, so what is owed is reading the result, not pushing
---

## Agent Brief

**Category:** bug
**Summary:** Read the result of the Rust CI workflow's run on the pushed branch and record it.

**Current behavior:**
`ISSUE-260826-0637-01` added a GitHub Actions workflow that runs the Rust suite, and closed with
all of its local acceptance criteria met. One criterion — that the workflow's first Actions run is
green — was recorded as met-in-principle and explicitly deferred to "whoever pushes", because the
branch carrying the workflow had not been pushed at that time.

The branch has since been pushed — see the 2026-09-02 correction below — so the workflow has almost
certainly run. What has not happened is anyone *reading* the result. Nothing has confirmed that its
runner image, toolchain pin, cache configuration, and checkout settings actually work; the deferred
criterion is undischarged not because the run is missing but because its conclusion is unrecorded.

**Desired behavior:**
The workflow has run on GitHub Actions at least once and its result is known. If green, the
deferred criterion is discharged. If red, the failure is triaged: an environment defect in the
workflow gets a record, and a genuine suite failure is handled as the suite failure it is.

**Why this is not delegable:**
It requires pushing to a remote an agent has no credentials for, and reading the run result in the
GitHub Actions web UI. Both are maintainer actions. The judgment on a red first run — whether it
indicates a workflow defect or a real regression — also wants a human, since a first run failing on
runner environment looks very different from one failing on test logic.

**Acceptance criteria:**
- [x] The branch carrying `.github/workflows/rust-tests.yml` is pushed to the remote. *(Satisfied
      before this record was filed — see the 2026-09-02 note.)*
- [ ] The workflow's first run has completed and its conclusion is recorded in this record's
      Triage Notes, with the run URL.
- [ ] If the run is green, this record closes and `ISSUE-260826-0637-01`'s deferred criterion is
      noted as discharged.
- [ ] If the run is red, a record is filed against whatever the failure identifies, and this
      record links it before closing.

**Out of scope:**
- Changing the workflow definition speculatively in anticipation of a failure. Observe first.
- Diagnosing intermittent test failures. `ISSUE-260901-0216-03` is closed: the mechanism was
  reproduced on 2026-09-02 and remediation moved to `PRD-260902-0301-01`. A red run on one of the
  tests that record names needs no new diagnosis — note it against the PRD and link it from here
  rather than absorbing it.

## Triage Notes

Filed 2026-09-01 during a completeness audit of the `docs-truth` epic. `ISSUE-260826-0637-01`'s
resolution records the deferral in its own words — the criterion is "owed by whoever pushes" — and
no record carried it forward.

Confirm the branch's unpushed state with `git status -sb` before acting; the audit observed it
ahead of the remote, but that is exactly the kind of state that changes.

Sequencing note: run this one first regardless. The CI run is a free probe for
`ISSUE-260901-0216-03`, whose open question is whether those intermittent failures still happen at
all — a red run gives that record its missing reproduction, and a green one is a data point against
the symptom still being live. Neither record blocks the other.

**Readiness gate:** not applicable — `ready-for-human`, not delegated to an agent.

**Correction, 2026-09-02 — this record's premise was true when written, and has since been overtaken.**

The brief above originally said the branch "had not been pushed. It still has not been", and that
"The workflow file therefore exists in the tree and has never executed." Both were accurate on the
day they were written and are now stale; the brief's **Current behavior** has been corrected in
place rather than left to be refuted further down.

Two earlier drafts of this correction each offered a causal story for how the stale claim survived —
first a misread `git status -sb`, then a push landing mid-session. Both were wrong, and neither is
needed. Only the verifiable timeline is recorded here.

Verified 2026-09-02:

- `git merge-base --is-ancestor 2f2e38d origin/feature/tmux-panes` exits 0 — the workflow commit
  **is** on the remote branch.
- `git cat-file -e origin/feature/tmux-panes:.github/workflows/rust-tests.yml` succeeds — the
  workflow file is present at the remote tip.
- `git reflog show refs/remotes/origin/feature/tmux-panes` records `update by push` at
  2026-09-01 21:30 -0400, and `2f2e38d` is not an ancestor of the previous (2026-08-25) push, so the
  workflow landed on the remote in that 2026-09-01 push.
- `.github/workflows/rust-tests.yml` triggers `on: push` with no branch filter.

So the workflow has almost certainly executed at least once. What is actually owed is **reading the
result**, which still needs a human: `gh` is installed but unauthenticated here, so neither the run
list nor its conclusion is reachable from this session.

The record stays `ready-for-human` for that reason, but the task is smaller than it was written to
be. The remaining acceptance criteria are unchanged and still stand.

One consequence worth carrying: if that run was red, it is a data point for
`ISSUE-260901-0216-03`'s question, which has since been answered independently — that record is
closed, its mechanism reproduced, and remediation moved to `PRD-260902-0301-01`. A red run on one of
the tests named there needs no new diagnosis; it is the same defect.
