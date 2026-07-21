<script lang="ts">
  import { onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { streamPane, type PaneStreamHandle } from "@/lib/api/client";
  import { store } from "@/lib/stores/workflowStore.svelte";
  import type { WorkflowDocument } from "@/lib/types/workflow";

  let {
    runId,
    streamToken,
    workflow,
  }: {
    runId: string | null;
    streamToken: string | null;
    workflow: WorkflowDocument | null;
  } = $props();

  let host: HTMLDivElement | undefined = $state();
  let ready = $state(false);

  let term: Terminal | undefined;
  let fit: FitAddon | undefined;
  let handle: PaneStreamHandle | null = null;
  let resizeObserver: ResizeObserver | undefined;

  function paneNodeId(pane: string): string {
    const separator = pane.lastIndexOf(":");
    return separator >= 0 ? pane.slice(separator + 1) : pane;
  }

  function paneLabel(pane: string): string {
    const nodeId = paneNodeId(pane);
    const node = workflow?.nodes.find((entry) => entry.id === nodeId);
    return node?.name || nodeId;
  }

  const paneOptions = $derived.by(() => {
    const observed = store.runObservability?.panes;
    if (observed?.length) {
      return observed.map((entry) => ({
        value: entry.pane,
        label: paneLabel(entry.pane),
        attachCommand: entry.attachCommand,
      }));
    }

    return [
      { value: "active", label: "Active pane", attachCommand: store.runObservability?.attachCommand ?? null },
      ...((workflow?.nodes ?? [])
        .filter(
          (node) =>
            node.kind.type === "run_agent" ||
            node.kind.type === "spawn" ||
            node.kind.type === "task",
        )
        .map((node) => ({
          value: node.id,
          label: node.name || node.id,
          attachCommand: store.runObservability?.attachCommand ?? null,
        }))),
    ];
  });

  const selectedAttachCommand = $derived(
    paneOptions.find((option) => option.value === store.selectedPane)?.attachCommand
      ?? store.runObservability?.attachCommand
      ?? null,
  );

  const statusLabel = $derived(
    store.paneStatus === "open"
      ? "Live"
      : store.paneStatus === "stalled"
        ? "Stalled"
        : store.paneStatus === "connecting"
          ? "Connecting…"
          : store.paneStatus === "closed"
            ? "Reconnecting…"
            : store.paneStatus === "unavailable"
              ? "Unavailable"
              : "Idle",
  );

  function requestFit() {
    try {
      fit?.fit();
    } catch {
      // Container not measurable yet; a later resize/observer tick will retry.
    }
  }

  onMount(() => {
    term = new Terminal({
      convertEol: false,
      cursorBlink: false,
      disableStdin: true,
      fontFamily: '"IBM Plex Mono", ui-monospace, SFMono-Regular, monospace',
      fontSize: 12,
      scrollback: 5000,
      theme: { background: "#05101e", foreground: "#dbe6ff" },
    });
    fit = new FitAddon();
    term.loadAddon(fit);

    if (host) {
      term.open(host);
      requestFit();
      resizeObserver = new ResizeObserver(() => requestFit());
      resizeObserver.observe(host);
    }
    ready = true;

    return () => {
      resizeObserver?.disconnect();
      resizeObserver = undefined;
      handle?.close();
      handle = null;
      term?.dispose();
      term = undefined;
      fit = undefined;
      store.setPaneStatus("idle");
    };
  });

  /* (Re)connect whenever the active run or the selected pane changes. The
   * effect also tears down the previous socket so we never double-stream. */
  $effect(() => {
    const activeRunId = runId;
    const activeStreamToken = streamToken;
    const pane = store.selectedPane;
    if (!ready || !term) return;

    handle?.close();
    handle = null;

    const liveTerm = term;
    liveTerm.reset();

    if (!activeRunId || !activeStreamToken) {
      store.setPaneStatus("idle");
      store.setPaneError("");
      return;
    }

    store.setPaneError("");
    store.setPaneStatus("connecting");
    handle = streamPane(activeRunId, pane, activeStreamToken, {
      onSnapshot: (bytes) => {
        // Full repaint: clear local buffer, then write the colored capture.
        liveTerm.reset();
        liveTerm.write(bytes);
        requestFit();
      },
      onData: (bytes) => liveTerm.write(bytes),
      onError: (message) => store.setPaneError(message),
      onStatus: (status) => store.setPaneStatus(status),
    });

    return () => {
      handle?.close();
      handle = null;
    };
  });

  function resync() {
    if (store.paneStatus === "open") {
      handle?.requestResync();
    } else {
      handle?.reconnect();
    }
  }
</script>

<div class="paneTerminal">
  <div class="paneTerminal__toolbar">
    <div>
      <small>Terminal</small>
      <h3>{runId ? "Live pane" : "No active run"}</h3>
    </div>
    <div class="paneTerminal__controls">
      <select
        class="paneTerminal__select"
        aria-label="Pane to stream"
        value={store.selectedPane}
        onchange={(event) => store.selectPane((event.currentTarget as HTMLSelectElement).value)}
        disabled={!runId}
      >
        {#each paneOptions as option (option.value)}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
      <span class="paneTerminal__status paneTerminal__status--{store.paneStatus}">
        {statusLabel}
      </span>
      <button
        class="button button--ghost"
        onclick={resync}
        disabled={!runId}
        title="Clear and repaint from a fresh snapshot"
      >
        Resync
      </button>
    </div>
  </div>

  {#if store.paneError}
    <div class="paneTerminal__error">{store.paneError}</div>
  {/if}

  {#if selectedAttachCommand}
    <div class="paneTerminal__attach">
      <code>{selectedAttachCommand}</code>
    </div>
  {/if}

  <div class="paneTerminal__screen" bind:this={host}></div>

  {#if !runId}
    <div class="paneTerminal__empty">
      Start or resume a run to view its live terminal panes here.
    </div>
  {/if}
</div>

<style>
  .paneTerminal {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 18px;
    overflow: hidden;
  }

  .paneTerminal__toolbar {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 12px;
    flex-shrink: 0;
  }

  .paneTerminal__toolbar small {
    text-transform: uppercase;
    letter-spacing: 0.16em;
    font-size: 0.72rem;
    color: var(--text-dim);
    font-family: "IBM Plex Mono", monospace;
  }

  .paneTerminal__toolbar h3 {
    margin: 4px 0 0;
    color: var(--text-bright);
  }

  .paneTerminal__controls {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  .paneTerminal__select {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--text-soft);
    padding: 6px 8px;
    max-width: 160px;
  }

  .paneTerminal__status {
    font-family: "IBM Plex Mono", monospace;
    font-size: 0.72rem;
    letter-spacing: 0.08em;
    color: var(--text-dim);
    padding: 4px 8px;
    border-radius: 999px;
    border: 1px solid var(--border);
    white-space: nowrap;
  }

  .paneTerminal__status--open {
    color: #86efac;
    border-color: rgba(134, 239, 172, 0.32);
  }

  .paneTerminal__status--connecting,
  .paneTerminal__status--closed {
    color: #fde68a;
    border-color: rgba(253, 230, 138, 0.32);
  }

  .paneTerminal__status--stalled {
    color: #fdba74;
    border-color: rgba(251, 146, 60, 0.32);
  }

  .paneTerminal__status--unavailable {
    color: #fda4af;
    border-color: rgba(248, 113, 113, 0.32);
  }

  .paneTerminal__error {
    flex-shrink: 0;
    color: #fda4af;
    font-size: 0.82rem;
    padding: 8px 12px;
    border-radius: 12px;
    border: 1px solid rgba(248, 113, 113, 0.3);
    background: rgba(248, 113, 113, 0.08);
  }

  .paneTerminal__attach {
    flex-shrink: 0;
    font-family: "IBM Plex Mono", monospace;
    font-size: 0.75rem;
    color: var(--text-dim);
    padding: 8px 12px;
    border-radius: 12px;
    border: 1px solid var(--border);
    background: var(--surface);
    overflow-x: auto;
  }

  .paneTerminal__attach code {
    white-space: nowrap;
  }

  .paneTerminal__screen {
    flex: 1;
    min-height: 240px;
    border-radius: 18px;
    border: 1px solid var(--border);
    background: #05101e;
    padding: 10px;
    overflow: hidden;
  }

  .paneTerminal__screen :global(.xterm) {
    height: 100%;
  }

  .paneTerminal__empty {
    flex-shrink: 0;
    color: var(--text-dim);
  }
</style>
