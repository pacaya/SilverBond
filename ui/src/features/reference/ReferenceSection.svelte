<script lang="ts">
  import type { ReferenceGroup } from "./referenceData";

  let { group }: { group: ReferenceGroup } = $props();

  let expanded = $state(false);
</script>

<section class="inspectorSection">
  <button
    class="inspectorSection__title inspectorSection__title--collapsible"
    onclick={() => (expanded = !expanded)}
  >
    <span class="chevron" class:chevron--open={expanded}>&#9654;</span>
    {group.title}
  </button>
  {#if expanded}
    <div class="referenceEntries">
      {#each group.entries as entry (entry.syntax)}
        <div class="referenceEntry">
          <code class="referenceEntry__syntax">{entry.syntax}</code>
          <p class="referenceEntry__desc">{entry.description}</p>
          {#if entry.example}
            <code class="referenceEntry__example">{entry.example}</code>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</section>
