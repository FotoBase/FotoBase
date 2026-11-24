// TASK-09-005 tests: which files reach the share sheet, and the share UI
// (select mode in both grids, share bar, viewer button). IPC is mocked.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ShareBar from "./ShareBar.svelte";
import ThumbnailGrid from "./ThumbnailGrid.svelte";
import Viewer from "./Viewer.svelte";
import type { GridItem } from "./grid";
import { gridState, resetGridState } from "./grid-state.svelte.js";
import { shareIds, sharePhotos } from "./share";

vi.mock("@tauri-apps/api/core", () => {
  const invoke = vi.fn();
  return { invoke, convertFileSrc: (p: string) => `asset://test${p}`, __invoke: invoke };
});
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ setFullscreen: vi.fn() }),
}));

import * as tauriCore from "@tauri-apps/api/core";
const invokeMock = (tauriCore as unknown as { __invoke: ReturnType<typeof vi.fn> }).__invoke;

const HASH = (n: number) => n.toString(16).padStart(32, "0");
const makeItems = (count: number): GridItem[] =>
  Array.from({ length: count }, (_, i) => ({ id: i + 1, filename: `p${i}.jpg`, hash: HASH(i) }));

const settle = async (): Promise<void> => {
  await new Promise((r) => setTimeout(r, 0));
  flushSync();
};

function stubPointer(coarse: boolean): void {
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) => ({
      matches: coarse && query === "(pointer: coarse)",
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  );
}

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) =>
    Promise.resolve(cmd === "share_photos" ? { shared: 2, failed: 0 } : null),
  );
});

afterEach(() => {
  cleanup();
  resetGridState();
  vi.unstubAllGlobals();
});

describe("share ids", () => {
  it("lists the selection in grid order, then ids outside the view", () => {
    const view = [{ id: 5 }, { id: 3 }, { id: 9 }];
    expect(shareIds(new Set([9, 100, 5, 7]), view)).toEqual([5, 9, 100, 7]);
  });

  it("hands the ids to the share_photos command", async () => {
    const reply = await sharePhotos([4, 2]);
    expect(invokeMock).toHaveBeenCalledWith("share_photos", { ids: [4, 2] });
    expect(reply).toEqual({ shared: 2, failed: 0 });
  });
});

describe("ShareBar", () => {
  it("renders nothing without a touch pointer", () => {
    stubPointer(false);
    render(ShareBar, { photos: makeItems(3) });
    expect(screen.queryByTestId("share-bar")).toBeNull();
  });

  it("shares every selected photo in one call, in grid order", async () => {
    stubPointer(true);
    render(ShareBar, { photos: makeItems(4) });
    await fireEvent.click(screen.getByTestId("share-select-toggle"));
    gridState.selection = new Set([3, 1, 2]);
    flushSync();

    expect(screen.getByTestId("share-count").textContent).toContain("3");
    await fireEvent.click(screen.getByTestId("share-action"));
    await settle();

    const calls = invokeMock.mock.calls.filter(([cmd]) => cmd === "share_photos");
    expect(calls).toEqual([["share_photos", { ids: [1, 2, 3] }]]);
  });

  it("disables the share action until something is selected", async () => {
    stubPointer(true);
    render(ShareBar, { photos: makeItems(2) });
    await fireEvent.click(screen.getByTestId("share-select-toggle"));
    expect((screen.getByTestId("share-action") as HTMLButtonElement).disabled).toBe(true);
  });

  it("leaves select mode and clears the selection on cancel", async () => {
    stubPointer(true);
    render(ShareBar, { photos: makeItems(2) });
    await fireEvent.click(screen.getByTestId("share-select-toggle"));
    gridState.selection = new Set([1]);
    flushSync();
    await fireEvent.click(screen.getByTestId("share-cancel"));
    expect(gridState.selectMode).toBe(false);
    expect(gridState.selection.size).toBe(0);
  });

  it("shows the backend error and keeps the selection when sharing fails", async () => {
    stubPointer(true);
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "share_photos" ? Promise.reject("no photo could be prepared for sharing") : Promise.resolve(null),
    );
    render(ShareBar, { photos: makeItems(2) });
    await fireEvent.click(screen.getByTestId("share-select-toggle"));
    gridState.selection = new Set([1]);
    flushSync();
    await fireEvent.click(screen.getByTestId("share-action"));
    await settle();

    expect(screen.getByTestId("share-error").textContent).toContain("no photo could be prepared");
    expect(gridState.selection.size).toBe(1);
  });

  it("reports photos that were left out", async () => {
    stubPointer(true);
    invokeMock.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "share_photos" ? { shared: 1, failed: 1 } : null),
    );
    render(ShareBar, { photos: makeItems(2) });
    await fireEvent.click(screen.getByTestId("share-select-toggle"));
    gridState.selection = new Set([1, 2]);
    flushSync();
    await fireEvent.click(screen.getByTestId("share-action"));
    await settle();

    expect(screen.getByTestId("share-error").textContent).toContain("1");
  });
});

describe("grid select mode", () => {
  it("toggles selection instead of opening a photo on tap", async () => {
    stubPointer(true);
    gridState.selectMode = true;
    const onopen = vi.fn();
    render(ThumbnailGrid, { items: makeItems(4), onopen });
    const cells = screen.getAllByTestId("grid-cell");

    await fireEvent.click(cells[0]);
    await fireEvent.click(cells[1]);
    expect(onopen).not.toHaveBeenCalled();
    expect([...gridState.selection].sort()).toEqual([1, 2]);

    await fireEvent.click(cells[0]);
    expect([...gridState.selection]).toEqual([2]);
  });

  it("still opens on tap outside select mode", async () => {
    stubPointer(true);
    const onopen = vi.fn();
    render(ThumbnailGrid, { items: makeItems(2), onopen });
    await fireEvent.click(screen.getAllByTestId("grid-cell")[0]);
    expect(onopen).toHaveBeenCalledTimes(1);
  });
});

describe("viewer share", () => {
  it("shares the current photo from the touch viewer", async () => {
    stubPointer(true);
    render(Viewer, { items: makeItems(3), startIndex: 1 });
    await settle();
    await fireEvent.click(screen.getByTestId("viewer-share"));
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("share_photos", { ids: [2] });
  });

  it("shows the error when the share sheet could not open", async () => {
    stubPointer(true);
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "share_photos" ? Promise.reject("sheet refused") : Promise.resolve(null),
    );
    render(Viewer, { items: makeItems(1) });
    await settle();
    await fireEvent.click(screen.getByTestId("viewer-share"));
    await settle();

    expect(screen.getByTestId("share-error").textContent).toContain("sheet refused");
  });

  it("has no share button without a touch pointer", async () => {
    stubPointer(false);
    render(Viewer, { items: makeItems(1) });
    await settle();
    expect(screen.queryByTestId("viewer-share")).toBeNull();
  });
});
