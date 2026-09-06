---
id: ISSUE-260902-0747-09
kind: issue
category: bug
status: needs-triage
summary: A production retry sleep defaulting to two seconds sits unreached behind a loop bound that no test currently trips, so the first test to set a retry count silently buys a two-second wall-clock wait inside the gate
adrs: [ADR-260902-0312-01]
terms: [Run, Logic Tier, Integration Tier]
blocked_by: [PRD-260902-0301-01]
---

## Deferral note

Deferred 2026-09-05, out of `PRD-260902-0301-01`'s delivery scope and into the backlog, following the
maintainer's decision to narrow that epic to the reproduced failure plus the forward-facing tier rule.
The narrowed epic delivers `-01`, `-11`, `-04`, `-03`, `-14` and a scoped `-10`; this record is good
work that is not that task.

Deferring it does not retire the problem it describes. It is retained debt under
`docs/adr/260902-0312-deterministic-test-tiers.md` § Retained debt: the tests it would have repaired
stay in the Integration Tier, stay in the non-gating job, and must not be described as fixed.

**Do not implement this brief as written without re-triage.** It was authored against the pre-narrowing
ADR and PRD, and its `blocked_by` chain assumes slices that are no longer sequenced.

**Known blocking defect, from the 2026-09-05 adversarial review.** The brief requires the new
default-executed retry test to match the existing build-profile timeout shims' test value and explicitly
permits it to pay that delay. Those shims resolve to 250ms (`src/runtime.rs:44`, `src/api.rs:70`), and
the retry loop performs a real `tokio::time::sleep` before the next attempt (`src/runtime.rs:3970`). A
shorter sleep is still sequencing work with a timer, which the ADR forbids in the Logic Tier without
exception — so this brief, implemented literally, adds a Logic Tier test that violates the rule the epic
exists to establish.

There is also a semantic trap in the literal-removal criterion: `unwrap_or(2)` appears both in the
`node_retry.delay` event data and in the actual wait (`src/runtime.rs:3967`). A build-profile shortcut
applied to both would make tests validate a different externally reported delay from production.

If revived: separate delay *policy* from *waiting*. Keep the configured duration as data, give the test a
controlled retry release or an immediately-completing wait adapter, and assert attempt outcomes and the
reported duration independently. Do not add another `cfg(test)` policy fork.

## Agent Brief

**Category:** bug
**Summary:** Make the cursor-task retry delay unable to charge a test the production default, and give the retry loop a test that actually reaches it.

**Baseline:** this slice is blocked by `ISSUE-260902-0747-01`, so every criterion below is read
against the tree **after** the tier selector exists.

**Current behavior:**
A residue carried here because it has no other home (`PRD-260902-0301-01` § Further Notes).

*A retry sleep no test reaches.* The cursor-task retry path sleeps for the node's configured retry
delay, defaulting to two seconds when unset. No current test reaches it: the attempt loop is sized
from the node's retry count, which is one when that field is unset, so the loop breaks before the
sleep. The latent cost appears the moment a test sets a retry count and leaves the delay unset —
note the polarity, because it inverts the intuition: setting a delay is what would *avoid* the
default, not what triggers it. Nothing in the tree stops that from happening, and when it does it
becomes a two-second wall-clock wait inside the gate.

Discovery commands. The premise is about the **retry count**, so search that; a `retry_delay` search
alone argues against the claim it looks like evidence for, because its hits are mostly
`retry_delay: None` — the condition that *triggers* the default rather than avoiding it:

```
rg -n 'retry_count' src/ tests/
rg -n 'retry_delay' src/runtime.rs
rg -n 'fn max_node_retry_attempts' -A 8 src/model.rs
```

Read the `retry_count` hits and check where each can reach `run_cursor_task`; that is what establishes
whether the sleep is reachable today, and it is what a later reader will need to recheck.

**Desired behavior:**

