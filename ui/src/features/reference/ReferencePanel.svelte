<script lang="ts">
  import type { WorkflowDocument } from "@/lib/types/workflow";
  import { allReferenceGroups } from "./referenceData";
  import ReferenceSection from "./ReferenceSection.svelte";

  let { workflow }: { workflow: WorkflowDocument | null } = $props();
</script>

<div class="referencePanel">
  {#if workflow && (workflow.nodes.length > 0 || workflow.variables.length > 0)}
    <section class="inspectorSection">
      <div class="inspectorSection__title">Your Workflow</div>
      {#if workflow.nodes.length > 0}
        <div class="referenceEntry">
          <p class="referenceEntry__desc" style="margin-bottom: 6px;">Nodes:</p>
          {#each workflow.nodes as node (node.id)}
            <div class="referenceEntry__row">
              <code class="referenceEntry__syntax">{node.name || node.id}</code>
              <code class="referenceEntry__example">{node.id}</code>
            </div>
          {/each}
        </div>
      {/if}
      {#if workflow.variables.length > 0}
        <div class="referenceEntry" style="margin-top: 8px;">
          <p class="referenceEntry__desc" style="margin-bottom: 6px;">Variables:</p>
          {#each workflow.variables.filter((v) => v.name) as variable (variable.name)}
            <div class="referenceEntry__row">
              <code class="referenceEntry__syntax">{"{{"}var:{variable.name}{"}}"}</code>
              {#if variable.default}
                <span class="referenceEntry__desc">default: {variable.default}</span>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {/if}

  {#each allReferenceGroups as group (group.title)}
    <ReferenceSection {group} />
  {/each}
</div>
