<script lang="ts">
  import type { InputBinding } from "@/lib/types/workflow";

  let {
    list,
    update,
  }: {
    list: InputBinding[];
    update: (field: string, value: unknown) => void;
  } = $props();
</script>

<section class="inspectorSection">
  <div class="inspectorSection__title">Inputs</div>
  <small class="helperText">Bind named inputs from context / variables / templates.</small>
  {#if list.length > 0}
    <div class="contextRow contextRow--header">
      <span class="columnLabel">Name</span>
      <span class="columnLabel">Source</span>
      <span></span>
    </div>
  {/if}
  {#each list as binding, index (index)}
    <div class="contextRow">
      <input
        value={binding.name}
        placeholder="name"
        oninput={(e) => update("inputs", list.map((b, i) => i === index ? { ...b, name: (e.target as HTMLInputElement).value } : b))}
      />
      <input
        value={binding.source}
        placeholder={"{{previous_output}} / alias"}
        oninput={(e) => update("inputs", list.map((b, i) => i === index ? { ...b, source: (e.target as HTMLInputElement).value } : b))}
      />
      <button
        class="button button--ghost"
        onclick={() => update("inputs", list.filter((_, i) => i !== index))}
      >
        Remove
      </button>
    </div>
  {/each}
  <button
    class="button button--ghost"
    onclick={() => update("inputs", [...list, { name: "", source: "" } satisfies InputBinding])}
  >
    + Add input
  </button>
</section>
