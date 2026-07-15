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
    onInteractionResponse: (sessionId: string, response: string) => void;
  } = $props();

  let approvalText = $state("");
  /* Per-interaction draft answers, keyed by sessionId, so concurrent prompts
   * don't share one input box (and answers don't bleed between cards). */
  let interactionTexts = $state<Record<string, string>>({});
  let logContainer: HTMLDivElement | undefined = $state();
  let approvalCard: HTMLDivElement | undefined = $state();
  let interactionsContainer: HTMLDivElement | undefined = $state();
  let stickToBottom = $state(true);

  const SCROLL_BOTTOM_THRESHOLD = 32;

  function handleLogScroll() {
    if (!logContainer) return;
    const distance =
      logContainer.scrollHeight - logContainer.scrollTop - logContainer.clientHeight;
    stickToBottom = distance <= SCROLL_BOTTOM_THRESHOLD;
  }

  /* auto-scroll log panel to bottom when the user is already at the bottom */
  $effect(() => {
    const _len = store.lines.length;
    if (logContainer && stickToBottom) {
      logContainer.scrollTop = logContainer.scrollHeight;
    }
  });

  /* auto-scroll approval/interaction card into view when it appears */
  $effect(() => {
    const el = (store.approval && approvalCard) ? approvalCard
             : (store.interactions.length && interactionsContainer) ? interactionsContainer
             : null;
    el?.scrollIntoView({ behavior: "smooth" });
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

  <div class="runPanel__log" bind:this={logContainer} onscroll={handleLogScroll}>
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

  {#if store.interactions.length > 0}
    <div class="interactions" bind:this={interactionsContainer}>
      {#if store.interactions.length > 1}
        <div class="interactions__count">
          {store.interactions.length} agents are waiting for a response — answer in any order.
        </div>
      {/if}
      {#each store.interactions as interaction (interaction.sessionId)}
        <div class="interactionCard">
          <div class="interactionCard__header">
            {#if interaction.interactionType === "destructive_warning"}
              <strong class="interactionCard__warning">Destructive Action Detected</strong>
            {:else if interaction.interactionType === "permission"}
              <strong>Permission Request</strong>
            {:else}
              <strong>Agent Question</strong>
            {/if}
          </div>
          <p>{interaction.description}</p>
          {#if interaction.outputSoFar}
            <pre>{interaction.outputSoFar}</pre>
          {/if}
          {#if interaction.interactionType === "question"}
            <textarea
              bind:value={interactionTexts[interaction.sessionId]}
              placeholder="Type your response..."
            ></textarea>
            <div class="interactionCard__actions">
              <button
                class="button button--primary"
                onclick={() => {
                  onInteractionResponse(interaction.sessionId, interactionTexts[interaction.sessionId] ?? "");
                  delete interactionTexts[interaction.sessionId];
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
                  onInteractionResponse(interaction.sessionId, "n");
                }}
              >
                Reject
              </button>
              <button
                class="button button--primary"
                onclick={() => {
                  onInteractionResponse(interaction.sessionId, "y");
                }}
              >
                {interaction.interactionType === "destructive_warning" ? "Confirm" : "Approve"}
              </button>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
