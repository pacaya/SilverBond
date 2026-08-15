<script lang="ts">
  import type { RuntimeCapabilities, WorkflowDocument, WorkflowNode } from "@/lib/types/workflow";
  import AccessProfileField from "../AccessProfileField.svelte";
  import ExtraArgsField from "../ExtraArgsField.svelte";
  import PaneNameField from "../PaneNameField.svelte";
  import WorkingDirectoryField from "../WorkingDirectoryField.svelte";
  import {
    SPAWN_CONFIG_TARGET,
    makeConfigWriter,
    spawnConfig,
  } from "../mergeConfig";

  let {
    node,
    workflow,
    capabilities,
    accessProfiles,
  }: {
    node: WorkflowNode;
    workflow: WorkflowDocument;
    capabilities: RuntimeCapabilities | undefined;
    accessProfiles: string[] | undefined;
  } = $props();

  let sc = $derived(spawnConfig(node));

  let update = $derived.by(() =>
    makeConfigWriter(node.id, SPAWN_CONFIG_TARGET),
  );
</script>

<section class="inspectorSection">
  <div class="inspectorSection__title">Spawn</div>
  <small class="helperText">Launches a long-lived agent/command into a managed PTY pane.</small>
  <label class="field">
    <span>Agent</span>
    <select
      value={sc.agent ?? ""}
      onchange={(e) => update("agent", (e.target as HTMLSelectElement).value || undefined)}
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
      onblur={(e) => update("command", (e.target as HTMLInputElement).value || undefined)}
    />
  </label>
  <PaneNameField
    value={sc.name}
    onchange={(value) => update("name", value)}
  />
  <label class="field">
    <span>Session name</span>
    <input
      value={sc.sessionName ?? ""}
      placeholder="optional tmux session"
      onblur={(e) => update("sessionName", (e.target as HTMLInputElement).value || undefined)}
    />
  </label>
  <AccessProfileField
    value={sc.access}
    profiles={accessProfiles}
    onchange={(value) => update("access", value)}
  />
  <WorkingDirectoryField
    value={sc.cwd}
    placeholder={workflow.cwd || "inherit workflow cwd"}
    onchange={(value) => update("cwd", value)}
  />
  <ExtraArgsField
    value={sc.extraArgs}
    onchange={(value) => update("extraArgs", value)}
  />
</section>
