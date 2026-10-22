// ImportDialog tests (TASK-01-017, PLAN §2A/§6 P1): "Exclude duplicates" is
// on by default; the dialog sends source/destination/naming and refreshes
// the library on completion; a running import can be cancelled; progress
// and the final report render. IPC is mocked — the component's job is
// decisions and display, the wrappers' is transport.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ImportDialog from "./ImportDialog.svelte";
import { cancelImport, importPhotos, onImportProgress } from "./import-photos";

vi.mock("./import-photos", () => ({
  importPhotos: vi.fn(),
  cancelImport: vi.fn(),
  onImportProgress: vi.fn(),
}));

afterEach(() => cleanup());

// flushSync flushes the synchronous DOM update; the component's handlers
// await their IPC mock, so one macrotask tick lets those continuations run.
const settle = async (): Promise<void> => {
  await flushSync();
  await new Promise((resolve) => setTimeout(resolve, 0));
};

let progressCb: ((p: unknown) => void) | null = null;

beforeEach(() => {
  vi.mocked(importPhotos).mockReset().mockResolvedValue({
    copied: 2,
    duplicates: 1,
    failed: 0,
    cancelled: false,
  });
  vi.mocked(cancelImport).mockReset().mockResolvedValue(undefined);
  progressCb = null;
  vi.mocked(onImportProgress).mockReset().mockImplementation(async (cb: (p: unknown) => void) => {
    progressCb = cb;
    return () => {};
  });
});

function renderOpen(onchanged = vi.fn()) {
  return render(ImportDialog, { open: true, onchanged });
}

describe("ImportDialog", () => {
  it("renders nothing when closed", () => {
    render(ImportDialog, { open: false, onchanged: vi.fn() });
    expect(screen.queryByTestId("import-backdrop")).toBeNull();
  });

  it("starts with Exclude duplicates checked (PLAN §2 default)", () => {
    renderOpen();
    const checkbox = screen.getByTestId("import-exclude-duplicates") as HTMLInputElement;
    expect(checkbox.checked).toBe(true);
  });

  it("the start button is disabled until source and destination are filled", async () => {
    renderOpen();
    expect((screen.getByTestId("import-start") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.input(screen.getByTestId("import-source"), { target: { value: "/card" } });
    fireEvent.input(screen.getByTestId("import-destination"), { target: { value: "/lib" } });
    await flushSync();
    expect((screen.getByTestId("import-start") as HTMLButtonElement).disabled).toBe(false);
  });

  it("sends the request with date naming and the duplicate exclusion", async () => {
    const onchanged = vi.fn();
    renderOpen(onchanged);
    fireEvent.input(screen.getByTestId("import-source"), { target: { value: "/card" } });
    fireEvent.input(screen.getByTestId("import-destination"), { target: { value: "/lib" } });
    fireEvent.click(screen.getByTestId("import-start"));
    await settle();
    expect(importPhotos).toHaveBeenCalledWith({
      source: "/card",
      destination: "/lib",
      naming: "date",
      customPattern: undefined,
      excludeDuplicates: true,
    });
    expect(onchanged).toHaveBeenCalled();
  });

  it("a custom naming sends the typed pattern", async () => {
    renderOpen();
    fireEvent.input(screen.getByTestId("import-source"), { target: { value: "/card" } });
    fireEvent.input(screen.getByTestId("import-destination"), { target: { value: "/lib" } });
    fireEvent.click(screen.getByTestId("import-naming-custom"));
    await flushSync();
    fireEvent.input(screen.getByTestId("import-custom-pattern"), {
      target: { value: "2026/Vacation" },
    });
    fireEvent.click(screen.getByTestId("import-start"));
    await settle();
    expect(importPhotos).toHaveBeenCalledWith(
      expect.objectContaining({ naming: "custom", customPattern: "2026/Vacation" }),
    );
  });

  it("shows the report after the import finishes", async () => {
    renderOpen();
    fireEvent.input(screen.getByTestId("import-source"), { target: { value: "/card" } });
    fireEvent.input(screen.getByTestId("import-destination"), { target: { value: "/lib" } });
    fireEvent.click(screen.getByTestId("import-start"));
    await settle();
    const report = screen.getByTestId("import-report");
    expect(report.textContent).toContain("2");
    expect(report.textContent).toContain("1");
  });

  it("a cancelled report says so", async () => {
    vi.mocked(importPhotos).mockResolvedValue({
      copied: 1,
      duplicates: 0,
      failed: 0,
      cancelled: true,
    });
    renderOpen();
    fireEvent.input(screen.getByTestId("import-source"), { target: { value: "/card" } });
    fireEvent.input(screen.getByTestId("import-destination"), { target: { value: "/lib" } });
    fireEvent.click(screen.getByTestId("import-start"));
    await settle();
    expect(screen.getByTestId("import-report").textContent).toMatch(/cancelled/i);
  });

  it("while running, cancel is offered and reaches the backend", async () => {
    let resolveRun: (r: unknown) => void = () => {};
    vi.mocked(importPhotos).mockReturnValue(
      new Promise((resolve) => {
        resolveRun = resolve;
      }),
    );
    renderOpen();
    // The progress subscription is armed when the dialog opens.
    expect(onImportProgress).toHaveBeenCalled();
    fireEvent.input(screen.getByTestId("import-source"), { target: { value: "/card" } });
    fireEvent.input(screen.getByTestId("import-destination"), { target: { value: "/lib" } });
    fireEvent.click(screen.getByTestId("import-start"));
    await settle();
    expect(screen.getByTestId("import-cancel")).toBeTruthy();
    fireEvent.click(screen.getByTestId("import-cancel"));
    await settle();
    expect(cancelImport).toHaveBeenCalled();
    resolveRun({ copied: 0, duplicates: 0, failed: 0, cancelled: true });
    await settle();
  });

  it("a failed import surfaces an error and does not refresh", async () => {
    vi.mocked(importPhotos).mockRejectedValue("backend said no");
    const onchanged = vi.fn();
    renderOpen(onchanged);
    fireEvent.input(screen.getByTestId("import-source"), { target: { value: "/card" } });
    fireEvent.input(screen.getByTestId("import-destination"), { target: { value: "/lib" } });
    fireEvent.click(screen.getByTestId("import-start"));
    await settle();
    expect(screen.getByTestId("import-error")).toBeTruthy();
    expect(onchanged).not.toHaveBeenCalled();
  });

  it("progress events render while running", async () => {
    let resolveRun: (r: unknown) => void = () => {};
    vi.mocked(importPhotos).mockReturnValue(
      new Promise((resolve) => {
        resolveRun = resolve;
      }),
    );
    renderOpen();
    fireEvent.input(screen.getByTestId("import-source"), { target: { value: "/card" } });
    fireEvent.input(screen.getByTestId("import-destination"), { target: { value: "/lib" } });
    fireEvent.click(screen.getByTestId("import-start"));
    await settle();
    progressCb?.({ files_done: 3, files_total: 10, current_file: "x.jpg" });
    await flushSync();
    expect(screen.getByTestId("import-progress").textContent).toContain("3");
    resolveRun({ copied: 10, duplicates: 0, failed: 0, cancelled: false });
    await settle();
  });
});
