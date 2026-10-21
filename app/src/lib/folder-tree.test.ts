// Folder-tree pure logic (TASK-01-015, PLAN §2B): real on-disk folders
// grouped under year nodes. Years sort newest-first; folders without a
// derivable year land in an "unknown" group at the end; folders sort by
// path inside each year so the tree is stable.

import { describe, expect, it } from "vitest";
import { folderName, groupFoldersByYear, type FolderInfo } from "./folder-tree";

const f = (id: number, path: string, year: number | null): FolderInfo => ({ id, path, year });

describe("groupFoldersByYear", () => {
  it("groups folders under their year, newest year first", () => {
    const nodes = groupFoldersByYear([
      f(1, "/photos/2021", 2021),
      f(2, "/photos/2023 a", 2023),
      f(3, "/photos/2023 b", 2023),
    ]);
    expect(nodes.map((n) => n.year)).toEqual([2023, 2021]);
    expect(nodes[0].folders.map((x) => x.id)).toEqual([2, 3]);
  });

  it("sorts folders by path within a year", () => {
    const nodes = groupFoldersByYear([
      f(1, "/z", 2022),
      f(2, "/a", 2022),
      f(3, "/m", 2022),
    ]);
    expect(nodes[0].folders.map((x) => x.path)).toEqual(["/a", "/m", "/z"]);
  });

  it("puts folders without a year in a trailing unknown group", () => {
    const nodes = groupFoldersByYear([
      f(1, "/no-date", null),
      f(2, "/2024", 2024),
      f(3, "/also-no-date", null),
    ]);
    expect(nodes.map((n) => n.year)).toEqual([2024, null]);
    expect(nodes[1].folders.map((x) => x.id)).toEqual([3, 1]);
  });

  it("returns an empty list for an empty catalog", () => {
    expect(groupFoldersByYear([])).toEqual([]);
  });
});

describe("folderName", () => {
  it("shows the last path segment", () => {
    expect(folderName("/photos/2023 trip")).toBe("2023 trip");
    expect(folderName("/")).toBe("");
  });
});
