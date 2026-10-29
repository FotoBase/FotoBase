// TASK-03-022 tests: the folder ini migration offer. The scan only offers; the
// dialog lists the folders and the user migrates or declines each one. IPC is
// mocked at the wrapper module.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setLocale } from "./i18n.svelte.js";
import FolderIniDialog from "./FolderIniDialog.svelte";
import {
  declineFolderIni,
  ingestFolderIni,
  folderIniPendingFolders,
  type FolderIniReport,
} from "./folder-ini";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const report = (extra: Partial<FolderIniReport> = {}): FolderIniReport => ({
  ini_files: 1,
  matched: 3,
  written: 3,
  unchanged: 0,
  unmatched: 0,
  stars: 2,
  faces: 4,
  faces_skipped: 1,
  albums_created: 1,
  failed: [],
  ...extra,
});

beforeEach(() => {
  setLocale("en");
  invoke.mockReset();
});
afterEach(cleanup);

describe("folder ini IPC wrappers", () => {
  it("calls the backend commands with the folder path", async () => {
    invoke.mockResolvedValueOnce(["/a"]);
    expect(await folderIniPendingFolders()).toEqual(["/a"]);
    expect(invoke).toHaveBeenLastCalledWith("folder_ini_pending_folders", undefined);

    invoke.mockResolvedValueOnce(null);
    expect(await folderIniPendingFolders()).toEqual([]);

    invoke.mockResolvedValueOnce(report());
    expect((await ingestFolderIni("/a")).stars).toBe(2);
    expect(invoke).toHaveBeenLastCalledWith("ingest_folder_ini", { path: "/a" });

    invoke.mockResolvedValueOnce(undefined);
    await declineFolderIni("/a");
    expect(invoke).toHaveBeenLastCalledWith("decline_folder_ini", { path: "/a" });
  });
});

describe("FolderIniDialog", () => {
  it("renders nothing when closed", () => {
    render(FolderIniDialog, { open: false, folders: ["/a"], onmigrated: vi.fn() });
    expect(screen.queryByTestId("folder-ini-backdrop")).toBeNull();
  });

  it("lists the offered folders", () => {
    render(FolderIniDialog, { open: true, folders: ["/a", "/b"], onmigrated: vi.fn() });
    expect(screen.getAllByTestId("folder-ini-folder")).toHaveLength(2);
  });

  it("migrates a folder, shows what was converted and tells the shell", async () => {
    invoke.mockResolvedValueOnce(report());
    const onmigrated = vi.fn();
    render(FolderIniDialog, { open: true, folders: ["/a", "/b"], onmigrated });

    await fireEvent.click(screen.getAllByTestId("folder-ini-migrate")[0]);

    await waitFor(() => expect(onmigrated).toHaveBeenCalledTimes(1));
    expect(invoke).toHaveBeenCalledWith("ingest_folder_ini", { path: "/a" });
    expect(screen.getAllByTestId("folder-ini-folder")).toHaveLength(1);
    const text = screen.getByTestId("folder-ini-report").textContent ?? "";
    expect(text).toContain("/a");
    expect(text).toContain("2 stars");
    expect(text).toContain("1 albums created");
    expect(text).toContain("4 faces");
  });

  it("keeps the folder listed when some photos failed, so it can be retried", async () => {
    invoke.mockResolvedValueOnce(report({ failed: ["IMG_1.JPG"] }));
    render(FolderIniDialog, { open: true, folders: ["/a"], onmigrated: vi.fn() });

    await fireEvent.click(screen.getByTestId("folder-ini-migrate"));

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("1 photos"));
    expect(screen.getAllByTestId("folder-ini-folder")).toHaveLength(1);
  });

  it("shows a backend error and keeps the folder", async () => {
    invoke.mockRejectedValueOnce("boom");
    const onmigrated = vi.fn();
    render(FolderIniDialog, { open: true, folders: ["/a"], onmigrated });

    await fireEvent.click(screen.getByTestId("folder-ini-migrate"));

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("boom"));
    expect(onmigrated).not.toHaveBeenCalled();
    expect(screen.getAllByTestId("folder-ini-folder")).toHaveLength(1);
  });

  it("declines one folder for good without migrating anything", async () => {
    invoke.mockResolvedValueOnce(undefined);
    const onmigrated = vi.fn();
    render(FolderIniDialog, { open: true, folders: ["/a", "/b"], onmigrated });

    await fireEvent.click(screen.getAllByTestId("folder-ini-decline")[1]);

    await waitFor(() => expect(screen.getAllByTestId("folder-ini-folder")).toHaveLength(1));
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("decline_folder_ini", { path: "/b" });
    expect(onmigrated).not.toHaveBeenCalled();
  });

  it("closing keeps the offers pending (nothing is declined)", async () => {
    render(FolderIniDialog, { open: true, folders: ["/a"], onmigrated: vi.fn() });
    await fireEvent.click(screen.getByTestId("folder-ini-close"));
    expect(screen.queryByTestId("folder-ini-backdrop")).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });
});
