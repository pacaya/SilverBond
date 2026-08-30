---
id: ISSUE-260826-0637-01
kind: issue
category: enhancement
status: done
summary: Add a CI workflow that runs the full cargo test suite, so the Rust suite protects the default branch
prd: PRD-260826-0009-01
terms: []
claimed_by: implement-issue@Mac-mini-4.local
claimed_at: 2026-08-30T01:08:01Z
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

## Context Pack — generated at claim (2026-08-30T01:08:01Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01):
- A CI job running the full `cargo test` suite is in scope, justified on repo hygiene (466 Rust tests, no CI has ever run them), not on the epic's documentation criterion.
- Bound: `cargo test` only — no clippy, no `rustfmt` gate, no build matrix, no cross-platform runners, no migration of `npm test`/Playwright into it.
- Making the workflow a required status check is explicitly declined — a repository setting, not a file in the tree.
- The suite must run unfiltered: the docs generator lives in an integration test, and `cargo build`/`cargo check` do not compile `tests/`, so `cargo test` is the only profile where its compile stop and freshness assertion fire.
- The docs freshness check "rides the same job" — the job must not be narrowed to a freshness diff mirroring `frontend-freshness.yml`.
- Only two workflows pre-exist (`canonical-v4-docs.yml`, `frontend-freshness.yml`); neither touches Rust, and this epic adds the repo's first Rust CI surface.
- Renaming the existing guard script and its workflow is out of scope — this issue adds a file and edits none.
- No engine code changes and no test fixes: the suite is green at authoring time; report rather than adjust if not.

**Test seam & Testing Decisions:** observable at the workflow file under `.github/workflows/` and the GitHub Actions run for it — read the new file against its siblings for top-level shape and step form, run by hand the single step invoking the suite, and parse the YAML only if a parser is already present (do not install one; a parse proves well-formedness only, not a valid action version or runner label). PRD Testing Decisions that touch this seam: the repo's established pattern for derived artifacts is regenerate-diff-fail in CI (`.githooks/pre-commit` plus `frontend-freshness.yml`); the CI job is what makes the generator's compile stop unskippable; and whether a failing run blocks a merge depends on branch protection, which is outside this PRD. The Actions-run criterion is bounded: no push authority or credentials are assumed, so close on the local criteria and record the Actions observation as owed.

**ADRs:**
- ADR-260815-2009-01 — Typed workflow contracts over a JSON-valued variable store (accepted); parent PRD's ADR, no direct bearing on this slice.

**Terms:** none listed on this issue (`terms: []`).

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md · docs/adr/INDEX.md · CONTEXT.md

## Code Review

Review file: `issue-260826-0637-01-code-review-20260830-013255.md`

Dual review (Claude + Codex), cross-verified and adjudicated inline, over 3 fix rounds. 17 raw findings (Claude 15 across three rounds, Codex 2) merged to 15 adjudicated entries; the 2 Codex findings both duplicated Claude findings and are the double-confirmed pair (H1, M1).

- H1 (HIGH): `cargo test` reports green on the runner while six tests silently self-skip — FIXED
- M1 (MEDIUM): No `permissions:` block, and checkout persists the token across arbitrary code execution — FIXED
- L1 (LOW): Cache key hashes an unrelated lockfile and omits the compiler version — FIXED
- L2 (LOW): `cargo test` runs without `--locked` — FIXED
- L3 (LOW): No `timeout-minutes`; the job inherits GitHub's 360-minute default — FIXED
- L4 (LOW): No `concurrency` group: superseded runs are never cancelled — FIXED
- L5 (LOW): Cargo cache is write-once — `target/` freezes at the first run — dismissed
- L6 (LOW): `~/.cargo/registry` cached wholesale, archiving regenerable extracted sources — dismissed
- L7 (LOW): The rustup step is close to a no-op on `ubuntu-latest` and pins nothing — dismissed
- L8 (LOW): The deliverable is untracked rather than staged — dismissed
- L9 (LOW): Suite-command parity across call sites (regression from round 1) — FIXED
- L10 (LOW): Concurrency policy cancelled the branch it exists to protect — FIXED
- L11 (LOW): Folding rustc into `restore-keys` discards compiler-independent caches — dismissed
- L12 (LOW): Raw `rustc -V` as a cache-key component, and silent degradation on capture failure — dismissed
- L13 (LOW): `CLAUDE.md` names a Rust suite command that is no longer canonical — FIXED

