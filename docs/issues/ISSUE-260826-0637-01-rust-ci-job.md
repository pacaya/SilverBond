---
id: ISSUE-260826-0637-01
kind: issue
category: enhancement
status: ready-for-agent
summary: Add a CI workflow that runs the full cargo test suite, so the Rust suite protects the default branch
prd: PRD-260826-0009-01
terms: []
---

## Agent Brief

**Category:** enhancement
**Summary:** Add a GitHub Actions workflow that runs the full `cargo test` suite on push and pull request.

**Current behavior:**
The repository's existing CI workflows — enumerate them with `ls .github/workflows/` — cover doc
phrase-grepping and frontend bundle freshness. None of them builds or runs any Rust: `rg -l
'cargo|rustup|toolchain' .github/workflows/` returns nothing. The crate's test suite — unit tests inside `src/` plus the `tests/http_api.rs`
integration target — has therefore never run in CI, and is green only for whoever remembers to
run it locally.

This matters beyond hygiene. The `docs-truth` epic adds a documentation generator that lives in
an integration test, so its compile stop and its freshness assertion fire under `cargo test`
and under nothing else — `cargo build` and `cargo check` do not compile `tests/`. Without a CI
job the epic's central guarantee is advisory.

**Desired behavior:**
A workflow runs the complete Rust suite on push and on pull request against the default branch,
and fails the run when any test fails. It runs the whole suite, not a filtered subset, so tests
added later — including the documentation generator's — are covered without editing the
workflow.

The job installs a stable Rust toolchain and caches the cargo registry and build directory in
whatever way is idiomatic for the action set in use. Keep it to one job on one runner.

**Key interfaces:**
- The workflow file joins whatever already exists under `.github/workflows/`; match the trigger
  style and naming conventions of the files `ls .github/workflows/` lists rather than inventing new
  ones. Those files trigger on push and pull request with no branch filter — read them for the exact form
  rather than copying it from here. Keep the no-filter part, so the workflow runs on feature
  branches.
- **Actions provenance.** Prefer first-party `actions/*`. A third-party action is permitted only
  when pinned to a full commit SHA — never a floating tag or branch ref. Raw `rustup` plus
  `actions/cache` is an acceptable first-party-only route. This is a standing rule for this
  workflow. Do not derive it from, or weaken it against, how the crate's Cargo dependencies happen
  to be pinned — that manifest is not a precedent either way.
- `just test-rust` is the local equivalent and should stay the command a developer runs; the
  CI job may invoke it or call `cargo test` directly, but the two must not diverge in what they
  cover.

**Acceptance criteria:**
- [ ] A workflow file under `.github/workflows/` invokes the full Rust suite. Observable:
      `rg -l 'cargo test|just test-rust' .github/workflows/` returns at least one file; no
      matches before this change.
- [ ] The workflow triggers on both `push` and `pull_request`. Observable: the file matched
      above contains both trigger keys.
- [ ] The suite is invoked unfiltered — no `--test`, `--lib`, or filter argument narrowing what
      runs. Verifiable by reading the invocation line.
- [ ] The workflow is structurally sound and the suite command it runs succeeds locally.
      Observable at the seam: read the new file against the ones `ls .github/workflows/` lists and
      confirm it carries the same top-level shape and step form, then run by hand the single step
      that invokes the test suite. Parse the file too if the machine already provides a YAML
      parser, and fall back to the read-against-siblings comparison when it does not — do not
      install one, and do not assume either way. Note the limit honestly rather than overclaiming:
      a parse proves only well-formedness. A typo'd action version and a wrong runner label both
      parse clean and are observable only on the Actions run, which the criterion below carries.
- [ ] `cargo test` passes on the branch at the time this issue closes. Observable: run it.
- [ ] The workflow's first run on the branch is green. Observable at the seam: the Actions run for
      this workflow. **Bounded:** neither GitHub credentials nor push authority are assumed. If
      credentials are unavailable, or if the branch has not been pushed so no run exists yet, close
      on the two local criteria above and record the Actions observation as owed by whoever pushes
      the branch. Do not push in order to manufacture a run.
- [ ] Every workflow that existed before this change is untouched — this issue adds a file and
      edits none. Observable: `git status --porcelain .github/workflows/` reports exactly one added
      path and no modifications.

**Out of scope:**
- Clippy, `rustfmt`, a build matrix, cross-platform runners, and migrating `npm test` or
  Playwright into this job. The bound is `cargo test` only.
- Making this workflow a required status check. That is a repository setting, not a file in the
  tree, and PRD-260826-0009-01 declines it explicitly.
- Fixing or skipping any test. The suite is green at authoring time; if it is not green when the
  work happens, report that rather than adjusting tests to make the job pass.
- The documentation generator itself and its freshness assertion. This job runs whatever tests
  exist; it does not know about them.

## Triage Notes

**Readiness gate (cold-reader): PASS** (round 4, full-enumeration)

Rounds 1-3 each found one defect of a single genus, and each remedy introduced the next: an
acceptance-criterion filesystem count, then a manifest count, then an environment inventory.
Round 4 ran as a full-enumeration round per the round-3 escalation rule and passed all five
prongs, with class 6 re-examined cold on both prongs and not firing.

Two non-blocking observations from that round were actioned after the verdict and before this
stamp, both strict removals asserting nothing new: a claim about how YAML reads a bare `on:`
key was struck (the round-4 reader demonstrated it differs between two parsers present on this
machine, under a criterion that forbids assuming which parser is available), and the sibling
trigger form was changed from a scalar rendering to a pointer at the files themselves.
Recorded because the gate ran against the text before these two edits.
