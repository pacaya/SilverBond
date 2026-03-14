<script lang="ts">
  let {
    open = false,
    title = "Confirm",
    message = "Are you sure?",
    confirmLabel = "Continue",
    cancelLabel = "Cancel",
    onconfirm,
    oncancel,
  }: {
    open: boolean;
    title?: string;
    message?: string;
    confirmLabel?: string;
    cancelLabel?: string;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();

  let dialogEl: HTMLDialogElement | undefined = $state();

  $effect(() => {
    if (!dialogEl) return;
    if (open && !dialogEl.open) {
      dialogEl.showModal();
    } else if (!open && dialogEl.open) {
      dialogEl.close();
    }
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={dialogEl}
  class="confirm-dialog"
  onclose={oncancel}
  onkeydown={(e) => { if (e.key === "Escape") { e.preventDefault(); oncancel(); } }}
>
  <h2 class="confirm-dialog__title">{title}</h2>
  <p class="confirm-dialog__message">{message}</p>
  <div class="confirm-dialog__actions">
    <button class="button button--ghost" onclick={oncancel}>{cancelLabel}</button>
    <button class="button button--primary" onclick={onconfirm}>{confirmLabel}</button>
  </div>
</dialog>

<style>
  .confirm-dialog {
    background: var(--surface-strong);
    color: var(--text);
    border: 1px solid var(--border-strong);
    border-radius: 12px;
    padding: 1.5rem;
    min-width: 340px;
    max-width: 460px;
  }

  .confirm-dialog::backdrop {
    background: rgba(0, 0, 0, 0.55);
  }

  .confirm-dialog__title {
    margin: 0 0 0.5rem;
    font-size: 1.1rem;
    font-weight: 600;
    color: var(--text-bright);
  }

  .confirm-dialog__message {
    margin: 0 0 1.25rem;
    font-size: 0.92rem;
    color: var(--text-soft);
    line-height: 1.5;
  }

  .confirm-dialog__actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
