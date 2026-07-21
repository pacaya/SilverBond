import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import PasswordDialog from "./PasswordDialog.svelte";

describe("PasswordDialog", () => {
  afterEach(() => {
    cleanup();
  });

  function setup(open = true) {
    const onsubmit = vi.fn();
    const oncancel = vi.fn();
    const result = render(PasswordDialog, {
      props: { open, title: "Unlock password", onsubmit, oncancel },
    });
    return { onsubmit, oncancel, ...result };
  }

  it("masks the secret with a password input", async () => {
    setup();
    const input = screen.getByLabelText("Password") as HTMLInputElement;
    expect(input.type).toBe("password");
    expect(input.autocomplete).toBe("current-password");
  });

  it("submits the entered secret", async () => {
    const { onsubmit, oncancel } = setup();
    const input = screen.getByLabelText("Password");
    await fireEvent.input(input, { target: { value: "s3cret" } });
    await fireEvent.click(screen.getByRole("button", { name: "Unlock" }));

    expect(onsubmit).toHaveBeenCalledWith("s3cret");
    expect(oncancel).not.toHaveBeenCalled();
  });

  it("cancels without submitting", async () => {
    const { onsubmit, oncancel } = setup();
    const input = screen.getByLabelText("Password");
    await fireEvent.input(input, { target: { value: "s3cret" } });
    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(oncancel).toHaveBeenCalled();
    expect(onsubmit).not.toHaveBeenCalled();
  });

  it("cancels on Escape", async () => {
    const { onsubmit, oncancel } = setup();
    await fireEvent.keyDown(screen.getByLabelText("Password"), { key: "Escape" });

    expect(oncancel).toHaveBeenCalled();
    expect(onsubmit).not.toHaveBeenCalled();
  });

  it("stays closed when open is false", () => {
    setup(false);
    expect(screen.queryByLabelText("Password")).not.toBeNull();
    const dialog = document.querySelector("dialog") as HTMLDialogElement;
    expect(dialog.open).toBe(false);
  });
});
