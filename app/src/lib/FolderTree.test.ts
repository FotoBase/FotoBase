// FolderTree component tests (TASK-01-015): year nodes render, clicking a
// folder toggles it in a multi-select, and right-click reports the folder
// for the context menu.

import { flushSync } from "svelte";
import { cleanup, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import FolderTree from "./FolderTree.svelte";
import type { FolderInfo } from "./folder-tree";

const f = (id: number, path: string, year: number | null): FolderInfo => ({ id, path, year });

afterEach(() => cleanup());

describe("FolderTree", () => {
  it("renders year nodes with their folders beneath", () => {
    render(FolderTree, {
      folders: [f(1, "/photos/2021", 2021), f(2, "/photos/2023", 2023)],
      selected: [],
      onselect: vi.fn(),
    });
    expect(screen.getByTestId("year-2023")).toBeTruthy();
    expect(screen.getByTestId("year-2021")).toBeTruthy();
    expect(screen.getByTestId("folder-2")).toBeTruthy();
    expect(screen.getByTestId("folder-1")).toBeTruthy();
  });

  it("clicking a folder selects it; clicking another adds to the selection", async () => {
    const onselect = vi.fn();
    render(FolderTree, {
      folders: [f(1, "/a", 2021), f(2, "/b", 2021)],
      selected: [],
      onselect,
    });

    (screen.getByTestId("folder-1") as HTMLElement).click();
    flushSync();
    expect(onselect).toHaveBeenLastCalledWith([1]);

    (screen.getByTestId("folder-2") as HTMLElement).click();
    flushSync();
    // Multi-select: both folders stay selected.
    expect(onselect).toHaveBeenLastCalledWith([1, 2]);
  });

  it("clicking a selected folder deselects it", async () => {
    const onselect = vi.fn();
    render(FolderTree, {
      folders: [f(1, "/a", 2021), f(2, "/b", 2021)],
      selected: [1, 2],
      onselect,
    });

    (screen.getByTestId("folder-1") as HTMLElement).click();
    flushSync();
    expect(onselect).toHaveBeenLastCalledWith([2]);
  });

  it("marks selected folders with aria-pressed", () => {
    render(FolderTree, {
      folders: [f(1, "/a", 2021)],
      selected: [1],
      onselect: vi.fn(),
    });
    expect(screen.getByTestId("folder-1").getAttribute("aria-pressed")).toBe("true");
  });

  it("right-click reports the folder and coordinates", () => {
    const onfoldercontextmenu = vi.fn();
    render(FolderTree, {
      folders: [f(1, "/a", 2021)],
      selected: [],
      onselect: vi.fn(),
      onfoldercontextmenu,
    });

    const ev = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 40,
      clientY: 60,
    });
    screen.getByTestId("folder-1").dispatchEvent(ev);
    flushSync();
    expect(onfoldercontextmenu).toHaveBeenCalledWith(
      expect.objectContaining({ folder: expect.objectContaining({ id: 1 }), x: 40, y: 60 }),
    );
  });

  it("an empty catalog renders the empty hint", () => {
    render(FolderTree, { folders: [], selected: [], onselect: vi.fn() });
    expect(screen.getByTestId("tree-empty")).toBeTruthy();
  });
});
