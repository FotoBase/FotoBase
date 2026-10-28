// TASK-03-011 tests: batch rename / move UI. The backend owns planning and
// the file work; the UI previews, shows collisions, and applies only a plan
// without any.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  baseName,
  batchMove,
  batchRename,
  DEFAULT_SPEC,
  isApplicable,
  previewBatchMove,
  previewBatchRename,
  type BatchPlan,
} from "./batch";
import BatchDialog from "./BatchDialog.svelte";
import { locales, setLocale, t } from "./i18n.svelte.js";
import PhotoTray from "./PhotoTray.svelte";
import { resetTray, tray } from "./tray.svelte.js";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import * as tauriCore from "@tauri-apps/api/core";

const invokeMock = vi.mocked(tauriCore.invoke);

const okPlan: BatchPlan = {
  moves: [
    { id: 1, from: "/lib/a/IMG_1.jpg", to: "/lib/a/trip-1.jpg" },
    { id: 2, from: "/lib/a/IMG_2.jpg", to: "/lib/a/trip-2.jpg" },
  ],
  collisions: [],
};

const clashPlan: BatchPlan = {
  moves: [okPlan.moves[0]],
  collisions: [
    { id: 2, from: "/lib/a/IMG_2.jpg", to: "/lib/a/trip-1.jpg", reason: "duplicate_target" },
  ],
};

const folders = [
  { id: 1, path: "/lib/a", year: 2023, scan_mode: "once" as const },
  { id: 2, path: "/lib/b", year: 2023, scan_mode: "once" as const },
];

beforeEach(() => {
  invokeMock.mockReset();
  resetTray();
  setLocale("en");
});

afterEach(cleanup);

describe("batch ipc wrappers", () => {
  it("forward ids, spec and policy to the matching commands", async () => {
    invokeMock.mockResolvedValue(okPlan);
    await previewBatchRename([1, 2], DEFAULT_SPEC, "refuse");
    expect(invokeMock).toHaveBeenLastCalledWith("preview_batch_rename", {
      ids: [1, 2],
      spec: DEFAULT_SPEC,
      policy: "refuse",
    });
    await batchRename([1, 2], DEFAULT_SPEC, "auto_suffix");
    expect(invokeMock).toHaveBeenLastCalledWith("batch_rename", {
      ids: [1, 2],
      spec: DEFAULT_SPEC,
      policy: "auto_suffix",
    });
    await previewBatchMove([1], "/lib/b", "refuse");
    expect(invokeMock).toHaveBeenLastCalledWith("preview_batch_move", {
      ids: [1],
      dest: "/lib/b",
      policy: "refuse",
    });
    await batchMove([1], "/lib/b", "auto_suffix");
    expect(invokeMock).toHaveBeenLastCalledWith("batch_move", {
      ids: [1],
      dest: "/lib/b",
      policy: "auto_suffix",
    });
  });
});

describe("batch helpers", () => {
  it("baseName handles both separators", () => {
    expect(baseName("/lib/a/IMG_1.jpg")).toBe("IMG_1.jpg");
    expect(baseName("C:\\lib\\a\\IMG_1.jpg")).toBe("IMG_1.jpg");
    expect(baseName("x.jpg")).toBe("x.jpg");
  });

  it("a plan is applicable with moves and without collisions", () => {
    expect(isApplicable(okPlan)).toBe(true);
    expect(isApplicable(clashPlan)).toBe(false);
    expect(isApplicable({ moves: [], collisions: [] })).toBe(false);
  });
});

describe("tray.refresh", () => {
  it("updates names and hashes of items that are in the tray, keeping hold", () => {
    tray.add([{ id: 1, filename: "IMG_1.jpg", hash: "aa" }]);
    tray.toggleHold(1);
    tray.refresh([
      { id: 1, filename: "trip-1.jpg", hash: "aa" },
      { id: 9, filename: "other.jpg", hash: "bb" },
    ]);
    expect(tray.items).toEqual([{ id: 1, filename: "trip-1.jpg", hash: "aa", held: true }]);
  });
});

describe("tray batch buttons", () => {
  it("are disabled for an empty tray and hand the tray's ids to the shell", async () => {
    const onbatch = vi.fn();
    render(PhotoTray, { onbatch });
    expect((screen.getByTestId("tray-rename") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("tray-move") as HTMLButtonElement).disabled).toBe(true);
    cleanup();

    tray.add([
      { id: 4, filename: "a.jpg", hash: null },
      { id: 7, filename: "b.jpg", hash: null },
    ]);
    render(PhotoTray, { onbatch });
    await fireEvent.click(screen.getByTestId("tray-rename"));
    expect(onbatch).toHaveBeenLastCalledWith({ mode: "rename", ids: [4, 7] });
    await fireEvent.click(screen.getByTestId("tray-move"));
    expect(onbatch).toHaveBeenLastCalledWith({ mode: "move", ids: [4, 7] });
  });
});

