// FolderTree component tests (TASK-01-015): year nodes render, clicking a
// folder toggles it in a multi-select, and right-click reports the folder
// for the context menu.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import FolderTree from "./FolderTree.svelte";
import type { FolderInfo } from "./folder-tree";

const f = (
  id: number,
  path: string,
  year: number | null,
): FolderInfo => ({ id, path, year, scan_mode: "once" });

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

describe("FolderTree hidden folders (TASK-03-009)", () => {
  it("marks revealed hidden folders so they render grayed", () => {
    render(FolderTree, {
      folders: [{ ...f(1, "/a", 2021), hidden: true }, f(2, "/b", 2021)],
      selected: [],
      onselect: vi.fn(),
    });
    expect(screen.getByTestId("folder-1").getAttribute("data-hidden")).toBe("true");
    expect(screen.getByTestId("folder-2").getAttribute("data-hidden")).toBeNull();
  });
});

describe("FolderTree sort and view (TASK-03-016)", () => {
  const folders: FolderInfo[] = [
    { ...f(1, "/p/Beta", 2021), total_size: 5, newest_mtime: 10 },
    { ...f(2, "/p/alpha", 2023), total_size: 50, newest_mtime: 30 },
    { ...f(3, "/p/gamma", 2023), total_size: 500, newest_mtime: 20 },
  ];
  const flatIds = (): string[] =>
    [...document.querySelectorAll("[data-testid='folder-flat'] button")].map((b) =>
      b.getAttribute("data-testid") ?? "",
    );

  it("tree view (default) keeps the year groups and no flat list", () => {
    render(FolderTree, { folders, onselect: vi.fn() });
    expect(screen.getByTestId("year-2023")).toBeTruthy();
    expect(screen.queryByTestId("folder-flat")).toBeNull();
  });

  it("flat view lists every folder in one list, sorted, without year headings", () => {
    render(FolderTree, {
      folders,
      onselect: vi.fn(),
      options: { view: "flat", sort: "size", reverse: false },
    });
    expect(screen.queryByTestId("year-2023")).toBeNull();
    expect(flatIds()).toEqual(["folder-3", "folder-2", "folder-1"]);
  });

  it("reverse flips the flat list", () => {
    render(FolderTree, {
      folders,
      onselect: vi.fn(),
      options: { view: "flat", sort: "date", reverse: true },
    });
    expect(flatIds()).toEqual(["folder-1", "folder-3", "folder-2"]);
  });

  it("the toolbar reports view, sort, and reverse changes", async () => {
    const onoptionschange = vi.fn();
    render(FolderTree, { folders, onselect: vi.fn(), onoptionschange });

    await fireEvent.click(screen.getByTestId("folder-view-flat"));
    expect(onoptionschange).toHaveBeenLastCalledWith({
      view: "flat",
      sort: "name",
      reverse: false,
    });

    await fireEvent.change(screen.getByTestId("folder-sort"), { target: { value: "size" } });
    expect(onoptionschange).toHaveBeenLastCalledWith({
      view: "tree",
      sort: "size",
      reverse: false,
    });

    await fireEvent.click(screen.getByTestId("folder-sort-reverse"));
    expect(onoptionschange).toHaveBeenLastCalledWith({
      view: "tree",
      sort: "name",
      reverse: true,
    });
  });

  it("selection survives a view switch", async () => {
    const { rerender } = render(FolderTree, {
      folders,
      selected: [2],
      onselect: vi.fn(),
    });
    await rerender({ options: { view: "flat", sort: "name", reverse: false } });
    expect(screen.getByTestId("folder-2").getAttribute("aria-pressed")).toBe("true");
  });
});
