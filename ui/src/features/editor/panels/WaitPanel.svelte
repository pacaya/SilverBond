<script lang="ts">
  import type { WorkflowNode } from "@/lib/types/workflow";
  import { makeConfigWriter, numOrUndef, WAIT_CONFIG_TARGET, waitConfig } from "../mergeConfig";

  let {
    node,
  }: {
    node: WorkflowNode;
  } = $props();

  let wc = $derived(waitConfig(node));

  let update = $derived.by(() =>
    makeConfigWriter(node.id, WAIT_CONFIG_TARGET),
  );
</script>

<section class="inspectorSection">
  <div class="inspectorSection__title">Wait</div>
  <small class="helperText">Blocks until the target pane is idle, ready, or prints a marker.</small>
  <label class="field">
    <span>Target pane</span>
    <input
      value={wc.target ?? ""}
      placeholder="active pane"
      onblur={(e) => update("target", (e.target as HTMLInputElement).value || undefined)}
    />
  </label>
  <label class="field">
    <span>Mode</span>
    <select value={wc.mode} onchange={(e) => update("mode", (e.target as HTMLSelectElement).value)}>
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
        onblur={(e) => update("marker", (e.target as HTMLInputElement).value || undefined)}
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
        oninput={(e) => update("idleSeconds", numOrUndef((e.target as HTMLInputElement).value))}
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
        oninput={(e) => update("readyStableSeconds", numOrUndef((e.target as HTMLInputElement).value))}
      />
    </label>
  {/if}
  <label class="field field--split">
    <span>Timeout (s)</span>
    <input
      type="number"
      value={wc.timeout ?? ""}
      placeholder="none"
      oninput={(e) => update("timeout", numOrUndef((e.target as HTMLInputElement).value))}
    />
  </label>
</section>