describe("BatchDialog rename", () => {
  function open(onapplied = vi.fn()) {
    render(BatchDialog, { open: true, mode: "rename", ids: [1, 2], folders, onapplied });
    return onapplied;
  }

  it("previews the new names from the backend", async () => {
    invokeMock.mockResolvedValue(okPlan);
    open();
    await waitFor(() => expect(screen.getAllByTestId("batch-row")).toHaveLength(2));
    expect(screen.getAllByTestId("batch-row")[0].textContent).toContain("IMG_1.jpg");
    expect(screen.getAllByTestId("batch-row")[0].textContent).toContain("trip-1.jpg");
    expect(invokeMock).toHaveBeenCalledWith(
      "preview_batch_rename",
      expect.objectContaining({ ids: [1, 2], policy: "refuse" }),
    );
  });

  it("blocks Apply while the plan has collisions and shows them", async () => {
    invokeMock.mockResolvedValue(clashPlan);
    open();
    await waitFor(() => expect(screen.getByTestId("batch-collision")).toBeTruthy());
    expect((screen.getByTestId("batch-apply") as HTMLButtonElement).disabled).toBe(true);
  });

  it("re-previews when the pattern changes", async () => {
    invokeMock.mockResolvedValue(okPlan);
    open();
    await waitFor(() => expect(screen.getAllByTestId("batch-row")).toHaveLength(2));
    invokeMock.mockClear();
    await fireEvent.input(screen.getByTestId("batch-pattern"), { target: { value: "holiday-{n}" } });
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "preview_batch_rename",
        expect.objectContaining({
          spec: expect.objectContaining({ pattern: "holiday-{n}" }),
        }),
      ),
    );
  });

  it("applies, reports the moves and closes", async () => {
    invokeMock.mockImplementation(async (cmd) =>
      cmd === "batch_rename" ? okPlan.moves : okPlan,
    );
    const onapplied = open();
    await waitFor(() =>
      expect((screen.getByTestId("batch-apply") as HTMLButtonElement).disabled).toBe(false),
    );
    await fireEvent.click(screen.getByTestId("batch-apply"));
    await waitFor(() => expect(onapplied).toHaveBeenCalledWith(okPlan.moves));
    expect(screen.queryByTestId("batch-dialog")).toBeNull();
  });

  it("keeps the dialog open and shows the backend error when applying fails", async () => {
    invokeMock.mockImplementation(async (cmd) => {
      if (cmd === "batch_rename") throw "batch aborted at /lib/a/IMG_2.jpg";
      return okPlan;
    });
    open();
    await waitFor(() =>
      expect((screen.getByTestId("batch-apply") as HTMLButtonElement).disabled).toBe(false),
    );
    await fireEvent.click(screen.getByTestId("batch-apply"));
    await waitFor(() => expect(screen.getByTestId("batch-error").textContent).toContain("IMG_2"));
    expect(screen.getByTestId("batch-dialog")).toBeTruthy();
  });
});

describe("BatchDialog move", () => {
  it("needs a destination before it previews, then moves", async () => {
    invokeMock.mockImplementation(async (cmd) => (cmd === "batch_move" ? okPlan.moves : okPlan));
    const onapplied = vi.fn();
    render(BatchDialog, { open: true, mode: "move", ids: [1, 2], folders, onapplied });
    expect(invokeMock).not.toHaveBeenCalled();
    expect((screen.getByTestId("batch-apply") as HTMLButtonElement).disabled).toBe(true);

    await fireEvent.change(screen.getByTestId("batch-folder"), { target: { value: "/lib/b" } });
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("preview_batch_move", {
        ids: [1, 2],
        dest: "/lib/b",
        policy: "refuse",
      }),
    );
    await waitFor(() =>
      expect((screen.getByTestId("batch-apply") as HTMLButtonElement).disabled).toBe(false),
    );
    await fireEvent.click(screen.getByTestId("batch-apply"));
    await waitFor(() => expect(onapplied).toHaveBeenCalledWith(okPlan.moves));
  });
});

describe("batch i18n", () => {
  it("every batch string exists in every locale", () => {
    const keys = [
      "tray.rename",
      "tray.move",
      "batch.renameTitle",
      "batch.moveTitle",
      "batch.pattern",
      "batch.patternHint",
      "batch.start",
      "batch.step",
      "batch.pad",
      "batch.folder",
      "batch.chooseFolder",
      "batch.policy",
      "batch.refuse",
      "batch.autoSuffix",
      "batch.apply",
      "batch.cancel",
      "batch.collision.duplicate_target",
      "batch.collision.target_exists",
      "batch.collision.sidecar_exists",
      "batch.failed",
      "batch.nothingToDo",
    ];
    for (const locale of locales) {
      setLocale(locale);
      for (const key of keys) {
        expect(t(key), `${locale}:${key}`).not.toBe(key);
      }
    }
  });
});