*The retry delay cannot charge a test the production default.* The delay resolves through a
build-profile shim, in the same shape as the two shims the repository already uses for the pane-stream
owner wait and the abort drain wait (`ADR-260902-0312-01` § The three exemplars, "Shim the timeout by
build profile"), so a test that sets a retry count pays a test-scale delay rather than the production
one. Match the test-build value the two existing shims use rather than inventing a third scale.

**A citation guard will go red if the shim is written the obvious way, and this record authorizes the
fix.** A `Duration`-returning shim moves `from_secs` out of `run_cursor_task`'s extent. `from_secs`
occurs many times elsewhere in the file, so `check_citation` (`tests/docs_catalog.rs:1820-1831`)
reports `DiscriminantOutsideSymbol` and the live out-of-crate test `workflow_schema_citations_resolve`
turns red. The guarded citation is `docs/workflow-schema.md:1073`. Two branches are authorized and
exactly one is taken: update that citation to the symbol the discriminant now sits in, or keep
`from_secs` inside `run_cursor_task`'s extent so the citation still resolves. Do not discover this by
running the suite — cost it while designing the shim. Production behavior outside a test build is unchanged: an operator's configured delay is honored
exactly as it is today, and the default an operator gets when the field is unset is unchanged.

Because the site is currently unreached, it also gains a test that actually exercises the retry loop —
setting a retry count and leaving the delay unset, which is the combination that trips the default.
That test asserts the retry behavior, waits on no clock, and is Logic Tier.

The out-of-crate constraint applies: the shim is a `cfg!(test)` construct and therefore does not reach
across the crate boundary, so no out-of-crate target may depend on it.

**Key interfaces:**
- The cursor-task retry delay resolution — a shim helper in the shape of the existing two, rather than
  an inline default at the sleep site.

**Acceptance criteria:**
- [ ] `rg -n 'unwrap_or\(2\)' src/runtime.rs` returns no matches; before this change it returns the
      retry-delay default, and every site it returns is one this change has to account for. Read the
      command's output rather than a list here: the default is read at more than one site and each
      has to lose the literal.
- [ ] A test sets a node's retry count while leaving its retry delay unset, drives the cursor-task
      retry loop through more than one attempt, and asserts the retry behavior. It ends no wait at a
      wall-clock deadline and asserts on no elapsed time; it may pay the shim's test-scale delay,
      which is what the shim exists to bound. It is executed by the default command; no test reaches
      the retry sleep before this change.
      Read tier membership from what the command reports it executed, not from
      `cargo test -- --list`.
- [ ] Outside a test build the retry delay is unchanged: an operator's configured delay is honored,
      and the default applied when the field is unset is the same value as before.
- [ ] No out-of-crate target reads the retry-delay shim.
- [ ] `workflow_schema_citations_resolve` passes after this change. If the shim moved `from_secs` out
      of `run_cursor_task`'s extent, the record states which of the two authorized branches was taken
      and the citation at `docs/workflow-schema.md:1073` resolves under it.

**Out of scope:**
- The HTTP target's clock waits — its polling helper and its unconditional sleeps. Those were part of
  this record until 2026-09-02 and are now `ISSUE-260902-0747-11`, which owns the whole of "the HTTP
  target stops waiting on the clock" and carries the epic's *regression* acceptance for the
  reproduced failure. The closing acceptance over the final gate population is
  `ISSUE-260902-0747-10`'s.
- Changing retry semantics — how many attempts a retry count buys, or what counts as a retryable
  failure.
- The runtime and API polling helpers — `ISSUE-260902-0747-05`.
- The testing document's stale claims about this target's transport and its test-case inventory. That
  is documentation drift owned by the docs-truth family.

## Triage Notes

Minted 2026-09-02 from `PRD-260902-0301-01`; breakdown approved by the maintainer the same day.
Blocked by `ISSUE-260902-0747-01` for the tier assignment in its acceptance criteria.

Both findings originate in `ISSUE-260901-0216-03` § "Related defects found during the audit", and the
PRD carries them forward in § Further Notes precisely because neither belongs to any other slice. The
retry finding is the one worth reading twice: it is latent rather than live, so a reviewer looking for
a currently-failing test will not find one, and the acceptance criterion above is what makes the site
reachable.

### Readiness gate round 1 — findings

**Readiness gate (cold-reader): FAIL** (round 1)

All ten `ISSUE-260902-0747-*` briefs were gated in parallel on 2026-09-02 (`cold-reader`, `model: opus`,
one per record, rubric `~/.claude/skills/triage/READINESS-GATE.md`). All ten returned FAIL. The reports
were not written to disk; the compressed findings below and in
`docs/handoff/deterministic-test-tiers-2026-09-02-1419.md` are the sole surviving record of them.

`status:` is `needs-info` because two upstream things are owed. `PRD-260902-0301-01` was amended after
this round (HTTP endpoint tests moved to the Logic Tier, storage split per-test, the Logic Tier rule
restated as isolation with controlled data) and is itself awaiting a re-gate; and
`ADR-260902-0312-01` now decides tier membership by **what a test isolates**, not by what it touches,
which every brief in this batch predates.

**Systematic defects across the batch** (fix in one sweep, not per record):

1. **Count-as-polarity** — pre-change polarity written as a count beside a discovery command
   ("It returns four test call sites as well before this change"). `AGENT-BRIEF.md`
   § *Qualitative polarity vs decaying state* requires qualitative phrasing, never a count.
   Present in `-02`, `-04`, `-06`, `-07`, `-08`, `-09`; the correct form is already used in `-01`,
   `-03`, `-05`, `-10`.
2. **Broken listing template** — "appears in the Logic Tier listing (`cargo test --locked -- --list`)
   after this change and did not before it" is false wherever a test is already deterministic.
   Confirmed false in `-07` and `-08`. Present in `-05` through `-09`.
3. **`cargo test -- --list` includes `#[ignore]`d tests** — proven by `regeneration_writes_to_disk`
   appearing in the listing. Any `#[ignore]`-shaped selector makes every listing-based criterion in
   the batch unfalsifiable. This constrains `-01`'s selector choice.
4. **Templated `just test-under-load` closing AC** in `-05`, `-06`, `-07`, `-09`, `-10` is already
   green at each record's declared baseline (post-`-01`); only the unexecutable qualifier carries
   content. `just test-under-load` is also not in version control and has no owning slice.

**This record:**

- Failure class 6 prong (b): two crates, two tiers, two independently-green AC subsets `{AC1,AC2}` and `{AC3–AC6}`, bound only by "no other home". **Split recommended.**
- The premise that the HTTP test "stays in the Integration Tier" is **stale** — the amended PRD places HTTP endpoint tests in the Logic Tier.
- `test_router_with_db()` already exists at `tests/http_api.rs:95` and returns the `Database` handle, so the fix is a helper swap rather than new plumbing (`creates_and_approves_runs` calls `test_router()` at `tests/http_api.rs:1185`).
- Count-as-polarity, broken listing template, templated closing AC (systematic defects 1, 2, 4).

### Readiness gate round 2 — findings

**Readiness gate (cold-reader): FAIL** (round 2)

- **Class 7 kind (a) — the discovery command does not derive the premise.** "No current test reaches it" rests on `retry_count`, but the only command searches `retry_delay`, whose hits are mostly `retry_delay: None` test lines — the condition that *triggers* the default. The command argues against the claim it is offered as evidence for. The premise itself is true: the only non-`None` `retry_count` settings are validation tests in `src/model.rs` that never reach `run_cursor_task`, and `max_node_retry_attempts(None)` is 1.
- **Class 7 kind (a) — AC1's "at both the site that logs it and the site that sleeps for it"** asserts cardinality and composition of the current tree beside the command.
- **Class 4, high value — the prescribed shim breaks a live out-of-crate test.** A `Duration`-returning shim in the shape of the existing two moves `from_secs` out of `run_cursor_task`'s extent; `from_secs` occurs 19 times elsewhere in the file, so `check_citation` (`tests/docs_catalog.rs:1820-1831`) reports `DiscriminantOutsideSymbol` and `workflow_schema_citations_resolve` goes red. The guarded citation is `docs/workflow-schema.md:1073`. The brief authorizes neither branch (edit the citation, or keep `from_secs` inside the symbol).
- **Class 4 — AC2 contradicts Desired behavior:** "waits on no clock" against "pays a test-scale delay". The shim's test-build value is unpinned; both in-tree shims use 250ms.
- Class 6 does not fire. Class 9 does not fire — AC1 and AC2 are genuinely red at baseline.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round2.md`

### Round 2 repairs applied 2026-09-02

Discovery now searches `retry_count`, which is what the reachability premise actually rests on; the
`retry_delay` search is kept with its polarity stated, since its hits are the trigger condition rather
than evidence against the premise. The citation-guard hazard the shim creates is costed in the brief
with both remedies authorized and a criterion that the guarded test still passes. AC2 no longer
contradicts Desired behavior — it forbids clock-ended waits and elapsed assertions rather than any
delay at all, since the shim's whole purpose is to bound one. Baseline stated. Awaiting round 3.

### Round 3 findings — 2026-09-02

**Readiness gate (cold-reader): FAIL** (round 3)

AC1's pre-change composition claim — the default read "at both the site that logs it and the site
that sleeps for it" — is replaced by the command's own output. The third discovery command was aimed
at `src/runtime.rs`, where `max_node_retry_attempts` is not defined; it now names `src/model.rs`. The
out-of-scope line attributing "the epic's acceptance" to `-11` now distinguishes regression acceptance
there from closing acceptance at `-10`.

Full round report: `docs/prd/adversary-reports/PRD-260902-0301-01-readiness-gate-260902-briefs-round3.md`.
Awaiting re-gate.
