<script module lang="ts">
  import type { AccessMode } from "@/lib/types/workflow";

  const accessModeDescriptions: Record<AccessMode, string> = {
    read_only: "Observe/analyze only. No file edits or shell commands.",
    edit: "Read and edit files. No shell command execution.",
    execute: "Edit files + run commands. Sandboxed to workspace.",
    unrestricted: "Full system + network access. For installs and deployments.",
  };

  const ACCESS_MODE_LABELS: Record<AccessMode, string> = {
    read_only: "read_only",
    edit: "edit",
    execute: "execute (default)",
    unrestricted: "unrestricted",
  };
</script>

<script lang="ts">
  import type {
    AgentCapabilities,
    AgentDefaults,
    ReasoningLevel,
  } from "@/lib/types/workflow";
  import { supportedAccessModes } from "./supportedAccessModes";

  let {
    caps,
    accessProfiles,
    values,
    update,
    placeholders,
  }: {
    caps: AgentCapabilities;
    accessProfiles: string[] | undefined;
    values: AgentDefaults;
    update: <K extends keyof AgentDefaults>(key: K, value: AgentDefaults[K]) => void;
    placeholders: { model: string; systemPrompt: string };
  } = $props();

  const availableModes = $derived(supportedAccessModes(accessProfiles));
  const currentMode = $derived(values.accessMode ?? "execute");
  const effectiveMode = $derived(
    availableModes.includes(currentMode) ? currentMode : availableModes[0],
  );
</script>

<label class="field">
  <span>Access mode</span>
  {#if effectiveMode}
    <select
      value={effectiveMode}
      onchange={(e) => {
        const val = (e.target as HTMLSelectElement).value as AccessMode;
        update("accessMode", val === "execute" ? undefined : val);
      }}
    >
      {#each availableModes as mode (mode)}
        <option value={mode}>{ACCESS_MODE_LABELS[mode]}</option>
      {/each}
    </select>
    <small class="helperText" style="margin-top: -4px;">
      {accessModeDescriptions[effectiveMode]}
    </small>
  {:else}
    <select disabled><option>No access modes available</option></select>
    <small class="issue issue--error">Agent has no supported access profiles.</small>
  {/if}
</label>

{#if caps.modelSelection}
  <label class="field">
    <span>Model</span>
    <input
      value={values.model ?? ""}
      placeholder={placeholders.model}
      onblur={(e) => update("model", (e.target as HTMLInputElement).value || undefined)}
    />
  </label>
{/if}

{#if caps.reasoningConfig}
  <label class="field">
    <span>Reasoning level</span>
    <select
      value={values.reasoningLevel ?? ""}
      onchange={(e) => {
        const val = (e.target as HTMLSelectElement).value as ReasoningLevel | "";
        update("reasoningLevel", val || undefined);
      }}
    >
      <option value="">agent default</option>
      <option value="low">low</option>
      <option value="medium">medium</option>
      <option value="high">high</option>
    </select>
  </label>
{/if}

{#if caps.systemPrompt}
  <label class="field">
    <span>System prompt</span>
    <textarea
      value={values.systemPrompt ?? ""}
      placeholder={placeholders.systemPrompt}
      onblur={(e) => update("systemPrompt", (e.target as HTMLTextAreaElement).value || undefined)}
      class="field--shortTextarea"
    ></textarea>
  </label>
{/if}

{#if caps.turnLimit}
  <label class="field field--split">
    <span>Max turns</span>
    <input
      type="number"
      value={values.maxTurns ?? ""}
      placeholder="default"
      oninput={(e) => {
        const val = (e.target as HTMLInputElement).value;
        update("maxTurns", val !== "" ? Number(val) : undefined);
      }}
    />
  </label>
{/if}

{#if caps.budgetLimit}
  <label class="field field--split">
    <span>Max budget (USD)</span>
    <input
      type="number"
      step="0.01"
      value={values.maxBudgetUsd ?? ""}
      placeholder="default"
      oninput={(e) => {
        const val = (e.target as HTMLInputElement).value;
        update("maxBudgetUsd", val !== "" ? Number(val) : undefined);
      }}
    />
  </label>
{/if}

{#if caps.webSearch}
  <label class="field toggle-field">
    <span>Web search</span>
    <input
      type="checkbox"
      class="toggle"
      checked={values.toolToggles?.webSearch ?? false}
      onchange={(e) => {
        const checked = (e.target as HTMLInputElement).checked;
        update("toolToggles", checked ? { webSearch: true } : undefined);
      }}
    />
  </label>
{/if}
