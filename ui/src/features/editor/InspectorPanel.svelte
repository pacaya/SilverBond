<script lang="ts">
  import { api } from "@/lib/api/client";
  import { store } from "@/lib/stores/workflowStore.svelte";
  import type {
    AccessMode,
    AgentCapabilities,
    AgentDefaults,
    AgentNodeConfig,
    ContextSource,
    InputBinding,
    ReasoningLevel,
    RunAsConfig,
    RuntimeCapabilities,
    SplitFailurePolicy,
    ValidationResponse,
    WorkflowDocument,
    WorkflowEdge,
    WorkflowNode,
  } from "@/lib/types/workflow";
  import PromptTextarea from "@/lib/components/PromptTextarea.svelte";
  import { buildSuggestions } from "@/lib/utils/templateSuggestions";
  import { sectionHasValues, SECTION_IDS } from "@/lib/utils/sectionUtils";
  import NodeHeader from "./NodeHeader.svelte";
  import AddSectionMenu from "./AddSectionMenu.svelte";
  import ConditionBuilder from "./ConditionBuilder.svelte";
  import SchemaPresets from "./SchemaPresets.svelte";

  let {
    workflow,
    validation,
    capabilities,
  }: {
    workflow: WorkflowDocument;
    validation: ValidationResponse | null;
    capabilities: RuntimeCapabilities | undefined;
  } = $props();

  // Operate on the document currently shown on the canvas (root, or a subflow
  // we drilled into). The `workflow` prop stays bound to the root.
  let activeWorkflow = $derived(store.activeWorkflow ?? workflow);

  let previousOutput = $state("");
  let testResult = $state("");
  let testLoading = $state(false);
  let testExpanded = $state(false);
  let showAgentDefaultsFor = $state<string | null>(null);

  /* Run As / Sandbox */
  let attachHintCopied = $state(false);

  function hasRunAsValues(config: RunAsConfig): boolean {
    return Boolean(
      config.user || config.socket || (config.command && config.command.length > 0),
    );
  }

  function updateRunAs(key: "user" | "socket", value: string) {
    store.updateWorkflow((wf) => {
      const next: RunAsConfig = { ...(wf.runAs ?? {}) };
      const trimmed = value.trim();
      if (trimmed) {
        next[key] = trimmed;
      } else {
        delete next[key];
      }
      wf.runAs = hasRunAsValues(next) ? next : undefined;
    });
  }

  function updateRunAsCommand(value: string) {
    store.updateWorkflow((wf) => {
      const next: RunAsConfig = { ...(wf.runAs ?? {}) };
      const tokens = value.split(/\n/).filter((line) => line.length > 0);
      if (tokens.length > 0) {
        next.command = tokens;
      } else {
        delete next.command;
      }
      wf.runAs = hasRunAsValues(next) ? next : undefined;
    });
  }

  let runAsAttachHint = $derived.by(() => {
    const runAs = activeWorkflow.runAs;
    const user = runAs?.user?.trim() || "<user>";
    const socket = runAs?.socket?.trim() || "<socket>";
    const prefix = runAs?.command && runAs.command.length > 0
      ? runAs.command.join(" ")
      : `sudo -u ${user}`;
    return `${prefix} tmux -L ${socket} attach -t <session>`;
  });

  async function copyAttachHint() {
    try {
      await navigator.clipboard.writeText(runAsAttachHint);
      attachHintCopied = true;
      setTimeout(() => {
        attachHintCopied = false;
      }, 1500);
    } catch {
      attachHintCopied = false;
    }
  }

  /* JSON editor local state */
  let jsonDrafts = $state<Record<string, string>>({});
  let jsonErrors = $state<Record<string, string>>({});

  function formatJsonDraft(key: string, value: unknown): string {
    return jsonDrafts[key] ?? (value ? JSON.stringify(value, null, 2) : "");
  }

  function setJsonDraft(key: string, value: string) {
    jsonDrafts[key] = value;
    jsonErrors[key] = "";
  }

  function getJsonError(key: string): string {
    return jsonErrors[key] ?? "";
  }

  function commitJson(key: string, onCommit: (val: unknown | null) => void) {
    const d = jsonDrafts[key] ?? "";
    if (!d.trim()) { onCommit(null); jsonErrors[key] = ""; return; }
    try { onCommit(JSON.parse(d)); jsonErrors[key] = ""; }
    catch (err) { jsonErrors[key] = err instanceof Error ? err.message : "Invalid JSON"; }
  }

  /* ── Session-level open sections (persists across node switches) ── */
  let manualSections = $state(new Set<string>());

  let autoSections = $derived.by(() => {
    if (!selectedNode) return new Set<string>();
    const auto = new Set<string>();
    for (const id of SECTION_IDS) {
      if (sectionHasValues(selectedNode, id)) auto.add(id);
    }
    if (selectedNode.responseFormat === "json") auto.add("output-schema");
    return auto;
  });

  function toggleSection(sectionId: string) {
    const next = new Set(manualSections);
    if (next.has(sectionId)) {
      next.delete(sectionId);
    } else {
      next.add(sectionId);
    }
    manualSections = next;
  }

  function removeSection(sectionId: string) {
    const next = new Set(manualSections);
    next.delete(sectionId);
    manualSections = next;
  }

  let openSections = $derived.by(() => {
    const combined = new Set(autoSections);
    for (const s of manualSections) combined.add(s);
    return combined;
  });

  /* Reset state when selection changes */
  $effect(() => {
    const _sel = store.selection;
    jsonDrafts = {};
    jsonErrors = {};
    previousOutput = "";
    testResult = "";
    testLoading = false;
    testExpanded = false;
    showAgentDefaultsFor = null;
  });

  let selectedNode = $derived.by(() => {
    const selection = store.selection;
    if (selection.kind !== "node") return null;
    return activeWorkflow.nodes.find((node) => node.id === selection.id) ?? null;
  });
  let selectedEdge = $derived.by(() => {
    const selection = store.selection;
    if (selection.kind !== "edge") return null;
    return activeWorkflow.edges.find((edge) => edge.id === selection.id) ?? null;
  });
  let edgeDisplayName = $derived.by(() => {
    if (!selectedEdge) return "";
    const fromNode = activeWorkflow.nodes.find((n) => n.id === selectedEdge.from);
    const toNode = activeWorkflow.nodes.find((n) => n.id === selectedEdge.to);
    const fromName = fromNode?.name || selectedEdge.from.slice(0, 8);
    const toName = toNode?.name || selectedEdge.to.slice(0, 8);
    return `${fromName} → ${toName}`;
  });
  let issues = $derived(validation?.issues ?? []);
  let promptSuggestions = $derived(
    selectedNode ? buildSuggestions(activeWorkflow, selectedNode.id) : [],
  );

  /** Get the capabilities object for the currently selected node's agent */
  let agentCaps = $derived.by((): AgentCapabilities | null => {
    if (!selectedNode || !capabilities) return null;
    const agentName = selectedNode.type === "run_agent"
      ? selectedNode.runAgentConfig?.agent ?? selectedNode.agent ?? "claude"
      : selectedNode.agent ?? "claude";
    return capabilities.agents[agentName]?.capabilities ?? null;
  });

  let selectedAgentValue = $derived.by(() => {
    if (!selectedNode) return "claude";
    if (selectedNode.type === "run_agent") {
      return selectedNode.runAgentConfig?.agent ?? selectedNode.agent ?? "claude";
    }
    return selectedNode.agent ?? "claude";
  });

  let selectedPromptValue = $derived.by(() => {
    if (!selectedNode) return "";
    if (selectedNode.type === "run_agent") {
      return selectedNode.runAgentConfig?.prompt ?? selectedNode.prompt;
    }
    return selectedNode.prompt;
  });

  /** Snapshot of selected node's agentConfig — single reactive read for the template */
  let nodeConfig = $derived<AgentNodeConfig>(selectedNode?.agentConfig ?? {});

  /** Update a single field on the selected node's agentConfig. Pass undefined to clear. */
  function updateNodeConfig<K extends keyof AgentNodeConfig>(key: K, value: AgentNodeConfig[K]) {
    store.updateWorkflow((wf) => {
      const n = wf.nodes.find((n) => n.id === selectedNode!.id);
      if (!n) return;
      if (!n.agentConfig) n.agentConfig = {};
      if (value === undefined) {
        delete (n.agentConfig as Record<string, unknown>)[key];
        if (Object.keys(n.agentConfig).length === 0) n.agentConfig = null;
      } else {
        n.agentConfig[key] = value;
      }
    });
  }

  function numOrUndef(v: string): number | undefined {
    return v !== "" ? Number(v) : undefined;
  }

  /* ── Typed config object updaters (decide / batch / primitives / subflow) ── */
  const CONFIG_DEFAULTS: Record<string, () => Record<string, unknown>> = {
    runAgentConfig: () => ({ killAfter: true }),
    decideConfig: () => ({ prompt: "", inputs: [], outcomes: [] }),
    batchConfig: () => ({ itemsBinding: "", maxConcurrent: 4, itemVar: "item", bodyEntry: "" }),
    spawnConfig: () => ({}),
    sendConfig: () => ({ text: "", enter: true }),
    waitConfig: () => ({ mode: "idle" }),
    captureConfig: () => ({ all: false, ansi: false }),
    killConfig: () => ({}),
    subflowConfig: () => ({ workflowName: "", inputs: [], maxDepth: 10 }),
  };

  /** Merge a field into a node's typed config object; pass undefined to clear it. */
  function updateConfig(configKey: string, field: string, value: unknown) {
    store.updateWorkflow((wf) => {
      const n = wf.nodes.find((x) => x.id === selectedNode!.id);
      if (!n) return;
      const rec = n as unknown as Record<string, unknown>;
      const base = rec[configKey] as Record<string, unknown> | null | undefined;
      const next: Record<string, unknown> = { ...(CONFIG_DEFAULTS[configKey]?.() ?? {}), ...(base ?? {}) };
      if (value === undefined) delete next[field];
      else next[field] = value;
      rec[configKey] = next;
    });
  }

  const updateRunAgent = (f: string, v: unknown) => updateConfig("runAgentConfig", f, v);
  const updateDecide = (f: string, v: unknown) => updateConfig("decideConfig", f, v);
  const updateBatch = (f: string, v: unknown) => updateConfig("batchConfig", f, v);
  const updateSpawn = (f: string, v: unknown) => updateConfig("spawnConfig", f, v);
  const updateSend = (f: string, v: unknown) => updateConfig("sendConfig", f, v);
  const updateWait = (f: string, v: unknown) => updateConfig("waitConfig", f, v);
  const updateCapture = (f: string, v: unknown) => updateConfig("captureConfig", f, v);
  const updateKill = (f: string, v: unknown) => updateConfig("killConfig", f, v);
  const updateSubflow = (f: string, v: unknown) => updateConfig("subflowConfig", f, v);

  /** Available subflow names from the workflow's embedded catalog. */
  let subflowNames = $derived(Object.keys(activeWorkflow.subflows ?? {}));

  /** Get workflow-level agent defaults for a given agent */
  function getAgentDefaults(agentName: string): AgentDefaults {
    return activeWorkflow.agentDefaults?.[agentName] ?? {};
  }

  /** Update a workflow-level agent default. Pass undefined to clear. */
  function updateAgentDefault<K extends keyof AgentDefaults>(agentName: string, key: K, value: AgentDefaults[K]) {
    store.updateWorkflow((wf) => {
      if (!wf.agentDefaults) wf.agentDefaults = {};
      if (!wf.agentDefaults[agentName]) wf.agentDefaults[agentName] = {};
      if (value === undefined) {
        delete (wf.agentDefaults[agentName] as Record<string, unknown>)[key];
        if (Object.keys(wf.agentDefaults[agentName]).length === 0) {
          delete wf.agentDefaults[agentName];
        }
        if (Object.keys(wf.agentDefaults).length === 0) {
          delete wf.agentDefaults;
        }
      } else {
        wf.agentDefaults[agentName][key] = value;
      }
    });
  }

  /** Capability badge labels for agent dropdown */
  const capBadges: Array<{ key: keyof AgentCapabilities; label: string }> = [
    { key: "nativeJsonSchema", label: "schema" },
    { key: "reasoningConfig", label: "reasoning" },
    { key: "systemPrompt", label: "sysprompt" },
    { key: "budgetLimit", label: "budget" },
    { key: "toolAllowlist", label: "tools" },
  ];

  const accessModeDescriptions: Record<AccessMode, string> = {
    read_only: "Observe/analyze only. No file edits or shell commands.",
    edit: "Read and edit files. No shell command execution.",
    execute: "Edit files + run commands. Sandboxed to workspace.",
    unrestricted: "Full system + network access. For installs and deployments.",
  };
