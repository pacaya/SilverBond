<script lang="ts">
  import { api, ApiError } from "@/lib/api/client";
  import { store } from "@/lib/stores/workflowStore.svelte";
  import type {
    AgentCapabilities,
    AgentNodeConfig,
    ContextSource,
    RunAgentConfig,
    RuntimeCapabilities,
    SplitFailurePolicy,
    ValidationResponse,
    WorkflowDocument,
    WorkflowNode,
  } from "@/lib/types/workflow";
  import PromptTextarea from "@/lib/components/PromptTextarea.svelte";
  import PasswordDialog from "@/lib/components/PasswordDialog.svelte";
  import { createPasswordPrompt } from "@/lib/components/passwordPrompt.svelte";
  import { buildSuggestions } from "@/lib/utils/templateSuggestions";
  import { sectionHasValues, SECTION_IDS } from "@/lib/utils/sectionUtils";
  import { activeValidationScope, issueMatchesScope } from "@/features/editor/flowNodes";
  import NodeHeader from "./NodeHeader.svelte";
  import AddSectionMenu from "./AddSectionMenu.svelte";
  import ConditionBuilder from "./ConditionBuilder.svelte";
  import EdgeInspector from "./EdgeInspector.svelte";
  import SchemaPresets from "./SchemaPresets.svelte";
  import PaneNameField from "./PaneNameField.svelte";
  import AccessProfileField from "./AccessProfileField.svelte";
  import WorkingDirectoryField from "./WorkingDirectoryField.svelte";
  import ExtraArgsField from "./ExtraArgsField.svelte";
  import AgentConfigFields from "./AgentConfigFields.svelte";
  import WorkflowInspector from "./WorkflowInspector.svelte";
  import DecidePanel from "./panels/DecidePanel.svelte";
  import ParallelBatchPanel from "./panels/ParallelBatchPanel.svelte";
  import SpawnPanel from "./panels/SpawnPanel.svelte";
  import SendPanel from "./panels/SendPanel.svelte";
  import WaitPanel from "./panels/WaitPanel.svelte";
  import CapturePanel from "./panels/CapturePanel.svelte";
  import KillPanel from "./panels/KillPanel.svelte";
  import SubflowPanel from "./panels/SubflowPanel.svelte";
  import { mergeConfig, numOrUndef, spawnConfig } from "./mergeConfig";
  import { supportedAccessModes } from "./supportedAccessModes";

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

  const unlockPrompt = createPasswordPrompt();

  async function runSelectedNodePreview() {
    if (!selectedNode) return null;
    const mockContext = { previousOutput };
    try {
      return await api.testNode(selectedNode, activeWorkflow.cwd, mockContext);
    } catch (error) {
      if (!(error instanceof ApiError) || error.code !== "privileged_unlock_required") {
        throw error;
      }
      const unlockSecret = await unlockPrompt.request({
        title: "Unlock password",
        message: "This node launches a process. Enter the unlock password to preview it.",
      });
      if (unlockSecret === null) {
        throw new Error("Preview cancelled");
      }
      return api.testNode(selectedNode, activeWorkflow.cwd, mockContext, unlockSecret);
    }
  }

  let issues = $derived(validation?.issues ?? []);
  let activeScope = $derived(activeValidationScope(store.drillStack));
  let promptSuggestions = $derived(
    selectedNode ? buildSuggestions(activeWorkflow, selectedNode.id) : [],
  );

  const DEFAULT_RUN_AGENT_CONFIG: RunAgentConfig = { killAfter: true };

  function nodeAgentConfig(node: WorkflowNode | null): AgentNodeConfig {
    if (!node) return {};
    if (node.kind.type !== "task" && node.kind.type !== "run_agent") return {};
    return node.kind.agentConfig ?? {};
  }

  function runAgentConfig(node: WorkflowNode): RunAgentConfig {
    return node.kind.type === "run_agent"
      ? node.kind.runAgentConfig ?? DEFAULT_RUN_AGENT_CONFIG
      : DEFAULT_RUN_AGENT_CONFIG;
  }

  /** Get the capabilities object for the currently selected node's agent */
  let agentCaps = $derived.by((): AgentCapabilities | null => {
    if (!selectedNode || !capabilities) return null;
    const agentName = selectedNode.kind.type === "run_agent"
      ? runAgentConfig(selectedNode).agent ?? selectedNode.agent ?? "claude"
      : selectedNode.agent ?? "claude";
    return capabilities.agents[agentName]?.capabilities ?? null;
  });

  let selectedAgentValue = $derived.by(() => {
    if (!selectedNode) return "claude";
    if (selectedNode.kind.type === "run_agent") {
      return runAgentConfig(selectedNode).agent ?? selectedNode.agent ?? "claude";
    }
    if (selectedNode.kind.type === "spawn") {
      return spawnConfig(selectedNode).agent ?? selectedNode.agent ?? "claude";
    }
    return selectedNode.agent ?? "claude";
  });

  let selectedAgentAccessProfiles = $derived.by(() => {
    if (!capabilities) return undefined;
    return capabilities.agents[selectedAgentValue]?.accessProfiles;
  });

  let selectedAgentHasNoAccessModes = $derived.by(() =>
    selectedAgentAccessProfiles !== undefined
      && supportedAccessModes(selectedAgentAccessProfiles).length === 0
  );

  let selectedPromptValue = $derived.by(() => {
    if (!selectedNode) return "";
    if (selectedNode.kind.type === "run_agent") {
      return runAgentConfig(selectedNode).prompt ?? selectedNode.prompt;
    }
    return selectedNode.prompt;
  });

  /** Snapshot of selected node's agentConfig — single reactive read for the template */
  let nodeConfig = $derived<AgentNodeConfig>(nodeAgentConfig(selectedNode));

  /** Update a single field on the selected node's agentConfig. Pass undefined to clear. */
  function updateNodeConfig<K extends keyof AgentNodeConfig>(key: K, value: AgentNodeConfig[K]) {
    store.updateWorkflow((wf) => {
      const n = wf.nodes.find((n) => n.id === selectedNode!.id);
      if (!n) return;
      if (n.kind.type !== "task" && n.kind.type !== "run_agent") return;
      if (!n.kind.agentConfig) n.kind.agentConfig = {};
      if (value === undefined) {
        delete (n.kind.agentConfig as Record<string, unknown>)[key];
        if (Object.keys(n.kind.agentConfig).length === 0) n.kind.agentConfig = null;
      } else {
        n.kind.agentConfig[key] = value;
      }
    });
  }

  function updateRunAgentConfig(field: string, value: unknown) {
    store.updateWorkflow((wf) => {
      const n = wf.nodes.find((x) => x.id === selectedNode!.id);
      if (!n || n.kind.type !== "run_agent") return;
      n.kind.runAgentConfig = mergeConfig(DEFAULT_RUN_AGENT_CONFIG, n.kind.runAgentConfig, field, value);
    });
  }

  const updateRunAgent = (f: string, v: unknown) => updateRunAgentConfig(f, v);

  /** Capability badge labels for agent dropdown */
  const capBadges: Array<{ key: keyof AgentCapabilities; label: string }> = [
    { key: "nativeJsonSchema", label: "schema" },
    { key: "reasoningConfig", label: "reasoning" },
    { key: "systemPrompt", label: "sysprompt" },
    { key: "budgetLimit", label: "budget" },
    { key: "toolAllowlist", label: "tools" },
  ];

  $effect(() => {
    if (!selectedNode || (selectedNode.kind.type !== "task" && selectedNode.kind.type !== "run_agent")) {
      return;
    }
    const availableModes = supportedAccessModes(selectedAgentAccessProfiles);
    const currentMode = nodeConfig.accessMode ?? "execute";
    const firstSupported = availableModes[0];
    if (firstSupported && !availableModes.includes(currentMode)) {
      updateNodeConfig("accessMode", firstSupported);
    }
  });
