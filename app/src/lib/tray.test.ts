// TASK-03-010 tests: the Photo Tray. Session-scoped UI state (no catalog, no
// sidecar writes); contents are photo ids that actions read and hand to the
// existing command paths.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { addToAlbum } from "./albums";
import { locales, setLocale, t } from "./i18n.svelte.js";
import PhotoTray from "./PhotoTray.svelte";
import { actionIds, resetTray, tray } from "./tray.svelte.js";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import * as tauriCore from "@tauri-apps/api/core";

const invokeMock = vi.mocked(tauriCore.invoke);

const photo = (id: number, folder = "a") => ({
  id,
  filename: `${folder}-${id}.jpg`,
  hash: id.toString(16).padStart(32, "0"),
});

beforeEach(() => {
  invokeMock.mockReset();
  resetTray();
  setLocale("en");
});

afterEach(cleanup);

describe("tray store", () => {
  it("starts empty", () => {
    expect(tray.count).toBe(0);
    expect(tray.ids).toEqual([]);
  });

  it("add keeps insertion order, skips duplicates, reports how many were new", () => {
    expect(tray.add([photo(1), photo(2)])).toBe(2);
    expect(tray.add([photo(2), photo(3)])).toBe(1);
    expect(tray.ids).toEqual([1, 2, 3]);
  });

  it("holds a cross-folder selection: items from different folders coexist", () => {
    tray.add([photo(1, "holiday")]);
    // Browsing to another folder does not touch the tray.
    tray.add([photo(7, "work")]);
    expect(tray.items.map((i) => i.filename)).toEqual(["holiday-1.jpg", "work-7.jpg"]);
  });

  it("new items are not held", () => {
    tray.add([photo(1)]);
    expect(tray.items[0].held).toBe(false);
    expect(tray.heldCount).toBe(0);
  });

  it("toggleHold pins and unpins one item", () => {
    tray.add([photo(1), photo(2)]);
    tray.toggleHold(2);
    expect(tray.items.map((i) => i.held)).toEqual([false, true]);
    tray.toggleHold(2);
    expect(tray.heldCount).toBe(0);
  });

  it("clearUnheld removes only unheld items; held ones survive", () => {
    tray.add([photo(1), photo(2), photo(3)]);
    tray.toggleHold(2);
    expect(tray.clearUnheld()).toBe(2);
    expect(tray.ids).toEqual([2]);
    expect(tray.items[0].held).toBe(true);
  });

  it("clearUnheld on a fully held tray changes nothing", () => {
    tray.add([photo(1)]);
    tray.toggleHold(1);
    expect(tray.clearUnheld()).toBe(0);
    expect(tray.ids).toEqual([1]);
  });

  it("adding an already-tray photo does not drop its hold", () => {
    tray.add([photo(1)]);
    tray.toggleHold(1);
    tray.add([photo(1)]);
    expect(tray.items[0].held).toBe(true);
  });

  it("remove drops one item, held or not", () => {
    tray.add([photo(1), photo(2)]);
    tray.toggleHold(1);
    tray.remove(1);
    expect(tray.ids).toEqual([2]);
  });

  it("has reports membership", () => {
    tray.add([photo(4)]);
    expect(tray.has(4)).toBe(true);
    expect(tray.has(5)).toBe(false);
  });

  it("resetTray empties everything", () => {
    tray.add([photo(1)]);
    tray.toggleHold(1);
    resetTray();
    expect(tray.count).toBe(0);
  });
});

describe("actionIds", () => {
  it("uses the tray when it has contents, ignoring the grid selection", () => {
    expect(actionIds(new Set([9]), [1, 2])).toEqual([1, 2]);
  });

  it("falls back to the grid selection when the tray is empty", () => {
    expect(actionIds(new Set([9, 10]), [])).toEqual([9, 10]);
  });
});

describe("tray actions (TASK-03-010 e2e flow)", () => {
  it("two photos from different folders, clear unheld, album-add: only the held one is affected", async () => {
    invokeMock.mockResolvedValue(1);
    tray.add([photo(1, "holiday"), photo(7, "work")]);
    tray.toggleHold(7);
    tray.clearUnheld();

    await addToAlbum(3, actionIds(new Set(), tray.ids));

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("add_to_album", { albumId: 3, fileIds: [7] });
  });
});

