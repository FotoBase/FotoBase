// Folder sort + view-mode logic (TASK-03-016, PLAN §6 P2a): pure comparator
// over the already-loaded folder list, plus the settings round-trip helpers.

import { describe, expect, it } from "vitest";
import {
  folderViewFromSettings,
  settingsWithFolderView,
  sortFolders,
  type FolderViewOptions,
} from "./folder-sort";
import { groupFoldersByYear, type FolderInfo } from "./folder-tree";
import { DEFAULT_SETTINGS } from "./settings";

const f = (
  id: number,
  path: string,
  extra: Partial<FolderInfo> = {},
): FolderInfo => ({ id, path, year: 2022, scan_mode: "once", ...extra });

const fixture: FolderInfo[] = [
  f(1, "/p/Beta", { file_count: 10, total_size: 500, newest_mtime: 2_000 }),
  f(2, "/p/alpha", { file_count: 3, total_size: 9_000, newest_mtime: 1_000 }),
  f(3, "/q/Gamma", { file_count: 7, total_size: 100, newest_mtime: 3_000 }),
  f(4, "/q/empty", { file_count: 0, total_size: 0, newest_mtime: null }),
];
const ids = (xs: FolderInfo[]): number[] => xs.map((x) => x.id);

describe("sortFolders", () => {
  it("name: case-insensitive by folder name, A to Z", () => {
    expect(ids(sortFolders(fixture, "name", false))).toEqual([2, 1, 4, 3]);
  });

  it("date: newest first, empty folders last", () => {
    expect(ids(sortFolders(fixture, "date", false))).toEqual([3, 1, 2, 4]);
  });

  it("size: largest first", () => {
    expect(ids(sortFolders(fixture, "size", false))).toEqual([2, 1, 3, 4]);
  });

  it("reverse flips every key", () => {
    expect(ids(sortFolders(fixture, "name", true))).toEqual([3, 4, 1, 2]);
    expect(ids(sortFolders(fixture, "date", true))).toEqual([4, 2, 1, 3]);
    expect(ids(sortFolders(fixture, "size", true))).toEqual([4, 3, 1, 2]);
  });

  it("ties fall back to path so the order is stable", () => {
    const same = [
      f(1, "/b", { total_size: 5 }),
      f(2, "/a", { total_size: 5 }),
    ];
    expect(ids(sortFolders(same, "size", false))).toEqual([2, 1]);
  });

  it("treats missing aggregates as empty and never mutates the input", () => {
    const input = [f(1, "/a"), f(2, "/b", { total_size: 1 })];
    const copy = [...input];
    expect(ids(sortFolders(input, "size", false))).toEqual([2, 1]);
    expect(input).toEqual(copy);
  });

  it("sorts a full-size library well inside a frame budget", () => {
    const big = Array.from({ length: 20_000 }, (_, i) =>
      f(i, `/lib/folder-${(i * 7919) % 20_000}`, {
        total_size: (i * 31) % 997,
        newest_mtime: (i * 17) % 1_000,
      }),
    );
    const start = performance.now();
    for (const key of ["name", "date", "size"] as const) sortFolders(big, key, false);
    expect(performance.now() - start).toBeLessThan(1_500);
  });
});

describe("groupFoldersByYear with a sort", () => {
  it("applies the sort inside each year group, years stay newest first", () => {
    const nodes = groupFoldersByYear(
      [
        f(1, "/a", { year: 2021, total_size: 1 }),
        f(2, "/b", { year: 2021, total_size: 9 }),
        f(3, "/c", { year: 2023, total_size: 5 }),
      ],
      { sort: "size", reverse: false },
    );
    expect(nodes.map((n) => n.year)).toEqual([2023, 2021]);
    expect(ids(nodes[1].folders)).toEqual([2, 1]);
  });
});

describe("folder view persistence helpers", () => {
  it("defaults to tree / name / not reversed (P1 behavior)", () => {
    expect(folderViewFromSettings(DEFAULT_SETTINGS)).toEqual({
      view: "tree",
      sort: "name",
      reverse: false,
    });
  });

  it("restores the persisted choice", () => {
    expect(
      folderViewFromSettings({
        ...DEFAULT_SETTINGS,
        folder_view: "flat",
        folder_sort: "size",
        folder_sort_reverse: true,
      }),
    ).toEqual({ view: "flat", sort: "size", reverse: true });
  });

  it("degrades unknown persisted values to the defaults", () => {
    const bogus = {
      ...DEFAULT_SETTINGS,
      folder_view: "carousel",
      folder_sort: "color",
    } as unknown as typeof DEFAULT_SETTINGS;
    expect(folderViewFromSettings(bogus)).toEqual({ view: "tree", sort: "name", reverse: false });
  });

  it("builds the settings to save only when something changed", () => {
    const same: FolderViewOptions = { view: "tree", sort: "name", reverse: false };
    expect(settingsWithFolderView(DEFAULT_SETTINGS, same)).toBeNull();
    expect(
      settingsWithFolderView(DEFAULT_SETTINGS, { view: "flat", sort: "date", reverse: true }),
    ).toEqual({
      ...DEFAULT_SETTINGS,
      folder_view: "flat",
      folder_sort: "date",
      folder_sort_reverse: true,
    });
  });
});
