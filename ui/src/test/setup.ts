import "@testing-library/jest-dom/vitest";

/* jsdom does not implement ResizeObserver; SvelteFlow and pane terminals need a shim. */
if (typeof globalThis.ResizeObserver === "undefined") {
  class ResizeObserverShim implements ResizeObserver {
    readonly [Symbol.toStringTag] = "ResizeObserver";

    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }

  globalThis.ResizeObserver = ResizeObserverShim;
}

/* jsdom does not implement matchMedia; SvelteFlow queries prefers-reduced-motion. */
if (typeof globalThis.matchMedia === "undefined") {
  globalThis.matchMedia = (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  });
}

/* jsdom does not implement <dialog> modal methods; provide a minimal shim so
   dialog-based components (ConfirmDialog, PasswordDialog) can be tested. */
if (typeof HTMLDialogElement !== "undefined") {
  const proto = HTMLDialogElement.prototype as HTMLDialogElement & {
    showModal?: () => void;
    show?: () => void;
    close?: (returnValue?: string) => void;
  };
  if (typeof proto.showModal !== "function") {
    proto.showModal = function showModal(this: HTMLDialogElement) {
      this.open = true;
    };
  }
  if (typeof proto.show !== "function") {
    proto.show = function show(this: HTMLDialogElement) {
      this.open = true;
    };
  }
  if (typeof proto.close !== "function") {
    proto.close = function close(this: HTMLDialogElement, returnValue?: string) {
      this.open = false;
      if (returnValue !== undefined) this.returnValue = returnValue;
      this.dispatchEvent(new Event("close"));
    };
  }
}
