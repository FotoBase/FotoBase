// TASK-03-008 tests: albums UI. Create / edit / delete, the sidebar list with
// counts, and membership toggles for the photo the viewer shows. Every write
// goes through the backend (`fb:Albums` sidecar first); these tests pin the
// IPC contract and the pure helpers.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import AlbumDialog from "./AlbumDialog.svelte";
import AlbumList from "./AlbumList.svelte";
import AlbumMembership from "./AlbumMembership.svelte";
import {
  albumSubtitle,
  isValidAlbumFields,
  listAlbumFiles,
  targetIds,
  todayIso,
  type Album,
} from "./albums";
import { locales, setLocale, t } from "./i18n.svelte.js";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import * as tauriCore from "@tauri-apps/api/core";

const invokeMock = vi.mocked(tauriCore.invoke);

const settle = async (): Promise<void> => {
  await new Promise((r) => setTimeout(r, 0));
  flushSync();
};

const album = (over: Partial<Album> = {}): Album => ({
  id: 1,
  uuid: "7d3c6a52-5a0e-4b3e-9a55-0d6a9f0f1a11",
  name: "Summer 2024",
  date: "2024-07-14",
  place: "Lisbon",
  description: null,
  count: 2,
  cover_file_id: null,
  cover_hash: null,
  ...over,
});

beforeEach(() => {
  invokeMock.mockReset();
  setLocale("en");
});

afterEach(cleanup);

describe("album helpers", () => {
  it("todayIso formats local time as YYYY-MM-DD", () => {
    expect(todayIso(new Date(2024, 0, 5))).toBe("2024-01-05");
    expect(todayIso(new Date(2024, 11, 31))).toBe("2024-12-31");
  });

  it("a form needs a name and an ISO date; place and description are optional", () => {
    const ok = { name: "Trip", date: "2024-05-05", place: "", description: "" };
    expect(isValidAlbumFields(ok)).toBe(true);
    expect(isValidAlbumFields({ ...ok, name: "   " })).toBe(false);
    expect(isValidAlbumFields({ ...ok, date: "" })).toBe(false);
    expect(isValidAlbumFields({ ...ok, date: "5/5/2024" })).toBe(false);
  });

  it("actions hit the whole selection when the clicked photo is in it", () => {
    expect(targetIds(2, new Set([1, 2, 3]))).toEqual([1, 2, 3]);
    expect(targetIds(9, new Set([1, 2, 3]))).toEqual([9]);
    expect(targetIds(9, new Set())).toEqual([9]);
  });

  it("the subtitle joins date and place when present", () => {
    expect(albumSubtitle({ date: "2024-07-14", place: "Lisbon" })).toBe("2024-07-14 · Lisbon");
    expect(albumSubtitle({ date: "2024-07-14", place: null })).toBe("2024-07-14");
    expect(albumSubtitle({ date: null, place: null })).toBe("");
  });

  it("listAlbumFiles sends the query only when it is not blank", async () => {
    invokeMock.mockResolvedValue([]);
    await listAlbumFiles(3, "   ");
    expect(invokeMock).toHaveBeenLastCalledWith("list_album_files", { albumId: 3 });
    await listAlbumFiles(3, " beach ");
    expect(invokeMock).toHaveBeenLastCalledWith("list_album_files", {
      albumId: 3,
      query: "beach",
    });
  });

  it("every album string exists in every locale", () => {
    const keys = [
      "albums.title",
      "albums.new",
      "albums.empty",
      "albums.edit",
      "albums.delete",
      "albums.deleteConfirm",
      "albums.setCover",
      "albums.removeFromPicked",
      "albums.newEllipsis",
      "albums.dialog.createTitle",
      "albums.dialog.editTitle",
      "albums.name",
      "albums.date",
      "albums.place",
      "albums.description",
      "albums.save",
      "albums.cancel",
      "albums.failed",
      "albums.membershipEmpty",
    ];
    for (const locale of locales) {
      setLocale(locale);
      for (const key of keys) expect(t(key), `${locale}:${key}`).not.toBe(key);
    }
  });
});

