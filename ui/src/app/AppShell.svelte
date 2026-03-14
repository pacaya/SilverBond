<script lang="ts">
  import { createQuery, createMutation, useQueryClient } from "@tanstack/svelte-query";
  import { api, streamRun } from "@/lib/api/client";
  import {
    cloneWorkflow,
    createEmptyWorkflow,
    duplicateWorkflowForEditing,
    ensureCanvas,
  } from "@/lib/types/workflow";
  import type { WorkflowDocument } from "@/lib/types/workflow";
  import { store } from "@/lib/stores/workflowStore.svelte";
  import Sidebar from "@/features/workflows/Sidebar.svelte";
  import GraphEditor from "@/features/editor/GraphEditor.svelte";
  import InspectorPanel from "@/features/editor/InspectorPanel.svelte";
  import RunPanel from "@/features/runtime/RunPanel.svelte";
  import HistoryPanel from "@/features/history/HistoryPanel.svelte";
  import ConfirmDialog from "@/lib/components/ConfirmDialog.svelte";
  import ReferencePanel from "@/features/reference/ReferencePanel.svelte";

  const queryClient = useQueryClient();

  function collectVariableOverrides(workflow: WorkflowDocument): Record<string, string> {
    return Object.fromEntries(
      workflow.variables.filter((v) => v.name).map((v) => [v.name, v.default])
    );
  }

  /* ── queries ──────────────────────────────────────────────────────── */

  const workflowsQuery = createQuery(() => ({ queryKey: ["workflows"], queryFn: api.workflows }));
  const templatesQuery = createQuery(() => ({ queryKey: ["templates"], queryFn: api.templates }));
  const capabilitiesQuery = createQuery(() => ({ queryKey: ["capabilities"], queryFn: api.capabilities }));

  /* ── mutations ────────────────────────────────────────────────────── */

  let saveMessage = $state("");
  let runMessage = $state("");
  let runStartTime = $state<number | null>(null);

  const saveMutation = createMutation(() => ({
    mutationFn: (payload: { name: string; workflow: WorkflowDocument }) =>
      api.saveWorkflow(payload.name, payload.workflow),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["workflows"] });
      saveMessage = "Saved";
      setTimeout(() => { saveMessage = ""; }, 1600);
    },
    onError: (err: Error) => {
      store.setError(`Save failed: ${err.message}`);
    },
  }));

  const deleteMutation = createMutation(() => ({
    mutationFn: (name: string) => api.deleteWorkflow(name),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["workflows"] });
      store.createWorkflow();
    },
    onError: (err: Error) => {
      store.setError(`Delete failed: ${err.message}`);
    },
  }));

  const validateMutation = createMutation(() => ({
    mutationFn: (wf: WorkflowDocument) => api.validateWorkflow(wf),
    onSuccess: (data: Awaited<ReturnType<typeof api.validateWorkflow>>) => store.setValidation(data),
  }));

  /* ── debounced validation (no dependency array footgun!) ──────────── */
  let validateTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    const wf = store.workflow;
    if (!wf) return;
    if (validateTimer) clearTimeout(validateTimer);
    validateTimer = setTimeout(() => {
      validateMutation.mutate(ensureCanvas(cloneWorkflow(wf)));
    }, 500);
    return () => {
      if (validateTimer) clearTimeout(validateTimer);
    };
  });

  /* ── derived state ────────────────────────────────────────────────── */

  let currentWorkflowName = $derived(store.workflow?.name ?? "");
  let workflows = $derived(workflowsQuery.data ?? []);
  let templates = $derived(templatesQuery.data ?? []);
  let capabilities = $derived(capabilitiesQuery.data);
  let workflowsLoading = $derived(workflowsQuery.isLoading);

  let issueSummary = $derived.by(() => {
    const issues = store.validation?.issues ?? [];
    return {
      errors: issues.filter((i) => i.severity === "error").length,
      warnings: issues.filter((i) => i.severity === "warning").length,
    };
  });

  /* ── confirm dialog state ────────────────────────────────────────── */

  let confirmOpen = $state(false);
  let confirmTitle = $state("Confirm");
  let confirmMessage = $state("");
  let confirmAction: (() => void) | null = $state(null);

  function requestConfirm(title: string, message: string, action: () => void) {
    confirmTitle = title;
    confirmMessage = message;
    confirmAction = action;
    confirmOpen = true;
  }

  function handleConfirm() {
    confirmOpen = false;
    confirmAction?.();
    confirmAction = null;
  }

  function handleCancel() {
    confirmOpen = false;
    confirmAction = null;
  }

  /* ── actions ──────────────────────────────────────────────────────── */

  function guardDirty(action: () => void) {
    if (store.dirty) {
      requestConfirm("Unsaved changes", "You have unsaved changes. Discard and continue?", action);
    } else {
      action();
    }
  }

  function openWorkflow(wf: WorkflowDocument) {
    guardDirty(() => {
      store.setWorkflow(duplicateWorkflowForEditing(wf));
      store.clearLines();
      store.resetRun();
      store.setPanelTab("output");
    });
  }

  function handleCreate() {
    guardDirty(() => {
      store.setWorkflow(createEmptyWorkflow());
      store.clearLines();
      store.resetRun();
    });
  }

  function exportWorkflow() {
    if (!store.workflow) return;
    const blob = new Blob([JSON.stringify(store.workflow, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${(store.workflow.name || "workflow").replace(/[^a-zA-Z0-9_-]+/g, "_")}.json`;
    anchor.click();
    URL.revokeObjectURL(url);
  }

  function showRunResult(hadError: boolean) {
    const elapsed = runStartTime ? ((Date.now() - runStartTime) / 1000).toFixed(1) : null;
    runMessage = hadError
      ? `Failed${elapsed ? ` (${elapsed}s)` : ""}`
      : `Completed${elapsed ? ` in ${elapsed}s` : ""}`;
    runStartTime = null;
    setTimeout(() => { runMessage = ""; }, 3000);
  }

  async function startRun(targetWorkflow: WorkflowDocument) {
    store.clearLines();
    store.setPanelTab("output");
    store.setRunState({ running: true, approval: null });
    runStartTime = Date.now();
    let hadError = false;
    try {
      const payload = await api.createRun(targetWorkflow, collectVariableOverrides(targetWorkflow));
      store.setRunState({ runId: payload.runId, running: true });
      await streamRun(payload.runId, (event) => store.applyRunEvent(event));
    } catch (err) {
      hadError = true;
      store.setError(`Run failed: ${err instanceof Error ? err.message : "Unknown error"}`);
    } finally {
      store.setRunState({ running: false, approval: null, runId: null });
      showRunResult(hadError);
      await queryClient.invalidateQueries({ queryKey: ["logs"] });
      await queryClient.invalidateQueries({ queryKey: ["interrupted-runs"] });
    }
  }

  async function resumeRun(runId: string) {
    store.setPanelTab("output");
    store.clearLines();
    store.setRunState({ running: true, runId });
    runStartTime = Date.now();
    let hadError = false;
    try {
      const payload = await api.resumeRun(runId);
      await streamRun(payload.runId, (event) => store.applyRunEvent(event));
    } catch (err) {
      hadError = true;
      store.setError(`Resume failed: ${err instanceof Error ? err.message : "Unknown error"}`);
    } finally {
      store.setRunState({ running: false, approval: null, runId: null });
      showRunResult(hadError);
      await queryClient.invalidateQueries({ queryKey: ["logs"] });
      await queryClient.invalidateQueries({ queryKey: ["interrupted-runs"] });
    }
  }

  async function restartFromNode(runId: string, nodeId: string) {
    store.setPanelTab("output");
    store.clearLines();
    store.setRunState({ running: true, runId });
    runStartTime = Date.now();
    let hadError = false;
    try {
      const payload = await api.restartFromNode(runId, nodeId);
      await streamRun(payload.runId, (event) => store.applyRunEvent(event));
    } catch (err) {
      hadError = true;
      store.setError(`Restart failed: ${err instanceof Error ? err.message : "Unknown error"}`);
    } finally {
      store.setRunState({ running: false, approval: null, runId: null });
      showRunResult(hadError);
      await queryClient.invalidateQueries({ queryKey: ["logs"] });
      await queryClient.invalidateQueries({ queryKey: ["interrupted-runs"] });
    }
  }

  /* ── keyboard shortcuts ───────────────────────────────────────────── */
  function handleKeydown(event: KeyboardEvent) {
    const isMod = event.metaKey || event.ctrlKey;

    // Cmd+S: save
    if (isMod && event.key === "s") {
      event.preventDefault();
      if (store.workflow && store.workflow.name) {
        saveMutation.mutate({ name: store.workflow.name, workflow: store.workflow });
      }
      return;
    }

    // Cmd+Z: undo
    if (isMod && !event.shiftKey && event.key === "z") {
      event.preventDefault();
      store.undo();
      return;
    }

    // Cmd+Shift+Z: redo
    if (isMod && event.shiftKey && event.key === "z") {
      event.preventDefault();
      store.redo();
      return;
    }

    // Delete/Backspace: remove selected node/edge (but not when in an input)
    if ((event.key === "Delete" || event.key === "Backspace") && !isInputFocused()) {
      if (store.selection.kind === "node" && store.selection.id) {
        const nodeId = store.selection.id;
        const node = store.workflow?.nodes.find(n => n.id === nodeId);
        requestConfirm(
          "Delete node",
          `Delete node "${node?.name || nodeId}"? Connected edges will also be removed.`,
          () => store.removeNode(nodeId),
        );
      } else if (store.selection.kind === "edge" && store.selection.id) {
        store.removeEdge(store.selection.id);
      }
      return;
    }
  }

  function isInputFocused(): boolean {
    const el = document.activeElement;
    if (!el) return false;
    const tag = el.tagName.toLowerCase();
    return tag === "input" || tag === "textarea" || tag === "select" || (el as HTMLElement).isContentEditable;
  }

  /* ── resizable inspector panel ─────────────────────────────────────── */
  let inspectorWidth = $state(360);
  let isResizing = $state(false);

  function startResize(event: PointerEvent) {
    event.preventDefault();
    isResizing = true;
    const startX = event.clientX;
    const startWidth = inspectorWidth;

    function onMove(e: PointerEvent) {
      const delta = startX - e.clientX;
      inspectorWidth = Math.max(280, Math.min(700, startWidth + delta));
    }

    function onUp() {
      isResizing = false;
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    }

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="shell" class:shell--resizing={isResizing}>
  <Sidebar
    {workflows}
    {templates}
    {currentWorkflowName}
    loading={workflowsLoading}
    onCreate={handleCreate}
    onOpen={openWorkflow}
    onDelete={(name) => deleteMutation.mutate(name)}
    onImport={(payload) => openWorkflow(ensureCanvas(payload))}
  />

  <main class="workspace">
    <header class="workspace__header">
      <div>
        <small>Graph editor</small>
        <h1>{store.workflow?.name || "Untitled workflow"}</h1>
      </div>
      <div class="workspace__status">
        {#if store.validation}
          <span class="statusPill">
            {issueSummary.errors} errors &middot; {issueSummary.warnings} warnings
          </span>
        {/if}
        {#if store.dirty}
          <span class="statusPill statusPill--dirty">Unsaved</span>
        {/if}
        {#if saveMessage}
          <span class="statusPill statusPill--saved">{saveMessage}</span>
        {/if}
        {#if runMessage}
          <span class="statusPill" class:statusPill--saved={!runMessage.startsWith("Failed")} class:statusPill--runFailed={runMessage.startsWith("Failed")}>
            {runMessage}
          </span>
        {/if}
        {#if store.errorMessage}
          <span class="statusPill" style="border-color: rgba(248, 113, 113, 0.3); color: var(--red);">{store.errorMessage}</span>
        {/if}
      </div>
      <div class="workspace__actions">
        <button class="button button--ghost" onclick={() => store.undo()} disabled={!store.canUndo} title="Undo (⌘Z)">
          Undo
        </button>
        <button class="button button--ghost" onclick={() => store.redo()} disabled={!store.canRedo} title="Redo (⌘⇧Z)">
          Redo
        </button>
        <button class="button button--ghost" onclick={exportWorkflow} disabled={!store.workflow} title="Export workflow as JSON">
          Export
        </button>
        <button
          class="button button--primary"
          data-testid="save-workflow"
          disabled={!store.workflow || !store.workflow.name}
          onclick={() => store.workflow && saveMutation.mutate({ name: store.workflow.name || "workflow", workflow: store.workflow })}
          title="Save workflow (⌘S)"
        >
          Save <kbd class="shortcutHint">⌘S</kbd>
        </button>
      </div>
    </header>

    {#if store.workflow}
      <div class="editorGrid" style="grid-template-columns: minmax(0, 1fr) auto {inspectorWidth}px;">
        <GraphEditor
          workflow={store.workflow}
          validation={store.validation}
          {capabilities}
        />
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="resizeHandle"
          class:resizeHandle--active={isResizing}
          onpointerdown={startResize}
        ></div>
        <InspectorPanel
          workflow={store.workflow}
          validation={store.validation}
          {capabilities}
        />
      </div>
    {:else}
      <div class="workspace__empty">Select a workflow from the sidebar, or create a new one to get started.</div>
    {/if}
  </main>

  <aside class="sidepanel">
    <div class="sidepanel__tabs">
      <button
        class={store.panelTab === "output" ? "sidepanel__tab sidepanel__tab--active" : "sidepanel__tab"}
        onclick={() => store.setPanelTab("output")}
      >
        Output
        {#if store.approval}<span class="approvalBadge"></span>{/if}
      </button>
      <button
        class={store.panelTab === "history" ? "sidepanel__tab sidepanel__tab--active" : "sidepanel__tab"}
        data-testid="history-tab"
        onclick={() => store.setPanelTab("history")}
      >
        History
      </button>
      <button
        class={store.panelTab === "reference" ? "sidepanel__tab sidepanel__tab--active" : "sidepanel__tab"}
        onclick={() => store.setPanelTab("reference")}
      >
        Reference
      </button>
    </div>
    {#if store.panelTab === "output"}
      <RunPanel
        workflow={store.workflow}
        isRunning={store.running}
        onRun={() => store.workflow && startRun(store.workflow)}
        onAbort={() => {
          if (store.runId) api.abortRun(store.runId);
        }}
        onApproval={(approved, userInput) => {
          if (store.runId) api.approveRun(store.runId, approved, userInput);
        }}
      />
    {:else if store.panelTab === "history"}
      <HistoryPanel
        onResume={(runId) => resumeRun(runId)}
        onRestart={(runId, nodeId) => restartFromNode(runId, nodeId)}
        onDismiss={(runId) => api.dismissRun(runId).then(() => queryClient.invalidateQueries({ queryKey: ["interrupted-runs"] }))}
      />
    {:else if store.panelTab === "reference"}
      <ReferencePanel workflow={store.workflow} />
    {/if}
  </aside>
</div>

<ConfirmDialog
  open={confirmOpen}
  title={confirmTitle}
  message={confirmMessage}
  confirmLabel="Discard"
  onconfirm={handleConfirm}
  oncancel={handleCancel}
/>
