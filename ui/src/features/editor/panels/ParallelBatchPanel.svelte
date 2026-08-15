<script lang="ts">
  import type { WorkflowDocument, WorkflowNode } from "@/lib/types/workflow";
  import {
    batchConfig,
    BATCH_CONFIG_TARGET,
    MAX_BATCH_CONCURRENT,
    makeConfigWriter,
    numOrUndef,
  } from "../mergeConfig";

  let {
    node,
    workflow,
  }: {
    node: WorkflowNode;
    workflow: WorkflowDocument;
  } = $props();

  let bc = $derived(batchConfig(node));

  let update = $derived.by(() =>
    makeConfigWriter(node.id, BATCH_CONFIG_TARGET),
  );
</script>

<section class="inspectorSection">
  <div class="inspectorSection__title">Parallel batch</div>
  <small class="helperText">Fans out over a collection, running the body subgraph once per item.</small>
  <label class="field">
    <span>Items binding</span>
    <input
      value={bc.itemsBinding}
      placeholder={"e.g. {{previous_output}} or context alias"}
      onblur={(e) => update("itemsBinding", (e.target as HTMLInputElement).value)}
    />
  </label>
  <label class="field">
    <span>Item variable</span>
    <input
      value={bc.itemVar}
      placeholder="item"
      onblur={(e) => update("itemVar", (e.target as HTMLInputElement).value)}
    />
  </label>
  <label class="field">
    <span>Body entry node</span>
    <select
      value={bc.bodyEntry}
      onchange={(e) => update("bodyEntry", (e.target as HTMLSelectElement).value)}
    >
      <option value="">Select node</option>
      {#each workflow.nodes.filter((n) => n.id !== node.id) as n (n.id)}
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
      oninput={(e) => {
        const raw = numOrUndef((e.target as HTMLInputElement).value) ?? 1;
        update(
          "maxConcurrent",
          Math.max(1, Math.min(MAX_BATCH_CONCURRENT, raw)),
        );
      }}
    />
  </label>
  <label class="field">
    <span>Collector variable</span>
    <input
      value={bc.collectorVar ?? ""}
      placeholder="optional — name to gather results"
      onblur={(e) => update("collectorVar", (e.target as HTMLInputElement).value || undefined)}
    />
  </label>
</section>
