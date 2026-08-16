---
id: ADR-260815-2009-03
status: accepted
terms: [Script Node]
---

# Script input indirection: no templating into source, env-var passing, exit-code-only success

Script node source is static text: the engine refuses `{{…}}` templates inside it. Dynamic values reach a script exclusively through environment variables (`SB_VAR_<NAME>`, plus an explicit env-map of bindings) and argv — except bindings whose serialized size exceeds the env/argv ceiling, which the engine writes to a per-execution file and exposes as a path in `SB_INPUT_<NAME>` (the documented `ARG_MAX` fallback); this is the env-indirection posture GH Actions and Argo converged on after the templating-into-source RCE class (string-built shell is string-built SQL). The exit code is the sole success signal — nonzero routes through the `failure` edge outcome when a failure edge is declared (ADR-260815-2009-02), terminal otherwise, never a run abort by itself — and stdout is a data channel with an explicit `parse: json | text` stage feeding `parsedOutput`, size-capped with a loud error, plus an opt-in `SB_OUTPUT` file for multi-named outputs. `parsedOutput` from any LLM node is treated as untrusted input (same threat model as `github.event.*`). Script nodes join the process-launch gate set (`node_launches_process`) behind the existing privileged gate (`authorize_and_prepare_run_security`, `src/api.rs:778`), and — having no pane — must still execute under the same `runAs` identity enforcement the tmux boundary applies to pane workloads, so a script never silently runs as the app user when the run is downgraded. A tmux pane is an execution boundary, not a security boundary.

## Considered Options

- **Allow templates in source with a lint warning** — rejected: every surveyed engine that kept the soft path has a documented security advisory for it; the hard ban costs only `grep "$SB_VAR_X"` ergonomics over `grep {{var:X}}`.
- **Stdout sentinel blocks for structured output** — rejected for pane-mediated nodes: GitHub deprecated `::set-output` precisely because scripts (and echoing LLMs) can forge the channel; for the synchronous script node stdout is directly captured from the child process, so plain stdout+parse is safe there.

## Evidence

- The env-indirection fix, `::set-output` deprecation rationale, and n8n's language-sandbox → process-isolation migration.
  Provenance: [docs/sources/prior-art-workflow-engines.md](../sources/prior-art-workflow-engines.md) §2 (GitHub Security Lab, GH changelog, n8n task-runners); 2026-08-15.
