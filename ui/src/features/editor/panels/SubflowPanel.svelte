<script lang="ts">
  import { store } from "@/lib/stores/workflowStore.svelte";
  import type { WorkflowDocument, WorkflowNode } from "@/lib/types/workflow";
  import InputBindingsEditor from "../InputBindingsEditor.svelte";
  import {
    makeConfigWriter,
    numOrUndef,
    subflowConfig,
    SUBFLOW_CONFIG_TARGET,
  } from "../mergeConfig";

  let {
    node,
    workflow,
  }: {
    node: WorkflowNode;
    workflow: WorkflowDocument;
  } = $props();

  let fc = $derived(subflowConfig(node));
  let subflowNames = $derived(Object.keys(workflow.subflows ?? {}));
  let referenced = $derived(workflow.subflows?.[fc.workflowName]);

  let update = $derived.by(() =>
    makeConfigWriter(node.id, SUBFLOW_CONFIG_TARGET),
  );
</script>

<section class="inspectorSection">
  <div class="inspectorSection__title">Compound ({node.kind.type})</div>
  <small class="helperText">References a reusable saved subgraph. Double-click the node on the canvas to drill in.</small>
  <label class="field">
    <span>Subflow</span>
    <select
      value={fc.workflowName}
      onchange={(e) => update("workflowName", (e.target as HTMLSelectElement).value)}
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
        onchange={(e) => update("exitNodeId", (e.target as HTMLSelectElement).value || undefined)}
      >
        <option value="">Infer single terminal</option>
        {#each referenced.nodes as n (n.id)}
          <option value={n.id}>{n.name}</option>
        {/each}
      </select>
    </label>
    <button class="button button--ghost" onclick={() => store.drillIntoSubflow(node.id)}>
      Open subgraph →
    </button>
  {:else if fc.workflowName}
    <div class="issue issue--warning">Subflow "{fc.workflowName}" is not defined in this workflow.</div>
  {/if}
  <label class="field field--split">
    <span>Max depth</span>
    <input
      type="number"
      min="1"
      value={fc.maxDepth ?? 10}
      oninput={(e) => update("maxDepth", numOrUndef((e.target as HTMLInputElement).value) ?? 10)}
    />
  </label>
</section>
<InputBindingsEditor list={fc.inputs ?? []} {update} />

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
</style>