</script>

{#if selectedNode}
  {@const nodeIssues = issues.filter(
    (i) => i.nodeId === selectedNode.id && issueMatchesScope(i, activeScope),
  )}
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
      {#if selectedNode.kind.type === "task" || selectedNode.kind.type === "run_agent"}
        <!-- PROMPT SECTION (always visible, primary) -->
        <section class="inspectorSection">
          <div class="inspectorSection__title">{selectedNode.kind.type === "run_agent" ? "Agent" : "Prompt"}</div>
          <label class="field">
            <span>Agent</span>
            <select
              value={selectedAgentValue}
              onchange={(e) => {
                const value = (e.target as HTMLSelectElement).value;
                if (selectedNode!.kind.type === "run_agent") {
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
            {#if selectedAgentHasNoAccessModes}
              <small class="issue issue--error">Agent has no supported access profiles.</small>
            {/if}
          </label>
          <div class="field field--prompt">
            <span>Prompt</span>
            <PromptTextarea
              value={selectedPromptValue}
              suggestions={promptSuggestions}
              oninput={(e) => {
                const value = (e.target as HTMLTextAreaElement).value;
                if (selectedNode!.kind.type === "run_agent") {
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
          {#if selectedNode.kind.type === "task"}
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
        {#if selectedNode.kind.type === "run_agent"}
          {@const rc = runAgentConfig(selectedNode)}
          <section class="inspectorSection">
            <div class="inspectorSection__title">Agent run</div>
            <small class="helperText">Spawns the agent in a PTY pane, waits for completion, then captures output.</small>
            <PaneNameField
              value={rc.name}
              onchange={(value) => updateRunAgent("name", value)}
            />
            <AccessProfileField
              value={rc.access}
              profiles={selectedAgentAccessProfiles}
              onchange={(value) => updateRunAgent("access", value)}
            />
            <WorkingDirectoryField
              value={rc.cwd}
              placeholder={activeWorkflow.cwd || "inherit workflow cwd"}
              onchange={(value) => updateRunAgent("cwd", value)}
            />
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
            <ExtraArgsField
              value={rc.extraArgs}
              onchange={(value) => updateRunAgent("extraArgs", value)}
            />
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
            <AgentConfigFields
              caps={agentCaps}
              accessProfiles={selectedAgentAccessProfiles}
              values={nodeConfig}
              update={(key, value) => updateNodeConfig(key as keyof AgentNodeConfig, value)}
              placeholders={{ model: "workflow default", systemPrompt: "workflow default" }}
            />

            <!-- Per-node working directory -->
            <WorkingDirectoryField
              value={selectedNode.cwd}
              placeholder={activeWorkflow.cwd || "inherit workflow cwd"}
              onchange={(value) => store.updateWorkflow((wf) => {
                const n = wf.nodes.find((n) => n.id === selectedNode!.id);
                if (n) n.cwd = value || null;
              })}
            />

            <!-- Continue session from (session reuse) -->
            {#if agentCaps?.sessionReuse}
              {@const currentAgent = selectedNode.agent ?? "claude"}
              {@const eligibleNodes = activeWorkflow.nodes.filter(
                (n) => n.kind.type === "task" && n.id !== selectedNode!.id && (n.agent ?? "claude") === currentAgent
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
                min="0"
                max="10"
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

      {:else if selectedNode.kind.type === "approval"}
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
      {:else if selectedNode.kind.type === "split"}
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
      {:else if selectedNode.kind.type === "collector"}
        <section class="inspectorSection">
          <div class="inspectorSection__title">Collector</div>
          <p style="margin: 0; color: var(--text-dim); line-height: 1.5;">
            Collectors wait for all inbound success paths in the current execution epoch, merge their inputs, and continue through a single success edge.
          </p>
        </section>

      {:else if selectedNode.kind.type === "decide"}
        <DecidePanel node={selectedNode} {promptSuggestions} />

      {:else if selectedNode.kind.type === "parallel_batch"}
        <ParallelBatchPanel node={selectedNode} workflow={activeWorkflow} />

      {:else if selectedNode.kind.type === "spawn"}
        <SpawnPanel
          node={selectedNode}
          workflow={activeWorkflow}
          {capabilities}
          accessProfiles={selectedAgentAccessProfiles}
        />

      {:else if selectedNode.kind.type === "send"}
        <SendPanel node={selectedNode} {promptSuggestions} />

      {:else if selectedNode.kind.type === "wait"}
        <WaitPanel node={selectedNode} />

      {:else if selectedNode.kind.type === "capture"}
        <CapturePanel node={selectedNode} />

      {:else if selectedNode.kind.type === "kill"}
        <KillPanel node={selectedNode} />

      {:else if selectedNode.kind.type === "subflow" || selectedNode.kind.type === "call"}
        <SubflowPanel node={selectedNode} workflow={activeWorkflow} />
      {/if}
    </div>

    <!-- STICKY TEST FOOTER (task nodes only) -->
    {#if selectedNode.kind.type === "task"}
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
                  const preview = await runSelectedNodePreview();
                  if (preview) testResult = JSON.stringify(preview, null, 2);
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
  <EdgeInspector edge={selectedEdge} {workflow} {capabilities} />

{:else}
  <WorkflowInspector {workflow} {capabilities} />
{/if}

<PasswordDialog
  open={unlockPrompt.open}
  title={unlockPrompt.title}
  message={unlockPrompt.message}
  onsubmit={(value) => unlockPrompt.submit(value)}
  oncancel={() => unlockPrompt.cancel()}
/>
