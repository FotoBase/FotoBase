// TASK-03-006 tests: tag path helpers and the add/remove toggle decision.
// IPC is mocked; the module's job is the decision (add vs remove) and the
// nested-tag path handling.

import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import * as tauriCore from "@tauri-apps/api/core";
import {
  isHierarchical,
  leafName,
  normalizeTag,
  toggleTag,
  toggleTagInList,
} from "./tags";

const invokeMock = vi.mocked(tauriCore.invoke);

describe("tag path helpers (TASK-03-006)", () => {
  it("flat tags are not hierarchical", () => {
    expect(isHierarchical("sea")).toBe(false);
  });

  it("a | in the name makes the tag hierarchical", () => {
    expect(isHierarchical("Vacation|2024")).toBe(true);
  });

  it("leafName returns the last path segment", () => {
    expect(leafName("Vacation|2024")).toBe("2024");
    expect(leafName("sea")).toBe("sea");
  });

  it("normalizeTag trims and collapses separator runs", () => {
    expect(normalizeTag("  Vacation |  2024 ")).toBe("Vacation|2024");
    expect(normalizeTag("sea")).toBe("sea");
    expect(normalizeTag("   ")).toBe("");
  });
});

describe("toggleTagInList", () => {
  it("adds a missing tag", () => {
    expect(toggleTagInList([], "sea")).toEqual(["sea"]);
  });

  it("removes a present tag", () => {
    expect(toggleTagInList(["sea", "sun"], "sea")).toEqual(["sun"]);
  });
});

describe("toggleTag (backend round-trip)", () => {
  it("removes via remove_tag when the file already has the tag", async () => {
    invokeMock.mockResolvedValueOnce(["sea"]).mockResolvedValueOnce([]);
    const result = await toggleTag(7, "sea");
    expect(invokeMock).toHaveBeenNthCalledWith(1, "get_tags", { fileId: 7 });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "remove_tag", {
      fileId: 7,
      tag: "sea",
    });
    expect(result).toEqual([]);
  });

  it("adds via add_tag when the file lacks the tag", async () => {
    invokeMock.mockResolvedValueOnce([]).mockResolvedValueOnce(["sea"]);
    const result = await toggleTag(7, "sea");
    expect(invokeMock).toHaveBeenNthCalledWith(1, "get_tags", { fileId: 7 });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "add_tag", {
      fileId: 7,
      tag: "sea",
    });
    expect(result).toEqual(["sea"]);
  });
});
