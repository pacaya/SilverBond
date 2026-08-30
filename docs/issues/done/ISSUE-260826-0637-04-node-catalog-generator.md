---
id: ISSUE-260826-0637-04
kind: issue
category: enhancement
status: done
summary: Generate the node-kind catalog from constructed Rust values into the marker blocks, with coverage closed against serde's own derives
prd: PRD-260826-0009-01
terms: [Node Kind, Workflow, Validation Issue]
blocked_by: [ISSUE-260826-0637-03]
claimed_by: implement-issue@Mac-mini-4
claimed_at: 2026-08-30T03:40:29Z
---

## Agent Brief

**Category:** enhancement
**Summary:** A new integration test that constructs every node kind as a Rust value, emits a
readable fragment and a complete valid workflow per kind into the schema reference's generated
blocks, and fails the suite when a kind is missing, mis-mapped or stale.

**Current behavior:**
The schema reference's node catalog is hand-written, which is how it came to document four of
fourteen kinds. After the scaffold issue the document has marker-delimited blocks inside
`## Node catalog` and nothing between them.

Nothing in the repository enumerates node kinds for documentation. The nearest existing pattern —
a round-trip test over a hand-seeded vector of `NodeKind` values — is the weaker version of what
this issue builds: its vector has no counterpart check, so omitting a variant both compiles and
passes.

**Desired behavior:**
A test target constructs each node kind as a Rust value, serializes it, and writes two things per
kind into the generated blocks: a readable node fragment a reader can copy, and a complete
workflow document that passes validation. The fragment is extracted from that same document, so
the two can never disagree.

Examples are constructed, never transcribed. This is the point of the design: no struct in the
model denies unknown fields, so a misspelled key in transcribed JSON deserializes clean and
validates clean. A constructed value cannot carry a field the struct lacks, because it would not
compile.

The library already supplies the variant skeleton — the conversion from the node-type tag enum to
`NodeKind` is an exhaustive match producing one value per variant. It supplies shape only: the
values it produces are defaults, and most of them fail validation, because a decide node needs
outcomes, a batch needs its bindings and body entry, a subflow needs a workflow name, and task
and run-agent nodes need an agent. So example data is hand-authored per kind on top of that
skeleton. What construction buys is not the absence of hand-written content but that the content
is compile-checked.

*Running the generator must not rewrite the tree.* By default it regenerates in memory and asserts
the committed markdown matches. Setting an environment variable makes the same code write instead,
and a `just` recipe wraps that so a contributor who trips the freshness assertion has a command to
run — the command the scaffold's `## Regenerating this document` section names. This is the
`expect-test` / `insta` pattern, not an invention.

**Coverage is closed against serde's derives, with no new dependency.** An exhaustive match proves
every input is handled; it does not produce one value per variant, so on its own it cannot
guarantee the catalog visits all fourteen. Four checks close it:

1. **Compile stop.** The example function is exhaustive over the node-type tag enum, so adding a
   variant fails to compile. This fires under `cargo test` only, because `cargo build` and
   `cargo check` do not compile the integration-test target — which is why the CI job matters.
2. **The tag enum's variant list, harvested from its own `Deserialize` derive.** The derived
   implementation hands the deserializer a static slice of wire names. A throwaway deserializer
   captures that slice and returns an error directly, without a visitor.
3. **The `NodeKind` variant list, harvested from *its* derive.** `NodeKind` is internally tagged,
   so the deserializer path above is unreachable for it — but feeding it a one-entry map whose tag
   is a sentinel makes the derive reject it through serde's unknown-variant error, whose expected
   argument is the full wire-tag list. Capture it structurally; do not parse the error's string.
4. **Set equality across both harvests, plus a round-trip.** The wire names the catalog emits must
   equal both harvested sets, and the catalog's declared order must have the same length, so an
   omission fails equality and a duplicate fails the length check. Each entry must round-trip
   through the value's own node-type accessor, catching an arm that returns the wrong kind.

Together these catch the case a bare exhaustive match cannot: a `NodeKind` variant added without a
matching tag-enum variant and mapped onto an existing tag. The `NodeKind` harvest gains it while
the catalog and the tag harvest do not.

