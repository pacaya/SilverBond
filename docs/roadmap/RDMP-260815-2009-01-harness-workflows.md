---
id: RDMP-260815-2009-01
scale: initiative
stakes: internal
gate: passed 2026-08-16
adrs: [ADR-260815-2009-01, ADR-260815-2009-02, ADR-260815-2009-03, ADR-260815-2009-04, ADR-260815-2009-05]
prds: [PRD-260826-0009-01]
source: grilling 2026-08-15 (conversation-entered; research ingested at docs/sources/prior-art-workflow-engines.md)
---

# Harness as Workflows

## Problem

The user's agent harness (issue-dev, implement-issue, and kin) lives as chat-skill prompt discipline; the goal is to represent it as SilverBond workflows where the logic lives in graph structure and scripts, most nodes run cheap models, and only high-stakes steps use SoTA models. Exploration established the engine already covers most primitives (subflows, conditions, parallel batch, budgets, long-lived panes); this initiative closes the verified gaps — typed contracts, a richer condition form, a script node, mutable run state, and profile-driven model routing — then proves expressiveness by shipping the two harness workflows. The port is idiomatic, not mechanism-for-mechanism: success is outcome-level — normalized record/commit equivalence plus measured cheap-model routing and demonstrated concurrent runs (acceptance criteria owned by the harness-port epic), not step-by-step equivalence.

## Dimension Scan

| Dimension | Verdict | Citation |
|---|---|---|
| problem/success | decided | ## Problem |
| scope boundary | decided | ## Coverage + per-epic exclusions in ## Epics |
| domain terms | decided | CONTEXT.md (seeded by this roadmap) |
| architecture shape | decided | ADR-260815-2009-01…05; existing engine shape ratified as-is (ARCHITECTURE.md); docs-truth refreshes the workflow-schema/execution-model docs |
| stack | decided | brownfield ratified — Rust backend + Svelte 5 runes (CLAUDE.md conventions); zero new runtime dependencies initiative-wide (existing tokio/serde_json/rusqlite suffice; the condition dialect is dependency-free by decision, ADR-260815-2009-02) |
| data/schema | decided | ADR-260815-2009-01 (schema bump to v5 with 4→5 normalization per the v2/v3→4 precedent; Value-typed checkpoint state; versioned checkpoint migration); stores extended: new app-level profiles file (ADR-260815-2009-04); the runs SQLite schema otherwise unchanged |
| contracts/integrations | decided | ADR-260815-2009-04 (tmux-tools agents registry stays installation-truth; driver seam extended with exactly one actual-model capture hook, owned by harness-port) |
| UX (if UI) | decided | run-start form (typed-contracts), condition builder (condition-ast), profile editor (profile-catalog), multi-run-ui epic |
| testing/seams | decided | ## Problem (outcome-level acceptance, harness-port epic); engine seams tested via existing `just test` suites (cargo test / vitest / playwright) |
| NFRs (stakes-triggered) | decided | security posture: ADR-260815-2009-03 + existing privileged process-launch gate (`authorize_and_prepare_run_security`, src/api.rs:778) |
| ops envelope | n/a | local desktop/localhost app (Tauri + embedded assets); no deploy/rollout infrastructure |

## Coverage

| Feature | Epic |
|---|---|
| Docs rewritten from the Rust truth (14 node kinds, v4 shapes) | docs-truth |
| Research artifact preserved in-repo | docs-truth |
| Declared typed inputs/outputs with required flags | typed-contracts |
| Save-time wiring validation + run-start boundary normalization | typed-contracts |
| JSON-valued variable store + single stringify at template sites | typed-contracts |
| Run-start typed-input form | typed-contracts |
| Condition leaf evaluator (typed operator families) | typed-contracts |
| Nested condition AST + onMissing + routable parse_error | condition-ast |
| ConditionBuilder UI regenerated from the condition AST | condition-ast |
| Script node (sh, interpreter-extensible, env-in, gated) | script-node |
| assignVars (binding-only, checkpointed) | cursor-state |
| Cursor-local writes; cross-branch aggregation (`collectorVar` lists, collector `{inputs, summary}` object, Value-typed) | cursor-state |
| App-global profile catalog (persisted, UI-edited) | profile-catalog |
| Variable-driven profile selection + resolved-profile recording | profile-catalog |
| issue-dev + implement-issue as workflows, outcome-validated | harness-port |
| Template-install operation into `WorkflowStore` | harness-port |
| Multi-run UI | multi-run-ui |
| Schema v5 bump, versioned checkpoint migration, subflow re-hydration staleness rule | typed-contracts |
| `failure` edge outcome in the graph model | condition-ast |
| Workflow-local `profiles` map, run-snapshot catalog freezing | profile-catalog |
| Node-level `profile` field, routing tiers, `decide` profile routing, continuation-compatibility validation | profile-catalog |
| Root-scope restart validation (quiescent-checkpoint predicate, root-cursor mirror) | cursor-state |
| Legacy operator family preserving v4 condition semantics | typed-contracts |
| Variable/checkpoint byte caps | cursor-state |
| Large-binding file fallback (`SB_INPUT`) | script-node |

