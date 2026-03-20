<script lang="ts">
  import type { StructuredCondition, SkipCondition } from "@/lib/types/workflow";

  type ConditionMode = "structured" | "skip";

  let {
    mode,
    value,
    onchange,
  }: {
    mode: ConditionMode;
    value: StructuredCondition | SkipCondition | null;
    onchange: (val: StructuredCondition | SkipCondition | null) => void;
  } = $props();

  let useRawJson = $state(false);
  let rawDraft = $state("");
  let rawError = $state("");

  // Structured fields for StructuredCondition
  let sField = $state("");
  let sOperator = $state("==");
  let sValue = $state("");

  // Structured fields for SkipCondition
  let skSource = $state("previous_output");
  let skType = $state("contains");
  let skValue = $state("");

  const structuredOperators = ["==", "!=", ">", "<", ">=", "<=", "contains", "not_contains"];
  const skipTypes = ["contains", "not_contains", "equals", "not_equals", "empty", "not_empty"];
  const skipSources = ["previous_output", "context", "variable"];

  // Sync from prop value to local state
  $effect(() => {
    if (mode === "structured" && value) {
      const v = value as StructuredCondition;
      sField = v.field ?? "";
      sOperator = v.operator ?? "==";
      sValue = v.value ?? "";
    } else if (mode === "skip" && value) {
      const v = value as SkipCondition;
      skSource = v.source ?? "previous_output";
      skType = v.type ?? "contains";
      skValue = v.value ?? "";
    }
    if (!useRawJson) {
      rawDraft = value ? JSON.stringify(value, null, 2) : "";
    }
  });

  function commitStructured() {
    let next: StructuredCondition | SkipCondition | null;
    if (mode === "structured") {
      next = (!sField && !sValue) ? null : { field: sField, operator: sOperator, value: sValue };
    } else {
      next = (!skValue && skType !== "empty" && skType !== "not_empty") ? null : { source: skSource, type: skType, value: skValue };
    }
    if (JSON.stringify(next) !== JSON.stringify(value)) {
      onchange(next);
    }
  }

  function commitRaw() {
    const d = rawDraft.trim();
    if (!d) { onchange(null); rawError = ""; return; }
    try {
      const parsed = JSON.parse(d);
      onchange(parsed);
      rawError = "";
    } catch (err) {
      rawError = err instanceof Error ? err.message : "Invalid JSON";
    }
  }
</script>

<div class="conditionBuilder">
  {#if useRawJson}
    <textarea
      class="conditionBuilder__raw"
      value={rawDraft}
      placeholder={mode === "structured"
        ? '{"field":"status","operator":"==","value":"done"}'
        : '{"source":"previous_output","type":"contains","value":"skip"}'}
      oninput={(e) => { rawDraft = (e.target as HTMLTextAreaElement).value; rawError = ""; }}
      onblur={commitRaw}
    ></textarea>
    {#if rawError}<small class="issue issue--error">{rawError}</small>{/if}
    <button class="linkBtn" onclick={() => { useRawJson = false; }}>
      Use builder
    </button>
  {:else}
    {#if mode === "structured"}
      <div class="conditionBuilder__row">
        <label class="conditionBuilder__field">
          <small>Field</small>
          <input
            value={sField}
            placeholder="status"
            oninput={(e) => sField = (e.target as HTMLInputElement).value}
            onblur={commitStructured}
          />
        </label>
        <label class="conditionBuilder__field conditionBuilder__field--narrow">
          <small>Operator</small>
          <select value={sOperator} onchange={(e) => { sOperator = (e.target as HTMLSelectElement).value; commitStructured(); }}>
            {#each structuredOperators as op (op)}
              <option value={op}>{op}</option>
            {/each}
          </select>
        </label>
        <label class="conditionBuilder__field">
          <small>Value</small>
          <input
            value={sValue}
            placeholder="done"
            oninput={(e) => sValue = (e.target as HTMLInputElement).value}
            onblur={commitStructured}
          />
        </label>
      </div>
    {:else}
      <div class="conditionBuilder__row">
        <label class="conditionBuilder__field">
          <small>Source</small>
          <select value={skSource} onchange={(e) => { skSource = (e.target as HTMLSelectElement).value; commitStructured(); }}>
            {#each skipSources as src (src)}
              <option value={src}>{src}</option>
            {/each}
          </select>
        </label>
        <label class="conditionBuilder__field">
          <small>Type</small>
          <select value={skType} onchange={(e) => { skType = (e.target as HTMLSelectElement).value; commitStructured(); }}>
            {#each skipTypes as t (t)}
              <option value={t}>{t}</option>
            {/each}
          </select>
        </label>
        <label class="conditionBuilder__field">
          <small>Value</small>
          <input
            value={skValue}
            placeholder="skip"
            oninput={(e) => skValue = (e.target as HTMLInputElement).value}
            onblur={commitStructured}
          />
        </label>
      </div>
    {/if}
    <button class="linkBtn" onclick={() => { useRawJson = true; rawDraft = value ? JSON.stringify(value, null, 2) : ""; }}>
      Raw JSON
    </button>
  {/if}
</div>