*Each emitted workflow crosses the document boundary.* Serialize it, read it back with strict
deserialization into the workflow struct, then run it through validation, and require zero
error-severity issues. (Validation applies defaulting internally, so invoking it separately is
harmless but redundant.) Validate the deserialized document, not the constructed value
— validating in place would test the Rust struct rather than the document a reader copies, and
would miss an asymmetric serde change that makes emitted JSON fail to load. Use strict
deserialization rather than the normalization path production ingress takes: that path runs the
flat-to-nested migration unconditionally and would rescue exactly the malformed shapes this epic
exists to eliminate.

**Key interfaces:**
- The generator lives in a **new file under `tests/`**, joining the crate's existing integration
  test. This location is load-bearing: an inline test module inside any `src/` file edits `src/`,
  and a new module under `src/` needs a declaration in the crate root, both of which the epic
  forbids. A file under `tests/` compiles as its own crate against the public API, and the types
  it needs — the node-type tag enum, the conversion into `NodeKind`, the defaulting function and
  the validation function — are all public.
- **A separate binary target is rejected** on a concrete collision: the crate declares no default
  run target and has one implicit binary, so a second makes unqualified `cargo run` ambiguous and
  breaks both the server recipe and the e2e harness.
- The generated blocks and their block-id scheme come from ISSUE-260826-0637-03. Rewrite only the
  span between a marker pair; never the markers themselves, and never text outside them.
- Zero new dependencies, including dev-dependencies. `serde` with `derive` and `serde_json` are
  already present and are all this needs. A variant-iteration derive crate would work but only by
  attaching a derive inside `src/`, which this epic forbids itself.

**Acceptance criteria:**
- [ ] A new integration-test target exists and runs under `cargo test`. Observable:
      `cargo test --test <name>` succeeds; before this change no such target exists and the
      command fails.
- [ ] Every node-kind wire tag the model defines appears inside a generated block in
      `docs/workflow-schema.md`. Observable: derive the tag set from the source, then for each
      tag confirm it falls between a `BEGIN GENERATED` and its matching `END GENERATED` marker.
      No tags appear inside markers before this change, because the blocks are empty.
- [ ] Removing one entry from the catalog's declared order makes the **set-equality** assertion
      fail, with a message naming the wire tag present in the harvested set and absent from the
      catalog. Restore it afterwards. "The suite goes red" does not discharge this: the freshness
      assertion reddens on the same mutation, so an implementer who built only freshness would pass
      a red-suite check while the coverage apparatus does not exist. The message must name the tag.
- [ ] Duplicating a catalog entry makes the **length** check fail, with a message distinguishing a
      length mismatch from a set mismatch.
- [ ] Making one example arm return a kind other than the one it is keyed to makes the
      **round-trip** check fail, with a message naming both the expected and the actual kind.
      Each of these three mutations must be identified by its own assertion's own message — three
      distinct signals, not one shared red.
- [ ] Every emitted workflow, after strict deserialization, defaulting and validation, produces
      zero error-severity validation issues. This is asserted by the suite, not by inspection.
- [ ] No emitted workflow trips the warning the validator raises when a node whose kind ignores
      task-execution config carries some of it anyway. Derive that predicate from the validator
      rather than from this brief — it covers more fields than a reader would guess, and the
      node-level prompt field is the one a generated example is most likely to trip, because it
      sits on every node regardless of kind. Assert this in the suite alongside the error-severity
      check; an error-only bar would let exactly this regression back in, which is why it is called
      out separately.
      **Do not substitute a zero-issues-of-any-severity bar.** It is unsatisfiable: a node with no
      outbound edges always warns that it is terminal, so every acyclic example trips it, and for
      `subflow` and `call` it is impossible outright — a referenced subflow body must have exactly
      one terminal node or validation errors, and the catalog validator then raises the terminal
      warning on that mandatory node. The only way to dodge it would be emitting cyclic example
      workflows, which is worse than the defect.
- [ ] Running `cargo test` does not modify any tracked file. Observable:
      `git status --porcelain --untracked-files=no` is byte-identical before and after a suite run.
      Restrict to tracked paths deliberately — the working tree carries untracked planning records,
      and an unrestricted comparison would be stricter than the criterion states.
- [ ] Setting the regeneration environment variable rewrites the generated blocks in place, and a
      `just` recipe invokes it. Observable: the recipe exists in the justfile and the command
      named by `## Regenerating this document` resolves to it.
