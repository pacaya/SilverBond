<script lang="ts">
  import { store } from "@/lib/stores/workflowStore.svelte";
  import type {
    AgentDefaults,
    RunAsConfig,
    RuntimeCapabilities,
    WorkflowDocument,
  } from "@/lib/types/workflow";
  import { mergeConfig } from "./mergeConfig";
  import { supportedAccessModes } from "./supportedAccessModes";
  import AgentConfigFields from "./AgentConfigFields.svelte";

  let {
    workflow,
    capabilities,
  }: {
    workflow: WorkflowDocument;
    capabilities: RuntimeCapabilities | undefined;
  } = $props();

  let activeWorkflow = $derived(store.activeWorkflow ?? workflow);

  let showAgentDefaultsFor = $state<string | null>(null);

  /* Run As / Sandbox */
  let attachHintCopied = $state(false);

  function hasRunAsValues(config: RunAsConfig): boolean {
    return Boolean(config.user || (config.command && config.command.length > 0));
  }

  function updateRunAs(key: "user", value: string) {
    store.updateWorkflow((wf) => {
      const next = mergeConfig({} as RunAsConfig, wf.runAs, key, value.trim() || undefined);
      wf.runAs = hasRunAsValues(next) ? next : undefined;
    });
  }

  function updateRunAsCommand(value: string) {
    store.updateWorkflow((wf) => {
      const tokens = value.split(/\n/).filter((line) => line.length > 0);
      const next = mergeConfig(
        {} as RunAsConfig,
        wf.runAs,
        "command",
        tokens.length > 0 ? tokens : undefined,
      );
      wf.runAs = hasRunAsValues(next) ? next : undefined;
    });
  }

  let runAsAttachHint = $derived.by(() => {
    const runAs = activeWorkflow.runAs;
    const user = runAs?.user?.trim() || "<user>";
    const prefix = runAs?.command && runAs.command.length > 0
      ? runAs.command.join(" ")
      : `sudo -u ${user}`;
    return `${prefix} tmux -L silverbond-<run-id> attach -t <session>`;
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

  /* Reset the expanded panel when the selection changes. This branch is not
     always freshly mounted when that happens — several store transitions set
     the selection to the workflow kind with a null id, and the ones reachable
     with nothing selected leave this branch mounted, so unmount semantics would
     not perform this reset. See every site assigning
     `selection = { kind: "workflow", id: null }` in workflowStore.svelte.ts
     to derive the current set. */
  $effect(() => {
    const _sel = store.selection; // subscribe-only read — re-run when selection changes
    showAgentDefaultsFor = null;
  });

  $effect(() => {
    const agentName = showAgentDefaultsFor;
    if (!agentName || !capabilities) return;

    const availableModes = supportedAccessModes(capabilities.agents[agentName]?.accessProfiles);
    const currentMode = activeWorkflow.agentDefaults?.[agentName]?.accessMode ?? "execute";
    const firstSupported = availableModes[0];
    if (firstSupported && !availableModes.includes(currentMode)) {
      updateAgentDefault(agentName, "accessMode", firstSupported);
    }
  });
</script>

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

  {#if !store.isDrilledIn}
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
        Launch spawned panes under a different user or via a custom command prefix.
        Every run uses its own dedicated tmux socket.
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
  {/if}

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
              <AgentConfigFields
                {caps}
                accessProfiles={agentInfo.accessProfiles}
                values={defaults}
                update={(key, value) => updateAgentDefault(agentName, key, value)}
                placeholders={{ model: "agent default", systemPrompt: "none" }}
              />
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

<style>
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
