// TASK-01-018: transport wrapper for the library item queries. The grid's
// items come from `list_files` (plain browse) or `search_files` (live FTS
// search over filenames + folders); which one runs is decided by the query,
// not by the caller. IPC is mocked here — this module's job is the routing
// decision, the backend's is the SQL.

import { beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@tauri-apps/api/core";
import { listLibraryItems } from "./search";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  vi.mocked(invoke).mockReset().mockResolvedValue([]);
});

describe("listLibraryItems", () => {
  it("routes a whitespace-only query to the plain list (no folder filter)", async () => {
    await listLibraryItems([], "   ");
    expect(invoke).toHaveBeenCalledWith("list_files");
  });

  it("routes a folder selection to the filtered list", async () => {
    await listLibraryItems([1, 2], "");
    expect(invoke).toHaveBeenCalledWith("list_files", { folders: [1, 2] });
  });

  it("routes a query to FTS search with no folder filter", async () => {
    await listLibraryItems([], "beach");
    expect(invoke).toHaveBeenCalledWith("search_files", { query: "beach", folders: null });
  });

  it("searches the intersection of query and folder selection", async () => {
    await listLibraryItems([3], "beach ");
    expect(invoke).toHaveBeenCalledWith("search_files", { query: "beach", folders: [3] });
  });

  it("returns the backend rows unchanged", async () => {
    vi.mocked(invoke).mockResolvedValue([{ id: 7, filename: "beach.jpg", hash: null }]);
    await expect(listLibraryItems([], "beach")).resolves.toEqual([
      { id: 7, filename: "beach.jpg", hash: null },
    ]);
  });
});