describe("PhotoTray panel", () => {
  const settle = (): void => flushSync();

  it("stays visible when empty, with a hint and no actions to run", () => {
    render(PhotoTray, { props: {} });
    expect(screen.getByTestId("photo-tray")).toBeTruthy();
    expect(screen.getByTestId("tray-empty")).toBeTruthy();
    expect((screen.getByTestId("tray-clear") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("tray-add-to-album") as HTMLButtonElement).disabled).toBe(true);
  });

  it("shows a thumbnail per item and the count", () => {
    tray.add([photo(1), photo(2)]);
    render(PhotoTray, { props: {} });
    expect(screen.getAllByTestId("tray-item")).toHaveLength(2);
    expect(screen.getByTestId("tray-count").textContent).toContain("2");
    const img = screen.getAllByTestId("tray-item")[0].querySelector("img");
    expect(img?.getAttribute("src")).toBe(`thumb://${photo(1).hash}`);
  });

  it("Hold button pins the item; Clear removes only unheld", async () => {
    tray.add([photo(1), photo(2)]);
    render(PhotoTray, { props: {} });

    await fireEvent.click(screen.getAllByTestId("tray-hold")[1]);
    settle();
    expect(screen.getAllByTestId("tray-item")[1].getAttribute("data-held")).toBe("true");

    await fireEvent.click(screen.getByTestId("tray-clear"));
    settle();
    expect(screen.getAllByTestId("tray-item")).toHaveLength(1);
    expect(tray.ids).toEqual([2]);
  });

  it("clicking a tray item asks the shell to jump to it", async () => {
    tray.add([photo(1), photo(2)]);
    const onjump = vi.fn();
    render(PhotoTray, { props: { onjump } });
    await fireEvent.click(screen.getAllByTestId("tray-jump")[1]);
    expect(onjump).toHaveBeenCalledWith(expect.objectContaining({ id: 2 }));
  });

  it("the remove button drops one item", async () => {
    tray.add([photo(1), photo(2)]);
    render(PhotoTray, { props: {} });
    await fireEvent.click(screen.getAllByTestId("tray-remove")[0]);
    settle();
    expect(tray.ids).toEqual([2]);
  });

  it("Add selection puts the grid selection into the tray", async () => {
    tray.add([photo(1)]);
    const resolve = vi.fn((ids: number[]) => ids.map((id) => photo(id)));
    render(PhotoTray, { props: { selectedIds: [5, 6], resolve } });
    await fireEvent.click(screen.getByTestId("tray-add-selection"));
    settle();
    expect(resolve).toHaveBeenCalledWith([5, 6]);
    expect(tray.ids).toEqual([1, 5, 6]);
  });

  it("Add selection is disabled with nothing selected", () => {
    tray.add([photo(1)]);
    render(PhotoTray, { props: { selectedIds: [] } });
    expect((screen.getByTestId("tray-add-selection") as HTMLButtonElement).disabled).toBe(true);
  });

  it("Add to album hands the tray ids and the anchor to the shell", async () => {
    tray.add([photo(1), photo(2)]);
    tray.toggleHold(2);
    tray.clearUnheld();
    const onaddtoalbum = vi.fn();
    render(PhotoTray, { props: { onaddtoalbum } });
    await fireEvent.click(screen.getByTestId("tray-add-to-album"));
    expect(onaddtoalbum).toHaveBeenCalledWith(
      expect.objectContaining({ ids: [2], x: expect.any(Number), y: expect.any(Number) }),
    );
  });
});

describe("tray strings", () => {
  it("every locale defines every tray key", () => {
    const keys = [
      "tray.title",
      "tray.count",
      "tray.add",
      "tray.empty",
      "tray.clear",
      "tray.hold",
      "tray.release",
      "tray.remove",
      "tray.addToAlbum",
      "menu.addToTray",
      "menu.removeFromTray",
    ];
    for (const locale of locales) {
      setLocale(locale);
      for (const key of keys) {
        expect(t(key), `${locale}:${key}`).not.toBe(key);
      }
    }
  });
});
