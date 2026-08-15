<script lang="ts">
  import type { WorkflowNode } from "@/lib/types/workflow";
  import { KILL_CONFIG_TARGET, killConfig, makeConfigWriter } from "../mergeConfig";

  let {
    node,
  }: {
    node: WorkflowNode;
  } = $props();

  let kc = $derived(killConfig(node));

  let update = $derived.by(() =>
    makeConfigWriter(node.id, KILL_CONFIG_TARGET),
  );
</script>

<section class="inspectorSection">
  <div class="inspectorSection__title">Kill</div>
  <small class="helperText">Terminates a running pane or tmux session.</small>
  <label class="field">
    <span>Target pane</span>
    <input
      value={kc.target ?? ""}
      placeholder="active pane"
      onblur={(e) => update("target", (e.target as HTMLInputElement).value || undefined)}
    />
  </label>
  <label class="field">
    <span>Session name</span>
    <input
      value={kc.sessionName ?? ""}
      placeholder="optional tmux session"
      onblur={(e) => update("sessionName", (e.target as HTMLInputElement).value || undefined)}
    />
  </label>
</section>
