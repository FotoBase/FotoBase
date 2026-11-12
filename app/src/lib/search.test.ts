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
  hasFaces: false,
  geotagged: false,
  takenSince: null,
  sort: "path",
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
      hasFaces: false,
      geotagged: false,
      takenSince: null,
      sort: "path",
    });
  });

  it("sends the geotagged filter (TASK-06-007)", async () => {
    await queryLibraryItems(view({ geotagged: true }));
    expect(vi.mocked(invoke).mock.calls[0][1]).toMatchObject({
      geotagged: true,
    });
  });

  it("sends the has-faces filter (TASK-05-007)", async () => {
    await queryLibraryItems(view({ hasFaces: true }));
    expect(vi.mocked(invoke).mock.calls[0][1]).toMatchObject({
      hasFaces: true,
    });
  });

  it("sends the capture-time sort (TASK-03-019)", async () => {
    await queryLibraryItems(view({ sort: "capture" }));
    expect(vi.mocked(invoke).mock.calls[0][1]).toMatchObject({
      sort: "capture",
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
    expect(vi.mocked(invoke).mock.calls[0][1]).toMatchObject({
      folders: [1, 2],
    });
    await queryLibraryItems(view({ album: 4 }));
    expect(vi.mocked(invoke).mock.calls[1][1]).toMatchObject({ album: 4 });
  });

  it("combines search with every filter in one call", async () => {
    await queryLibraryItems(
      view({
        query: "beach",
        starred: true,
        videosOnly: true,
        takenSince: "2026-01-01",
      }),
    );
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("query_files", {
      text: "beach",
      folders: null,
      album: null,
      starred: true,
      videosOnly: true,
      hasFaces: false,
      geotagged: false,
      takenSince: "2026-01-01",
      sort: "path",
    });
  });

  it("returns the backend rows unchanged", async () => {
    vi.mocked(invoke).mockResolvedValue([
      { id: 7, filename: "beach.jpg", hash: null },
    ]);
    await expect(queryLibraryItems(view({ query: "beach" }))).resolves.toEqual([
      { id: 7, filename: "beach.jpg", hash: null },
    ]);
  });
});

// TASK-07-003: the semantic blend path rides the same view into the
// backend's `query_files_semantic` command; a missing opt-in model degrades
// to FTS-only plus an enable hint (PLAN §6 P3).

import {
  modelMissingError,
  queryLibraryItemsSemantic,
  semanticContributed,
} from "./search";

describe("queryLibraryItemsSemantic", () => {
  it("sends the same view shape as queryLibraryItems", async () => {
    await queryLibraryItemsSemantic(view({ query: "beach" }));
    expect(invoke).toHaveBeenCalledWith("query_files_semantic", {
      text: "beach",
      folders: null,
      album: null,
      starred: false,
      videosOnly: false,
      hasFaces: false,
      geotagged: false,
      takenSince: null,
      sort: "path",
    });
  });

  it("returns null (fallback signal) when the model is not downloaded", async () => {
    vi.mocked(invoke).mockRejectedValueOnce("clip-text-model-missing");
    await expect(
      queryLibraryItemsSemantic(view({ query: "beach" })),
    ).resolves.toEqual({
      items: [],
      modelMissing: true,
    });
  });

  it("returns null on any backend failure — never blocks the grid", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("backend down"));
    await expect(
      queryLibraryItemsSemantic(view({ query: "beach" })),
    ).resolves.toBeNull();
  });

  it("returns the blended rows", async () => {
    const rows = [{ id: 1, filename: "a.jpg", hash: null }];
    vi.mocked(invoke).mockResolvedValueOnce(rows);
    await expect(
      queryLibraryItemsSemantic(view({ query: "beach" })),
    ).resolves.toEqual({
      items: rows,
      modelMissing: false,
    });
  });
});

describe("modelMissingError", () => {
  it("recognizes the backend's model-missing error", () => {
    expect(modelMissingError("clip-text-model-missing")).toBe(true);
    expect(modelMissingError(new Error("clip-text-model-missing"))).toBe(true);
    expect(modelMissingError("some other failure")).toBe(false);
  });
});

describe("semanticContributed", () => {
  const fts = [
    { id: 1, filename: "a.jpg", hash: null },
    { id: 2, filename: "b.jpg", hash: null },
  ];
  it("flags ids the FTS result did not have", () => {
    expect(
      semanticContributed(fts, [{ id: 3, filename: "c.jpg", hash: null }]),
    ).toBe(true);
    expect(semanticContributed(fts, [])).toBe(false);
  });
  it("stays false when the blend only reorders FTS hits", () => {
    expect(semanticContributed(fts, [fts[1], fts[0]])).toBe(false);
  });
});
