// TASK-05-006 tests: the People Manager dialog. Named + unnamed entries
// with counts and representative thumbnails, rename with a non-blank name,
// and the unnamed queue entry point. The IPC wrapper module is mocked; the
// wrappers themselves are covered by the Rust integration tests
// (app/src-tauri/tests/people_manager.rs).

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import PeopleManagerDialog from "./PeopleManagerDialog.svelte";
import {
  entryThumbUri,
  peopleEntries,
  renamePerson,
  type PeopleEntry,
  type QueueCluster,
} from "./people";
import { setLocale } from "./i18n.svelte.js";

vi.mock("./people", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./people")>()),
  peopleEntries: vi.fn(),
  renamePerson: vi.fn(),
}));

import {
  peopleEntries as entriesMock,
  renamePerson as renameMock,
} from "./people";

const REP: QueueCluster = {
  cluster_id: 1,
  face_count: 7,
  face_id: 101,
  file_id: 11,
  x: 0.1,
  y: 0.2,
  width: 0.3,
  height: 0.25,
  hash: "a".repeat(16),
};
const NAMED: PeopleEntry = {
  person_id: 5,
  cluster_id: null,
  name: "Alice",
  face_count: 7,
  representative: REP,
};
const UNNAMED: PeopleEntry = {
  person_id: null,
  cluster_id: 9,
  name: null,
  face_count: 3,
  representative: { ...REP, face_id: 202, file_id: 12 },
};
const BARE: PeopleEntry = {
  person_id: 6,
  cluster_id: null,
  name: "Lonely",
  face_count: 0,
  representative: null,
};

beforeEach(() => {
  setLocale("en");
  vi.mocked(entriesMock).mockReset().mockResolvedValue([NAMED, UNNAMED, BARE]);
  vi.mocked(renameMock)
    .mockReset()
    .mockResolvedValue({ id: 5, name: "Alicia" });
});
afterEach(cleanup);

describe("people manager", () => {
  it("is closed by default", () => {
    const { container } = render(PeopleManagerDialog, {
      onchanged: () => {},
      onOpenQueue: () => {},
    });
    expect(container.querySelector(".pm-backdrop")).toBeNull();
    expect(entriesMock).not.toHaveBeenCalled();
  });

  it("lists named and unnamed entries with counts and thumbnails", async () => {
    render(PeopleManagerDialog, {
      open: true,
      onchanged: () => {},
      onOpenQueue: () => {},
    });
    await waitFor(() =>
      expect(screen.getAllByTestId("pm-entry")).toHaveLength(3),
    );
    expect(screen.getByText("Alice")).toBeTruthy();
    expect(screen.getByText("Lonely")).toBeTruthy();
    expect(entryThumbUri(NAMED)).toBe("thumb://aaaaaaaaaaaaaaaa?v=101");
    const imgs = screen.getAllByRole("img");
    expect(imgs).toHaveLength(2);
  });

  it("shows the empty state with no entries", async () => {
    vi.mocked(entriesMock).mockResolvedValue([]);
    render(PeopleManagerDialog, {
      open: true,
      onchanged: () => {},
      onOpenQueue: () => {},
    });
    await waitFor(() => expect(screen.getByTestId("pm-empty")).toBeTruthy());
  });

  it("renames a person with a non-blank name and reports the change", async () => {
    const onchanged = vi.fn();
    render(PeopleManagerDialog, {
      open: true,
      onchanged,
      onOpenQueue: () => {},
    });
    await waitFor(() =>
      expect(screen.getAllByTestId("pm-entry")).toHaveLength(3),
    );

    await fireEvent.click(screen.getAllByTestId("pm-rename")[0]);
    const input = screen.getByTestId("pm-rename-input") as HTMLInputElement;
    expect(input.value).toBe("Alice");
    await fireEvent.input(input, { target: { value: "  Alicia  " } });
    await fireEvent.click(screen.getByTestId("pm-rename-save"));

    await waitFor(() => expect(renameMock).toHaveBeenCalledWith(5, "Alicia"));
    expect(onchanged).toHaveBeenCalled();
    await waitFor(() => expect(screen.getByText("Alicia")).toBeTruthy());
  });

  it("refuses a blank rename", async () => {
    render(PeopleManagerDialog, {
      open: true,
      onchanged: () => {},
      onOpenQueue: () => {},
    });
    await waitFor(() =>
      expect(screen.getAllByTestId("pm-entry")).toHaveLength(3),
    );
    await fireEvent.click(screen.getAllByTestId("pm-rename")[0]);
    const input = screen.getByTestId("pm-rename-input") as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "   " } });
    expect(
      (screen.getByTestId("pm-rename-save") as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(renameMock).not.toHaveBeenCalled();
  });

  it("surfaces rename errors", async () => {
    vi.mocked(renameMock).mockRejectedValue("name in use");
    render(PeopleManagerDialog, {
      open: true,
      onchanged: () => {},
      onOpenQueue: () => {},
    });
    await waitFor(() =>
      expect(screen.getAllByTestId("pm-entry")).toHaveLength(3),
    );
    await fireEvent.click(screen.getAllByTestId("pm-rename")[0]);
    await fireEvent.input(screen.getByTestId("pm-rename-input"), {
      target: { value: "Bob" },
    });
    await fireEvent.click(screen.getByTestId("pm-rename-save"));
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
  });

  it("points unnamed clusters at the unnamed queue", async () => {
    const onOpenQueue = vi.fn();
    render(PeopleManagerDialog, {
      open: true,
      onchanged: () => {},
      onOpenQueue,
    });
    await waitFor(() =>
      expect(screen.getAllByTestId("pm-entry")).toHaveLength(3),
    );
    await fireEvent.click(screen.getByTestId("pm-review-queue"));
    expect(onOpenQueue).toHaveBeenCalledTimes(1);
  });
});
