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
  linkContact,
  peopleEntries,
  renamePerson,
  searchContacts,
  unlinkContact,
  type PeopleEntry,
  type QueueCluster,
} from "./people";
import { setLocale } from "./i18n.svelte.js";

vi.mock("./people", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./people")>()),
  peopleEntries: vi.fn(),
  renamePerson: vi.fn(),
  searchContacts: vi.fn(),
  linkContact: vi.fn(),
  unlinkContact: vi.fn(),
}));

import {
  peopleEntries as entriesMock,
  renamePerson as renameMock,
  searchContacts as searchMock,
  linkContact as linkMock,
  unlinkContact as unlinkMock,
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
  contact_id: null,
  contact: null,
};
const UNNAMED: PeopleEntry = {
  person_id: null,
  cluster_id: 9,
  name: null,
  face_count: 3,
  representative: { ...REP, face_id: 202, file_id: 12 },
  contact_id: null,
  contact: null,
};
const BARE: PeopleEntry = {
  person_id: 6,
  cluster_id: null,
  name: "Lonely",
  face_count: 0,
  representative: null,
  contact_id: null,
  contact: null,
};

beforeEach(() => {
  setLocale("en");
  vi.mocked(entriesMock).mockReset().mockResolvedValue([NAMED, UNNAMED, BARE]);
  vi.mocked(renameMock)
    .mockReset()
    .mockResolvedValue({ id: 5, name: "Alicia" });
  vi.mocked(searchMock)
    .mockReset()
    .mockResolvedValue([
      { id: "os-1", name: "Alice Example", email: "alice@example.local" },
    ]);
  vi.mocked(linkMock)
    .mockReset()
    .mockResolvedValue({
      id: 5,
      name: "Alice",
      contact: { name: "Alice Example", email: "alice@example.local" },
    });
  vi.mocked(unlinkMock).mockReset().mockResolvedValue({ id: 5, name: "Alice" });
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

// ---- Contacts link (TASK-05-011, PLAN §2G local contacts only) ----

describe("people manager contacts link", () => {
  it("links a person to a local contact and shows the resolved contact", async () => {
    render(PeopleManagerDialog, {
      open: true,
      onchanged: () => {},
      onOpenQueue: () => {},
    });
    await waitFor(() =>
      expect(screen.getAllByTestId("pm-entry")).toHaveLength(3),
    );

    await fireEvent.click(screen.getAllByTestId("pm-link")[0]);
    await fireEvent.input(screen.getByTestId("pm-contact-query"), {
      target: { value: "alice" },
    });
    await fireEvent.click(screen.getByTestId("pm-contact-search"));
    await waitFor(() => expect(searchMock).toHaveBeenCalledWith("alice"));

    await fireEvent.click(screen.getAllByTestId("pm-contact-hit")[0]);
    await waitFor(() => expect(linkMock).toHaveBeenCalledWith(5, "os-1"));
    await waitFor(() =>
      expect(screen.getByTestId("pm-contact").textContent).toBe(
        "Alice Example <alice@example.local>",
      ),
    );
    // The link button is replaced by unlink once a contact is linked
    // (only Lonely's link button remains).
    expect(screen.queryAllByTestId("pm-link")).toHaveLength(1);
    expect(screen.getByTestId("pm-unlink")).toBeTruthy();
  });

  it("unlinks a person", async () => {
    vi.mocked(entriesMock).mockResolvedValue([
      {
        ...NAMED,
        contact_id: "os-1",
        contact: { name: "Alice Example", email: "alice@example.local" },
      },
      UNNAMED,
      BARE,
    ]);
    render(PeopleManagerDialog, {
      open: true,
      onchanged: () => {},
      onOpenQueue: () => {},
    });
    await waitFor(() => expect(screen.getByTestId("pm-contact")).toBeTruthy());
    await fireEvent.click(screen.getByTestId("pm-unlink"));
    await waitFor(() => expect(unlinkMock).toHaveBeenCalledWith(5));
    await waitFor(() => expect(screen.queryByTestId("pm-contact")).toBeNull());
    expect(screen.getAllByTestId("pm-link")).toHaveLength(2);
  });

  it("shows an empty local search politely", async () => {
    vi.mocked(searchMock).mockResolvedValue([]);
    render(PeopleManagerDialog, {
      open: true,
      onchanged: () => {},
      onOpenQueue: () => {},
    });
    await waitFor(() =>
      expect(screen.getAllByTestId("pm-entry")).toHaveLength(3),
    );
    await fireEvent.click(screen.getAllByTestId("pm-link")[0]);
    await fireEvent.input(screen.getByTestId("pm-contact-query"), {
      target: { value: "nobody" },
    });
    await fireEvent.click(screen.getByTestId("pm-contact-search"));
    await waitFor(() =>
      expect(screen.getByTestId("pm-contact-empty")).toBeTruthy(),
    );
    expect(linkMock).not.toHaveBeenCalled();
  });

  it("surfaces link errors", async () => {
    vi.mocked(linkMock).mockRejectedValue("no contact");
    render(PeopleManagerDialog, {
      open: true,
      onchanged: () => {},
      onOpenQueue: () => {},
    });
    await waitFor(() =>
      expect(screen.getAllByTestId("pm-entry")).toHaveLength(3),
    );
    await fireEvent.click(screen.getAllByTestId("pm-link")[0]);
    await fireEvent.input(screen.getByTestId("pm-contact-query"), {
      target: { value: "alice" },
    });
    await fireEvent.click(screen.getByTestId("pm-contact-search"));
    await fireEvent.click(screen.getAllByTestId("pm-contact-hit")[0]);
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
  });
});
