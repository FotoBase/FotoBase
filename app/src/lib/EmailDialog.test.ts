// TASK-08-006 tests: the Email dialog — a confirmable size (default 1600 px),
// IPC to `email_photos`, and a result message that says which mail flow
// opened. IPC is mocked at the Tauri core module.

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import EmailDialog from "./EmailDialog.svelte";
import { DEFAULT_EMAIL_EDGE, EMAIL_SIZES, emailPhotos } from "./email-share";
import { setLocale } from "./i18n.svelte.js";
import PhotoTray from "./PhotoTray.svelte";
import { resetTray, tray } from "./tray.svelte.js";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invoke(...a),
}));

beforeEach(() => {
  setLocale("en");
  invoke.mockReset();
  invoke.mockResolvedValue({ delivery: "attached", attached: 2, failed: 0 });
});
afterEach(() => {
  cleanup();
  resetTray();
});

describe("email IPC wrapper", () => {
  it("sends the ids and the chosen longest edge", async () => {
    await emailPhotos([7, 3], 1024);
    expect(invoke).toHaveBeenLastCalledWith("email_photos", {
      ids: [7, 3],
      request: { max_edge: 1024 },
    });
  });

  it("offers ~1600 px as the default among the sizes", () => {
    expect(DEFAULT_EMAIL_EDGE).toBe(1600);
    expect(EMAIL_SIZES).toContain(DEFAULT_EMAIL_EDGE);
  });
});

describe("EmailDialog", () => {
  it("emails at the default size and confirms the attached flow", async () => {
    render(EmailDialog, { open: true, ids: [1, 2] });
    await fireEvent.click(screen.getByTestId("email-run"));
    await waitFor(() => expect(screen.getByTestId("email-result")).toBeTruthy());
    expect(invoke).toHaveBeenCalledWith("email_photos", {
      ids: [1, 2],
      request: { max_edge: 1600 },
    });
    expect(screen.getByTestId("email-result").textContent).toContain("attached");
  });

  it("uses the size the user picked", async () => {
    render(EmailDialog, { open: true, ids: [1] });
    await fireEvent.change(screen.getByTestId("email-size"), { target: { value: "1024" } });
    await fireEvent.click(screen.getByTestId("email-run"));
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(invoke).toHaveBeenLastCalledWith("email_photos", {
      ids: [1],
      request: { max_edge: 1024 },
    });
  });

  it("tells the user to drag files in when only mailto opened", async () => {
    invoke.mockResolvedValue({ delivery: "mailto_revealed", attached: 3, failed: 1 });
    render(EmailDialog, { open: true, ids: [1, 2, 3, 4] });
    await fireEvent.click(screen.getByTestId("email-run"));
    await waitFor(() => expect(screen.getByTestId("email-result")).toBeTruthy());
    const text = screen.getByTestId("email-result").textContent ?? "";
    expect(text).toContain("folder");
    expect(text).toContain("1");
  });

  it("shows the error when the backend fails", async () => {
    invoke.mockRejectedValue("no photo could be resized for email");
    render(EmailDialog, { open: true, ids: [1] });
    await fireEvent.click(screen.getByTestId("email-run"));
    await waitFor(() => expect(screen.getByTestId("email-error")).toBeTruthy());
    expect(screen.getByTestId("email-error").textContent).toContain("no photo could be resized");
  });

  it("cannot run without photos", () => {
    render(EmailDialog, { open: true, ids: [] });
    expect((screen.getByTestId("email-run") as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("PhotoTray email entry", () => {
  it("the Email… action hands the tray's photos to the shell", async () => {
    tray.add([
      { id: 4, filename: "d.jpg", hash: "h4" },
      { id: 5, filename: "e.jpg", hash: "h5" },
    ]);
    const onemail = vi.fn();
    render(PhotoTray, { onemail });
    await fireEvent.click(screen.getByTestId("tray-email"));
    expect(onemail).toHaveBeenCalledWith({ ids: [4, 5] });
  });

  it("is disabled while the tray is empty", () => {
    render(PhotoTray, {});
    expect((screen.getByTestId("tray-email") as HTMLButtonElement).disabled).toBe(true);
  });
});