- [ ] Regeneration actually writes, and is idempotent. Observable at the seam, in this order:
      copy the working tree; in the copy, mechanically perturb one generated block (delete its last
      line); run the regeneration there and assert the block returns byte-identical to its
      committed form. That perturbation is the positive control — without it, a write mode that
      silently never writes (mis-named variable, marker-lookup miss, swallowed path error) passes
      an idempotence check trivially, and passes the no-tracked-file-modified criterion at the same
      time. Only then assert that a second regeneration on the unperturbed tree produces no diff.
- [ ] Zero dependency lines are added to `Cargo.toml`. Observable: the dependency and
      dev-dependency sections are byte-identical before and after.
- [ ] Nothing under `src/` or `ui/` is modified.

**Out of scope:**
- Generating anything other than the node catalog: no prose, no field tables, no execution-model
  content, and no block in any file other than `docs/workflow-schema.md`.
- The Tier P tables that surround the catalog. A serialized example shows *a* value and can never
  label it as *the* default — for kinds that skip defaulted fields on serialization it omits the
  field entirely. Defaults live in hand-written tables with their own issues.
- Extending the generator to the bundled workflow templates.
- Generating field tables from JSON Schema. That is a second generator, excluded by the PRD.
- Adding the CI job. It is ISSUE-260826-0637-01 and does not block this work.

## Context Pack — generated at claim (2026-08-30T03:40:29Z)

**PRD decisions relevant to this slice** (PRD-260826-0009-01):
- Tier G = the node-kind catalog only; generator emits marker-delimited blocks, never whole documents, and only in `docs/workflow-schema.md`.
- Examples are constructed Rust values on the `From<WorkflowNodeType> for NodeKind` skeleton (`src/model.rs:820`), not transcribed JSON — no model struct denies unknown fields.
- Coverage closed by four checks: compile stop on an exhaustive `example_for`; `WorkflowNodeType` harvest from its `Deserialize` derive (`src/model.rs:158-159`, `rename_all = "snake_case"`); `NodeKind` harvest via `unknown_variant`'s `expected` (internally tagged, `src/model.rs:694-695`); set equality + length + `node_type()` round-trip (`src/model.rs:756`).
- Generator lives at `tests/docs_catalog.rs`; in-memory freshness assert by default, writes only under `SB_REGEN_DOCS=1`, wrapped by a `just` recipe. A second binary target is rejected (no `default-run`; `cargo run` in `justfile:17-18` would go ambiguous).
- Zero new dependencies or dev-dependencies; nothing under `src/` or `ui/` is edited.
- Field tables/defaults stay Tier P hand-written — a serialized example cannot label a default, and skip-heavy configs omit fields entirely.

**Test seam & Testing Decisions:** observable at the integration-test target `tests/docs_catalog.rs` run under `cargo test --test docs_catalog`, plus the marker spans in `docs/workflow-schema.md:141-249`. PRD Testing Decisions: (1) compile-time construction; (2) freshness diff against committed markdown + set equality + round-trip; (3) `NodeKind` derive harvest; (4) serialize → strict `serde_json::from_value::<WorkflowV3>()` → `ensure_defaults` (`src/model.rs:1116`) → `validate_workflow` (`src/model.rs:1422`), asserting zero `error`-severity issues. Strict deserialization deliberately, not `normalize_workflow_value` — `migrate_v2_nodes_to_v3_kind` fires unconditionally (`src/model.rs:1152`) and would rescue malformed shapes.

