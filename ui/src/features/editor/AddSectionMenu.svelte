<script lang="ts">
  import type { AgentCapabilities, WorkflowNode } from "@/lib/types/workflow";
  import { sectionHasValues, SECTION_DEFS, isSectionAvailable } from "@/lib/utils/sectionUtils";

  let {
    node,
    agentCaps,
    openSections,
    onToggle,
  }: {
    node: WorkflowNode;
    agentCaps: AgentCapabilities | null;
    openSections: Set<string>;
    onToggle: (sectionId: string) => void;
  } = $props();

  let availableSections = $derived(
    SECTION_DEFS.filter((s) => isSectionAvailable(s.id, agentCaps) && !openSections.has(s.id))
  );
</script>

{#if availableSections.length > 0}
  <div class="addSectionMenu">
    <div class="addSectionMenu__title">+ Add section</div>
    <div class="addSectionMenu__list">
      {#each availableSections as section (section.id)}
        <button
          class="addSectionMenu__item"
          title={section.tooltip}
          onclick={() => onToggle(section.id)}
        >
          <span class="addSectionMenu__label">{section.label}</span>
          {#if sectionHasValues(node, section.id)}
            <span class="addSectionMenu__dot" title="Has configured values"></span>
          {/if}
        </button>
      {/each}
    </div>
  </div>
{/if}
