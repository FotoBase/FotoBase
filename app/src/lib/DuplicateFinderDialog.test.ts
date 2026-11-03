// TASK-04-019 tests: the duplicate finder dialog. Hashing report, group
// review with keep/suggest-delete (keep is the default — nothing is deleted
// unless the user marks it), the at-least-one-keeper rule, explicit
// confirmation before any delete, and the cancel path that deletes nothing.
// The IPC wrapper module is mocked; the wrappers themselves are covered by
// the Rust integration tests.

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import DuplicateFinderDialog from "./DuplicateFinderDialog.svelte";
import {
  canDelete,
  markedIds,
  type DuplicateGroup,
} from "./duplicates";
import { setLocale } from "./i18n.svelte.js";

vi.mock("./duplicates", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./duplicates")>()),
  findDuplicates: vi.fn(),
  duplicateGroups: vi.fn(),
  deleteDuplicateFiles: vi.fn(),
}));

import {
  deleteDuplicateFiles,
  duplicateGroups,
  findDuplicates,
} from "./duplicates";

const GROUP: DuplicateGroup = {
  file_ids: [11, 22, 33],
  paths: ["/p/a.jpg", "/p/b.jpg", "/p/c.jpg"],
  filenames: ["a.jpg", "b.jpg", "c.jpg"],
  hashes: ["a".repeat(32), "b".repeat(32), null],
};

beforeEach(() => {
  setLocale("en");
  vi.mocked(findDuplicates).mockReset().mockResolvedValue({ hashed: 4, failed: 0 });
  vi.mocked(duplicateGroups).mockReset().mockResolvedValue([GROUP]);
  vi.mocked(deleteDuplicateFiles).mockReset().mockResolvedValue(1);
});
afterEach(cleanup);

describe("review-state rules", () => {
  it("marks only what the user picked", () => {
    expect(markedIds(GROUP, new Set([22]))).toEqual([22]);
    expect(markedIds(GROUP, new Set())).toEqual([]);
  });

  it("refuses to delete a whole group", () => {
    expect(canDelete(GROUP, new Set([11, 22]))).toBe(true);
    expect(canDelete(GROUP, new Set([11, 22, 33]))).toBe(false);
    expect(canDelete(GROUP, new Set())).toBe(false);
  });
});

describe("DuplicateFinderDialog", () => {
  it("runs the finder and lists the groups", async () => {
    render(DuplicateFinderDialog, { open: true, onapplied: () => {} });
    await fireEvent.click(screen.getByTestId("dupes-run"));

    await waitFor(() => {
      expect(findDuplicates).toHaveBeenCalled();
      expect(screen.getAllByTestId("dupe-group").length).toBe(1);
    });
    const group = screen.getByTestId("dupe-group");
    expect(group.textContent).toContain("a.jpg");
    expect(group.textContent).toContain("c.jpg");
  });

  it("requires explicit confirmation before deleting, then refreshes", async () => {
    const onapplied = vi.fn();

    render(DuplicateFinderDialog, { open: true, onapplied });
    await fireEvent.click(screen.getByTestId("dupes-run"));
    await waitFor(() => expect(screen.getAllByTestId("dupe-member").length).toBe(3));

    // Mark one member for deletion.
    await fireEvent.click(screen.getAllByTestId("dupe-mark")[0]);
    // The delete button only appears with the confirm gate.
    await fireEvent.click(screen.getByTestId("dupes-delete"));
    expect(deleteDuplicateFiles).not.toHaveBeenCalled();

    await fireEvent.click(screen.getByTestId("dupes-confirm"));
    await waitFor(() => {
      expect(deleteDuplicateFiles).toHaveBeenCalledWith([11]);
      expect(onapplied).toHaveBeenCalled();
    });
  });

  it("cancel deletes nothing", async () => {
    render(DuplicateFinderDialog, { open: true, onapplied: () => {} });
    await fireEvent.click(screen.getByTestId("dupes-run"));
    await waitFor(() => expect(screen.getAllByTestId("dupe-member").length).toBe(3));

    await fireEvent.click(screen.getAllByTestId("dupe-mark")[0]);
    await fireEvent.click(screen.getByTestId("dupes-delete"));
    await fireEvent.click(screen.getByTestId("dupes-cancel-delete"));

    expect(deleteDuplicateFiles).not.toHaveBeenCalled();
    expect(screen.queryByTestId("dupes-confirm")).toBeNull();
  });

  it("shows the hashing report including decode failures", async () => {
    vi.mocked(findDuplicates).mockResolvedValue({ hashed: 3, failed: 1 });
    vi.mocked(duplicateGroups).mockResolvedValue([]);

    render(DuplicateFinderDialog, { open: true, onapplied: () => {} });
    await fireEvent.click(screen.getByTestId("dupes-run"));
    await waitFor(() => {
      const status = screen.getByTestId("dupes-status");
      expect(status.textContent).toContain("3");
      expect(status.textContent).toContain("1");
      expect(screen.getByTestId("dupes-empty")).toBeDefined();
    });
  });
});
