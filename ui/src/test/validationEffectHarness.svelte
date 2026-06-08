<script lang="ts">
  import { store } from "@/lib/stores/workflowStore.svelte";
  import { ensureCanvas } from "@/lib/types/workflow";
  import type { WorkflowDocument } from "@/lib/types/workflow";

  let {
    onValidate,
    debounceMs = 500,
  }: {
    onValidate: (wf: WorkflowDocument) => void;
    debounceMs?: number;
  } = $props();

  let validateTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    const snap = $state.snapshot(store.workflow);
    if (!snap) return;
    if (validateTimer) clearTimeout(validateTimer);
    validateTimer = setTimeout(() => {
      onValidate(ensureCanvas(snap as WorkflowDocument));
    }, debounceMs);
    return () => {
      if (validateTimer) clearTimeout(validateTimer);
    };
  });
</script>