**Concrete surfaces the brief names:**
- Tag enum `WorkflowNodeType` — `src/model.rs:159` (derive/rename at `:157-158`); `NodeKind` — `:695` (tag attr `:694`); `NodeKind::node_type()` — `:756`; `WorkflowNode::node_type()`/`node_type_str()` — `:913`, `:917`; `impl From<WorkflowNodeType> for NodeKind` — `:820-860`.
- Defaulting `ensure_defaults` — `src/model.rs:1116`; validation entry `validate_workflow` — `:1422`. All public via `pub mod model` (`src/lib.rs:6`).
- Ignored-task-execution-config warning: predicate `has_task_execution_config` — `src/model.rs:3116-3130`, reading `agent`, non-empty `prompt`, `context_sources`, `response_format`, `output_schema`, `retry_count`, `retry_delay`, `timeout`, `skip_condition`, `loop_max_iterations`, `loop_condition`, `kind.agent_config()`, `cwd`. Fires for `Split | Collector` at `:1925-1927`; existing coverage test `warns_when_non_task_nodes_keep_task_execution_fields` at `:3917`. The unavoidable terminal-node warning is at `:1916-1923`.
- Weaker prior pattern: `node_kind_round_trip_all_variants` — `src/model.rs:4181`, a hand-seeded `Vec<NodeKind>` with no counterpart check.
- Existing integration target: `tests/http_api.rs` (the only file under `tests/`).
- Markers landed by ISSUE-260826-0637-03: `<!-- BEGIN GENERATED: node-catalog:<wire-tag>:fragment -->` / `:workflow`, one pair each, 14 kinds, `docs/workflow-schema.md:141-249`; hand-written `### <wire-tag>` headings sit above each pair. `## Regenerating this document` (`:276-288`) names `just regen-docs` and attributes it to this issue.
- `justfile` has no `regen-docs` recipe yet; convention is a `#` comment line above each recipe, kebab-case names (`test-rust`, `check-v4-docs`).
- `Cargo.toml:6-35` `[dependencies]` (incl. `serde` with `derive` at `:22`, `serde_json` `:23`) and `[dev-dependencies]` `:37-39` must stay byte-identical.

**ADRs:**
- ADR-260815-2009-01 — Typed workflow contracts over a JSON-valued variable store · accepted (forward reference only; v4 is canonical here).

**Terms:**
- `Node Kind` — the fourteen-variant tagged union in a node's required `kind` object, selecting behavior and config shape; its bare `type` tag is what `/api/capabilities` publishes.
- `Workflow` — a versioned node/edge graph definition (schema `version: 4`; v2–v3 normalize forward at ingest), including its subflow catalog.
- `Validation Issue` — one `{severity, nodeId?, scope?, message}` entry; `severity` is only ever `error` or `warning` (no `info`); validation is enforced at run start, never at save.

**Full artifacts:** docs/prd/PRD-260826-0009-01-docs-from-rust-truth.md · docs/adr/INDEX.md · CONTEXT.md · docs/workflow-schema.md · docs/issues/done/ISSUE-260826-0637-03-schema-doc-scaffold.md

## Code Review

Review file: `issue-260826-0637-04-code-review-20260830-041331.md`

Dual review (Claude + Codex), cross-verified and adjudicated inline, then three fix rounds (cap 4). **ACCEPTED** — both reviewers returned zero new findings on the final round and independently recommended close. 24 findings: 20 FIXED, 1 deferred, 3 dismissed.

Round 1 (cursor) closed 11 of 12 but shipped the tree red — it left its own duplicate-entry verification mutation in `CATALOG_ORDER` while reporting a green suite — and its gate redesign introduced four defects that were really one design fault. Same-site escalation was invoked; round 2 (codex) removed the mechanism against five stated properties. Round 3 (cursor, narrow) closed three assertion-precision findings.

