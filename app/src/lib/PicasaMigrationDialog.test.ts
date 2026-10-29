// TASK-03-022 tests: the Picasa.ini migration offer. The scan only offers; the
// dialog lists the folders and the user migrates or declines each one. IPC is
// mocked at the wrapper module.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setLocale } from "./i18n.svelte.js";
import PicasaMigrationDialog from "./PicasaMigrationDialog.svelte";
import {
  declinePicasaFolder,
  ingestPicasaFolder,
  picasaPendingFolders,
  type PicasaReport,
} from "./picasa";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const report = (extra: Partial<PicasaReport> = {}): PicasaReport => ({
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

describe("picasa IPC wrappers", () => {
  it("calls the backend commands with the folder path", async () => {
    invoke.mockResolvedValueOnce(["/a"]);
    expect(await picasaPendingFolders()).toEqual(["/a"]);
    expect(invoke).toHaveBeenLastCalledWith("picasa_pending_folders", undefined);

    invoke.mockResolvedValueOnce(null);
    expect(await picasaPendingFolders()).toEqual([]);

    invoke.mockResolvedValueOnce(report());
    expect((await ingestPicasaFolder("/a")).stars).toBe(2);
    expect(invoke).toHaveBeenLastCalledWith("picasa_ingest_folder", { path: "/a" });

    invoke.mockResolvedValueOnce(undefined);
    await declinePicasaFolder("/a");
    expect(invoke).toHaveBeenLastCalledWith("picasa_decline_folder", { path: "/a" });
  });
});

describe("PicasaMigrationDialog", () => {
  it("renders nothing when closed", () => {
    render(PicasaMigrationDialog, { open: false, folders: ["/a"], onmigrated: vi.fn() });
    expect(screen.queryByTestId("picasa-backdrop")).toBeNull();
  });

  it("lists the offered folders", () => {
    render(PicasaMigrationDialog, { open: true, folders: ["/a", "/b"], onmigrated: vi.fn() });
    expect(screen.getAllByTestId("picasa-folder")).toHaveLength(2);
  });

  it("migrates a folder, shows what was converted and tells the shell", async () => {
    invoke.mockResolvedValueOnce(report());
    const onmigrated = vi.fn();
    render(PicasaMigrationDialog, { open: true, folders: ["/a", "/b"], onmigrated });

    await fireEvent.click(screen.getAllByTestId("picasa-migrate")[0]);

    await waitFor(() => expect(onmigrated).toHaveBeenCalledTimes(1));
    expect(invoke).toHaveBeenCalledWith("picasa_ingest_folder", { path: "/a" });
    expect(screen.getAllByTestId("picasa-folder")).toHaveLength(1);
    const text = screen.getByTestId("picasa-report").textContent ?? "";
    expect(text).toContain("/a");
    expect(text).toContain("2 stars");
    expect(text).toContain("1 albums created");
    expect(text).toContain("4 faces");
  });

  it("keeps the folder listed when some photos failed, so it can be retried", async () => {
    invoke.mockResolvedValueOnce(report({ failed: ["IMG_1.JPG"] }));
    render(PicasaMigrationDialog, { open: true, folders: ["/a"], onmigrated: vi.fn() });

    await fireEvent.click(screen.getByTestId("picasa-migrate"));

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("1 photos"));
    expect(screen.getAllByTestId("picasa-folder")).toHaveLength(1);
  });

  it("shows a backend error and keeps the folder", async () => {
    invoke.mockRejectedValueOnce("boom");
    const onmigrated = vi.fn();
    render(PicasaMigrationDialog, { open: true, folders: ["/a"], onmigrated });

    await fireEvent.click(screen.getByTestId("picasa-migrate"));

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("boom"));
    expect(onmigrated).not.toHaveBeenCalled();
    expect(screen.getAllByTestId("picasa-folder")).toHaveLength(1);
  });

  it("declines one folder for good without migrating anything", async () => {
    invoke.mockResolvedValueOnce(undefined);
    const onmigrated = vi.fn();
    render(PicasaMigrationDialog, { open: true, folders: ["/a", "/b"], onmigrated });

    await fireEvent.click(screen.getAllByTestId("picasa-decline")[1]);

    await waitFor(() => expect(screen.getAllByTestId("picasa-folder")).toHaveLength(1));
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("picasa_decline_folder", { path: "/b" });
    expect(onmigrated).not.toHaveBeenCalled();
  });

  it("closing keeps the offers pending (nothing is declined)", async () => {
    render(PicasaMigrationDialog, { open: true, folders: ["/a"], onmigrated: vi.fn() });
    await fireEvent.click(screen.getByTestId("picasa-close"));
    expect(screen.queryByTestId("picasa-backdrop")).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });
});
