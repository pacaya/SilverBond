<script lang="ts">
  import clsx from "clsx";
  import { remapWorkflowIds } from "@/lib/types/workflow";
  import type { TemplateItem, WorkflowDocument, WorkflowItem } from "@/lib/types/workflow";

  let {
    workflows,
    templates,
    currentWorkflowName,
    loading = false,
    onCreate,
    onOpen,
    onDelete,
    onImport,
  }: {
    workflows: WorkflowItem[];
    templates: TemplateItem[];
    currentWorkflowName: string;
    loading?: boolean;
    onCreate: () => void;
    onOpen: (workflow: WorkflowDocument) => void;
    onDelete: (name: string) => void;
    onImport: (workflow: WorkflowDocument) => void;
  } = $props();

  let fileInput: HTMLInputElement | undefined = $state();

  async function handleFileChange(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    try {
      const payload = JSON.parse(await file.text()) as WorkflowDocument;
      if (!payload.nodes || !Array.isArray(payload.nodes) || !Array.isArray(payload.edges)) {
        alert("Invalid workflow file: missing nodes or edges array.");
        return;
      }
      onImport(payload);
    } catch {
      alert("Failed to parse workflow file. Ensure it is valid JSON.");
    }
    input.value = "";
  }

  function handleDelete(name: string) {
    if (confirm(`Delete workflow "${name}"? This cannot be undone.`)) {
      onDelete(name);
    }
  }
</script>

<aside class="sidebar">
  <div class="sidebar__header">Workflows</div>
  <div class="sidebar__section">
    <button class="button button--primary" data-testid="new-workflow" onclick={onCreate}>
      New workflow
    </button>
    <button class="button button--ghost" onclick={() => fileInput?.click()}>
      Import v3 JSON
    </button>
    <input
      bind:this={fileInput}
      hidden
      type="file"
      accept=".json,application/json"
      onchange={handleFileChange}
    />
  </div>

  <div class="sidebar__header">Saved</div>
  <div class="sidebar__list sidebar__list--scroll">
    {#if loading}
      <div class="sidebar__empty">Loading...</div>
    {:else if workflows.length === 0}
      <div class="sidebar__empty">No saved workflows yet. Create one above or import a JSON file.</div>
    {:else}
      {#each workflows as item (item.filename)}
        <div
          class={clsx("sidebar__item", {
            "sidebar__item--active": item.name === currentWorkflowName,
          })}
        >
          <button class="sidebar__itemButton" onclick={() => onOpen(item.workflow)}>
            <span>{item.name}</span>
            <small>{item.workflow.nodes.length} nodes</small>
          </button>
          <button
            class="sidebar__delete"
            onclick={() => handleDelete(item.name)}
            aria-label={`Delete ${item.name}`}
          >
            &times;
          </button>
        </div>
      {/each}
    {/if}
  </div>

  <div class="sidebar__header">Templates</div>
  <div class="sidebar__list">
    {#if templates.length === 0}
      <div class="sidebar__empty">No templates</div>
    {:else}
      {#each templates as item (item.templateFile)}
        <button
          class="sidebar__template"
          onclick={() => onOpen(remapWorkflowIds(item.workflow))}
        >
          <strong>{item.name}</strong>
          <span>{item.description ?? "Workflow starter"}</span>
        </button>
      {/each}
    {/if}
  </div>
</aside>
