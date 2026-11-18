// TASK-07-008 tests: the "On this day" memories panel. Year sections render
// newest first, an empty result shows the friendly empty state, activating
// a photo hands the whole memory list to the viewer, and the date helper
// uses the local calendar day. The IPC wrapper is mocked; the query itself
// is covered by the catalog integration tests.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import MemoriesPanel from "./MemoriesPanel.svelte";
import { flattenMemories, loadMemories, localToday, type MemoryYear } from "./memories";
import { setLocale } from "./i18n.svelte.js";

vi.mock("./memories", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./memories")>()),
  loadMemories: vi.fn(),
}));

const photo = (id: number, hash: string | null) => ({ id, filename: `p${id}.jpg`, hash });
const YEARS: MemoryYear[] = [
  { year: 2024, files: [photo(2, "b".repeat(16))] },
  { year: 2023, files: [photo(1, "a".repeat(16)), photo(3, null)] },
];

beforeEach(() => {
  setLocale("en");
  vi.mocked(loadMemories).mockReset().mockResolvedValue(YEARS);
});
afterEach(cleanup);

describe("memories helpers", () => {
  it("formats the local calendar day, zero-padded", () => {
    expect(localToday(new Date(2024, 0, 5, 23, 59))).toBe("2024-01-05");
    expect(localToday(new Date(2024, 9, 20))).toBe("2024-10-20");
  });

  it("flattens year groups in display order", () => {
    expect(flattenMemories(YEARS).map((f) => f.id)).toEqual([2, 1, 3]);
  });
});

describe("MemoriesPanel", () => {
  it("renders one section per year, newest first, with its photos", async () => {
    render(MemoriesPanel, { open: true });
    const sections = await screen.findAllByTestId("memories-year");
    expect(sections.map((s) => s.dataset.year)).toEqual(["2024", "2023"]);
    expect(screen.getAllByTestId("memories-photo")).toHaveLength(3);
    expect(screen.queryByTestId("memories-empty")).toBeNull();
  });

  it("sends the grid view and today's date to the query", async () => {
    const view = {
      folders: [4],
      album: null,
      query: "beach",
      starred: false,
      videosOnly: false,
      hasFaces: false,
      geotagged: false,
      takenSince: null,
      sort: "path" as const,
    };
    render(MemoriesPanel, { open: true, view });
    await waitFor(() => expect(loadMemories).toHaveBeenCalledWith(localToday(), view));
  });

  it("shows the friendly empty state when there are no memories", async () => {
    vi.mocked(loadMemories).mockResolvedValue([]);
    render(MemoriesPanel, { open: true });
    await waitFor(() => expect(screen.getByTestId("memories-empty")).toBeTruthy());
    expect(screen.queryAllByTestId("memories-year")).toHaveLength(0);
  });

  it("opens the clicked photo with the full memory list", async () => {
    const onopenphoto = vi.fn();
    render(MemoriesPanel, { open: true, onopenphoto });
    const photos = await screen.findAllByTestId("memories-photo");
    await fireEvent.click(photos[1]);
    expect(onopenphoto).toHaveBeenCalledWith(YEARS[1].files[0], flattenMemories(YEARS));
  });

  it("reports a failed query without crashing", async () => {
    vi.mocked(loadMemories).mockRejectedValue("boom");
    render(MemoriesPanel, { open: true });
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("boom"));
  });
});