## Epics

## docs-truth: Documentation from the Rust truth

Status: active (PRD-260826-0009-01)

Rewrite `docs/workflow-schema.md` and `docs/execution-model.md` from the Rust source: all 14 node kinds, v4 tagged shapes, edges with conditions, subflows, variables, the tmux node family, and run/checkpoint semantics. No engine code changes. This epic establishes the baseline, not a frozen snapshot: every later schema-changing epic owns updating these two documents, and its `CONTEXT.md` terms, for its delta — the terms it owns are those its ADRs name (`grep '(planned' CONTEXT.md` for the outstanding set). Sharpest exclusion: no schema changes are designed here — this documents what exists so later epics diff against truth.

## typed-contracts: Typed workflow contracts

Status: pending
Blocked by: docs-truth
Invariants: ADR-260815-2009-01, ADR-260815-2009-02, ADR-260815-2009-03, ADR-260815-2009-04, ADR-260815-2009-05

The function seam end-to-end: declared typed inputs/outputs (`string | number | boolean | enum | json`, `required`) on workflows and subflows, save-time subflow wiring validation, run-start override parsing with one-time boundary normalization, the internal variable store migrated to JSON values with one stringify function at template sites — and the run-start typed-input form as the slice's UI face (today's form silently resends stored defaults). Lands the complete v5 document grammar once — serde and validation for the full condition AST (combinators included), the `script` node variant, the `profiles` map, the node-level `profile` field, `assignVars`, and the contracts — with 4→5 normalization per the v2/v3→4 precedent (flat conditions normalize to single-leaf ASTs mechanically); later epics deliver the semantics behind validation gates that reject not-yet-supported features with clear errors, so the version always discriminates document shape. Ships the condition leaf evaluator (legacy + typed operator families, behavior-identical routing) so normalized documents execute; migrated v4 conditions use the `legacy` operator family preserving v4's stringify-then-compare semantics (ADR-260815-2009-02). All v4-pinned surfaces move together — known pins include the CLAUDE.md canonical-version line, the frontend literal `version: 4` type and constructors (`ui/src/lib/types/workflow.ts:269`, `workflowStore.svelte.ts:898`), the `upgrade_run_workflow_json` version predicate (`src/storage.rs:1333`), `scripts/check-canonical-v4-docs.sh` + its CI workflow, the README and docs/README version references, `ARCHITECTURE.md`'s canonical-version statement, the v4 examples in `docs/api-reference.md`, the hardcoded supported-versions error string (`src/model.rs:1143`), stored `workflows/*.json` documents, and the bundled `templates/*.json` re-stamped v5 — and the enumeration is deliberately not the contract: the epic's acceptance includes a repo-wide version-string sweep (`rg -in 'version.*4|canonical'` over code, docs, scripts, templates, and stored workflows) with every hit migrated or justified. Includes the versioned checkpoint migration (`Value`-typed state migrated on read, per the `upgrade_run_workflow_json` precedent) and the subflow staleness rule: catalog entries carry explicit `ref`/`inline` provenance in the v5 grammar — `ref` entries re-hydrated from the store and re-validated at save/validate/run, `inline` entries authoritative as written. Enum declarations carry their allowed-value set inline. Sharpest exclusion: no JSON-Schema validation of `json`-typed values (additive later).

## condition-ast: Condition AST

Status: pending
Blocked by: typed-contracts
Invariants: ADR-260815-2009-02

Implement the semantics of the v5 condition AST whose grammar typed-contracts lands: the evaluator for `all`/`any`/`not` over typed-operator leaves with per-leaf `onMissing`, `parse_error` routing as all-fields-missing, and the ConditionBuilder UI regenerated from the new form. Implements the `failure` edge outcome (grammar landed in typed-contracts): `onMissing: route` and declared node failures travel over it when a failure edge exists, terminal as today otherwise. One dialect across edge and loop conditions; `skipCondition` is untouched — it keeps its current pre-execution shape and semantics (raw-string source + contains/regex) and is out of this initiative's scope. Sharpest exclusion: no string expression language, ever (ADR-260815-2009-02).

## script-node: Script node

Status: pending
Blocked by: typed-contracts, condition-ast
Invariants: ADR-260815-2009-01, ADR-260815-2009-03

A first-class synchronous `script` node kind: `{interpreter: "sh", source}` with static source, env-var/argv inputs, stdout capture with `parse: json | text`, opt-in `SB_OUTPUT` file for named outputs, exit-code-only success routing through the `failure` edge outcome, size caps, and an inspector panel. The node config declares its named outputs (name → type from the contract type set); stdout and `SB_OUTPUT` values are boundary-normalized against that declaration (ADR-260815-2009-01). Bindings whose serialized size exceeds the env/argv ceiling are passed via file — the engine writes the value and exports its path as `SB_INPUT_<NAME>`. Runs under the same `runAs` identity enforcement as pane workloads despite having no pane, and joins the process-launch gate set (`node_launches_process`). Sharpest exclusion: `sh` only — js/python/lua are additive interpreters later.