describe("Album list (TASK-03-008)", () => {
  it("lists albums with their photo counts", () => {
    render(AlbumList, {
      props: {
        albums: [album(), album({ id: 2, name: "Winter", count: 0, date: null, place: null })],
        selected: null,
        onselect: () => {},
        oncreate: () => {},
      },
    });

    expect(screen.getByTestId("album-1").textContent).toContain("Summer 2024");
    expect(screen.getByTestId("album-count-1").textContent).toBe("2");
    expect(screen.getByTestId("album-count-2").textContent).toBe("0");
  });

  it("shows an empty state and still offers to create one", () => {
    const oncreate = vi.fn();
    render(AlbumList, { props: { albums: [], selected: null, onselect: () => {}, oncreate } });

    expect(screen.getByTestId("albums-empty")).toBeTruthy();
    screen.getByTestId("album-new").click();
    expect(oncreate).toHaveBeenCalled();
  });

  it("clicking selects an album, clicking the selected one clears the selection", async () => {
    const onselect = vi.fn();
    const { rerender } = render(AlbumList, {
      props: { albums: [album()], selected: null, onselect, oncreate: () => {} },
    });

    await fireEvent.click(screen.getByTestId("album-1"));
    expect(onselect).toHaveBeenLastCalledWith(1);

    await rerender({ albums: [album()], selected: 1, onselect, oncreate: () => {} });
    expect(screen.getByTestId("album-1").getAttribute("aria-pressed")).toBe("true");
    await fireEvent.click(screen.getByTestId("album-1"));
    expect(onselect).toHaveBeenLastCalledWith(null);
  });

  it("right-click reports the album and the pointer position", async () => {
    const oncontextmenu = vi.fn();
    render(AlbumList, {
      props: {
        albums: [album()],
        selected: null,
        onselect: () => {},
        oncreate: () => {},
        oncontextmenu,
      },
    });

    await fireEvent.contextMenu(screen.getByTestId("album-1"), { clientX: 12, clientY: 34 });

    expect(oncontextmenu).toHaveBeenCalledWith({ album: album(), x: 12, y: 34 });
  });

  it("shows the cover through thumb:// only, never image bytes", () => {
    render(AlbumList, {
      props: {
        albums: [album({ cover_file_id: 7, cover_hash: "0123456789abcdef0123456789abcdef" })],
        selected: null,
        onselect: () => {},
        oncreate: () => {},
      },
    });

    const img = screen.getByTestId("album-cover-1") as HTMLImageElement;
    expect(img.getAttribute("src")).toBe("thumb://0123456789abcdef0123456789abcdef");
  });
});

