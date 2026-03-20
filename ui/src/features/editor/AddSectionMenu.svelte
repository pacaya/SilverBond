<script lang="ts">
  import type { AgentCapabilities, WorkflowNode } from "@/lib/types/workflow";
  import { sectionHasValues } from "@/lib/utils/sectionUtils";
  import type { SectionId } from "@/lib/utils/sectionUtils";

  interface SectionDef {
    id: SectionId;
    label: string;
    tooltip: string;
  }

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

  const allSections: SectionDef[] = [
    { id: "agent-tuning", label: "Agent tuning", tooltip: "Customize how the agent behaves for this node" },
    { id: "guards-retry", label: "Guards & retry", tooltip: "Add failure recovery and time limits" },
    { id: "loop-control", label: "Loop control", tooltip: "Make this node repeat until a condition is met" },
    { id: "output-schema", label: "Output schema", tooltip: "Define the structure of the response" },
    { id: "tool-permissions", label: "Tool permissions", tooltip: "Control which tools the agent can use" },
    { id: "skip-condition", label: "Skip condition", tooltip: "Skip this node based on previous output" },
  ];

  function isAvailable(sectionId: string): boolean {
    if (!agentCaps) return sectionId !== "agent-tuning" && sectionId !== "tool-permissions";
    if (sectionId === "tool-permissions") return !!agentCaps.toolAllowlist;
    return true;
  }

  /** Sections not currently open and not auto-shown */
  let availableSections = $derived(
    allSections.filter((s) => isAvailable(s.id) && !openSections.has(s.id))
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
