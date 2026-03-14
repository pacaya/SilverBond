<script lang="ts">
  import AutocompletePopup from "./AutocompletePopup.svelte";
  import { getCaretCoordinates } from "@/lib/utils/caretPosition";
  import type { AutocompleteSuggestion } from "@/lib/utils/templateSuggestions";

  let {
    value,
    oninput,
    suggestions,
  }: {
    value: string;
    oninput: (e: Event) => void;
    suggestions: AutocompleteSuggestion[];
  } = $props();

  let textareaEl: HTMLTextAreaElement | undefined = $state();
  let popupVisible = $state(false);
  let popupPosition = $state({ top: 0, left: 0 });
  let selectedIndex = $state(0);
  let filteredItems = $state<AutocompleteSuggestion[]>([]);
  let triggerStart = $state(-1);

  function getPartialToken(): string | null {
    if (!textareaEl) return null;
    const pos = textareaEl.selectionStart;
    const text = textareaEl.value.substring(0, pos);
    const idx = text.lastIndexOf("{{");
    if (idx === -1) return null;
    // Make sure there's no closing }} between the {{ and cursor
    const between = text.substring(idx + 2);
    if (between.includes("}}")) return null;
    triggerStart = idx;
    return between;
  }

  function updateSuggestions() {
    const partial = getPartialToken();
    if (partial === null) {
      popupVisible = false;
      return;
    }
    const lower = partial.toLowerCase();
    filteredItems = suggestions.filter(
      (s) =>
        s.label.toLowerCase().includes(lower) ||
        s.insertText.toLowerCase().includes(`{{${lower}`),
    );
    selectedIndex = 0;
    if (filteredItems.length === 0) {
      popupVisible = false;
      return;
    }
    // Position popup
    if (textareaEl) {
      const coords = getCaretCoordinates(textareaEl, textareaEl.selectionStart);
      popupPosition = {
        top: coords.top + 24,
        left: Math.min(coords.left, 200),
      };
    }
    popupVisible = true;
  }

  function insertSuggestion(item: AutocompleteSuggestion) {
    if (!textareaEl || triggerStart === -1) return;
    const before = textareaEl.value.substring(0, triggerStart);
    const after = textareaEl.value.substring(textareaEl.selectionStart);
    const newValue = before + item.insertText + after;
    // Set value through the native input event so the parent's oninput fires
    textareaEl.value = newValue;
    const cursorPos = triggerStart + item.insertText.length;
    textareaEl.setSelectionRange(cursorPos, cursorPos);
    textareaEl.dispatchEvent(new Event("input", { bubbles: true }));
    popupVisible = false;
  }

  function handleInput(e: Event) {
    oninput(e);
    updateSuggestions();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!popupVisible) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedIndex = (selectedIndex + 1) % filteredItems.length;
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedIndex =
        (selectedIndex - 1 + filteredItems.length) % filteredItems.length;
    } else if (e.key === "Enter" || e.key === "Tab") {
      e.preventDefault();
      insertSuggestion(filteredItems[selectedIndex]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      popupVisible = false;
    }
  }

  function handlePopupClick(e: MouseEvent) {
    const target = (e.target as HTMLElement).closest("[data-index]");
    if (target) {
      const idx = Number(target.getAttribute("data-index"));
      insertSuggestion(filteredItems[idx]);
    }
  }
</script>

<div class="promptTextarea-wrapper">
  <textarea
    bind:this={textareaEl}
    {value}
    oninput={handleInput}
    onkeydown={handleKeydown}
  ></textarea>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div onclick={handlePopupClick}>
    <AutocompletePopup
      items={filteredItems}
      {selectedIndex}
      visible={popupVisible}
      position={popupPosition}
    />
  </div>
</div>
