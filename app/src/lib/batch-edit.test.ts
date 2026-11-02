// TASK-04-011 tests: batch edit ops. The backend applies one §2E op to
// every selected photo's sidecar recipe through the job queue; this module
// is the IPC wrapper plus the op catalog the dialog renders.

import { cleanup, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  batchEdit,
  BATCH_EDIT_EVENT,
  BATCH_OPS,
  cancelBatchEdit,
  type BatchEditReport,
} from "./batch-edit";
import BatchEditDialog from "./BatchEditDialog.svelte";
import { locales, setLocale, t } from "./i18n.svelte.js";
import PhotoTray from "./PhotoTray.svelte";
import { resetTray, tray } from "./tray.svelte.js";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import * as tauriCore from "@tauri-apps/api/core";
import * as tauriEvent from "@tauri-apps/api/event";

const invokeMock = vi.mocked(tauriCore.invoke);
const listenMock = vi.mocked(tauriEvent.listen);

beforeEach(() => {
  invokeMock.mockReset();
  listenMock.mockReset();
  listenMock.mockResolvedValue(() => {});
});

afterEach(() => {
  cleanup();
  resetTray();
});

describe("op catalog", () => {
  it("covers every §2E batch op exactly once", () => {
    const ids = BATCH_OPS.map((o) => o.id);
    expect(ids).toEqual([
      "rotate_cw",
      "rotate_ccw",
      "auto_contrast",
      "auto_color",
      "sepia",
      "sharpen",
      "warmify",
      "film_grain",
      "bw",
      "auto_red_eye",
      "show_text",
      "hide_text",
    ]);
  });

  it("every op has a translation key that resolves", () => {
    for (const locale of locales) {
      setLocale(locale);
      for (const op of BATCH_OPS) {
        const label = t(op.labelKey);
        expect(label, `${op.id} in ${locale}`).not.toBe(op.labelKey);
        expect(label.length).toBeGreaterThan(0);
      }
    }
  });
});

describe("IPC wrappers", () => {
  it("batchEdit invokes the backend command with ids and op", async () => {
    const report: BatchEditReport = { applied: 2, failed: [], cancelled: false };
    invokeMock.mockResolvedValue(report);
    await expect(batchEdit([1, 2, 3], "sepia")).resolves.toEqual(report);
    expect(invokeMock).toHaveBeenCalledWith("batch_edit", { ids: [1, 2, 3], op: "sepia" });
  });

  it("cancelBatchEdit invokes the cancel command", async () => {
    await cancelBatchEdit();
    expect(invokeMock).toHaveBeenCalledWith("cancel_batch_edit", {});
  });

  it("onBatchEditProgress subscribes to the backend progress event", async () => {
    const { onBatchEditProgress } = await import("./batch-edit");
    const cb = vi.fn();
    await onBatchEditProgress(cb);
    expect(listenMock).toHaveBeenCalledWith(BATCH_EDIT_EVENT, expect.any(Function));
  });
});

describe("BatchEditDialog", () => {
  function progressEvents(): Array<(e: { payload: unknown }) => void> {
    const handlers: Array<(e: { payload: unknown }) => void> = [];
    listenMock.mockImplementation(async (_event, handler) => {
      handlers.push(handler as (e: { payload: unknown }) => void);
      return () => {};
    });
    return handlers;
  }

  it("renders one button per §2E op and fires the batch for the picked op", async () => {
    invokeMock.mockResolvedValue({ applied: 3, failed: [], cancelled: false });
    render(BatchEditDialog, { open: true, ids: [7, 8, 9], onapplied: () => {} });

    const rotate = screen.getByTestId("batch-edit-op-rotate_cw");
    await rotate.click();
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("batch_edit", { ids: [7, 8, 9], op: "rotate_cw" }),
    );
  });

  it("shows progress while running and a Cancel button that cancels", async () => {
    const handlers = progressEvents();
    let resolveRun!: (r: BatchEditReport) => void;
    invokeMock.mockReturnValue(
      new Promise<BatchEditReport>((resolve) => (resolveRun = resolve)),
    );
    render(BatchEditDialog, { open: true, ids: [1, 2], onapplied: () => {} });

    const rotate = screen.getByTestId("batch-edit-op-rotate_cw");
    await rotate.click();

    // Progress arrives while the batch is in flight.
    await waitFor(() => expect(handlers.length).toBeGreaterThan(0));
    handlers[0]({ payload: { done: 1, total: 2 } });
    await waitFor(() => expect(screen.getByTestId("batch-edit-progress").textContent).toContain("1"));

    const cancel = screen.getByTestId("batch-edit-cancel");
    await cancel.click();
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("cancel_batch_edit", {}),
    );

    resolveRun({ applied: 1, failed: [], cancelled: true });
    await waitFor(() => expect(screen.getByTestId("batch-edit-cancelled")).toBeTruthy());
  });

  it("reports the outcome and refreshes when the run completes", async () => {
    progressEvents();
    invokeMock.mockResolvedValue({
      applied: 2,
      failed: [{ id: 9, error: "boom" }],
      cancelled: false,
    });
    const onapplied = vi.fn();
    render(BatchEditDialog, { open: true, ids: [8, 9], onapplied });

    await screen.getByTestId("batch-edit-op-bw").click();
    await waitFor(() => expect(screen.getByTestId("batch-edit-done")).toBeTruthy());
    expect(screen.getByTestId("batch-edit-done").textContent).toContain("1");
    expect(onapplied).toHaveBeenCalled();
  });

  it("is inert when closed", () => {
    render(BatchEditDialog, { open: false, ids: [1], onapplied: () => {} });
    expect(screen.queryByTestId("batch-edit-dialog")).toBeNull();
  });
});

describe("PhotoTray batch edit entry", () => {
  it("the Edit… action hands the tray's photos to the shell", async () => {
    tray.add([
      { id: 4, filename: "d.jpg", hash: "h4" },
      { id: 5, filename: "e.jpg", hash: "h5" },
    ]);
    const onbatch = vi.fn();
    render(PhotoTray, { onbatch });

    await screen.getByTestId("tray-edit").click();
    expect(onbatch).toHaveBeenCalledWith({ mode: "edit", ids: [4, 5] });
  });
});
