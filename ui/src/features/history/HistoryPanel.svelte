<script lang="ts">
  import { createQuery } from "@tanstack/svelte-query";
  import { api } from "@/lib/api/client";
  import type { NodeExecutionEntry } from "@/lib/types/workflow";
  import { formatTokens } from "@/lib/utils/format";

  let {
    onResume,
    onRestart,
    onDismiss,
  }: {
    onResume: (runId: string) => void;
    onRestart: (runId: string, nodeId: string) => void;
    onDismiss: (runId: string) => void;
  } = $props();

  let selectedLogId = $state<string | null>(null);

  const interruptedRuns = createQuery(() => ({ queryKey: ["interrupted-runs"], queryFn: api.interruptedRuns }));
  const logs = createQuery(() => ({ queryKey: ["logs"], queryFn: api.logs }));
  const selectedLog = createQuery(() => ({
    queryKey: ["log", selectedLogId],
    queryFn: () => api.log(selectedLogId!),
    enabled: Boolean(selectedLogId),
  }));

  function timeAgo(dateStr: string): string {
    const now = Date.now();
    const then = new Date(dateStr).getTime();
    const diff = Math.max(0, now - then);
    const seconds = Math.floor(diff / 1000);
    if (seconds < 60) return "just now";
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes} min ago`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    return `${days}d ago`;
  }

  function outcomeBadgeClass(entry: NodeExecutionEntry): string {
    if (entry.success) return "metaBadge--success";
    if (entry.outcome === "error_max_turns") return "metaBadge--warning";
    if (entry.outcome === "error_max_budget") return "metaBadge--warning";
    return "metaBadge--error";
  }

  function outcomeLabel(entry: NodeExecutionEntry): string {
    if (entry.success) return "success";
    if (entry.errorType) return String(entry.errorType);
    return "failed";
  }

  let interruptedRunItems = $derived(interruptedRuns.data ?? []);
  let completedLogs = $derived(logs.data ?? []);
  let selectedLogData = $derived(selectedLog.data ?? null);
  let executionEntries = $derived((selectedLogData?.nodeExecutions ?? []) as NodeExecutionEntry[]);

  let runSummary = $derived.by(() => {
    if (!executionEntries.length) return null;
    let totalCost = 0;
    let hasCost = false;
    let totalInput = 0;
    let totalOutput = 0;
    let hasTokens = false;
    let succeeded = 0;
    let failed = 0;
    for (const e of executionEntries) {
      if (typeof e.costUsd === "number") { totalCost += e.costUsd; hasCost = true; }
      if (typeof e.inputTokens === "number") { totalInput += e.inputTokens; hasTokens = true; }
      if (typeof e.outputTokens === "number") { totalOutput += e.outputTokens; hasTokens = true; }
      if (e.success) succeeded++; else failed++;
    }
    return { totalCost, hasCost, totalInput, totalOutput, hasTokens, succeeded, failed };
  });
</script>

<div class="historyPanel">
  {#if selectedLogId && selectedLogData}
    <div class="historyDetail">
      <button class="button button--ghost" onclick={() => { selectedLogId = null; }}>
        Back
      </button>
      <h3>{selectedLogData.workflowName}</h3>
      <p>
        {new Date(selectedLogData.startTime).toLocaleString()} &middot; {selectedLogData.totalDuration}s &middot;
        {selectedLogData.aborted ? "aborted" : "completed"}
      </p>

      {#if runSummary}
        <div class="runSummary">
          <span class="runSummary__item">
            <span class="metaBadge metaBadge--success">{runSummary.succeeded}</span>
            {#if runSummary.failed > 0}
              <span class="metaBadge metaBadge--error">{runSummary.failed}</span>
            {/if}
            nodes
          </span>
          {#if runSummary.hasTokens}
            <span class="runSummary__item">{formatTokens(runSummary.totalInput)}→{formatTokens(runSummary.totalOutput)} tok</span>
          {/if}
          {#if runSummary.hasCost}
            <span class="runSummary__item">${runSummary.totalCost.toFixed(4)}</span>
          {/if}
        </div>
      {/if}

      <div class="historyDetail__section">
        <div class="inspectorSection__title">Node executions</div>
        {#each executionEntries as entry, index (`${String(entry.nodeId ?? "")}-${index}`)}
          {@const nodeId = String(entry.nodeId ?? "")}
          <div class="historyExecution">
            <div class="historyExecution__header">
              <strong>{String(entry.nodeName ?? nodeId)}</strong>
              <span class="metaBadge {outcomeBadgeClass(entry)}">{outcomeLabel(entry)}</span>
              {#if selectedLogData.runId && nodeId}
                <button
                  class="button button--ghost"
                  onclick={() => onRestart(selectedLogData.runId!, nodeId)}
                >
                  Restart here
                </button>
              {/if}
            </div>
            <div class="historyExecution__meta">
              {#if entry.duration}<span>{entry.duration}s</span>{/if}
              {#if entry.modelUsed}<span>{entry.modelUsed}</span>{/if}
              {#if entry.agent && entry.agent !== "system" && entry.agent !== "user"}<span>{entry.agent}</span>{/if}
              {#if typeof entry.inputTokens === "number" || typeof entry.outputTokens === "number"}
                <span>{formatTokens(entry.inputTokens ?? 0)}→{formatTokens(entry.outputTokens ?? 0)} tok</span>
              {/if}
              {#if typeof entry.thinkingTokens === "number" && entry.thinkingTokens > 0}
                <span>{formatTokens(entry.thinkingTokens)} think</span>
              {/if}
              {#if typeof entry.cacheReadTokens === "number" && entry.cacheReadTokens > 0}
                <span>{formatTokens(entry.cacheReadTokens)} cached</span>
              {/if}
              {#if typeof entry.costUsd === "number"}
                <span>${entry.costUsd.toFixed(4)}</span>
              {/if}
              {#if typeof entry.numTurns === "number" && entry.numTurns > 1}
                <span>{entry.numTurns} turns</span>
              {/if}
              {#if entry.agentSessionId}
                <span class="historyExecution__sessionId" title="Click to copy session ID">
                  <button class="historyExecution__copyBtn" onclick={() => navigator.clipboard.writeText(entry.agentSessionId!)}>
                    {entry.agentSessionId.slice(0, 8)}...
                  </button>
                </span>
              {/if}
            </div>
            <pre>{String(entry.output ?? "") || "(empty)"}</pre>
          </div>
        {/each}
      </div>
    </div>
  {:else}
    <div class="historyPanel__section">
      <div class="inspectorSection__title">Interrupted runs</div>
      {#if interruptedRunItems.length}
        {#each interruptedRunItems as run (run.runId)}
          <div class="historyCard">
            <strong>{run.workflowName}</strong>
            <span>{run.currentNodeName || "Paused"}</span>
            <div class="historyCard__actions">
              <button class="button button--ghost" onclick={() => onResume(run.runId)}>
                Resume
              </button>
              {#if run.currentNodeId}
                <button
                  class="button button--ghost"
                  onclick={() => onRestart(run.runId, run.currentNodeId!)}
                >
                  Restart node
                </button>
              {/if}
              <button class="button button--danger" onclick={() => onDismiss(run.runId)}>
                Dismiss
              </button>
            </div>
          </div>
        {/each}
      {:else}
        <div class="sidebar__empty">No interrupted runs right now.</div>
      {/if}
    </div>

    <div class="historyPanel__section">
      <div class="inspectorSection__title">Completed runs</div>
      {#if completedLogs.length}
        {#each completedLogs as log (log.id)}
          <button class="historyCard historyCard--button" data-testid="completed-run-card" onclick={() => { selectedLogId = log.id; }} title={new Date(log.startTime).toLocaleString()}>
            <strong>{log.workflowName}</strong>
            <span>
              {timeAgo(log.startTime)} &middot; {log.totalDuration}s &middot;
              {log.aborted ? "aborted" : "completed"}
              {#if typeof log.totalCostUsd === "number"}
                &middot; ${log.totalCostUsd.toFixed(4)}
              {/if}
              {#if typeof log.totalInputTokens === "number"}
                &middot; {formatTokens(log.totalInputTokens)} tok in
              {/if}
            </span>
          </button>
        {/each}
      {:else}
        <div class="sidebar__empty">Run a workflow to see its history here.</div>
      {/if}
    </div>
  {/if}
</div>
