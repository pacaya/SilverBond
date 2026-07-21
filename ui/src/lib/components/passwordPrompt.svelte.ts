/**
 * Deferred-resolver bridge between the declarative <PasswordDialog> modal and
 * imperative `await`-style call sites that previously used `window.prompt`.
 *
 * `request()` opens the dialog and resolves with the entered secret, or `null`
 * when the user cancels (Escape, Cancel button, or dialog close).
 */
export function createPasswordPrompt() {
  let open = $state(false);
  let title = $state("Unlock password");
  let message = $state("");
  let resolve: ((value: string | null) => void) | null = null;

  function settle(value: string | null) {
    const pending = resolve;
    resolve = null;
    open = false;
    pending?.(value);
  }

  return {
    get open() {
      return open;
    },
    get title() {
      return title;
    },
    get message() {
      return message;
    },
    request(options: { title?: string; message?: string } = {}): Promise<string | null> {
      // A second request while one is pending cancels the first.
      settle(null);
      title = options.title ?? "Unlock password";
      message = options.message ?? "";
      open = true;
      return new Promise<string | null>((res) => {
        resolve = res;
      });
    },
    submit(value: string) {
      settle(value);
    },
    cancel() {
      settle(null);
    },
  };
}

export type PasswordPrompt = ReturnType<typeof createPasswordPrompt>;
