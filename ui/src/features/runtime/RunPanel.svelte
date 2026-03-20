<script lang="ts">
  import { store } from "@/lib/stores/workflowStore.svelte";
  import type { WorkflowDocument } from "@/lib/types/workflow";

  let {
    workflow,
    isRunning,
    onRun,
    onAbort,
    onApproval,
    onInteractionResponse,
  }: {
    workflow: WorkflowDocument | null;
    isRunning: boolean;
    onRun: () => void;
    onAbort: () => void;
    onApproval: (approved: boolean, userInput: string) => void;
    onInteractionResponse: (response: string) => void;
  } = $props();

  let approvalText = $state("");
  let interactionText = $state("");
  let logContainer: HTMLDivElement | undefined = $state();
  let approvalCard: HTMLDivElement | undefined = $state();
  let interactionCard: HTMLDivElement | undefined = $state();

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

  /* auto-scroll interaction card into view when it appears */
  $effect(() => {
    if (store.interaction && interactionCard) {
      interactionCard.scrollIntoView({ behavior: "smooth" });
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

  {#if store.interaction}
    <div class="interactionCard" bind:this={interactionCard}>
      <div class="interactionCard__header">
        {#if store.interaction.interactionType === "destructive_warning"}
          <strong class="interactionCard__warning">Destructive Action Detected</strong>
        {:else if store.interaction.interactionType === "permission"}
          <strong>Permission Request</strong>
        {:else}
          <strong>Agent Question</strong>
        {/if}
      </div>
      <p>{store.interaction.description}</p>
      {#if store.interaction.outputSoFar}
        <pre>{store.interaction.outputSoFar}</pre>
      {/if}
      {#if store.interaction.interactionType === "question"}
        <textarea
          bind:value={interactionText}
          placeholder="Type your response..."
        ></textarea>
        <div class="interactionCard__actions">
          <button
            class="button button--primary"
            onclick={() => {
              onInteractionResponse(interactionText);
              interactionText = "";
            }}
          >
            Submit
          </button>
        </div>
      {:else}
        <div class="interactionCard__actions">
          <button
            class="button button--ghost"
            onclick={() => {
              onInteractionResponse("n");
            }}
          >
            Reject
          </button>
          <button
            class="button button--primary"
            onclick={() => {
              onInteractionResponse("y");
            }}
          >
            {store.interaction.interactionType === "destructive_warning" ? "Confirm" : "Approve"}
          </button>
        </div>
      {/if}
    </div>
  {/if}
</div>