- H1 (HIGH): Regeneration positive control never exercises the env-gated write path — FIXED
- M1 (MEDIUM): Generator bypasses the `NodeKind::from(WorkflowNodeType)` skeleton; doc comment says otherwise — FIXED
- M2 (MEDIUM): Stale or orphan `node-catalog:*` marker blocks are never rejected — FIXED
- M3 (MEDIUM): Three prose passages still describe the generator as not yet existing — FIXED
- M4 (MEDIUM): Marker span lookup duplicated (promoted in-diff duplication smell) — FIXED
- L1 (LOW): `SB_REGEN_DOCS` gate accepts any value — FIXED
- L2 (LOW): Exported `SB_REGEN_DOCS` races sibling tests — FIXED in round 2 (round 1 attempt failed)
- L3 (LOW): `regeneration_is_idempotent` carries no independent signal — FIXED
- L4 (LOW): `parallel_batch` binds an undeclared variable — FIXED (Codex's STILL_BROKEN overturned on read evidence)
- L5 (LOW): Validation failure messages do not name the kind — FIXED
- L6 (LOW): Examples emit an avoidable empty-prompt warning — FIXED
- L7 (LOW): Subflow child body's `name` duplicates the parent's — FIXED
- C1 (CRITICAL): Unreverted verification mutation shipped the tree red — FIXED
- M5 (MEDIUM): `cargo test -- --include-ignored` hard-failed — FIXED
- L8 (LOW): Positive control and justfile binding never ran under `cargo test` — FIXED
- L9 (LOW): `just regen-docs` wrote without coverage or validation assertions — FIXED
- L10 (LOW): Marker inventory ignored orphan closing markers — FIXED
- L11 (LOW): P2 guard accepted any panic as proof — FIXED
- L12 (LOW): Two regression mutations collapsed to one diagnostic — FIXED
- L13 (LOW): Justfile binding cemented a `--locked` convention mismatch — FIXED
- L14 (LOW): Narrow writer/reader overlap under `SB_REGEN_DOCS=1` plus `--include-ignored` — deferred: two combined opt-ins to reach it, and the fix would reopen a write path now mutation-proven across a full round; worth a follow-up issue
- D1 (CLAUDE-3): `has_task_execution_config` transcribed rather than derived — dismissed: the derived warning-message filter is the contract check and covers subflow scopes
- D2 (CLAUDE-10): Lowercase wire tag as node `name` — dismissed: cosmetic, no contract constrains casing
- D3 (CLAUDE-11): Examples publish `limits: {0, 0}` — dismissed: `src/model.rs:240-244` defines that as the canonical absent-equivalent form

Smells: 5 advisory, all merged into `docs/issues/SMELLS-LEDGER.md` (all newly appended). No graduation rows. One duplication smell was promoted to finding M4 by the in-diff rule.

Carried forward, not attributable to this diff: `runtime::tests::completed_run_cleans_non_persistent_active_panes` and `runtime::tests::decide_abort_returns_promptly_and_kills_pane` fail intermittently at `src/runtime.rs:9055` on a tmux sentinel timeout. Both reviewers flagged them independently across two rounds; they will make the CI job from ISSUE-260826-0637-01 flaky and deserve their own issue.

## Triage Notes

**Readiness gate (cold-reader): PASS** (round 2)

Round 1 returned findings on three class-9 arm-B criteria; all were fixed before round 2.
Round 2 ran the full nine-class rubric and verified the design's load-bearing serde premises by
compiling probes outside the repository: both variant-harvest techniques return the full wire-tag
list, structurally and without string parsing, using only dependencies the crate already carries.

**Readiness gate (cold-reader): REOPENED** (round 3, 2026-08-26, breakdown-adversary finding)

The round-9 breakdown adversary found that the generated-example criterion admitted only
error-severity issues, so an example reproducing the ignored-task-field warnings the drift audit
records could be regenerated while satisfying every criterion. The bar now covers any severity.

**Readiness gate (cold-reader): FAIL** (round 4, full-enumeration) — class 4, unverifiable
acceptance criterion.

The round-3 tightening to "zero validation issues of any severity" is unsatisfiable. Any node with
no outbound edges emits a terminal-node warning, so every acyclic example trips it; and for
`subflow` and `call` the bar is provably impossible, because a referenced subflow body must have
exactly one terminal node (an error otherwise) and the catalog validator runs the same graph-body
pass over it, so that mandatory terminal node always warns. The only way to satisfy the bar for the
other twelve kinds would be to emit cyclic example workflows, which documents an infinitely-looping
workflow as the copyable example. The tightening also contradicts this issue's own PRD, whose
Testing Decisions check 4 specifies zero *error*-severity issues.

The adversary finding that motivated the tightening stands: the ignored-task-field condition is a
warning, not an error, so an error-only bar does permit that regression. The premise is sound; the
bar was not. Reconciliation between this brief and the PRD is owed before re-gate.

**Readiness gate (cold-reader): PASS** (round 5, full-enumeration)

The round-4 class-4 finding is discharged. The replacement pair was verified jointly satisfiable
for all fourteen kinds by deriving both conditions from source and constructing a minimal example
per kind: the two bars are orthogonal, because the ignored-field predicate reads only optional
node-level fields while every error rule on a split or collector reads edges and merge keys, and
defaulting cannot reintroduce either. It closes drift-audit items 31-32 exactly — the live Split
and Collector examples carry three of the predicate's disjuncts between them — and is non-vacuous,
since the catalog must document both kinds. The predicate is reachable in two executable hops, and
agreement with the PRD's Testing Decisions check 4 is restored rather than relocated. All eight
factual claims in the new prohibition paragraph reproduce true. Classes 1-5 swept 30 surfaces,
class 7 swept 37, class 9 arm A executed every in-scope observable (red) and arm B's four triggered
requirements were each satisfied. No class fired.

## Resolution

**Commit:** `feat: generate the node-kind catalog from constructed Rust values (ISSUE-260826-0637-04)`

**Route:** implementation `cursor` — route-picker classified the whole issue as a well-specified single-file integration test with no cross-module or codex-only signals. Fix rounds were routed independently: round 1 `cursor`, round 2 **`codex`** (escalated — the round-1 fix introduced four defects in one region, so round 2 was briefed as a mechanism removal against five stated properties, and the batch carried cargo-harness, `#[ignore]`/`--include-ignored`, test-parallelism and CI-invocation semantics), round 3 `cursor` (three local assertion changes, no redesign).

**TDD:** `red-green at tests/docs_catalog.rs` (observable via `cargo test --test docs_catalog`). The trigger fired because the acceptance criteria require each coverage assertion to be shown red under its own targeted mutation — assertion-first is the only way to discharge that, and every round was verified by re-running those mutations rather than by reading the new code.

**What shipped:** a new integration-test target `tests/docs_catalog.rs` that constructs all fourteen node kinds on the `From<WorkflowNodeType>` skeleton, emits a fragment and a complete valid workflow per kind into the 28 marker-delimited blocks of `docs/workflow-schema.md`, and closes coverage against serde's own derives via two structural harvests (the tag enum's `deserialize_enum` variant slice; `NodeKind`'s `unknown_variant` expected slice), set equality, a length check, and two independent round-trips. Each emitted workflow crosses the document boundary — serialize, strict deserialize, default, validate — asserting zero error-severity issues and no ignored-task-execution-config warning. Regeneration is compare-only by default and writes only through one guarded entry point, wrapped by `just regen-docs`. Nothing under `src/` or `ui/` was modified and `Cargo.toml` is byte-identical.

**Review telemetry:** dual review (Claude + Codex), cross-verified and adjudicated inline; **3 fix rounds of a cap of 4**. 24 findings — by severity 1 CRITICAL, 1 HIGH, 5 MEDIUM, 17 LOW; by outcome **20 FIXED, 1 deferred, 3 dismissed**. The CRITICAL (a red tree from an unreverted verification mutation) and one MEDIUM were introduced by fix round 1, not present in the original implementation. Same-site escalation was invoked once, at round 2. Adjudication went against a reviewer four times on read evidence: three of Claude's findings dismissed, and Codex's `STILL_BROKEN` on L4 overturned because it cited only the `var:` branch of `resolve_input_binding_value` and stopped before the bare-name fallback at the function's tail. 5 advisory smells merged into `docs/issues/SMELLS-LEDGER.md`; one duplication smell was promoted to finding M4 by the in-diff rule. Review file: `issue-260826-0637-04-code-review-20260830-041331.md`.

**Suite:** `SUITE: PASS` — 601 tests green (Rust 472 across lib/`docs_catalog`/`http_api`, frontend vitest 129), 0 failed, 1 ignored by default and 0 with `--include-ignored`. `git status --porcelain --untracked-files=no` byte-identical before and after the run. `just test-e2e` was deliberately not run: it builds the frontend into the tracked `public/` tree, which would violate the working-tree constraint this issue is verified under, and it needs Playwright browser binaries.

**Follow-up worth filing (not a blocker, not caused by this diff):** `runtime::tests::completed_run_cleans_non_persistent_active_panes` and `runtime::tests::decide_abort_returns_promptly_and_kills_pane` fail intermittently at `src/runtime.rs:9055` with `timed out waiting for path .../tmux-pane-killed`. Both reviewers observed them independently across two rounds; they passed cleanly in the closing suite run. They will make the CI job from ISSUE-260826-0637-01 flaky. Finding L14 (a writer/reader overlap reachable only by combining `SB_REGEN_DOCS=1` with `--include-ignored`) is deferred and also worth a follow-up.

**Closed:** 2026-08-30 (UTC)
