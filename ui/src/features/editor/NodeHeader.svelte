<script lang="ts">
  import { store } from "@/lib/stores/workflowStore.svelte";
  import type { WorkflowDocument, WorkflowNode } from "@/lib/types/workflow";

  let {
    node,
    workflow,
  }: {
    node: WorkflowNode;
    workflow: WorkflowDocument;
  } = $props();

  let showMenu = $state(false);

  const typeColors: Record<string, string> = {
    task: "var(--blue)",
    approval: "var(--amber)",
    split: "var(--teal)",
    collector: "var(--purple)",
  };

  function toggleEntryNode() {
    store.updateWorkflow((wf) => {
      wf.entryNodeId = wf.entryNodeId === node.id ? "" : node.id;
    });
    showMenu = false;
  }

  function duplicateNode() {
    const pos = workflow.ui?.canvas?.nodes[node.id];
    const newPos = pos ? { x: pos.x + 40, y: pos.y + 40 } : undefined;
    store.addNode(node.type, newPos);
    const newId = store.selection.kind === "node" ? store.selection.id : null;
    if (newId) {
      const clone = structuredClone(node);
      store.updateWorkflow((wf) => {
        const n = wf.nodes.find((n) => n.id === newId);
        if (!n) return;
        Object.assign(n, clone, { id: newId, name: `${node.name} copy` });
      });
    }
    showMenu = false;
  }

  function copyNodeId() {
    navigator.clipboard.writeText(node.id);
    showMenu = false;
  }

  function deleteNode() {
    if (confirm(`Delete node "${node.name || node.id}"? Connected edges will also be removed.`)) {
      store.removeNode(node.id);
    }
  }
</script>

<div class="nodeHeader">
  <span
    class="nodeHeader__typeBadge"
    style="--badge-color: {typeColors[node.type] ?? 'var(--text-dim)'}"
  >
    {node.type}
  </span>
  <input
    class="nodeHeader__name"
    value={node.name}
    oninput={(e) => store.updateWorkflow((wf) => {
      const n = wf.nodes.find((n) => n.id === node.id);
      if (n) n.name = (e.target as HTMLInputElement).value;
    })}
  />
  <div class="nodeHeader__actions">
    <div class="nodeHeader__menuWrap">
      <button
        class="nodeHeader__menuBtn"
        title="More actions"
        onclick={() => showMenu = !showMenu}
      >
        &#8942;
      </button>
      {#if showMenu}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="nodeHeader__menuBackdrop" onclick={() => showMenu = false}></div>
        <div class="nodeHeader__menu">
          <button class="nodeHeader__menuItem" onclick={toggleEntryNode}>
            {workflow.entryNodeId === node.id ? "Unset entry node" : "Set as entry node"}
          </button>
          <button class="nodeHeader__menuItem" onclick={duplicateNode}>
            Duplicate node
          </button>
          <button class="nodeHeader__menuItem" onclick={copyNodeId}>
            Copy node ID
          </button>
        </div>
      {/if}
    </div>
    <button
      class="nodeHeader__deleteBtn"
      title="Delete node (⌫)"
      onclick={deleteNode}
    >
      &#128465;
    </button>
  </div>
</div>
