<script lang="ts">
  import type { AutocompleteSuggestion } from "@/lib/utils/templateSuggestions";

  let {
    items,
    selectedIndex,
    visible,
    position,
  }: {
    items: AutocompleteSuggestion[];
    selectedIndex: number;
    visible: boolean;
    position: { top: number; left: number };
  } = $props();
</script>

{#if visible && items.length > 0}
  <div
    class="autocomplete-popup"
    style="top: {position.top}px; left: {position.left}px;"
  >
    {#each items as item, i (item.insertText)}
      <div
        class="autocomplete-item"
        class:autocomplete-item--active={i === selectedIndex}
        data-index={i}
      >
        <span class="autocomplete-item__label">{item.label}</span>
        <span class="autocomplete-item__desc">{item.description}</span>
      </div>
    {/each}
  </div>
{/if}