describe("Album dialog (TASK-03-008)", () => {
  it("creates an album with name, date, place and description", async () => {
    const created = album({ id: 5, name: "Trip" });
    invokeMock.mockResolvedValue(created);
    const onsaved = vi.fn();
    render(AlbumDialog, { props: { open: true, onsaved } });

    await fireEvent.input(screen.getByTestId("album-name"), { target: { value: "Trip" } });
    await fireEvent.input(screen.getByTestId("album-date"), { target: { value: "2024-05-05" } });
    await fireEvent.input(screen.getByTestId("album-place"), { target: { value: "Rome" } });
    await fireEvent.input(screen.getByTestId("album-description"), {
      target: { value: "Long weekend" },
    });
    await fireEvent.click(screen.getByTestId("album-save"));
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("create_album", {
      name: "Trip",
      date: "2024-05-05",
      place: "Rome",
      description: "Long weekend",
    });
    expect(onsaved).toHaveBeenCalledWith(created);
  });

  it("defaults the date to today and sends blank place/description as null", async () => {
    invokeMock.mockResolvedValue(album());
    render(AlbumDialog, { props: { open: true, onsaved: () => {} } });

    const date = (screen.getByTestId("album-date") as HTMLInputElement).value;
    expect(date).toBe(todayIso());
    await fireEvent.input(screen.getByTestId("album-name"), { target: { value: "Plain" } });
    await fireEvent.click(screen.getByTestId("album-save"));
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("create_album", {
      name: "Plain",
      date,
      place: null,
      description: null,
    });
  });

  it("cannot be saved without a name", async () => {
    render(AlbumDialog, { props: { open: true, onsaved: () => {} } });

    expect((screen.getByTestId("album-save") as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.click(screen.getByTestId("album-save"));
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("edits an existing album with its fields prefilled", async () => {
    invokeMock.mockResolvedValue(album({ name: "Renamed" }));
    const onsaved = vi.fn();
    render(AlbumDialog, {
      props: { open: true, album: album({ description: "notes" }), onsaved },
    });

    expect((screen.getByTestId("album-name") as HTMLInputElement).value).toBe("Summer 2024");
    expect((screen.getByTestId("album-place") as HTMLInputElement).value).toBe("Lisbon");
    expect((screen.getByTestId("album-description") as HTMLTextAreaElement).value).toBe("notes");
    await fireEvent.input(screen.getByTestId("album-name"), { target: { value: "Renamed" } });
    await fireEvent.click(screen.getByTestId("album-save"));
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("update_album", {
      albumId: 1,
      name: "Renamed",
      date: "2024-07-14",
      place: "Lisbon",
      description: "notes",
    });
    expect(onsaved).toHaveBeenCalled();
  });

  it("keeps the dialog open and shows the backend error when saving fails", async () => {
    invokeMock.mockRejectedValue("album name is empty");
    const onsaved = vi.fn();
    render(AlbumDialog, { props: { open: true, onsaved } });
    await fireEvent.input(screen.getByTestId("album-name"), { target: { value: "x" } });

    await fireEvent.click(screen.getByTestId("album-save"));
    await settle();

    expect(screen.getByTestId("album-error").textContent).toContain("album name is empty");
    expect(onsaved).not.toHaveBeenCalled();
    expect(screen.getByTestId("album-dialog")).toBeTruthy();
  });

  it("Cancel closes without calling the backend", async () => {
    render(AlbumDialog, { props: { open: true, onsaved: () => {} } });

    await fireEvent.click(screen.getByTestId("album-cancel"));
    await settle();

    expect(screen.queryByTestId("album-dialog")).toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("Album membership in the info pane (TASK-03-008)", () => {
  function backend(memberOf: number[]): number[] {
    const state = [...memberOf];
    invokeMock.mockImplementation(((cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "list_albums") {
        return Promise.resolve([album(), album({ id: 2, name: "Winter", count: 0 })]);
      }
      if (cmd === "get_file_albums") return Promise.resolve([...state]);
      if (cmd === "add_to_album") {
        state.push(args?.albumId as number);
        return Promise.resolve(1);
      }
      if (cmd === "remove_from_album") {
        state.splice(state.indexOf(args?.albumId as number), 1);
        return Promise.resolve(1);
      }
      return Promise.resolve(null);
    }) as unknown as typeof tauriCore.invoke);
    return state;
  }

  it("shows every album, checked for the ones the photo is in", async () => {
    backend([1]);
    render(AlbumMembership, { props: { photoId: 9 } });
    await settle();

    expect((screen.getByTestId("album-member-1") as HTMLInputElement).checked).toBe(true);
    expect((screen.getByTestId("album-member-2") as HTMLInputElement).checked).toBe(false);
  });

  it("checking a box adds the photo, unchecking removes it", async () => {
    backend([1]);
    const onchanged = vi.fn();
    render(AlbumMembership, { props: { photoId: 9, onchanged } });
    await settle();

    await fireEvent.click(screen.getByTestId("album-member-2"));
    await settle();
    expect(invokeMock).toHaveBeenCalledWith("add_to_album", { albumId: 2, fileIds: [9] });
    expect((screen.getByTestId("album-member-2") as HTMLInputElement).checked).toBe(true);

    await fireEvent.click(screen.getByTestId("album-member-1"));
    await settle();
    expect(invokeMock).toHaveBeenCalledWith("remove_from_album", { albumId: 1, fileIds: [9] });
    expect((screen.getByTestId("album-member-1") as HTMLInputElement).checked).toBe(false);
    expect(onchanged).toHaveBeenCalledTimes(2);
  });

  it("points at the sidebar when there are no albums yet", async () => {
    invokeMock.mockResolvedValue([]);
    render(AlbumMembership, { props: { photoId: 9 } });
    await settle();

    expect(screen.getByTestId("album-membership-empty")).toBeTruthy();
  });

  it("shows an error and reverts the box when the write fails", async () => {
    invokeMock.mockImplementation(((cmd: string) => {
      if (cmd === "list_albums") return Promise.resolve([album()]);
      if (cmd === "get_file_albums") return Promise.resolve([]);
      return Promise.reject("sidecar not writable");
    }) as unknown as typeof tauriCore.invoke);
    render(AlbumMembership, { props: { photoId: 9 } });
    await settle();

    await fireEvent.click(screen.getByTestId("album-member-1"));
    await settle();

    expect(screen.getByTestId("album-membership-error").textContent).toContain(
      "sidecar not writable",
    );
    expect((screen.getByTestId("album-member-1") as HTMLInputElement).checked).toBe(false);
  });
});
