---
id: ADR-260815-2009-02
status: accepted
terms: [Condition, onMissing, Failure Outcome]
---

# One condition dialect: an owned nested AST with typed operators and an onMissing policy

Deterministic branching uses a single, engine-owned condition form across post-execution conditions (edge conditions and `loopCondition`): a nested AST of `all` / `any` / `not` combinators over typed-operator leaves `{field, op, value, onMissing}`, generalizing the existing `StructuredCondition` triple (`src/model.rs:130`), which becomes the degenerate single-leaf case. `skipCondition` is out of this initiative's scope: it keeps its current pre-execution shape and semantics (raw-string source + contains/not_contains/regex, `src/model.rs:136`) untouched — its source-then-string-match model has no `field` for a typed leaf to address, so unification would change behavior or be vacuous. Operators are explicitly typed families (`eq_string`, `eq_number`, `is_true`, …) — never inferring — and each leaf carries `onMissing: treat-as-false | error | route`, distinguishing "field is false" from "the model never emitted the field"; a node-level `parse_error` evaluates as all-fields-missing. Routed outcomes travel over a new `failure` edge outcome added to the graph model (`WorkflowEdgeOutcome`) — taken when a failure edge is declared, terminal as today otherwise; leaf-conflict precedence within one evaluation: any triggered `error` leaf → error, else any triggered `route` leaf → the failure edge, else the AST evaluates with missing-as-false. Migrated v4 conditions become single-leaf ASTs with a `legacy` operator family (`eq_legacy`, `ne_legacy`, the relational `gt_legacy`/`lt_legacy`/`gte_legacy`/`lte_legacy` with v4's parse-both-sides-as-f64 coercion, plus the string `contains`/`matches`) preserving v4's exact stringify-then-compare semantics (`src/model.rs:3182`) — coercion is admitted only in grandfathered leaves, visibly marked, with a one-click upgrade to typed operators in the builder. The leaf evaluator (legacy + typed families, behavior-identical routing) ships with the v5 grammar in the typed-contracts epic so normalized documents execute from day one; this decision's epic ships the combinators, `onMissing`, and failure routing. Values are always bound into an evaluation context, never substituted into expression source text. No second dialect, no string expression language, no new runtime dependency; computed predicates are factored through a script node feeding a simple condition.

## Considered Options

- **CEL (`cel` crate)** — rejected: a missing map key is a hard evaluation error, semantically wrong for LLM output that simply omitted a field; no Rust type-checker; adding it alongside the structured form reproduces Argo's multi-dialect mess, including the failure mode where the wrong dialect silently works (argo-workflows #7576).
- **JSONLogic via `jsonlogic-rs`** — rejected: standard JSONLogic operators deliberately mirror JavaScript coercion (`"1" == 1` is true), reintroducing the coercion class typed operators exist to kill; deviating from the spec immediately makes the crate (low bus factor, frozen spec) buy nothing over an owned AST.
- **Textual expression language with template substitution** — rejected outright: substituting values into expression source is the documented injection class (Argo's own docs concede quotes in a parameter invalidate the `when:` expression).

## Evidence

- Missing-field semantics, dialect proliferation, and injection-by-substitution failure modes across GH Actions, Argo, CEL, and JSONLogic.
  Provenance: [docs/sources/prior-art-workflow-engines.md](../sources/prior-art-workflow-engines.md) §1 (cited issues therein); 2026-08-15.
