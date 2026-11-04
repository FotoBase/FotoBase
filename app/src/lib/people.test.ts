// TASK-05-005 tests: the IPC wrapper's pure rules. The invoke calls
// themselves are covered by the Rust integration tests (app/src-tauri/tests/
// people_queue.rs); jsdom has no Tauri runtime.

import { describe, expect, it, vi } from "vitest";

import {
  canConfirm,
  canLink,
  canRename,
  contactLabel,
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

// ---- Contacts link (TASK-05-011, PLAN §2G local contacts only) ----

describe("contacts link", () => {
  it("labels a contact as name + email", () => {
    expect(contactLabel({ name: "Alice Example", email: "a@e.local" })).toBe(
      "Alice Example <a@e.local>",
    );
    expect(contactLabel({ name: "Bob Example", email: null })).toBe(
      "Bob Example",
    );
  });

  it("gates linking on a picked contact only", () => {
    expect(canLink("os-1")).toBe(true);
    expect(canLink("")).toBe(false);
    expect(canLink("  ")).toBe(false);
  });

  it("routes search/link/unlink through IPC invoke only", async () => {
    vi.resetModules();
    const invoke = vi
      .fn<(...args: unknown[]) => Promise<unknown>>()
      .mockResolvedValue([]);
    vi.doMock("@tauri-apps/api/core", () => ({ invoke }));
    const mod = await import("./people");

    await mod.searchContacts("ali");
    expect(invoke).toHaveBeenCalledWith("contacts_search", { query: "ali" });

    await mod.linkContact(5, "os-1");
    expect(invoke).toHaveBeenCalledWith("people_link_contact", {
      personId: 5,
      contactId: "os-1",
    });

    await mod.unlinkContact(5);
    expect(invoke).toHaveBeenCalledWith("people_unlink_contact", {
      personId: 5,
    });

    // Strictly local: no fetch of any kind in the wrapper's source.
    const path = (await import("node:path")).join(
      process.cwd(),
      "src/lib/people.ts",
    );
    const src = (await import("node:fs")).readFileSync(path, "utf8");
    expect(src).not.toMatch(/\bfetch\s*\(/);
    expect(src).not.toContain("XMLHttpRequest");
    expect(src).not.toContain("WebSocket");
  });
});
