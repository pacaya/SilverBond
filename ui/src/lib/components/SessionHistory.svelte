<script lang="ts">
  type Entry = {
    role: "user" | "agent";
    content: string;
    timestamp: string;
  };

  let {
    sessionId,
    visible = false,
    onclose,
  }: {
    sessionId: string;
    visible?: boolean;
    onclose: () => void;
  } = $props();

  let entries = $state<Entry[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    if (!visible || !sessionId) return;

    loading = true;
    error = null;
    entries = [];

    fetch(`/api/sessions/${sessionId}/history`)
      .then((res) => {
        if (!res.ok) throw new Error(`Request failed: ${res.status} ${res.statusText}`);
        return res.json();
      })
      .then((data: Entry[]) => {
        entries = data;
      })
      .catch((err: unknown) => {
        error = err instanceof Error ? err.message : String(err);
      })
      .finally(() => {
        loading = false;
      });
  });

  function formatTimestamp(ts: string): string {
    const d = new Date(ts);
    if (isNaN(d.getTime())) return ts;
    return d.toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }
</script>

{#if visible}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="overlay" onkeydown={(e) => { if (e.key === "Escape") onclose(); }}>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="overlay__backdrop" onclick={onclose}></div>
    <div class="panel" role="dialog" aria-modal="true" aria-label="Session History">
      <div class="panel__header">
        <h2 class="panel__title">Session History</h2>
        <button class="panel__close button button--ghost" onclick={onclose} aria-label="Close">✕</button>
      </div>

      <div class="panel__body">
        {#if loading}
          <div class="state state--loading">
            <span class="spinner" aria-hidden="true"></span>
            <span>Loading...</span>
          </div>
        {:else if error}
          <div class="state state--error">
            <span class="state__icon" aria-hidden="true">!</span>
            <span>{error}</span>
          </div>
        {:else if entries.length === 0}
          <div class="state state--empty">No entries yet.</div>
        {:else}
          <ul class="entries">
            {#each entries as entry (entry.timestamp + entry.role)}
              <li class="entry entry--{entry.role}">
                <div class="entry__meta">
                  <span class="entry__badge entry__badge--{entry.role}">
                    {entry.role === "user" ? "You" : "Agent"}
                  </span>
                  <time class="entry__time" datetime={entry.timestamp}>
                    {formatTimestamp(entry.timestamp)}
                  </time>
                </div>
                <pre class="entry__content">{entry.content}</pre>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: flex-start;
    justify-content: flex-end;
  }

  .overlay__backdrop {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
  }

  .panel {
    position: relative;
    z-index: 1;
    background: var(--surface-strong);
    color: var(--text);
    border-left: 1px solid var(--border-strong);
    width: 480px;
    max-width: 100vw;
    height: 100vh;
    display: flex;
    flex-direction: column;
    box-shadow: -4px 0 24px rgba(0, 0, 0, 0.3);
  }

  .panel__header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 1rem 1.25rem;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .panel__title {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
    color: var(--text-bright);
  }

  .panel__close {
    font-size: 1rem;
    line-height: 1;
    padding: 0.25rem 0.5rem;
  }

  .panel__body {
    flex: 1;
    overflow-y: auto;
    padding: 1rem;
  }

  .state {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    justify-content: center;
    padding: 2rem 1rem;
    font-size: 0.9rem;
    color: var(--text-soft);
  }

  .state--error {
    color: var(--color-error, #e05252);
  }

  .state__icon {
    font-weight: 700;
    font-size: 1.1rem;
  }

  .spinner {
    display: inline-block;
    width: 1rem;
    height: 1rem;
    border: 2px solid var(--border);
    border-top-color: var(--text-soft);
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .entries {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .entry {
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
  }

  .entry__meta {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .entry__badge {
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0.15rem 0.5rem;
    border-radius: 4px;
  }

  .entry__badge--user {
    background: var(--color-accent, #3b82f6);
    color: #fff;
  }

  .entry__badge--agent {
    background: var(--surface-muted, #3a3a3a);
    color: var(--text-soft);
    border: 1px solid var(--border);
  }

  .entry__time {
    font-size: 0.78rem;
    color: var(--text-soft);
  }

  .entry__content {
    margin: 0;
    padding: 0.625rem 0.75rem;
    background: var(--surface-muted, rgba(255, 255, 255, 0.05));
    border: 1px solid var(--border);
    border-radius: 6px;
    font-family: ui-monospace, "SFMono-Regular", Consolas, monospace;
    font-size: 0.82rem;
    line-height: 1.55;
    color: var(--text);
    white-space: pre-wrap;
    word-break: break-word;
    overflow-x: auto;
  }
</style>
