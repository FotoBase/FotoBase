// TASK-07-006 tests: burst suggestions ride the stack-suggestion path — the
// backend ranks the group best-first, so accepting a burst stacks with the
// suggested best on top, and the dialog marks it. IPC is mocked.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setLocale } from "./i18n.svelte.js";
import StackSuggestionsDialog from "./StackSuggestionsDialog.svelte";
import { stackPhotos, type StackSuggestion } from "./stacks";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const burst = (extra: Partial<StackSuggestion> = {}): StackSuggestion => ({
  kind: "burst",
  file_ids: [2, 1, 3],
  filenames: ["sharp.jpg", "blurry.jpg", "third.jpg"],
  best_file_id: 2,
  ...extra,
});

beforeEach(() => {
  setLocale("en");
  invoke.mockReset();
});
afterEach(cleanup);

describe("burst suggestions", () => {
  it("accepting a burst stacks with the suggested best first (it becomes the top)", async () => {
    invoke.mockResolvedValue([burst()]);
    const onapplied = vi.fn();
    render(StackSuggestionsDialog, { open: true, onapplied });
    await waitFor(() => expect(screen.getAllByTestId("stack-suggestion")).toHaveLength(1));

    await fireEvent.click(screen.getByTestId("stack-suggestion-accept"));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("stack_photos", { fileIds: [2, 1, 3] }));
    expect(onapplied).toHaveBeenCalledTimes(1);
  });

  it("marks the suggested best shot in the group's row", async () => {
    invoke.mockResolvedValue([burst()]);
    render(StackSuggestionsDialog, { open: true, onapplied: vi.fn() });
    await waitFor(() => expect(screen.getAllByTestId("stack-suggestion")).toHaveLength(1));

    const row = screen.getByTestId("stack-suggestion");
    expect(row.textContent).toContain("sharp.jpg");
    expect(screen.getByTestId("stack-suggestion-best").textContent).toContain("sharp.jpg");
  });

  it("a burst without a scored best shows no marker", async () => {
    invoke.mockResolvedValueOnce([
      { kind: "burst", file_ids: [1, 2], filenames: ["a.jpg", "b.jpg"] },
    ]);
    render(StackSuggestionsDialog, { open: true, onapplied: vi.fn() });
    await waitFor(() => expect(screen.getAllByTestId("stack-suggestion")).toHaveLength(1));
    expect(screen.queryByTestId("stack-suggestion-best")).toBeNull();
  });

  it("the wrapper forwards the backend's ranked order untouched", async () => {
    invoke.mockResolvedValueOnce([burst()]);
    const groups = await import("./stacks").then((m) => m.stackSuggestions());
    expect(groups[0].file_ids).toEqual([2, 1, 3]);
    expect(groups[0].best_file_id).toBe(2);
    expect(invoke).toHaveBeenLastCalledWith("stack_suggestions", undefined);
  });

  it("stackPhotos still accepts plain stacking calls", async () => {
    invoke.mockResolvedValueOnce("stack-9");
    expect(await stackPhotos([1, 2])).toBe("stack-9");
    expect(invoke).toHaveBeenLastCalledWith("stack_photos", { fileIds: [1, 2] });
  });
});
