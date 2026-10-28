// TASK-03-012 / TASK-03-009 tests: the catalog-trash view and the hidden
// controls. The backend owns every mutation; the UI lists the trash, undoes
// single entries or a whole action type, and reports what could not be
// restored.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { locales, setLocale, t } from "./i18n.svelte.js";
import { DEFAULT_SETTINGS } from "./settings";
import {
  groupTrash,
  hideFolder,
  hidePhotos,
  listTrash,
  trashActionKey,
  undoTrash,
  undoTrashAll,
  unhideFolder,
  unhidePhotos,
  type TrashEntry,
} from "./trash";
import TrashDialog from "./TrashDialog.svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import * as tauriCore from "@tauri-apps/api/core";

const invokeMock = vi.mocked(tauriCore.invoke);

const removed: TrashEntry = {
  id: 3,
  action: "album_remove",
  path: "/lib/a/IMG_1.jpg",
  name: "IMG_1.jpg",
  detail: "Trip",
  created_at: 1_760_000_000,
};
const hiddenPhoto: TrashEntry = {
  id: 2,
  action: "hide",
  path: "/lib/a/IMG_2.jpg",
  name: "IMG_2.jpg",
  detail: null,
  created_at: 1_760_000_100,
};
const hiddenFolder: TrashEntry = {
  id: 4,
  action: "hide",
  path: "/lib/private",
  name: "private",
  detail: "folder",
  created_at: 1_760_000_200,
};
const unlinked: TrashEntry = {
  id: 1,
  action: "unlink",
  path: "/lib/old",
  name: "old",
  detail: null,
  created_at: 1_760_000_300,
};

const report = { restored: 1, skipped: 0, rescan: [] as string[] };

beforeEach(() => {
  invokeMock.mockReset();
  setLocale("en");
});

afterEach(cleanup);

describe("trash ipc wrappers", () => {
  it("forward to the matching commands", async () => {
    invokeMock.mockResolvedValue(null);
    await hidePhotos([1, 2]);
    expect(invokeMock).toHaveBeenLastCalledWith("hide_photos", { fileIds: [1, 2] });
    await unhidePhotos([1]);
    expect(invokeMock).toHaveBeenLastCalledWith("unhide_photos", { fileIds: [1] });
    await hideFolder("/lib/a");
    expect(invokeMock).toHaveBeenLastCalledWith("hide_folder", { path: "/lib/a" });
    await unhideFolder("/lib/a");
    expect(invokeMock).toHaveBeenLastCalledWith("unhide_folder", { path: "/lib/a" });
    await undoTrash(7);
    expect(invokeMock).toHaveBeenLastCalledWith("undo_trash", { id: 7 });
    await undoTrashAll("hide");
    expect(invokeMock).toHaveBeenLastCalledWith("undo_trash_all", { action: "hide" });
  });

  it("listTrash returns the entries, empty without a backend payload", async () => {
    invokeMock.mockResolvedValueOnce([removed]);
    expect(await listTrash()).toEqual([removed]);
    invokeMock.mockResolvedValueOnce(null);
    expect(await listTrash()).toEqual([]);
  });
});

describe("trash helpers", () => {
  it("groups entries by action in a fixed order, skipping empty groups", () => {
    const groups = groupTrash([unlinked, hiddenPhoto, removed, hiddenFolder]);
    expect(groups.map((g) => g.action)).toEqual(["album_remove", "hide", "unlink"]);
    expect(groups[1].entries.map((e) => e.id)).toEqual([2, 4]);
    expect(groupTrash([hiddenPhoto]).map((g) => g.action)).toEqual(["hide"]);
    expect(groupTrash([])).toEqual([]);
  });

  it("tells a folder hide from a photo hide", () => {
    expect(trashActionKey(hiddenPhoto)).toBe("trash.action.hide");
    expect(trashActionKey(hiddenFolder)).toBe("trash.action.hideFolder");
    expect(trashActionKey(removed)).toBe("trash.action.album_remove");
    expect(trashActionKey(unlinked)).toBe("trash.action.unlink");
  });
});

