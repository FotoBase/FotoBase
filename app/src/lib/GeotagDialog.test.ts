// TASK-06-006 tests: the geotag dialog. One location applied to the
// selection, a whole folder, or a whole album; progress, per-photo
// failures, and undo. IPC is mocked at the wrapper module.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import GeotagDialog from "./GeotagDialog.svelte";
import {
  cancelGeotag,
  onGeotagProgress,
  runGeotagTool,
  undoGeotag,
  type GeotagProgress,
  type GeotagToolReport,
} from "./geotag-tool";
import { setLocale } from "./i18n.svelte.js";

vi.mock("./geotag-tool", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./geotag-tool")>()),
  runGeotagTool: vi.fn(),
  cancelGeotag: vi.fn(),
  undoGeotag: vi.fn(),
  onGeotagProgress: vi.fn(),
}));

const okReport: GeotagToolReport = { applied: 2, failed: [], cancelled: false };

let progressCb: ((p: GeotagProgress) => void) | null = null;

beforeEach(() => {
  setLocale("en");
  vi.mocked(runGeotagTool).mockReset().mockResolvedValue(okReport);
  vi.mocked(undoGeotag).mockReset().mockResolvedValue(2);
  vi.mocked(cancelGeotag).mockReset().mockResolvedValue(undefined);
  progressCb = null;
  vi.mocked(onGeotagProgress).mockReset().mockImplementation(async (cb) => {
    progressCb = cb;
    return () => {};
  });
});

afterEach(cleanup);

interface OpenArgs {
  ids?: number[];
  folderId?: number | null;
  albumId?: number | null;
}

function renderOpen({ ids = [1, 2], folderId = null, albumId = null }: OpenArgs = {}) {
  return render(GeotagDialog, {
    open: true,
    ids,
    folderId,
    albumId,
    onapplied: vi.fn(),
  });
}

const apply = (): HTMLButtonElement => screen.getByTestId("geotag-apply") as HTMLButtonElement;

async function typeCoords(lat = "52.52", lon = "13.405") {
  await fireEvent.input(screen.getByTestId("geotag-lat"), { target: { value: lat } });
  await fireEvent.input(screen.getByTestId("geotag-lon"), { target: { value: lon } });
}

describe("GeotagDialog", () => {
  it("renders nothing when closed", () => {
    render(GeotagDialog, { open: false, ids: [1], folderId: null, albumId: null, onapplied: vi.fn() });
    expect(screen.queryByTestId("geotag-dialog")).toBeNull();
  });

  it("keeps Apply disabled until both coordinates are valid", async () => {
    renderOpen();
    expect(apply().disabled).toBe(true);
    await fireEvent.input(screen.getByTestId("geotag-lat"), { target: { value: "91" } });
    await fireEvent.input(screen.getByTestId("geotag-lon"), { target: { value: "0" } });
    expect(apply().disabled).toBe(true);
    await fireEvent.input(screen.getByTestId("geotag-lat"), { target: { value: "52.52" } });
    expect(apply().disabled).toBe(false);
  });

  it("applies to the selection by default", async () => {
    renderOpen({ ids: [3, 4] });
    await typeCoords();
    await fireEvent.click(apply());
    await waitFor(() => expect(runGeotagTool).toHaveBeenCalledWith(
      { kind: "ids", ids: [3, 4] },
      52.52,
      13.405,
    ));
    await waitFor(() => expect(screen.getByTestId("geotag-done")).toBeTruthy());
  });

  it("targets the whole folder or album when chosen", async () => {
    renderOpen({ ids: [1], folderId: 7, albumId: 9 });
    await fireEvent.change(screen.getByTestId("geotag-scope"), { target: { value: "folder" } });
    await typeCoords();
    await fireEvent.click(apply());
    await waitFor(() => expect(runGeotagTool).toHaveBeenCalledWith(
      { kind: "folder", folder_id: 7 },
      52.52,
      13.405,
    ));

    cleanup();
    renderOpen({ ids: [1], folderId: 7, albumId: 9 });
    await fireEvent.change(screen.getByTestId("geotag-scope"), { target: { value: "album" } });
    await typeCoords();
    await fireEvent.click(apply());
    await waitFor(() => expect(runGeotagTool).toHaveBeenCalledWith(
      { kind: "album", album_id: 9 },
      52.52,
      13.405,
    ));
  });

  it("shows progress and the undo button after a run", async () => {
    let release: (r: GeotagToolReport) => void = () => {};
    vi.mocked(runGeotagTool).mockImplementation(
      () => new Promise<GeotagToolReport>((resolve) => (release = resolve)),
    );
    renderOpen();
    await typeCoords();
    await fireEvent.click(apply());
    await waitFor(() => expect(runGeotagTool).toHaveBeenCalled());
    progressCb?.({ done: 1, total: 2 });
    await waitFor(() => expect(screen.getByTestId("geotag-progress")).toBeTruthy());
    release(okReport);
    await waitFor(() => expect(screen.getByTestId("geotag-done")).toBeTruthy());
    expect(screen.getByTestId("geotag-undo")).toBeTruthy();
  });

  it("undoes the run and reports the restored count", async () => {
    renderOpen();
    await typeCoords();
    await fireEvent.click(apply());
    await waitFor(() => expect(screen.getByTestId("geotag-undo")).toBeTruthy());
    await fireEvent.click(screen.getByTestId("geotag-undo"));
    await waitFor(() => expect(undoGeotag).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("geotag-undone").textContent).toContain("2"));
  });

  it("cancels a running pass", async () => {
    vi.mocked(runGeotagTool).mockImplementation(
      () => new Promise<GeotagToolReport>(() => undefined),
    );
    renderOpen();
    await typeCoords();
    await fireEvent.click(apply());
    await fireEvent.click(screen.getByTestId("geotag-cancel"));
    await waitFor(() => expect(cancelGeotag).toHaveBeenCalled());
  });
});
