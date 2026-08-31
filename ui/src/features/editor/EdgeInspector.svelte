<script lang="ts">
  import { store } from "@/lib/stores/workflowStore.svelte";
  import type {
    RuntimeCapabilities,
    WorkflowDocument,
    WorkflowEdge,
  } from "@/lib/types/workflow";
  import ConditionBuilder from "./ConditionBuilder.svelte";

  let {
    edge,
    workflow,
    capabilities,
    capabilitiesError = false,
  }: {
    edge: WorkflowEdge;
    workflow: WorkflowDocument;
    capabilities: RuntimeCapabilities | undefined;
    capabilitiesError?: boolean;
  } = $props();

  let activeWorkflow = $derived(store.activeWorkflow ?? workflow);

  let edgeDisplayName = $derived.by(() => {
    const fromNode = activeWorkflow.nodes.find((n) => n.id === edge.from);
    const toNode = activeWorkflow.nodes.find((n) => n.id === edge.to);
    const fromName = fromNode?.name || edge.from.slice(0, 8);
    const toName = toNode?.name || edge.to.slice(0, 8);
    return `${fromName} → ${toName}`;
  });

  function updateSelectedEdge(mutate: (edge: WorkflowEdge) => void) {
    store.updateWorkflow((wf) => {
      const found = wf.edges.find((ed) => ed.id === edge.id);
      if (found) mutate(found);
    });
  }
</script>

<div class="inspector">
  <div class="inspector__header">
    <div>
      <small>Edge</small>
      <h3>{edgeDisplayName}</h3>
    </div>
    <button class="button button--danger" onclick={() => store.removeEdge(edge.id)}>
      Delete
    </button>
  </div>

  <section class="inspectorSection">
    <div class="inspectorSection__title">Routing</div>
    {#if capabilities?.supportedEdgeOutcomes}
      <label class="field">
        <span>Outcome</span>
        <select
          value={edge.outcome}
          onchange={(e) => updateSelectedEdge((found) => {
            found.outcome = (e.target as HTMLSelectElement).value as WorkflowEdge["outcome"];
          })}
        >
          {#each capabilities.supportedEdgeOutcomes as outcome (outcome)}
            <option value={outcome}>{outcome}</option>
          {/each}
        </select>
      </label>
    {:else}
      <div class="field">
        <span>Outcome</span>
        <span class="field__pending">
          {capabilitiesError ? "Outcomes unavailable." : "Loading outcomes…"}
        </span>
      </div>
    {/if}
    <label class="field">
      <span>Label</span>
      <input
        value={edge.label ?? ""}
        oninput={(e) => updateSelectedEdge((found) => {
          found.label = (e.target as HTMLInputElement).value || null;
        })}
      />
    </label>
    <label class="field">
      <span>Branch id</span>
      <input
        value={edge.branchId ?? ""}
        oninput={(e) => updateSelectedEdge((found) => {
          found.branchId = (e.target as HTMLInputElement).value || null;
        })}
      />
    </label>
    <!-- Condition -->
    <label class="field">
      <span>Condition</span>
      <ConditionBuilder
        mode="structured"
        value={edge.condition ?? null}
        onchange={(val) => updateSelectedEdge((found) => {
          found.condition = val as WorkflowEdge["condition"] ?? null;
        })}
      />
    </label>
  </section>
</div>

<style>
  .field__pending {
    color: var(--text-dim);
    font-size: 13px;
  }
</style>
