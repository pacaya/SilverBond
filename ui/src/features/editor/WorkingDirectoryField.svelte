<script lang="ts">
  let {
    value = "",
    placeholder,
    onchange,
  }: {
    value?: string | null;
    placeholder: string;
    onchange: (value: string | undefined) => void;
  } = $props();
</script>

<label class="field">
  <span>Working directory</span>
  <input
    value={value ?? ""}
    {placeholder}
    onblur={(e) => onchange((e.target as HTMLInputElement).value || undefined)}
  />
</label>
