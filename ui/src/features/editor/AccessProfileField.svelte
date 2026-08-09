<script lang="ts">
  let {
    value = "",
    profiles = [],
    onchange,
  }: {
    value?: string | null;
    profiles?: string[];
    onchange: (value: string | undefined) => void;
  } = $props();
</script>

<label class="field">
  <span>Access</span>
  <select
    value={value ?? ""}
    onchange={(e) => onchange((e.target as HTMLSelectElement).value || undefined)}
  >
    <option value="">registry default</option>
    {#each profiles ?? [] as profile (profile)}
      <option value={profile}>{profile}</option>
    {/each}
  </select>
</label>