describe("TrashDialog", () => {
  it("lists every pending entry with its action, photo and album", async () => {
    invokeMock.mockResolvedValue([unlinked, hiddenFolder, hiddenPhoto, removed]);
    render(TrashDialog, { open: true, onchanged: vi.fn() });

    await waitFor(() => expect(screen.getAllByTestId("trash-row")).toHaveLength(4));
    expect(invokeMock).toHaveBeenCalledWith("list_trash", undefined);
    const rows = screen.getAllByTestId("trash-row").map((r) => r.textContent ?? "");
    expect(rows.some((r) => r.includes("IMG_1.jpg") && r.includes("Trip"))).toBe(true);
    expect(rows.some((r) => r.includes("IMG_2.jpg"))).toBe(true);
    expect(rows.some((r) => r.includes("private"))).toBe(true);
    expect(rows.some((r) => r.includes("old"))).toBe(true);
    expect(screen.getAllByTestId("trash-time")).toHaveLength(4);
  });

  it("shows an empty state", async () => {
    invokeMock.mockResolvedValue([]);
    render(TrashDialog, { open: true, onchanged: vi.fn() });
    await waitFor(() => expect(screen.getByTestId("trash-empty")).toBeTruthy());
  });

  it("undoes one entry, reloads the list and tells the shell", async () => {
    const onchanged = vi.fn();
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_trash") return invokeMock.mock.calls.some((c) => c[0] === "undo_trash") ? [] : [hiddenPhoto];
      if (cmd === "undo_trash") return report;
      return null;
    });
    render(TrashDialog, { open: true, onchanged });
    await waitFor(() => expect(screen.getAllByTestId("trash-row")).toHaveLength(1));

    await fireEvent.click(screen.getByTestId("trash-undo"));

    await waitFor(() => expect(screen.getByTestId("trash-empty")).toBeTruthy());
    expect(invokeMock).toHaveBeenCalledWith("undo_trash", { id: 2 });
    expect(onchanged).toHaveBeenCalledTimes(1);
  });

  it("offers undo-all per action type", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_trash") return [hiddenPhoto, hiddenFolder, removed];
      return report;
    });
    render(TrashDialog, { open: true, onchanged: vi.fn() });
    await waitFor(() => expect(screen.getAllByTestId("trash-row")).toHaveLength(3));

    expect(screen.queryByTestId("trash-undo-all-unlink")).toBeNull();
    await fireEvent.click(screen.getByTestId("trash-undo-all-hide"));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("undo_trash_all", { action: "hide" }),
    );
    expect(screen.getByTestId("trash-undo-all-album_remove")).toBeTruthy();
  });

  it("says when an undo could not be applied any more", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_trash") return [removed];
      if (cmd === "undo_trash") return { restored: 0, skipped: 1, rescan: [] };
      return null;
    });
    render(TrashDialog, { open: true, onchanged: vi.fn() });
    await waitFor(() => expect(screen.getAllByTestId("trash-row")).toHaveLength(1));

    await fireEvent.click(screen.getByTestId("trash-undo"));

    await waitFor(() => expect(screen.getByTestId("trash-skipped")).toBeTruthy());
  });

  it("shows a backend failure and keeps the entry", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_trash") return [removed];
      throw new Error("sidecar is read-only");
    });
    render(TrashDialog, { open: true, onchanged: vi.fn() });
    await waitFor(() => expect(screen.getAllByTestId("trash-row")).toHaveLength(1));

    await fireEvent.click(screen.getByTestId("trash-undo"));

    await waitFor(() => expect(screen.getByTestId("trash-error").textContent).toContain("read-only"));
    expect(screen.getAllByTestId("trash-row")).toHaveLength(1);
  });

  it("closes with the button", async () => {
    invokeMock.mockResolvedValue([]);
    render(TrashDialog, { open: true, onchanged: vi.fn() });
    await waitFor(() => expect(screen.getByTestId("trash-dialog")).toBeTruthy());
    await fireEvent.click(screen.getByTestId("trash-close"));
    expect(screen.queryByTestId("trash-dialog")).toBeNull();
  });
});

describe("settings defaults", () => {
  it("keep hidden items out of sight and the undo window at 30 days", () => {
    expect(DEFAULT_SETTINGS.show_hidden).toBe(false);
    expect(DEFAULT_SETTINGS.trash_window_days).toBe(30);
  });
});

describe("i18n", () => {
  it("has every trash and hide string in every locale", () => {
    const keys = [
      "app.trash",
      "trash.title",
      "trash.empty",
      "trash.undo",
      "trash.undoAll",
      "trash.close",
      "trash.skipped",
      "trash.failed",
      "trash.action.album_remove",
      "trash.action.hide",
      "trash.action.hideFolder",
      "trash.action.unlink",
      "menu.hide",
      "menu.unhide",
      "menu.hideFolder",
      "menu.unhideFolder",
      "options.showHidden",
      "options.trashWindow",
    ];
    for (const locale of locales) {
      setLocale(locale);
      for (const key of keys) {
        expect(t(key), `${locale}:${key}`).not.toBe(key);
      }
    }
  });
});
