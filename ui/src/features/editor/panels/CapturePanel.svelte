<script lang="ts">
  import type { WorkflowNode } from "@/lib/types/workflow";
  import {
    CAPTURE_CONFIG_TARGET,
    captureConfig,
    makeConfigWriter,
    numOrUndef,
  } from "../mergeConfig";

  let {
    node,
  }: {
    node: WorkflowNode;
  } = $props();

  let cc = $derived(captureConfig(node));

  let update = $derived.by(() =>
    makeConfigWriter(node.id, CAPTURE_CONFIG_TARGET),
  );
</script>

<section class="inspectorSection">
  <div class="inspectorSection__title">Capture</div>
  <small class="helperText">Captures pane output into the run context for downstream nodes.</small>
  <label class="field">
    <span>Target pane</span>
    <input
      value={cc.target ?? ""}
      placeholder="active pane"
      onblur={(e) => update("target", (e.target as HTMLInputElement).value || undefined)}
    />
  </label>
  <label class="field field--split">
    <span>Lines</span>
    <input
      type="number"
      value={cc.lines ?? ""}
      placeholder="visible"
      disabled={cc.all}
      oninput={(e) => update("lines", numOrUndef((e.target as HTMLInputElement).value))}
    />
  </label>
  <label class="field toggle-field">
    <span>Capture all scrollback</span>
    <input
      type="checkbox"
      class="toggle"
      checked={cc.all ?? false}
      onchange={(e) => update("all", (e.target as HTMLInputElement).checked)}
    />
  </label>
  <label class="field toggle-field">
    <span>Include ANSI</span>
    <input
      type="checkbox"
      class="toggle"
      checked={cc.ansi ?? false}
      onchange={(e) => update("ansi", (e.target as HTMLInputElement).checked)}
    />
  </label>
</section>
