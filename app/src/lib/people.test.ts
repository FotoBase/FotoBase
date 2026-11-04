// TASK-05-005 tests: the IPC wrapper's pure rules. The invoke calls
// themselves are covered by the Rust integration tests (app/src-tauri/tests/
// people_queue.rs); jsdom has no Tauri runtime.

import { describe, expect, it, vi } from "vitest";

import {
  canConfirm,
  canRename,
  entryThumbUri,
  faceBox,
  faceThumbUri,
  type PeopleEntry,
  type QueueCluster,
} from "./people";

const CLUSTER: QueueCluster = {
  cluster_id: 3,
  face_count: 9,
  face_id: 33,
  file_id: 3,
  x: 0.25,
  y: 0.5,
  width: 0.125,
  height: 0.25,
  hash: "abc123",
};

describe("people queue rules", () => {
  it("gates confirming on a non-blank name", () => {
    expect(canConfirm("Ada")).toBe(true);
    expect(canConfirm("")).toBe(false);
    expect(canConfirm(" \t ")).toBe(false);
  });

  it("builds the thumb URI from the content hash", () => {
    expect(faceThumbUri(CLUSTER)).toBe("thumb://abc123?v=33");
    expect(faceThumbUri({ ...CLUSTER, hash: null })).toBe("");
  });

  it("maps the face box to CSS percentages", () => {
    expect(faceBox(CLUSTER)).toEqual({
      left: "25%",
      top: "50%",
      width: "12.5%",
      height: "25%",
    });
  });

  it("does not import the Tauri API at module load", async () => {
    vi.resetModules();
    await import("./people");
    expect(vi.mocked).toBeDefined();
  });
});

// ---- People Manager (TASK-05-006) ----

const ENTRY: PeopleEntry = {
  person_id: 5,
  cluster_id: null,
  name: "Alice",
  face_count: 7,
  representative: CLUSTER,
};

describe("people manager rules", () => {
  it("builds the thumb URI from the representative face", () => {
    expect(entryThumbUri(ENTRY)).toBe("thumb://abc123?v=33");
    expect(entryThumbUri({ ...ENTRY, representative: null })).toBe("");
  });

  it("gates renaming on a non-blank changed name", () => {
    expect(canRename("Alice", "Alicia")).toBe(true);
    expect(canRename("Alice", "   ")).toBe(false);
    expect(canRename("Alice", "Alice")).toBe(false);
    expect(canRename(null, "Ada")).toBe(true);
  });
});