## cursor-state: Cursor-scoped state

Status: pending
Blocked by: typed-contracts
Invariants: ADR-260815-2009-05

`assignVars` on nodes — binding-only, checkpointed, cursor-local (grammar landed in typed-contracts; this epic implements the write semantics). Cross-branch aggregation rides the existing declared paths, Value-typed: the ordered-list contract is `parallel_batch.collectorVar`'s; the generic collector keeps its `{inputs, summary}` object shape; no new aggregation surface. Restart-from-node is valid only when the checkpoint carries no active split families and no call frames (a quiescent root state), and only at targets needing no arrival context (batch bodies, collectors, and nodes whose templates or bindings reference `previous_output`/`branch_*` are rejected by validation); at quiescent persistence and terminal completion the single root cursor's variable map is mirrored into the checkpoint map restart seeds from. Per-variable and per-checkpoint byte caps fail loudly at write time. Gives loops accumulating state (exclusion lists, counters) a home the engine can checkpoint and resume. Sharpest exclusion: no shared mutable variables across live parallel branches, by construction.

## profile-catalog: Profile catalog and routing

Status: pending
Blocked by: typed-contracts
Invariants: ADR-260815-2009-04

App-global profile store persisted as an app-level `profiles.json` beside `workflows/` (UI-edited), bundling agent/model/reasoning/toolToggles plus a declared routing tier (`cheap | standard | high`). Workflow-local profiles live in a new `profiles` map, distinct from `agentDefaults` (which stays keyed by agent name); nodes select via a `profile` field mutually exclusive with `agent` at validation (v5 grammar fields landed in typed-contracts; this epic implements resolution), with explicit node-level config overriding profile fields; precedence node → workflow `profiles` → app catalog → driver defaults. Variables carry profile names allow-listed against declared profiles; the catalog is frozen into the run's immutable snapshot at start (as `workflow_json` freezes today, so resume/restart cannot drift), and selectors resolve against the frozen catalog whenever they bind — root selectors at run start, callee selectors at subflow call time — with hard errors on unknowns; the resolved profile is recorded per execution. The `decide` path routes through the same profile resolution via the canonical node-level `profile` field (no second selector on `DecideConfig`; `DecideConfig.model` is invalid alongside a profile; current behavior when unset) with its invocations recorded in the execution log, and session-linked nodes (`continueSessionFrom`) must resolve to the same effective session configuration as their source — same profile and identical session-shaping node overrides — validated at bind time, since a reused pane keeps its spawn configuration and any difference could never take effect. Sharpest exclusion: no export-bundling of referenced profiles into shared workflow documents (additive later).

## harness-port: Harness workflows

Status: pending
Blocked by: condition-ast, script-node, cursor-state, profile-catalog
Invariants: ADR-260815-2009-01, ADR-260815-2009-02, ADR-260815-2009-03, ADR-260815-2009-04, ADR-260815-2009-05

Author `implement-issue` and `issue-dev` as shipped workflow templates — idiomatic ports where engine structure replaces the skills' harness coping mechanisms. Includes the template-install operation — instantiating the shipped templates into `WorkflowStore` under their canonical names, since subflow resolution consults `WorkflowStore` only and today templates are list/open-only with a manual save. Composition is an acceptance property: `issue-dev` must invoke `implement-issue` through a real subflow/call edge with typed inputs and outputs — two non-composed ports do not satisfy the initiative. Outcome-level acceptance: (a) a normalized-record oracle — same status transitions and one commit per issue, volatile fields (timestamps, context packs) excluded; (b) a model-routing criterion read from the resolved-profile execution records (ADR-260815-2009-04's guaranteed log entry): resolved model ids match the expected model named per node class in the acceptance spec, cross-checked against actual-model evidence — the epic wires actual-model capture into `model_used` where the CLI exposes it (the field currently has no producer in `src/`), and that cross-check is mandatory for the high-stakes nodes — profile tiers remain organizational labels, never the oracle; (c) a backend-level demonstration of two concurrent runs of one workflow with different variable values; (d) a structural criterion — every step the skills implement mechanically (frontier queries, git checks, status transitions, ledger writes) is a script node or a deterministic conditioned edge — never a `decide` route (which is LLM-powered) and never inside an LLM prompt — and each LLM node's prompt carries a single responsibility. The ported workflows run with the orchestrator-LLM fallback disabled, so every model invocation is node-scoped and logged. Gap-fixing is bounded to defects and small extensions of the six prior epics' features — new capabilities go back to the roadmap. Sharpest exclusion: no step-for-step mechanism parity (no actor-style inter-node messaging).

## multi-run-ui: Multi-run UI

Status: pending

Frontend tracking of concurrent runs of the same or different workflows — the backend already supports N-parallel execution; the UI tracks a single `runId` today. Deliberately last: pure UI surface, zero engine risk, not needed until overlapping harness runs are real (concurrent-run capability itself is proven backend-level in harness-port). Sharpest exclusion: no scheduling/queueing layer — runs are started by the user or by workflows.
