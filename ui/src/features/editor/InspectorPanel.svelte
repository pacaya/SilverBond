<script lang="ts">
  import { api } from "@/lib/api/client";
  import { store } from "@/lib/stores/workflowStore.svelte";
  import type {
    AccessMode,
    AgentCapabilities,
    AgentDefaults,
    AgentNodeConfig,
    ContextSource,
    ReasoningLevel,
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

  let previousOutput = $state("");
  let testResult = $state("");
  let testLoading = $state(false);
  let testExpanded = $state(false);
  let showAgentDefaultsFor = $state<string | null>(null);

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

  function autoShowSections(node: WorkflowNode): Set<string> {
    const auto = new Set<string>();
    for (const id of SECTION_IDS) {
      if (sectionHasValues(node, id)) auto.add(id);
    }
    return auto;
  }

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
    if (!selectedNode) return new Set<string>();
    const combined = new Set(autoShowSections(selectedNode));
    for (const s of manualSections) combined.add(s);
    // Auto-show output schema when response format is json
    if (selectedNode.responseFormat === "json") combined.add("output-schema");
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
    return workflow.nodes.find((node) => node.id === selection.id) ?? null;
  });
  let selectedEdge = $derived.by(() => {
    const selection = store.selection;
    if (selection.kind !== "edge") return null;
    return workflow.edges.find((edge) => edge.id === selection.id) ?? null;
  });
  let edgeDisplayName = $derived.by(() => {
    if (!selectedEdge) return "";
    const fromNode = workflow.nodes.find((n) => n.id === selectedEdge.from);
    const toNode = workflow.nodes.find((n) => n.id === selectedEdge.to);
    const fromName = fromNode?.name || selectedEdge.from.slice(0, 8);
    const toName = toNode?.name || selectedEdge.to.slice(0, 8);
    return `${fromName} → ${toName}`;
  });
  let issues = $derived(validation?.issues ?? []);
  let promptSuggestions = $derived(
    selectedNode ? buildSuggestions(workflow, selectedNode.id) : [],
  );

  /** Get the capabilities object for the currently selected node's agent */
  let agentCaps = $derived.by((): AgentCapabilities | null => {
    if (!selectedNode || !capabilities) return null;
    const agentName = selectedNode.agent ?? "claude";
    return capabilities.agents[agentName]?.capabilities ?? null;
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

  /** Get workflow-level agent defaults for a given agent */
  function getAgentDefaults(agentName: string): AgentDefaults {
    return workflow.agentDefaults?.[agentName] ?? {};
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

{#if selectedNode}
  {@const nodeIssues = issues.filter((i) => i.nodeId === selectedNode.id)}
  <div class="inspector inspector--withFooter">
    <!-- Compact Header -->
    <NodeHeader node={selectedNode} {workflow} />

    {#if nodeIssues.length > 0}
      <div class="issueList">
        {#each nodeIssues as issue (issue.message)}
          <div class="issue issue--{issue.severity}">{issue.message}</div>
        {/each}
      </div>
    {/if}

    <!-- Scrollable content area -->
    <div class="inspector__body">
      {#if selectedNode.type === "task"}
        <!-- PROMPT SECTION (always visible, primary) -->
        <section class="inspectorSection">
          <div class="inspectorSection__title">Prompt</div>
          <label class="field">
            <span>Agent</span>
            <select
              value={selectedNode.agent ?? "claude"}
              onchange={(e) => store.updateWorkflow((wf) => {
                const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                if (n) n.agent = (e.target as HTMLSelectElement).value;
              })}
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
              value={selectedNode.prompt}
              suggestions={promptSuggestions}
              oninput={(e) => store.updateWorkflow((wf) => {
                const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                if (n) n.prompt = (e.target as HTMLTextAreaElement).value;
              })}
            />
          </div>
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
        </section>

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
                {#each workflow.nodes.filter((n) => n.id !== selectedNode!.id) as n (n.id)}
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
                placeholder={workflow.cwd || "inherit workflow cwd"}
                onblur={(e) => store.updateWorkflow((wf) => {
                  const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                  if (n) n.cwd = (e.target as HTMLInputElement).value || null;
                })}
              />
            </label>

            <!-- Continue session from (session reuse) -->
            {#if agentCaps?.sessionReuse}
              {@const currentAgent = selectedNode.agent ?? "claude"}
              {@const eligibleNodes = workflow.nodes.filter(
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
                  const preview = await api.testNode(selectedNode!, workflow.cwd, { previousOutput });
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
        <h3>{workflow.name || "Untitled workflow"}</h3>
      </div>
    </div>

    <section class="inspectorSection">
      <div class="inspectorSection__title">Workflow</div>
      <label class="field">
        <span>Name</span>
        <input
          value={workflow.name ?? ""}
          oninput={(e) => store.updateWorkflow((wf) => {
            wf.name = (e.target as HTMLInputElement).value;
          })}
        />
      </label>
      <label class="field">
        <span>Goal</span>
        <textarea
          value={workflow.goal}
          oninput={(e) => store.updateWorkflow((wf) => {
            wf.goal = (e.target as HTMLTextAreaElement).value;
          })}
        ></textarea>
      </label>
      <label class="field">
        <span>Working directory</span>
        <input
          value={workflow.cwd}
          oninput={(e) => store.updateWorkflow((wf) => {
            wf.cwd = (e.target as HTMLInputElement).value;
          })}
        />
      </label>
      <label class="field">
        <span>Entry node</span>
        <select
          value={workflow.entryNodeId}
          onchange={(e) => store.updateWorkflow((wf) => {
            wf.entryNodeId = (e.target as HTMLSelectElement).value;
          })}
        >
          <option value="">Select node</option>
          {#each workflow.nodes as node (node.id)}
            <option value={node.id}>{node.name}</option>
          {/each}
        </select>
      </label>
      <label class="field toggle-field">
        <span>Orchestrator</span>
        <input
          type="checkbox"
          class="toggle"
          checked={workflow.useOrchestrator}
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
          value={workflow.limits.maxTotalSteps}
          oninput={(e) => store.updateWorkflow((wf) => {
            wf.limits.maxTotalSteps = Number((e.target as HTMLInputElement).value);
          })}
        />
      </label>
      <label class="field field--split">
        <span>Max visits per node</span>
        <input
          type="number"
          value={workflow.limits.maxVisitsPerNode}
          oninput={(e) => store.updateWorkflow((wf) => {
            wf.limits.maxVisitsPerNode = Number((e.target as HTMLInputElement).value);
          })}
        />
      </label>
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
      {#if workflow.variables.length > 0}
        <div class="contextRow contextRow--header">
          <span class="columnLabel">Name</span>
          <span class="columnLabel">Default value</span>
          <span></span>
        </div>
      {/if}
      {#each workflow.variables as variable, index (`${variable.name}-${index}`)}
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
