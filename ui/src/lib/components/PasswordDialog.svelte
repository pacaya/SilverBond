<script lang="ts">
  let {
    open = false,
    title = "Unlock password",
    message = "",
    submitLabel = "Unlock",
    cancelLabel = "Cancel",
    onsubmit,
    oncancel,
  }: {
    open: boolean;
    title?: string;
    message?: string;
    submitLabel?: string;
    cancelLabel?: string;
    onsubmit: (value: string) => void;
    oncancel: () => void;
  } = $props();

  let dialogEl: HTMLDialogElement | undefined = $state();
  let inputEl: HTMLInputElement | undefined = $state();
  let value = $state("");

  $effect(() => {
    if (!dialogEl) return;
    if (open && !dialogEl.open) {
      value = "";
      dialogEl.showModal();
      inputEl?.focus();
    } else if (!open && dialogEl.open) {
      dialogEl.close();
    }
  });

  function submit(event: SubmitEvent) {
    event.preventDefault();
    const secret = value;
    value = "";
    onsubmit(secret);
  }

  function cancel() {
    value = "";
    oncancel();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={dialogEl}
  class="password-dialog"
  aria-label={title}
  onclose={cancel}
  onkeydown={(e) => { if (e.key === "Escape") { e.preventDefault(); cancel(); } }}
>
  <form class="password-dialog__form" onsubmit={submit}>
    <h2 class="password-dialog__title">{title}</h2>
    {#if message}
      <p class="password-dialog__message">{message}</p>
    {/if}
    <label class="password-dialog__label" for="password-dialog-input">Password</label>
    <input
      bind:this={inputEl}
      bind:value
      id="password-dialog-input"
      class="password-dialog__input"
      type="password"
      autocomplete="current-password"
    />
    <div class="password-dialog__actions">
      <button type="button" class="button button--ghost" onclick={cancel}>{cancelLabel}</button>
      <button type="submit" class="button button--primary">{submitLabel}</button>
    </div>
  </form>
</dialog>

<style>
  .password-dialog {
    background: var(--surface-strong);
    color: var(--text);
    border: 1px solid var(--border-strong);
    border-radius: 12px;
    padding: 1.5rem;
    min-width: 340px;
    max-width: 460px;
  }

  .password-dialog::backdrop {
    background: rgba(0, 0, 0, 0.55);
  }

  .password-dialog__title {
    margin: 0 0 0.5rem;
    font-size: 1.1rem;
    font-weight: 600;
    color: var(--text-bright);
  }

  .password-dialog__message {
    margin: 0 0 1rem;
    font-size: 0.92rem;
    color: var(--text-soft);
    line-height: 1.5;
  }

  .password-dialog__label {
    display: block;
    margin-bottom: 0.35rem;
    font-size: 0.8rem;
    color: var(--text-soft);
  }

  .password-dialog__input {
    width: 100%;
    box-sizing: border-box;
    margin-bottom: 1.25rem;
    padding: 0.5rem 0.6rem;
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    font-size: 0.92rem;
  }

  .password-dialog__actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
