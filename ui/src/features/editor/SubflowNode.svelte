<script lang="ts">
  import { Handle, Position, type NodeProps } from "@xyflow/svelte";
  import { store } from "@/lib/stores/workflowStore.svelte";

  /** Data shape produced by buildFlowNodes() for subflow / call nodes. */
  interface CompoundData {
    label: string;
    nodeType: "subflow" | "call";
    subflowName: string;
    inputs: string[];
    output: string;
    missing: boolean;
  }

  let { id, data }: NodeProps = $props();
  let d = $derived(data as unknown as CompoundData);

  function handleDblClick(event: MouseEvent) {
    event.stopPropagation();
    if (!store.drillIntoSubflow(id) && d.missing) {
      store.setError(`Subflow "${d.subflowName || "(unset)"}" is not defined in this workflow.`);
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="compoundNode"
  class:compoundNode--missing={d.missing}
  ondblclick={handleDblClick}
>
  <Handle type="target" position={Position.Left} />

  <div class="compoundNode__header">
    <span class="compoundNode__badge">{d.nodeType === "call" ? "call" : "subflow"}</span>
    <span class="compoundNode__name">{d.label}</span>
  </div>

  <div class="compoundNode__ref" title="Referenced subgraph">
    {#if d.subflowName}
      ▸ {d.subflowName}{d.missing ? " (unresolved)" : ""}
    {:else}
      ▸ <em>no subflow selected</em>
    {/if}
  </div>

  <div class="compoundNode__ports">
    <div class="compoundNode__portCol compoundNode__portCol--in">
      <span class="compoundNode__portLabel">in</span>
      {#if d.inputs.length}
        {#each d.inputs as input (input)}
          <span class="compoundNode__port">{input || "—"}</span>
        {/each}
      {:else}
        <span class="compoundNode__port compoundNode__port--empty">none</span>
      {/if}
    </div>
    <div class="compoundNode__portCol compoundNode__portCol--out">
      <span class="compoundNode__portLabel">out</span>
      <span class="compoundNode__port">{d.output || "result"}</span>
    </div>
  </div>

  <div class="compoundNode__hint">double-click to drill in</div>

  <Handle type="source" position={Position.Right} />
</div>

<style>
  .compoundNode {
    width: 220px;
    border-radius: 16px;
    background: rgba(9, 18, 33, 0.94);
    border: 1px solid rgba(56, 189, 248, 0.45);
    border-left: 4px solid rgba(56, 189, 248, 0.82);
    box-shadow:
      6px 6px 0 -2px rgba(9, 18, 33, 0.94),
      6px 6px 0 -1px rgba(56, 189, 248, 0.3),
      0 18px 40px rgba(2, 8, 23, 0.36);
    padding: 12px 14px;
    color: var(--text-bright);
    font-size: 12px;
  }

  .compoundNode--missing {
    border-color: rgba(248, 113, 113, 0.55);
    border-left-color: rgba(248, 113, 113, 0.8);
  }

  .compoundNode__header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }

  .compoundNode__badge {
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding: 2px 6px;
    border-radius: 999px;
    background: rgba(56, 189, 248, 0.16);
    color: rgba(125, 211, 252, 0.95);
  }

  .compoundNode__name {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .compoundNode__ref {
    font-size: 11px;
    color: var(--text-dim);
    margin-bottom: 8px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .compoundNode__ports {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }

  .compoundNode__portCol {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
    flex: 1;
  }

  .compoundNode__portCol--out {
    align-items: flex-end;
    text-align: right;
  }

  .compoundNode__portLabel {
    font-size: 8px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-dim);
  }

  .compoundNode__port {
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 6px;
    background: rgba(148, 163, 184, 0.12);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }

  .compoundNode__port--empty {
    color: var(--text-dim);
    background: transparent;
    font-style: italic;
  }

  .compoundNode__hint {
    margin-top: 8px;
    font-size: 9px;
    color: var(--text-dim);
    opacity: 0.7;
    text-align: center;
  }
</style>
