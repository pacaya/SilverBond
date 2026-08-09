<script lang="ts">
  let {
    value = [],
    onchange,
  }: {
    value?: string[] | null;
    onchange: (value: string[] | undefined) => void;
  } = $props();
</script>

<label class="field">
  <span>Extra args</span>
  <textarea
    value={(value ?? []).join("\n")}
    placeholder={"--flag\nvalue"}
    onblur={(e) => {
      const args = (e.target as HTMLTextAreaElement).value.split("\n").map((s) => s.trim()).filter(Boolean);
      onchange(args.length ? args : undefined);
    }}
    class="field--shortTextarea"
  ></textarea>
</label>
