<script lang="ts">
  import { store } from "@/lib/stores/workflowStore.svelte";
  import type { WorkflowDocument } from "@/lib/types/workflow";

  let {
    workflow,
    isRunning,
    onRun,
    onAbort,
    onApproval,
  }: {
    workflow: WorkflowDocument | null;
    isRunning: boolean;
    onRun: () => void;
    onAbort: () => void;
    onApproval: (approved: boolean, userInput: string) => void;
  } = $props();

  let approvalText = $state("");
  let logContainer: HTMLDivElement | undefined = $state();
  let approvalCard: HTMLDivElement | undefined = $state();

  /* auto-scroll log panel to bottom */
  $effect(() => {
    const _len = store.lines.length;
    if (logContainer) {
      logContainer.scrollTop = logContainer.scrollHeight;
    }
  });

  /* auto-scroll approval card into view when it appears */
  $effect(() => {
    if (store.approval && approvalCard) {
      approvalCard.scrollIntoView({ behavior: "smooth" });
    }
  });
</script>

<div class="runPanel">
  <div class="runPanel__toolbar">
    <div>
      <small>Runtime</small>
      <h3>{workflow?.name || "No workflow loaded"}</h3>
    </div>
    <div class="runPanel__actions">
      <button class="button button--primary" data-testid="run-workflow" onclick={onRun} disabled={!workflow || isRunning}>
        {isRunning ? "Running" : "Run"}
      </button>
      <button class="button button--ghost" onclick={onAbort} disabled={!isRunning}>
        Abort
      </button>
    </div>
  </div>

  <div class="runPanel__log" bind:this={logContainer}>
    {#if store.lines.length === 0}
      <div class="runPanel__empty">Press Run to execute and see output here.</div>
    {:else}
      {#each store.lines as line, index (`${line.text}-${index}`)}
        <div class="logLine logLine--{line.tone}">
          {line.text}
        </div>
      {/each}
    {/if}
  </div>

  {#if store.approval}
    <div class="approvalCard" bind:this={approvalCard}>
      <div class="approvalCard__header">
        <strong>{store.approval.nodeName}</strong>
        <span>Approval required</span>
      </div>
      <p>{store.approval.prompt}</p>
      <pre>{store.approval.lastOutput || "(no previous output)"}</pre>
      <textarea
        bind:value={approvalText}
        placeholder="Optional input or notes for the next node"
      ></textarea>
      <div class="approvalCard__actions">
        <button
          class="button button--ghost"
          onclick={() => {
            onApproval(false, approvalText);
            approvalText = "";
          }}
        >
          Reject
        </button>
        <button
          class="button button--primary"
          data-testid="approve-run"
          onclick={() => {
            onApproval(true, approvalText);
            approvalText = "";
          }}
        >
          Approve
        </button>
      </div>
    </div>
  {/if}
</div>
