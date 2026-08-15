<script lang="ts">
  import type { WorkflowNode } from "@/lib/types/workflow";
  import type { AutocompleteSuggestion } from "@/lib/utils/templateSuggestions";
  import PromptTextarea from "@/lib/components/PromptTextarea.svelte";
  import {
    DECIDE_CONFIG_TARGET,
    decideConfig,
    makeConfigWriter,
  } from "../mergeConfig";
  import InputBindingsEditor from "../InputBindingsEditor.svelte";

  let {
    node,
    promptSuggestions,
  }: {
    node: WorkflowNode;
    promptSuggestions: AutocompleteSuggestion[];
  } = $props();

  let dc = $derived(decideConfig(node));

  let update = $derived.by(() =>
    makeConfigWriter(node.id, DECIDE_CONFIG_TARGET),
  );
</script>

<section class="inspectorSection">
  <div class="inspectorSection__title">Decide</div>
  <small class="helperText">An LLM reads the inputs and picks one outcome; each outcome maps to a branch edge.</small>
  <div class="field field--prompt">
    <span>Decision prompt</span>
    <PromptTextarea
      value={dc.prompt}
      suggestions={promptSuggestions}
      oninput={(e) => update("prompt", (e.target as HTMLTextAreaElement).value)}
    />
  </div>
  <label class="field">
    <span>Model</span>
    <input
      value={dc.model ?? ""}
      placeholder="claude-haiku-4-5"
      onblur={(e) => update("model", (e.target as HTMLInputElement).value || undefined)}
    />
  </label>
  <label class="field">
    <span>Outcomes (one per line)</span>
    <textarea
      value={(dc.outcomes ?? []).join("\n")}
      placeholder={"approve\nreject\nescalate"}
      onblur={(e) => {
        const list = (e.target as HTMLTextAreaElement).value.split("\n").map((s) => s.trim()).filter(Boolean);
        update("outcomes", list);
      }}
      class="field--shortTextarea"
    ></textarea>
    <small class="helperText">Use these as branch ids on the outgoing edges.</small>
  </label>
</section>
<InputBindingsEditor list={dc.inputs ?? []} {update} />
