<script lang="ts">
  let {
    value = "",
    onchange,
  }: {
    value?: string | null;
    onchange: (value: string | undefined) => void;
  } = $props();
</script>

<label class="field">
  <span>Pane name</span>
  <input
    value={value ?? ""}
    placeholder="auto"
    onblur={(e) => onchange((e.target as HTMLInputElement).value || undefined)}
  />
</label>
