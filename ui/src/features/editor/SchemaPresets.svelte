<script lang="ts">
  let {
    value,
    onchange,
  }: {
    value: Record<string, unknown> | null;
    onchange: (val: Record<string, unknown> | null) => void;
  } = $props();

  let useCustom = $state(false);
  let jsonDraft = $state("");
  let jsonError = $state("");

  const presets: Array<{ label: string; description: string; schema: Record<string, unknown>; json: string }> = [
    {
      label: "Key-value object",
      description: "Object with string key-value pairs",
      schema: {
        type: "object",
        properties: { key: { type: "string" }, value: { type: "string" } },
        required: ["key", "value"],
      },
      json: '{"type":"object","properties":{"key":{"type":"string"},"value":{"type":"string"}},"required":["key","value"]}',
    },
    {
      label: "Status + message",
      description: "Status string and message",
      schema: {
        type: "object",
        properties: {
          status: { type: "string", enum: ["success", "error", "pending"] },
          message: { type: "string" },
        },
        required: ["status", "message"],
      },
      json: '{"type":"object","properties":{"status":{"type":"string","enum":["success","error","pending"]},"message":{"type":"string"}},"required":["status","message"]}',
    },
    {
      label: "List of items",
      description: "Array of string items",
      schema: {
        type: "object",
        properties: { items: { type: "array", items: { type: "string" } } },
        required: ["items"],
      },
      json: '{"type":"object","properties":{"items":{"type":"array","items":{"type":"string"}}},"required":["items"]}',
    },
  ];

  // Sync from prop
  $effect(() => {
    jsonDraft = value ? JSON.stringify(value, null, 2) : "";
    if (value) {
      const valueJson = JSON.stringify(value);
      const match = presets.some((p) => p.json === valueJson);
      useCustom = !match;
    } else {
      useCustom = false;
    }
  });

  function applyPreset(schema: Record<string, unknown>) {
    onchange(schema);
    useCustom = false;
  }

  function clearSchema() {
    onchange(null);
    useCustom = false;
    jsonDraft = "";
    jsonError = "";
  }

  function commitJson() {
    const d = jsonDraft.trim();
    if (!d) { onchange(null); jsonError = ""; return; }
    try {
      onchange(JSON.parse(d));
      jsonError = "";
    } catch (err) {
      jsonError = err instanceof Error ? err.message : "Invalid JSON";
    }
  }

  let valueJson = $derived(value ? JSON.stringify(value) : "");

  function isActivePreset(presetJson: string): boolean {
    return valueJson === presetJson;
  }
</script>

<div class="schemaPresets">
  {#if useCustom}
    <textarea
      class="schemaPresets__editor"
      value={jsonDraft}
      placeholder={'{"type":"object","properties":{"field":{"type":"string"}},"required":["field"]}'}
      oninput={(e) => { jsonDraft = (e.target as HTMLTextAreaElement).value; jsonError = ""; }}
      onblur={commitJson}
    ></textarea>
    {#if jsonError}<small class="issue issue--error">{jsonError}</small>{/if}
    <div class="schemaPresets__actions">
      <button class="linkBtn" onclick={() => useCustom = false}>
        Use preset
      </button>
      <button class="linkBtn" onclick={clearSchema}>
        Clear
      </button>
    </div>
  {:else}
    <div class="schemaPresets__grid">
      {#each presets as preset (preset.label)}
        <button
          class="schemaPresets__preset"
          class:schemaPresets__preset--active={isActivePreset(preset.json)}
          onclick={() => applyPreset(preset.schema)}
        >
          <span class="schemaPresets__presetLabel">{preset.label}</span>
          <small class="schemaPresets__presetDesc">{preset.description}</small>
        </button>
      {/each}
    </div>
    <div class="schemaPresets__actions">
      <button class="linkBtn" onclick={() => { useCustom = true; jsonDraft = value ? JSON.stringify(value, null, 2) : ""; }}>
        Edit as JSON
      </button>
      {#if value}
        <button class="linkBtn" onclick={clearSchema}>
          Clear
        </button>
      {/if}
    </div>
  {/if}
</div>
