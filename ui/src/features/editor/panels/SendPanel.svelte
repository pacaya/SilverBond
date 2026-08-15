<script lang="ts">
  import type { WorkflowNode } from "@/lib/types/workflow";
  import PromptTextarea from "@/lib/components/PromptTextarea.svelte";
  import type { AutocompleteSuggestion } from "@/lib/utils/templateSuggestions";
  import { makeConfigWriter, SEND_CONFIG_TARGET, sendConfig } from "../mergeConfig";

  let {
    node,
    promptSuggestions,
  }: {
    node: WorkflowNode;
    promptSuggestions: AutocompleteSuggestion[];
  } = $props();

  let sd = $derived(sendConfig(node));

  let update = $derived.by(() =>
    makeConfigWriter(node.id, SEND_CONFIG_TARGET),
  );
</script>

<section class="inspectorSection">
  <div class="inspectorSection__title">Send</div>
  <small class="helperText">Sends text / keystrokes to a running pane.</small>
  <label class="field">
    <span>Target pane</span>
    <input
      value={sd.target ?? ""}
      placeholder="active pane"
      onblur={(e) => update("target", (e.target as HTMLInputElement).value || undefined)}
    />
  </label>
  <div class="field field--prompt">
    <span>Text</span>
    <PromptTextarea
      value={sd.text}
      suggestions={promptSuggestions}
      oninput={(e) => update("text", (e.target as HTMLTextAreaElement).value)}
    />
  </div>
  <label class="field toggle-field">
    <span>Press Enter after</span>
    <input
      type="checkbox"
      class="toggle"
      checked={sd.enter ?? true}
      onchange={(e) => update("enter", (e.target as HTMLInputElement).checked)}
    />
  </label>
</section>
