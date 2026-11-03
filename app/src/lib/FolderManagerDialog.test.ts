// FolderManagerDialog tests (TASK-01-016, PLAN §2A): every catalog folder
// shows its mode and can switch between Scan Once / Scan Always; removal
// needs an explicit confirmation; the per-folder face-detection toggle
// (TASK-05-010, PLAN §2 Tools menu) sends its decision like every other
// control. IPC is mocked — the component's job is decisions and ordering,
// the wrappers' is transport.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import FolderManagerDialog from "./FolderManagerDialog.svelte";
import { removeFolder, setFolderFace, setFolderMode } from "./folder-manager";
import type { FolderInfo } from "./folder-tree";

vi.mock("./folder-manager", () => ({
  setFolderMode: vi.fn(),
  setFolderFace: vi.fn(),
  removeFolder: vi.fn(),
}));

afterEach(() => cleanup());

// flushSync flushes the synchronous DOM update; the component's handlers
// await their IPC mock, so one macrotask tick lets those continuations run.
const settle = async (): Promise<void> => {
  await flushSync();
  await new Promise((resolve) => setTimeout(resolve, 0));
};

const f = (
  id: number,
  path: string,
  scan_mode: FolderInfo["scan_mode"],
  face_detect = false,
): FolderInfo => ({ id, path, year: null, scan_mode, face_detect });

const FOLDERS: FolderInfo[] = [
  f(1, "/photos/2023 trip", "always", true),
  f(2, "/photos/archive", "once", false),
];

beforeEach(() => {
  vi.mocked(setFolderMode).mockReset().mockResolvedValue(undefined);
  vi.mocked(setFolderFace).mockReset().mockResolvedValue(undefined);
  vi.mocked(removeFolder).mockReset().mockResolvedValue(1);
});

describe("FolderManagerDialog", () => {
  it("renders nothing when closed", () => {
    render(FolderManagerDialog, {
      open: false,
      folders: FOLDERS,
      onchanged: vi.fn(),
    });
    expect(screen.queryByTestId("folder-manager-dialog")).toBeNull();
  });

  it("lists every folder with its current mode", () => {
    render(FolderManagerDialog, {
      open: true,
      folders: FOLDERS,
      onchanged: vi.fn(),
    });
    const watched = screen.getByTestId(
      "folder-mode-/photos/2023 trip",
    ) as HTMLSelectElement;
    const unwatched = screen.getByTestId(
      "folder-mode-/photos/archive",
    ) as HTMLSelectElement;
    expect(watched.value).toBe("always");
    expect(unwatched.value).toBe("once");
    expect(watched.options.length).toBe(2); // once / always
  });

  it("applying Scan Always sends the mode and refreshes the library", async () => {
    const onchanged = vi.fn();
    render(FolderManagerDialog, { open: true, folders: FOLDERS, onchanged });
    const select = screen.getByTestId(
      "folder-mode-/photos/archive",
    ) as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "always" } });
    await settle();
    expect(setFolderMode).toHaveBeenCalledWith("/photos/archive", "always");
    expect(onchanged).toHaveBeenCalled();
  });

  it("remove asks for confirmation before dropping the folder", async () => {
    const onchanged = vi.fn();
    render(FolderManagerDialog, { open: true, folders: FOLDERS, onchanged });

    // First click only reveals the confirmation — nothing is sent yet.
    fireEvent.click(screen.getByTestId("folder-remove-/photos/archive"));
    await flushSync();
    expect(removeFolder).not.toHaveBeenCalled();
    expect(
      screen.getByTestId("folder-confirm-remove-/photos/archive"),
    ).toBeTruthy();

    // Cancel backs out.
    fireEvent.click(screen.getByTestId("folder-cancel-remove-/photos/archive"));
    await flushSync();
    expect(
      screen.queryByTestId("folder-confirm-remove-/photos/archive"),
    ).toBeNull();

    // Confirm removes: the call goes out and the library refreshes.
    fireEvent.click(screen.getByTestId("folder-remove-/photos/archive"));
    await settle();
    fireEvent.click(
      screen.getByTestId("folder-confirm-remove-/photos/archive"),
    );
    await settle();
    expect(removeFolder).toHaveBeenCalledWith("/photos/archive");
    expect(onchanged).toHaveBeenCalled();
  });

  it("a failed mode change surfaces an error and does not refresh", async () => {
    vi.mocked(setFolderMode).mockRejectedValue("backend said no");
    const onchanged = vi.fn();
    render(FolderManagerDialog, { open: true, folders: FOLDERS, onchanged });
    fireEvent.change(screen.getByTestId("folder-mode-/photos/archive"), {
      target: { value: "always" },
    });
    await settle();
    expect(screen.getByTestId("folder-manager-error")).toBeTruthy();
    expect(onchanged).not.toHaveBeenCalled();
  });

  it("shows each folder's face-detection state in an enabled checkbox", () => {
    render(FolderManagerDialog, {
      open: true,
      folders: FOLDERS,
      onchanged: vi.fn(),
    });
    const on = screen.getByTestId(
      "face-toggle-/photos/2023 trip",
    ) as HTMLInputElement;
    const off = screen.getByTestId(
      "face-toggle-/photos/archive",
    ) as HTMLInputElement;
    expect(on.disabled).toBe(false);
    expect(off.disabled).toBe(false);
    expect(on.checked).toBe(true);
    expect(off.checked).toBe(false);
  });

  it("toggling face detection sends the decision and refreshes", async () => {
    const onchanged = vi.fn();
    render(FolderManagerDialog, { open: true, folders: FOLDERS, onchanged });
    fireEvent.click(screen.getByTestId("face-toggle-/photos/archive"));
    await settle();
    expect(setFolderFace).toHaveBeenCalledWith("/photos/archive", true);
    expect(onchanged).toHaveBeenCalled();
  });

  it("a failed face-detection change surfaces an error and does not refresh", async () => {
    vi.mocked(setFolderFace).mockRejectedValue("backend said no");
    const onchanged = vi.fn();
    render(FolderManagerDialog, { open: true, folders: FOLDERS, onchanged });
    fireEvent.click(screen.getByTestId("face-toggle-/photos/archive"));
    await settle();
    expect(screen.getByTestId("folder-manager-error")).toBeTruthy();
    expect(onchanged).not.toHaveBeenCalled();
  });
});