</script>

<!-- Shared agent config fields: used for both node-level and workflow-level defaults -->
{#snippet agentConfigFields(
  caps: AgentCapabilities,
  values: AgentDefaults,
  update: <K extends keyof AgentDefaults>(key: K, value: AgentDefaults[K]) => void,
  placeholders: { model: string; systemPrompt: string },
)}
  <label class="field">
    <span>Access mode</span>
    <select
      value={values.accessMode ?? "execute"}
      onchange={(e) => {
        const val = (e.target as HTMLSelectElement).value as AccessMode;
        update("accessMode", val === "execute" ? undefined : val);
      }}
    >
      <option value="read_only">read_only</option>
      <option value="edit">edit</option>
      <option value="execute">execute (default)</option>
      <option value="unrestricted">unrestricted</option>
    </select>
    <small class="helperText" style="margin-top: -4px;">
      {accessModeDescriptions[values.accessMode ?? "execute"]}
    </small>
  </label>

  {#if caps.modelSelection}
    <label class="field">
      <span>Model</span>
      <input
        value={values.model ?? ""}
        placeholder={placeholders.model}
        onblur={(e) => update("model", (e.target as HTMLInputElement).value || undefined)}
      />
    </label>
  {/if}

  {#if caps.reasoningConfig}
    <label class="field">
      <span>Reasoning level</span>
      <select
        value={values.reasoningLevel ?? ""}
        onchange={(e) => {
          const val = (e.target as HTMLSelectElement).value as ReasoningLevel | "";
          update("reasoningLevel", val || undefined);
        }}
      >
        <option value="">agent default</option>
        <option value="low">low</option>
        <option value="medium">medium</option>
        <option value="high">high</option>
      </select>
    </label>
  {/if}

  {#if caps.systemPrompt}
    <label class="field">
      <span>System prompt</span>
      <textarea
        value={values.systemPrompt ?? ""}
        placeholder={placeholders.systemPrompt}
        onblur={(e) => update("systemPrompt", (e.target as HTMLTextAreaElement).value || undefined)}
        class="field--shortTextarea"
      ></textarea>
    </label>
  {/if}

  {#if caps.turnLimit}
    <label class="field field--split">
      <span>Max turns</span>
      <input
        type="number"
        value={values.maxTurns ?? ""}
        placeholder="default"
        oninput={(e) => {
          const val = (e.target as HTMLInputElement).value;
          update("maxTurns", val !== "" ? Number(val) : undefined);
        }}
      />
    </label>
  {/if}

  {#if caps.budgetLimit}
    <label class="field field--split">
      <span>Max budget (USD)</span>
      <input
        type="number"
        step="0.01"
        value={values.maxBudgetUsd ?? ""}
        placeholder="default"
        oninput={(e) => {
          const val = (e.target as HTMLInputElement).value;
          update("maxBudgetUsd", val !== "" ? Number(val) : undefined);
        }}
      />
    </label>
  {/if}

  {#if caps.webSearch}
    <label class="field toggle-field">
      <span>Web search</span>
      <input
        type="checkbox"
        class="toggle"
        checked={values.toolToggles?.webSearch ?? false}
        onchange={(e) => {
          const checked = (e.target as HTMLInputElement).checked;
          update("toolToggles", checked ? { webSearch: true } : undefined);
        }}
      />
    </label>
  {/if}
{/snippet}

<!-- Reusable input-binding editor (decide / subflow / call). `update` writes the whole list. -->
{#snippet inputBindings(list: InputBinding[], update: (field: string, value: unknown) => void)}
  <section class="inspectorSection">
    <div class="inspectorSection__title">Inputs</div>
    <small class="helperText">Bind named inputs from context / variables / templates.</small>
    {#if list.length > 0}
      <div class="contextRow contextRow--header">
        <span class="columnLabel">Name</span>
        <span class="columnLabel">Source</span>
        <span></span>
      </div>
    {/if}
    {#each list as binding, index (index)}
      <div class="contextRow">
        <input
          value={binding.name}
          placeholder="name"
          oninput={(e) => update("inputs", list.map((b, i) => i === index ? { ...b, name: (e.target as HTMLInputElement).value } : b))}
        />
        <input
          value={binding.source}
          placeholder={"{{previous_output}} / alias"}
          oninput={(e) => update("inputs", list.map((b, i) => i === index ? { ...b, source: (e.target as HTMLInputElement).value } : b))}
        />
        <button
          class="button button--ghost"
          onclick={() => update("inputs", list.filter((_, i) => i !== index))}
        >
          Remove
        </button>
      </div>
    {/each}
    <button
      class="button button--ghost"
      onclick={() => update("inputs", [...list, { name: "", source: "" } satisfies InputBinding])}
    >
      + Add input
    </button>
  </section>
{/snippet}

{#if selectedNode}
  {@const nodeIssues = issues.filter((i) => i.nodeId === selectedNode.id)}
  <div class="inspector inspector--withFooter">
    <!-- Compact Header -->
    <NodeHeader node={selectedNode} workflow={activeWorkflow} />

    {#if nodeIssues.length > 0}
      <div class="issueList">
        {#each nodeIssues as issue (issue.message)}
          <div class="issue issue--{issue.severity}">{issue.message}</div>
        {/each}
      </div>
    {/if}

    <!-- Scrollable content area -->
    <div class="inspector__body">
      {#if selectedNode.type === "task" || selectedNode.type === "run_agent"}
        <!-- PROMPT SECTION (always visible, primary) -->
        <section class="inspectorSection">
          <div class="inspectorSection__title">{selectedNode.type === "run_agent" ? "Agent" : "Prompt"}</div>
          <label class="field">
            <span>Agent</span>
            <select
              value={selectedAgentValue}
              onchange={(e) => {
                const value = (e.target as HTMLSelectElement).value;
                if (selectedNode!.type === "run_agent") {
                  updateRunAgent("agent", value);
                  return;
                }
                store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (n) n.agent = value;
                });
              }}
            >
              {#each Object.entries(capabilities?.agents ?? {}) as [agent, info] (agent)}
                <option value={agent} disabled={!info.available}>{agent}{!info.available ? " (not installed)" : ""}</option>
              {/each}
            </select>
            {#if agentCaps}
              <div class="capBadges">
                {#each capBadges.filter((b) => agentCaps![b.key]) as badge (badge.key)}
                  <span class="capBadge">{badge.label}</span>
                {/each}
              </div>
            {/if}
          </label>
          <div class="field field--prompt">
            <span>Prompt</span>
            <PromptTextarea
              value={selectedPromptValue}
              suggestions={promptSuggestions}
              oninput={(e) => {
                const value = (e.target as HTMLTextAreaElement).value;
                if (selectedNode!.type === "run_agent") {
                  updateRunAgent("prompt", value);
                  return;
                }
                store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (n) n.prompt = value;
                });
              }}
            />
          </div>
          {#if selectedNode.type === "task"}
            <label class="field">
              <span>Response format</span>
              <select
                value={selectedNode.responseFormat ?? "text"}
                onchange={(e) => store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (n) n.responseFormat = (e.target as HTMLSelectElement).value as WorkflowNode["responseFormat"];
                })}
              >
                <option value="text">text</option>
                <option value="json">json</option>
              </select>
            </label>
          {/if}
        </section>

        <!-- RUN_AGENT PTY CONFIG (one-shot agent in a managed pane) -->
        {#if selectedNode.type === "run_agent"}
          {@const rc = selectedNode.runAgentConfig ?? { killAfter: true }}
          <section class="inspectorSection">
            <div class="inspectorSection__title">Agent run</div>
            <small class="helperText">Spawns the agent in a PTY pane, waits for completion, then captures output.</small>
            <label class="field">
              <span>Pane name</span>
              <input
                value={rc.name ?? ""}
                placeholder="auto"
                onblur={(e) => updateRunAgent("name", (e.target as HTMLInputElement).value || undefined)}
              />
            </label>
            <label class="field">
              <span>Access</span>
              <input
                value={rc.access ?? ""}
                placeholder="inherit (e.g. read_only / execute)"
                onblur={(e) => updateRunAgent("access", (e.target as HTMLInputElement).value || undefined)}
              />
            </label>
            <label class="field">
              <span>Working directory</span>
              <input
                value={rc.cwd ?? ""}
                placeholder={activeWorkflow.cwd || "inherit workflow cwd"}
                onblur={(e) => updateRunAgent("cwd", (e.target as HTMLInputElement).value || undefined)}
              />
            </label>
            <label class="field field--split">
              <span>Timeout (s)</span>
              <input
                type="number"
                value={rc.timeout ?? ""}
                placeholder="none"
                oninput={(e) => updateRunAgent("timeout", numOrUndef((e.target as HTMLInputElement).value))}
              />
            </label>
            <label class="field field--split">
              <span>Idle seconds</span>
              <input
                type="number"
                min="0"
                step="0.5"
                value={rc.idleSeconds ?? ""}
                placeholder="default"
                oninput={(e) => updateRunAgent("idleSeconds", numOrUndef((e.target as HTMLInputElement).value))}
              />
            </label>
            <label class="field field--split">
              <span>Ready-stable (s)</span>
              <input
                type="number"
                min="0"
                step="0.5"
                value={rc.readyStableSeconds ?? ""}
                placeholder="default"
                oninput={(e) => updateRunAgent("readyStableSeconds", numOrUndef((e.target as HTMLInputElement).value))}
              />
            </label>
            <label class="field">
              <span>Until marker</span>
              <input
                value={rc.until ?? ""}
                placeholder="text marking completion"
                onblur={(e) => updateRunAgent("until", (e.target as HTMLInputElement).value || undefined)}
              />
            </label>
            <label class="field">
              <span>Extra args</span>
              <textarea
                value={(rc.extraArgs ?? []).join("\n")}
                placeholder={"--flag\nvalue"}
                onblur={(e) => {
                  const args = (e.target as HTMLTextAreaElement).value.split("\n").map((s) => s.trim()).filter(Boolean);
                  updateRunAgent("extraArgs", args.length ? args : undefined);
                }}
                class="field--shortTextarea"
              ></textarea>
            </label>
            <label class="field toggle-field">
              <span>Kill pane after</span>
              <input
                type="checkbox"
                class="toggle"
                checked={rc.killAfter ?? true}
                onchange={(e) => updateRunAgent("killAfter", (e.target as HTMLInputElement).checked)}
              />
            </label>
          </section>
        {/if}

        <!-- CONTEXT SOURCES (promoted, always visible) -->
        <section class="inspectorSection">
          <div class="inspectorSection__title">Context sources</div>
          {#each (selectedNode.contextSources ?? []) as context, index (`${context.name}-${index}`)}
            <div class="contextRow">
              <input
                value={context.name}
                placeholder="alias"
                oninput={(e) => store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (!n) return;
                  n.contextSources = n.contextSources ?? [];
                  n.contextSources[index].name = (e.target as HTMLInputElement).value;
                })}
              />
              <select
                value={context.nodeId}
                onchange={(e) => store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (!n) return;
                  n.contextSources = n.contextSources ?? [];
                  n.contextSources[index].nodeId = (e.target as HTMLSelectElement).value;
                })}
              >
                <option value="">Select node</option>
                {#each activeWorkflow.nodes.filter((n) => n.id !== selectedNode!.id) as n (n.id)}
                  <option value={n.id}>{n.name}</option>
                {/each}
              </select>
              <button
                class="button button--ghost"
                onclick={() => store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (!n) return;
                  n.contextSources = (n.contextSources ?? []).filter((_, i) => i !== index);
                })}
              >
                Remove
              </button>
            </div>
          {/each}
          <button
            class="button button--ghost"
            onclick={() => store.updateWorkflow((wf) => {
              const n = wf.nodes.find((n) => n.id === selectedNode!.id);
              if (!n) return;
              n.contextSources = [...(n.contextSources ?? []), { name: "", nodeId: "" } satisfies ContextSource];
            })}
          >
            + Add source
          </button>
        </section>

        <!-- OPT-IN SECTIONS -->

        <!-- Agent Tuning -->
        {#if openSections.has("agent-tuning") && agentCaps}
          <section class="inspectorSection inspectorSection--removable">
            <div class="inspectorSection__titleRow">
              <div class="inspectorSection__title">Agent tuning</div>
              <button class="inspectorSection__removeBtn" title="Hide section" onclick={() => removeSection("agent-tuning")}>&times;</button>
            </div>
            {@render agentConfigFields(
              agentCaps,
              nodeConfig,
              (key, value) => updateNodeConfig(key as keyof AgentNodeConfig, value),
              { model: "workflow default", systemPrompt: "workflow default" },
            )}

            <!-- Per-node working directory -->
            <label class="field">
              <span>Working directory</span>
              <input
                value={selectedNode.cwd ?? ""}
                placeholder={activeWorkflow.cwd || "inherit workflow cwd"}
                onblur={(e) => store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (n) n.cwd = (e.target as HTMLInputElement).value || null;
                })}
              />
            </label>

            <!-- Continue session from (session reuse) -->
            {#if agentCaps?.sessionReuse}
              {@const currentAgent = selectedNode.agent ?? "claude"}
              {@const eligibleNodes = activeWorkflow.nodes.filter(
                (n) => n.type === "task" && n.id !== selectedNode!.id && (n.agent ?? "claude") === currentAgent
              )}
              {#if eligibleNodes.length > 0}
                <label class="field">
                  <span>Continue session from</span>
                  <select
                    value={selectedNode.continueSessionFrom ?? ""}
                    onchange={(e) => store.updateWorkflow((wf) => {
                      const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                      if (n) n.continueSessionFrom = (e.target as HTMLSelectElement).value || null;
                    })}
                  >
                    <option value="">None (fresh session)</option>
                    {#each eligibleNodes as n (n.id)}
                      <option value={n.id}>{n.name}</option>
                    {/each}
                  </select>
                </label>
              {/if}
            {/if}
          </section>
        {/if}

        <!-- Guards & Retry -->
        {#if openSections.has("guards-retry")}
          <section class="inspectorSection inspectorSection--removable">
            <div class="inspectorSection__titleRow">
              <div class="inspectorSection__title">Guards & retry</div>
              <button class="inspectorSection__removeBtn" title="Hide section" onclick={() => removeSection("guards-retry")}>&times;</button>
            </div>
            <label class="field field--split">
              <span>Timeout</span>
              <input
                type="number"
                value={selectedNode.timeout ?? ""}
                oninput={(e) => store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (n) n.timeout = (e.target as HTMLInputElement).value ? Number((e.target as HTMLInputElement).value) : null;
                })}
              />
            </label>
            <label class="field field--split">
              <span>Retry count</span>
              <input
                type="number"
                value={selectedNode.retryCount ?? ""}
                oninput={(e) => store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (n) n.retryCount = (e.target as HTMLInputElement).value ? Number((e.target as HTMLInputElement).value) : null;
                })}
              />
            </label>
            <label class="field field--split">
              <span>Retry delay</span>
              <input
                type="number"
                value={selectedNode.retryDelay ?? ""}
                oninput={(e) => store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (n) n.retryDelay = (e.target as HTMLInputElement).value ? Number((e.target as HTMLInputElement).value) : null;
                })}
              />
            </label>
          </section>
        {/if}

        <!-- Loop Control -->
        {#if openSections.has("loop-control")}
          <section class="inspectorSection inspectorSection--removable">
            <div class="inspectorSection__titleRow">
              <div class="inspectorSection__title">Loop control</div>
              <button class="inspectorSection__removeBtn" title="Hide section" onclick={() => removeSection("loop-control")}>&times;</button>
            </div>
            <label class="field field--split">
              <span>Max iterations</span>
              <input
                type="number"
                value={selectedNode.loopMaxIterations ?? ""}
                oninput={(e) => store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (n) n.loopMaxIterations = (e.target as HTMLInputElement).value ? Number((e.target as HTMLInputElement).value) : null;
                })}
              />
            </label>
            <label class="field">
              <span>Loop condition</span>
              <ConditionBuilder
                mode="structured"
                value={selectedNode.loopCondition ?? null}
                onchange={(val) => store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (n) n.loopCondition = val as WorkflowNode["loopCondition"] ?? null;
                })}
              />
            </label>
          </section>
        {/if}

        <!-- Output Schema -->
        {#if openSections.has("output-schema")}
          <section class="inspectorSection inspectorSection--removable">
            <div class="inspectorSection__titleRow">
              <div class="inspectorSection__title">Output schema</div>
              <button class="inspectorSection__removeBtn" title="Hide section" onclick={() => removeSection("output-schema")}>&times;</button>
            </div>
            <SchemaPresets
              value={selectedNode.outputSchema ?? null}
              onchange={(val) => store.updateWorkflow((wf) => {
                const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                if (n) n.outputSchema = val;
              })}
            />
          </section>
        {/if}

        <!-- Tool Permissions -->
        {#if openSections.has("tool-permissions") && agentCaps?.toolAllowlist}
          <section class="inspectorSection inspectorSection--removable">
            <div class="inspectorSection__titleRow">
              <div class="inspectorSection__title">Tool permissions</div>
              <button class="inspectorSection__removeBtn" title="Hide section" onclick={() => removeSection("tool-permissions")}>&times;</button>
            </div>
            <small class="helperText">Overrides access mode for this agent.</small>
            <label class="field">
              <span>Allowed tools</span>
              <textarea
                value={(nodeConfig.allowedTools ?? []).join("\n")}
                placeholder={'Read\nEdit\nBash(git *)'}
                onblur={(e) => {
                  const tools = (e.target as HTMLTextAreaElement).value.split("\n").map((s) => s.trim()).filter(Boolean);
                  updateNodeConfig("allowedTools", tools.length ? tools : undefined);
                }}
                class="field--shortTextarea"
              ></textarea>
            </label>
            <label class="field">
              <span>Disallowed tools</span>
              <textarea
                value={(nodeConfig.disallowedTools ?? []).join("\n")}
                placeholder={'Bash(rm *)'}
                onblur={(e) => {
                  const tools = (e.target as HTMLTextAreaElement).value.split("\n").map((s) => s.trim()).filter(Boolean);
                  updateNodeConfig("disallowedTools", tools.length ? tools : undefined);
                }}
                class="field--shortTextarea"
              ></textarea>
            </label>
          </section>
        {/if}

        <!-- Skip Condition -->
        {#if openSections.has("skip-condition")}
          <section class="inspectorSection inspectorSection--removable">
            <div class="inspectorSection__titleRow">
              <div class="inspectorSection__title">Skip condition</div>
              <button class="inspectorSection__removeBtn" title="Hide section" onclick={() => removeSection("skip-condition")}>&times;</button>
            </div>
            <ConditionBuilder
              mode="skip"
              value={selectedNode.skipCondition ?? null}
              onchange={(val) => store.updateWorkflow((wf) => {
                const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                if (n) n.skipCondition = val as WorkflowNode["skipCondition"] ?? null;
              })}
            />
          </section>
        {/if}

        <!-- ADD SECTION MENU -->
        <AddSectionMenu
          node={selectedNode}
          {agentCaps}
          {openSections}
          onToggle={toggleSection}
        />

      {:else if selectedNode.type === "approval"}
        <section class="inspectorSection">
          <div class="inspectorSection__title">Approval</div>
          <div class="field field--prompt">
            <span>Prompt</span>
            <PromptTextarea
              value={selectedNode.prompt}
              suggestions={promptSuggestions}
              oninput={(e) => store.updateWorkflow((wf) => {
                const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                if (n) n.prompt = (e.target as HTMLTextAreaElement).value;
              })}
            />
          </div>
        </section>
      {:else if selectedNode.type === "split"}
        <section class="inspectorSection">
          <div class="inspectorSection__title">Split</div>
          <label class="field">
            <span>Failure policy</span>
            <select
              value={selectedNode.splitFailurePolicy ?? "best_effort_continue"}
              onchange={(e) => store.updateWorkflow((wf) => {
                const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                if (n) n.splitFailurePolicy = (e.target as HTMLSelectElement).value as SplitFailurePolicy;
              })}
            >
              <option value="best_effort_continue">best_effort_continue</option>
              <option value="fail_fast_cancel">fail_fast_cancel</option>
              <option value="drain_then_fail">drain_then_fail</option>
            </select>
          </label>
        </section>
      {:else if selectedNode.type === "collector"}
        <section class="inspectorSection">
          <div class="inspectorSection__title">Collector</div>
          <p style="margin: 0; color: var(--text-dim); line-height: 1.5;">
            Collectors wait for all inbound success paths in the current execution epoch, merge their inputs, and continue through a single success edge.
          </p>
        </section>

      {:else if selectedNode.type === "decide"}
        {@const dc = selectedNode.decideConfig ?? { prompt: "", inputs: [], outcomes: [] }}
        <section class="inspectorSection">
          <div class="inspectorSection__title">Decide</div>
          <small class="helperText">An LLM reads the inputs and picks one outcome; each outcome maps to a branch edge.</small>
          <div class="field field--prompt">
            <span>Decision prompt</span>
            <PromptTextarea
              value={dc.prompt}
              suggestions={promptSuggestions}
              oninput={(e) => updateDecide("prompt", (e.target as HTMLTextAreaElement).value)}
            />
          </div>
          <label class="field">
            <span>Model</span>
            <input
              value={dc.model ?? ""}
              placeholder="claude-haiku-4-5"
              onblur={(e) => updateDecide("model", (e.target as HTMLInputElement).value || undefined)}
            />
          </label>
          <label class="field">
            <span>Outcomes (one per line)</span>
            <textarea
              value={(dc.outcomes ?? []).join("\n")}
              placeholder={"approve\nreject\nescalate"}
              onblur={(e) => {
                const list = (e.target as HTMLTextAreaElement).value.split("\n").map((s) => s.trim()).filter(Boolean);
                updateDecide("outcomes", list);
              }}
              class="field--shortTextarea"
            ></textarea>
            <small class="helperText">Use these as branch ids on the outgoing edges.</small>
          </label>
        </section>
        {@render inputBindings(dc.inputs ?? [], updateDecide)}

      {:else if selectedNode.type === "parallel_batch"}
        {@const bc = selectedNode.batchConfig ?? { itemsBinding: "", maxConcurrent: 4, itemVar: "item", bodyEntry: "" }}
        <section class="inspectorSection">
          <div class="inspectorSection__title">Parallel batch</div>
          <small class="helperText">Fans out over a collection, running the body subgraph once per item.</small>
          <label class="field">
            <span>Items binding</span>
            <input
              value={bc.itemsBinding}
              placeholder={"e.g. {{previous_output}} or context alias"}
              onblur={(e) => updateBatch("itemsBinding", (e.target as HTMLInputElement).value)}
            />
          </label>
          <label class="field">
            <span>Item variable</span>
            <input
              value={bc.itemVar}
              placeholder="item"
              onblur={(e) => updateBatch("itemVar", (e.target as HTMLInputElement).value)}
            />
          </label>
          <label class="field">
            <span>Body entry node</span>
            <select
              value={bc.bodyEntry}
              onchange={(e) => updateBatch("bodyEntry", (e.target as HTMLSelectElement).value)}
            >
              <option value="">Select node</option>
              {#each activeWorkflow.nodes.filter((n) => n.id !== selectedNode!.id) as n (n.id)}
                <option value={n.id}>{n.name}</option>
              {/each}
            </select>
          </label>
          <label class="field field--split">
            <span>Max concurrent</span>
            <input
              type="number"
              min="1"
              value={bc.maxConcurrent}
              oninput={(e) => updateBatch("maxConcurrent", numOrUndef((e.target as HTMLInputElement).value) ?? 1)}
            />
          </label>
          <label class="field">
            <span>Collector variable</span>
            <input
              value={bc.collectorVar ?? ""}
              placeholder="optional — name to gather results"
              onblur={(e) => updateBatch("collectorVar", (e.target as HTMLInputElement).value || undefined)}
            />
          </label>
        </section>

      {:else if selectedNode.type === "spawn"}
        {@const sc = selectedNode.spawnConfig ?? {}}
        <section class="inspectorSection">
          <div class="inspectorSection__title">Spawn</div>
          <small class="helperText">Launches a long-lived agent/command into a managed PTY pane.</small>
          <label class="field">
            <span>Agent</span>
            <select
              value={sc.agent ?? ""}
              onchange={(e) => updateSpawn("agent", (e.target as HTMLSelectElement).value || undefined)}
            >
              <option value="">(use command)</option>
              {#each Object.entries(capabilities?.agents ?? {}) as [agent, info] (agent)}
                <option value={agent} disabled={!info.available}>{agent}{!info.available ? " (not installed)" : ""}</option>
              {/each}
            </select>
          </label>
          <label class="field">
            <span>Command</span>
            <input
              value={sc.command ?? ""}
              placeholder="overrides agent, e.g. npm run dev"
              onblur={(e) => updateSpawn("command", (e.target as HTMLInputElement).value || undefined)}
            />
          </label>
          <label class="field">
            <span>Pane name</span>
            <input
              value={sc.name ?? ""}
              placeholder="auto"
              onblur={(e) => updateSpawn("name", (e.target as HTMLInputElement).value || undefined)}
            />
          </label>
          <label class="field">
            <span>Session name</span>
            <input
              value={sc.sessionName ?? ""}
              placeholder="optional tmux session"
              onblur={(e) => updateSpawn("sessionName", (e.target as HTMLInputElement).value || undefined)}
            />
          </label>
          <label class="field">
            <span>Access</span>
            <input
              value={sc.access ?? ""}
              placeholder="inherit"
              onblur={(e) => updateSpawn("access", (e.target as HTMLInputElement).value || undefined)}
            />
          </label>
          <label class="field">
            <span>Working directory</span>
            <input
              value={sc.cwd ?? ""}
              placeholder={activeWorkflow.cwd || "inherit workflow cwd"}
              onblur={(e) => updateSpawn("cwd", (e.target as HTMLInputElement).value || undefined)}
            />
          </label>
          <label class="field">
            <span>Extra args</span>
            <textarea
              value={(sc.extraArgs ?? []).join("\n")}
              placeholder={"--flag\nvalue"}
              onblur={(e) => {
                const args = (e.target as HTMLTextAreaElement).value.split("\n").map((s) => s.trim()).filter(Boolean);
                updateSpawn("extraArgs", args.length ? args : undefined);
              }}
              class="field--shortTextarea"
            ></textarea>
          </label>
        </section>

      {:else if selectedNode.type === "send"}
        {@const sd = selectedNode.sendConfig ?? { text: "", enter: true }}
        <section class="inspectorSection">
          <div class="inspectorSection__title">Send</div>
          <small class="helperText">Sends text / keystrokes to a running pane.</small>
          <label class="field">
            <span>Target pane</span>
            <input
              value={sd.target ?? ""}
              placeholder="active pane"
              onblur={(e) => updateSend("target", (e.target as HTMLInputElement).value || undefined)}
            />
          </label>
          <div class="field field--prompt">
            <span>Text</span>
            <PromptTextarea
              value={sd.text}
              suggestions={promptSuggestions}
              oninput={(e) => updateSend("text", (e.target as HTMLTextAreaElement).value)}
            />
          </div>
          <label class="field toggle-field">
            <span>Press Enter after</span>
            <input
              type="checkbox"
              class="toggle"
              checked={sd.enter ?? true}
              onchange={(e) => updateSend("enter", (e.target as HTMLInputElement).checked)}
            />
          </label>
        </section>

      {:else if selectedNode.type === "wait"}
        {@const wc = selectedNode.waitConfig ?? { mode: "idle" }}
        <section class="inspectorSection">
          <div class="inspectorSection__title">Wait</div>
          <small class="helperText">Blocks until the target pane is idle, ready, or prints a marker.</small>
          <label class="field">
            <span>Target pane</span>
            <input
              value={wc.target ?? ""}
              placeholder="active pane"
              onblur={(e) => updateWait("target", (e.target as HTMLInputElement).value || undefined)}
            />
          </label>
          <label class="field">
            <span>Mode</span>
            <select value={wc.mode} onchange={(e) => updateWait("mode", (e.target as HTMLSelectElement).value)}>
              <option value="idle">idle</option>
              <option value="ready">ready</option>
              <option value="until">until (marker)</option>
            </select>
          </label>
          {#if wc.mode === "until"}
            <label class="field">
              <span>Marker</span>
              <input
                value={wc.marker ?? ""}
                placeholder="text to wait for"
                onblur={(e) => updateWait("marker", (e.target as HTMLInputElement).value || undefined)}
              />
            </label>
          {/if}
          {#if wc.mode === "idle"}
            <label class="field field--split">
              <span>Idle seconds</span>
              <input
                type="number"
                min="0"
                step="0.5"
                value={wc.idleSeconds ?? ""}
                placeholder="default"
                oninput={(e) => updateWait("idleSeconds", numOrUndef((e.target as HTMLInputElement).value))}
              />
            </label>
          {/if}
          {#if wc.mode === "ready"}
            <label class="field field--split">
              <span>Ready-stable (s)</span>
              <input
                type="number"
                min="0"
                step="0.5"
                value={wc.readyStableSeconds ?? ""}
                placeholder="default"
                oninput={(e) => updateWait("readyStableSeconds", numOrUndef((e.target as HTMLInputElement).value))}
              />
            </label>
          {/if}
          <label class="field field--split">
            <span>Timeout (s)</span>
            <input
              type="number"
              value={wc.timeout ?? ""}
              placeholder="none"
              oninput={(e) => updateWait("timeout", numOrUndef((e.target as HTMLInputElement).value))}
            />
          </label>
        </section>

      {:else if selectedNode.type === "capture"}
        {@const cc = selectedNode.captureConfig ?? { all: false, ansi: false }}
        <section class="inspectorSection">
          <div class="inspectorSection__title">Capture</div>
          <small class="helperText">Captures pane output into the run context for downstream nodes.</small>
          <label class="field">
            <span>Target pane</span>
            <input
              value={cc.target ?? ""}
              placeholder="active pane"
              onblur={(e) => updateCapture("target", (e.target as HTMLInputElement).value || undefined)}
            />
          </label>
          <label class="field field--split">
            <span>Lines</span>
            <input
              type="number"
              value={cc.lines ?? ""}
              placeholder="visible"
              disabled={cc.all}
              oninput={(e) => updateCapture("lines", numOrUndef((e.target as HTMLInputElement).value))}
            />
          </label>
          <label class="field toggle-field">
            <span>Capture all scrollback</span>
            <input
              type="checkbox"
              class="toggle"
              checked={cc.all ?? false}
              onchange={(e) => updateCapture("all", (e.target as HTMLInputElement).checked)}
            />
          </label>
          <label class="field toggle-field">
            <span>Include ANSI</span>
            <input
              type="checkbox"
              class="toggle"
              checked={cc.ansi ?? false}
              onchange={(e) => updateCapture("ansi", (e.target as HTMLInputElement).checked)}
            />
          </label>
        </section>

      {:else if selectedNode.type === "kill"}
        {@const kc = selectedNode.killConfig ?? {}}
        <section class="inspectorSection">
          <div class="inspectorSection__title">Kill</div>
          <small class="helperText">Terminates a running pane or tmux session.</small>
          <label class="field">
            <span>Target pane</span>
            <input
              value={kc.target ?? ""}
              placeholder="active pane"
              onblur={(e) => updateKill("target", (e.target as HTMLInputElement).value || undefined)}
            />
          </label>
          <label class="field">
            <span>Session name</span>
            <input
              value={kc.sessionName ?? ""}
              placeholder="optional tmux session"
              onblur={(e) => updateKill("sessionName", (e.target as HTMLInputElement).value || undefined)}
            />
          </label>
        </section>

      {:else if selectedNode.type === "subflow" || selectedNode.type === "call"}
        {@const fc = selectedNode.subflowConfig ?? { workflowName: "", inputs: [], maxDepth: 10 }}
        {@const referenced = activeWorkflow.subflows?.[fc.workflowName]}
        <section class="inspectorSection">
          <div class="inspectorSection__title">Compound ({selectedNode.type})</div>
          <small class="helperText">References a reusable saved subgraph. Double-click the node on the canvas to drill in.</small>
          <label class="field">
            <span>Subflow</span>
            <select
              value={fc.workflowName}
              onchange={(e) => updateSubflow("workflowName", (e.target as HTMLSelectElement).value)}
            >
              <option value="">Select subflow</option>
              {#each subflowNames as name (name)}
                <option value={name}>{name}</option>
              {/each}
              {#if fc.workflowName && !subflowNames.includes(fc.workflowName)}
                <option value={fc.workflowName}>{fc.workflowName} (unresolved)</option>
              {/if}
            </select>
          </label>
          {#if referenced}
            <div class="contractBox">
              <div class="contractBox__row">
                <span class="contractBox__label">entry</span>
                <span class="contractBox__value">{referenced.nodes.find((n) => n.id === referenced.entryNodeId)?.name ?? "—"}</span>
              </div>
              <div class="contractBox__row">
                <span class="contractBox__label">exit</span>
                <span class="contractBox__value">{referenced.nodes.find((n) => n.id === fc.exitNodeId)?.name ?? "(single terminal)"}</span>
              </div>
              <div class="contractBox__row">
                <span class="contractBox__label">nodes</span>
                <span class="contractBox__value">{referenced.nodes.length}</span>
              </div>
            </div>
            <label class="field">
              <span>Exit node</span>
              <select
                value={fc.exitNodeId ?? ""}
                onchange={(e) => updateSubflow("exitNodeId", (e.target as HTMLSelectElement).value || undefined)}
              >
                <option value="">Infer single terminal</option>
                {#each referenced.nodes as n (n.id)}
                  <option value={n.id}>{n.name}</option>
                {/each}
              </select>
            </label>
            <button class="button button--ghost" onclick={() => store.drillIntoSubflow(selectedNode!.id)}>
              Open subgraph →
            </button>
          {:else if fc.workflowName}
            <div class="issue issue--warning">Subflow "{fc.workflowName}" is not defined in this activeWorkflow.</div>
          {/if}
          <label class="field field--split">
            <span>Max depth</span>
            <input
              type="number"
              min="1"
              value={fc.maxDepth ?? 10}
              oninput={(e) => updateSubflow("maxDepth", numOrUndef((e.target as HTMLInputElement).value) ?? 10)}
            />
          </label>
        </section>
        {@render inputBindings(fc.inputs ?? [], updateSubflow)}
      {/if}
    </div>

    <!-- STICKY TEST FOOTER (task nodes only) -->
    {#if selectedNode.type === "task"}
      <div class="testFooter" class:testFooter--expanded={testExpanded}>
        <button
          class="testFooter__bar"
          onclick={() => testExpanded = !testExpanded}
        >
          <span class="testFooter__icon">{testExpanded ? "▼" : "▶"}</span>
          <span>Test</span>
          {#if testLoading}
            <span class="testFooter__spinner"></span>
          {/if}
        </button>
        {#if testExpanded}
          <div class="testFooter__body">
            <label class="field">
              <span>Previous output</span>
              <textarea bind:value={previousOutput} class="field--shortTextarea"></textarea>
            </label>
            <button
              class="button button--primary"
              disabled={testLoading}
              onclick={async () => {
                testLoading = true;
                try {
                  const preview = await api.testNode(selectedNode!, activeWorkflow.cwd, { previousOutput });
                  testResult = JSON.stringify(preview, null, 2);
                } finally {
                  testLoading = false;
                }
              }}
            >
              {testLoading ? "Running..." : "Run preview"}
            </button>
            {#if testResult}
              <pre class="previewBlock">{testResult}</pre>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>

{:else if selectedEdge}
  <div class="inspector">
    <div class="inspector__header">
      <div>
        <small>Edge</small>
        <h3>{edgeDisplayName}</h3>
      </div>
      <button class="button button--danger" onclick={() => store.removeEdge(selectedEdge!.id)}>
        Delete
      </button>
    </div>

    <section class="inspectorSection">
      <div class="inspectorSection__title">Routing</div>
      <label class="field">
        <span>Outcome</span>
        <select
          value={selectedEdge.outcome}
          onchange={(e) => store.updateWorkflow((wf) => {
            const edge = wf.edges.find((ed) => ed.id === selectedEdge!.id);
            if (edge) edge.outcome = (e.target as HTMLSelectElement).value as WorkflowEdge["outcome"];
          })}
        >
          {#each (capabilities?.supportedEdgeOutcomes ?? ["success", "reject", "branch", "loop_continue", "loop_exit"]) as outcome (outcome)}
            <option value={outcome}>{outcome}</option>
          {/each}
        </select>
      </label>
      <label class="field">
        <span>Label</span>
        <input
          value={selectedEdge.label ?? ""}
          oninput={(e) => store.updateWorkflow((wf) => {
            const edge = wf.edges.find((ed) => ed.id === selectedEdge!.id);
            if (edge) edge.label = (e.target as HTMLInputElement).value || null;
          })}
        />
      </label>
      <label class="field">
        <span>Branch id</span>
        <input
          value={selectedEdge.branchId ?? ""}
          oninput={(e) => store.updateWorkflow((wf) => {
            const edge = wf.edges.find((ed) => ed.id === selectedEdge!.id);
            if (edge) edge.branchId = (e.target as HTMLInputElement).value || null;
          })}
        />
      </label>
      <!-- Condition -->
      <label class="field">
        <span>Condition</span>
        <ConditionBuilder
          mode="structured"
          value={selectedEdge.condition ?? null}
          onchange={(val) => store.updateWorkflow((wf) => {
            const edge = wf.edges.find((ed) => ed.id === selectedEdge!.id);
            if (edge) edge.condition = val as WorkflowEdge["condition"] ?? null;
          })}
        />
      </label>
    </section>
  </div>

{:else}
  <!-- Workflow-level inspector -->
  <div class="inspector">
    <div class="inspector__header">
      <div>
        <small>Workflow</small>
        <h3>{activeWorkflow.name || "Untitled workflow"}</h3>
      </div>
    </div>

    <section class="inspectorSection">
      <div class="inspectorSection__title">Workflow</div>
      <label class="field">
        <span>Name</span>
        <input
          value={activeWorkflow.name ?? ""}
          oninput={(e) => store.updateWorkflow((wf) => {
            wf.name = (e.target as HTMLInputElement).value;
          })}
        />
      </label>
      <label class="field">
        <span>Goal</span>
        <textarea
          value={activeWorkflow.goal}
          oninput={(e) => store.updateWorkflow((wf) => {
            wf.goal = (e.target as HTMLTextAreaElement).value;
          })}
        ></textarea>
      </label>
      <label class="field">
        <span>Working directory</span>
        <input
          value={activeWorkflow.cwd}
          oninput={(e) => store.updateWorkflow((wf) => {
            wf.cwd = (e.target as HTMLInputElement).value;
          })}
        />
      </label>
      <label class="field">
        <span>Entry node</span>
        <select
          value={activeWorkflow.entryNodeId}
          onchange={(e) => store.updateWorkflow((wf) => {
            wf.entryNodeId = (e.target as HTMLSelectElement).value;
          })}
        >
          <option value="">Select node</option>
          {#each activeWorkflow.nodes as node (node.id)}
            <option value={node.id}>{node.name}</option>
          {/each}
        </select>
      </label>
      <label class="field toggle-field">
        <span>Orchestrator</span>
        <input
          type="checkbox"
          class="toggle"
          checked={activeWorkflow.useOrchestrator}
          onchange={(e) => store.updateWorkflow((wf) => {
            wf.useOrchestrator = (e.target as HTMLInputElement).checked;
          })}
        />
      </label>
    </section>

    <section class="inspectorSection">
      <div class="inspectorSection__title">Limits</div>
      <label class="field field--split">
        <span>Max total steps</span>
        <input
          type="number"
          value={activeWorkflow.limits.maxTotalSteps}
          oninput={(e) => store.updateWorkflow((wf) => {
            wf.limits.maxTotalSteps = Number((e.target as HTMLInputElement).value);
          })}
        />
      </label>
      <label class="field field--split">
        <span>Max visits per node</span>
        <input
          type="number"
          value={activeWorkflow.limits.maxVisitsPerNode}
          oninput={(e) => store.updateWorkflow((wf) => {
            wf.limits.maxVisitsPerNode = Number((e.target as HTMLInputElement).value);
          })}
        />
      </label>
    </section>

    <section class="inspectorSection">
      <div class="inspectorSection__title">Run As / Sandbox</div>
      <small class="helperText">
        Launch spawned panes under a different user, via a custom command prefix,
        and on a dedicated tmux socket.
      </small>
      <label class="field">
        <span>User</span>
        <input
          value={activeWorkflow.runAs?.user ?? ""}
          placeholder="e.g. sandbox"
          oninput={(e) => updateRunAs("user", (e.target as HTMLInputElement).value)}
        />
      </label>
      <label class="field">
        <span>Command prefix</span>
        <textarea
          value={(activeWorkflow.runAs?.command ?? []).join("\n")}
          placeholder={"sudo\n-u\nsandbox"}
          oninput={(e) => updateRunAsCommand((e.target as HTMLTextAreaElement).value)}
          class="field--shortTextarea"
        ></textarea>
        <small class="helperText">One argv element per line. Takes precedence over "User" when set.</small>
      </label>
      <label class="field">
        <span>Socket</span>
        <input
          value={activeWorkflow.runAs?.socket ?? ""}
          placeholder="e.g. silverbond"
          oninput={(e) => updateRunAs("socket", (e.target as HTMLInputElement).value)}
        />
      </label>
      <div class="field">
        <span>Attach hint</span>
        <div class="attachHint">
          <code class="attachHint__cmd">{runAsAttachHint}</code>
          <button
            class="button button--ghost"
            type="button"
            onclick={copyAttachHint}
          >
            {attachHintCopied ? "Copied" : "Copy"}
          </button>
        </div>
      </div>
    </section>

    <!-- Workflow Agent Defaults -->
    {#if capabilities}
      <section class="inspectorSection">
        <div class="inspectorSection__title">Agent defaults</div>
        {#each Object.entries(capabilities.agents).filter(([_, info]) => info.available) as [agentName, agentInfo] (agentName)}
          {@const caps = agentInfo.capabilities}
          {@const defaults = getAgentDefaults(agentName)}
          <div>
            <button
              class="inspectorSection__title inspectorSection__title--collapsible"
              onclick={() => showAgentDefaultsFor = showAgentDefaultsFor === agentName ? null : agentName}
            >
              <span class="chevron" class:chevron--open={showAgentDefaultsFor === agentName}>&#9654;</span>
              {agentName}
            </button>
            {#if showAgentDefaultsFor === agentName}
              <div class="agentDefaultsBody">
                {@render agentConfigFields(
                  caps,
                  defaults,
                  (key, value) => updateAgentDefault(agentName, key, value),
                  { model: "agent default", systemPrompt: "none" },
                )}
              </div>
            {/if}
          </div>
        {/each}
      </section>
    {/if}

    <section class="inspectorSection">
      <div class="inspectorSection__title">Variables</div>
      {#if activeWorkflow.variables.length > 0}
        <div class="contextRow contextRow--header">
          <span class="columnLabel">Name</span>
          <span class="columnLabel">Default value</span>
          <span></span>
        </div>
      {/if}
      {#each activeWorkflow.variables as variable, index (`${variable.name}-${index}`)}
        <div class="contextRow">
          <input
            value={variable.name}
            placeholder="name"
            oninput={(e) => store.updateWorkflow((wf) => {
              wf.variables[index].name = (e.target as HTMLInputElement).value;
            })}
          />
          <input
            value={variable.default}
            placeholder="default"
            oninput={(e) => store.updateWorkflow((wf) => {
              wf.variables[index].default = (e.target as HTMLInputElement).value;
            })}
          />
          <button
            class="button button--ghost"
            onclick={() => store.updateWorkflow((wf) => {
              wf.variables = wf.variables.filter((_, i) => i !== index);
            })}
          >
            Remove
          </button>
        </div>
      {/each}
      <button
        class="button button--ghost"
        onclick={() => store.updateWorkflow((wf) => {
          wf.variables.push({ name: "", default: "" });
        })}
      >
        Add variable
      </button>
    </section>
  </div>
{/if}

<style>
  .contractBox {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 6px 0 4px;
    padding: 8px 10px;
    border: 1px solid rgba(56, 189, 248, 0.3);
    border-radius: 10px;
    background: rgba(56, 189, 248, 0.06);
  }

  .contractBox__row {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 12px;
  }

  .contractBox__label {
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-size: 10px;
  }

  .contractBox__value {
    color: var(--text-bright);
    text-align: right;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .attachHint {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .attachHint__cmd {
    flex: 1;
    min-width: 0;
    padding: 10px 12px;
    border-radius: 14px;
    border: 1px solid var(--border);
    background: rgba(15, 23, 42, 0.66);
    color: var(--text-bright);
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
    overflow-x: auto;
    white-space: nowrap;
  }
</style>
