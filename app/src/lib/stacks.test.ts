// TASK-03-021 tests: stacking helpers — badge rules, the badge's hit area on
// the canvas grid, which photos the menu may stack, and the IPC wrappers.

import { beforeEach, describe, expect, it, vi } from "vitest";

import type { GridItem } from "./grid";
import {
  badgeText,
  canStack,
  hasStackBadge,
  inStackBadge,
  listStack,
  setStackTop,
  stackPhotos,
  stackSuggestions,
  unstack,
  STACK_BADGE,
} from "./stacks";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const item = (id: number, extra: Partial<GridItem> = {}): GridItem => ({
  id,
  filename: `p${id}.jpg`,
  hash: null,
  ...extra,
});

beforeEach(() => invoke.mockReset());

describe("stack badge", () => {
  it("shows for a stack of two or more and carries the size", () => {
    expect(hasStackBadge(item(1, { stack_id: "s", stack_count: 3 }))).toBe(true);
    expect(badgeText(item(1, { stack_id: "s", stack_count: 3 }))).toBe("3");
  });

  it("does not show for a plain photo, a lone leftover member, or an old payload", () => {
    expect(hasStackBadge(item(1))).toBe(false);
    expect(hasStackBadge(item(1, { stack_id: null, stack_count: 0 }))).toBe(false);
    expect(hasStackBadge(item(1, { stack_id: "s", stack_count: 1 }))).toBe(false);
  });

  it("hits only inside the top-right corner of the cell", () => {
    const size = 160;
    const { width, height, margin } = STACK_BADGE;
    expect(inStackBadge(size - margin - 1, margin + 1, size)).toBe(true);
    expect(inStackBadge(size - margin - width, margin, size)).toBe(true);
    expect(inStackBadge(10, 10, size)).toBe(false);
    expect(inStackBadge(size - 2, margin + 1, size)).toBe(false);
    expect(inStackBadge(size - margin - 1, margin + height + 2, size)).toBe(false);
  });
});

describe("canStack", () => {
  it("needs two different photos", () => {
    expect(canStack([item(1)])).toBe(false);
    expect(canStack([item(1), item(2)])).toBe(true);
  });

  it("allows joining one existing stack but not merging two", () => {
    expect(canStack([item(1, { stack_id: "a", stack_count: 2 }), item(2)])).toBe(true);
    expect(
      canStack([item(1, { stack_id: "a", stack_count: 2 }), item(2, { stack_id: "b", stack_count: 2 })]),
    ).toBe(false);
  });

  it("is false when everything is already in the same stack", () => {
    expect(
      canStack([item(1, { stack_id: "a", stack_count: 2 }), item(2, { stack_id: "a", stack_count: 2 })]),
    ).toBe(false);
  });
});

describe("stack IPC", () => {
  it("stacks, switches the top and unstacks through the backend commands", async () => {
    invoke.mockResolvedValueOnce("stack-1");
    expect(await stackPhotos([3, 4])).toBe("stack-1");
    expect(invoke).toHaveBeenLastCalledWith("stack_photos", { fileIds: [3, 4] });

    await setStackTop(4);
    expect(invoke).toHaveBeenLastCalledWith("set_stack_top", { fileId: 4 });

    await unstack("stack-1");
    expect(invoke).toHaveBeenLastCalledWith("unstack", { stackId: "stack-1" });
  });

  it("lists members and suggestions, tolerating a null answer", async () => {
    invoke.mockResolvedValueOnce([item(1)]);
    expect(await listStack("s")).toHaveLength(1);
    expect(invoke).toHaveBeenLastCalledWith("list_stack", { stackId: "s" });

    invoke.mockResolvedValueOnce(null);
    expect(await stackSuggestions()).toEqual([]);
    expect(invoke).toHaveBeenLastCalledWith("stack_suggestions", undefined);
  });
});