**Same-site escalation fired once.** Round 1's verification surfaced findings inside the regions round 1 had just patched, so round 2 ran as a redesign round: L9 was clustered with L2 and L10 with L4, and each fix was required to remove the named mechanism rather than the symptom. Both mechanisms were confirmed gone by both reviewers.

Smells: 3 advisory (all LOW, none promoted — every Duplication entry names a second site outside the issue diff), merged into `docs/issues/SMELLS-LEDGER.md`. Graduation advisory: none emitted.

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

## Resolution

**Commit:** `feat: add a CI workflow that runs the full cargo test suite (ISSUE-260826-0637-01)`

**Route:** `codex` for the implementation and for fix rounds 1-2 (CI/CD, workflow config, and security-sensitive token handling — the measured Codex slice); `cursor` for fix round 3 (a two-line markdown edit, no code or architecture). Route chosen per round by `route-picker`.

**TDD:** n/a (linear) — the deliverable is a CI configuration file, not behavior-changing code, so the acceptance criteria are read-and-run checks against the file rather than a red-green cycle at a code seam.

**What landed:**
- `.github/workflows/rust-tests.yml` (new) — one job on one runner, unfiltered `push` and `pull_request` triggers, `cargo test --locked` unfiltered, first-party actions only (`actions/checkout@v4`, `actions/cache@v4`) plus raw `rustup`, `permissions: contents: read` with `persist-credentials: false`, `timeout-minutes: 20`, a default-branch-aware concurrency group, `tmux`/`zsh` installed so the environment-gated tests actually execute, and a cache keyed on the root `Cargo.lock` plus the rustc version.
- `justfile` — `test-rust` runs `cargo test --locked`, and `test` now delegates via `test: test-rust` instead of spelling the command a second time.
- `CLAUDE.md` — two lines updated to the canonical command.

Every pre-existing workflow is untouched: `git status --porcelain .github/workflows/` reports exactly one added path and no modifications.

**Review telemetry:** 17 raw findings (Claude 15, Codex 2) → 15 adjudicated. By severity: 1 HIGH, 1 MEDIUM, 13 LOW. Outcomes: **9 FIXED, 6 dismissed, 0 deferred**. Fix rounds used: **3 of 4**. Same-site escalation fired once, converting round 2 into a redesign round (L9 clustered with L2, L10 with L4); both mechanisms confirmed removed by both reviewers.

**Suite:** PASS. `just test` → `cargo test --locked` then `npm test`: **595 passed, 0 failed, 0 skipped** (Rust 466 = 449 lib + 17 `tests/http_api.rs`; frontend 129 across 13 files), ~38 s. `--locked` caused no dependency-resolution failure. Only pre-existing warnings appeared. Playwright e2e not run — out of this issue's bound.

**Owed observation (bounded acceptance criterion, unchanged from the brief).** The workflow's first Actions run is still unobserved: the branch is unpushed and no GitHub credentials were assumed or used, so per the criterion's explicit bound this issue closes on the local criteria. Whoever pushes `feature/tmux-panes` owes that check. Two things make it more than a formality:

- The H1 fix converted five previously-inert tests into five that execute headlessly for the first time (`src/tmux_exec.rs:3875`, `:3942`, `:3961`, `:3981`, `:4014`). Whether they pass on a hosted runner — tmux server start, `zsh -lic` with no tty — is observable only on that run. A red first run is a real possibility and would be new information, not a regression from this work.
- A typo'd action version or a wrong runner label parses clean locally and surfaces only on the Actions run, exactly as the structural-soundness criterion states.

The root-gated test at `src/api.rs:4826-4835` (euid==0 plus `/usr/bin/sudo` plus a `daemon` user) remains deliberately self-skipping — a hosted runner cannot satisfy it, and making CI run as root was out of scope.

**Closed:** 2026-08-30 (UTC).
