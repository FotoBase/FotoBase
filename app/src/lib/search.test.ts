// TASK-03-014 (successor of TASK-01-018): transport wrapper for the library's
// one query. Folder selection, album view, live search text and the filter
// state all travel to the backend's single `query_files` command, so every
// combination ANDs in SQL. IPC is mocked here — this module's job is the
// argument mapping, the backend's is the SQL.

import { beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@tauri-apps/api/core";
import { queryLibraryItems, type LibraryView } from "./search";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  vi.mocked(invoke).mockReset().mockResolvedValue([]);
});

const view = (over: Partial<LibraryView> = {}): LibraryView => ({
  folders: [],
  album: null,
  query: "",
  starred: false,
  videosOnly: false,
  takenSince: null,
  ...over,
});

describe("queryLibraryItems", () => {
  it("sends an unfiltered view as all-null / false", async () => {
    await queryLibraryItems(view());
    expect(invoke).toHaveBeenCalledWith("query_files", {
      text: null,
      folders: null,
      album: null,
      starred: false,
      videosOnly: false,
      takenSince: null,
    });
  });

  it("treats a whitespace-only query as no search text", async () => {
    await queryLibraryItems(view({ query: "   " }));
    expect(vi.mocked(invoke).mock.calls[0][1]).toMatchObject({ text: null });
  });

  it("trims the search text", async () => {
    await queryLibraryItems(view({ query: " beach " }));
    expect(vi.mocked(invoke).mock.calls[0][1]).toMatchObject({ text: "beach" });
  });

  it("passes the folder selection and album through", async () => {
    await queryLibraryItems(view({ folders: [1, 2] }));
    expect(vi.mocked(invoke).mock.calls[0][1]).toMatchObject({ folders: [1, 2] });
    await queryLibraryItems(view({ album: 4 }));
    expect(vi.mocked(invoke).mock.calls[1][1]).toMatchObject({ album: 4 });
  });

  it("combines search with every filter in one call", async () => {
    await queryLibraryItems(
      view({ query: "beach", starred: true, videosOnly: true, takenSince: "2026-01-01" }),
    );
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("query_files", {
      text: "beach",
      folders: null,
      album: null,
      starred: true,
      videosOnly: true,
      takenSince: "2026-01-01",
    });
  });

  it("returns the backend rows unchanged", async () => {
    vi.mocked(invoke).mockResolvedValue([{ id: 7, filename: "beach.jpg", hash: null }]);
    await expect(queryLibraryItems(view({ query: "beach" }))).resolves.toEqual([
      { id: 7, filename: "beach.jpg", hash: null },
    ]);
  });
});
