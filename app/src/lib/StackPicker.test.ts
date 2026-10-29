// TASK-03-021 tests: the stack picker overlay (members, top-of-stack
// switch, unstack) and the suggestions dialog (offer to stack RAW+JPEG
// pairs and edited versions). IPC is mocked at the wrapper module.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import StackPicker from "./StackPicker.svelte";
import StackSuggestionsDialog from "./StackSuggestionsDialog.svelte";
import type { GridItem } from "./grid";
import { setLocale } from "./i18n.svelte.js";
import {
  listStack,
  setStackTop,
  stackPhotos,
  stackSuggestions,
  unstack,
  type StackSuggestion,
} from "./stacks";

vi.mock("./stacks", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./stacks")>()),
  listStack: vi.fn(),
  setStackTop: vi.fn(),
  unstack: vi.fn(),
  stackPhotos: vi.fn(),
  stackSuggestions: vi.fn(),
}));

const HASH = (n: number) => n.toString(16).padStart(32, "0");
const member = (id: number, name: string): GridItem => ({
  id,
  filename: name,
  hash: HASH(id),
  stack_id: "s1",
  stack_count: 2,
});

beforeEach(() => {
  setLocale("en");
  vi.mocked(listStack)
    .mockReset()
    .mockResolvedValue([member(1, "IMG_1.jpg"), member(2, "IMG_1.cr2")]);
  vi.mocked(setStackTop).mockReset().mockResolvedValue();
  vi.mocked(unstack).mockReset().mockResolvedValue();
  vi.mocked(stackPhotos).mockReset().mockResolvedValue("s1");
});

afterEach(cleanup);

describe("StackPicker", () => {
  it("renders nothing when closed", () => {
    render(StackPicker, { open: false, stackId: "s1", x: 0, y: 0, onchanged: vi.fn() });
    expect(screen.queryByTestId("stack-picker")).toBeNull();
  });

  it("expands the stack: every member, the top marked first", async () => {
    render(StackPicker, { open: true, stackId: "s1", x: 10, y: 20, onchanged: vi.fn() });

    const rows = await screen.findAllByTestId("stack-member");
    expect(rows.map((r) => r.textContent)).toEqual([
      expect.stringContaining("IMG_1.jpg"),
      expect.stringContaining("IMG_1.cr2"),
    ]);
    expect(rows[0].getAttribute("data-top")).toBe("true");
    expect(rows[1].getAttribute("data-top")).toBeNull();
    expect(listStack).toHaveBeenCalledWith("s1");
  });

  it("switches the top of the stack and refreshes", async () => {
    const onchanged = vi.fn();
    render(StackPicker, { open: true, stackId: "s1", x: 0, y: 0, onchanged });
    await screen.findAllByTestId("stack-member");
    vi.mocked(listStack).mockResolvedValue([member(2, "IMG_1.cr2"), member(1, "IMG_1.jpg")]);

    await fireEvent.click(screen.getByTestId("stack-make-top-2"));

    await waitFor(() => expect(setStackTop).toHaveBeenCalledWith(2));
    await waitFor(() => expect(onchanged).toHaveBeenCalled());
    const rows = await screen.findAllByTestId("stack-member");
    expect(rows[0].textContent).toContain("IMG_1.cr2");
    expect(rows[0].getAttribute("data-top")).toBe("true");
  });

  it("offers no make-top on the current top", async () => {
    render(StackPicker, { open: true, stackId: "s1", x: 0, y: 0, onchanged: vi.fn() });
    await screen.findAllByTestId("stack-member");
    expect(screen.queryByTestId("stack-make-top-1")).toBeNull();
  });

  it("unstacks, tells the shell and closes", async () => {
    const onchanged = vi.fn();
    render(StackPicker, { open: true, stackId: "s1", x: 0, y: 0, onchanged });
    await screen.findAllByTestId("stack-member");

    await fireEvent.click(screen.getByTestId("stack-unstack"));

    await waitFor(() => expect(unstack).toHaveBeenCalledWith("s1"));
    await waitFor(() => expect(onchanged).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByTestId("stack-picker")).toBeNull());
  });

  it("reports a failed change instead of failing silently", async () => {
    vi.mocked(setStackTop).mockRejectedValue("sidecar is read-only");
    render(StackPicker, { open: true, stackId: "s1", x: 0, y: 0, onchanged: vi.fn() });
    await screen.findAllByTestId("stack-member");

    await fireEvent.click(screen.getByTestId("stack-make-top-2"));

    expect((await screen.findByRole("alert")).textContent).toContain("sidecar is read-only");
  });

  it("closes on Escape", async () => {
    render(StackPicker, { open: true, stackId: "s1", x: 0, y: 0, onchanged: vi.fn() });
    await screen.findAllByTestId("stack-member");
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("stack-picker")).toBeNull();
  });
});

const suggestions: StackSuggestion[] = [
  { kind: "raw_jpeg", file_ids: [1, 2], filenames: ["IMG_1.JPG", "IMG_1.CR2"] },
  { kind: "edited", file_ids: [3, 4], filenames: ["beach.jpg", "beach-edit.jpg"] },
];

describe("StackSuggestionsDialog", () => {
  beforeEach(() => {
    vi.mocked(stackSuggestions).mockReset().mockResolvedValue(suggestions);
  });

  it("lists each suggested group with its kind and files", async () => {
    render(StackSuggestionsDialog, { open: true, onapplied: vi.fn() });

    const rows = await screen.findAllByTestId("stack-suggestion");
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain("RAW + JPEG");
    expect(rows[0].textContent).toContain("IMG_1.CR2");
    expect(rows[1].textContent).toContain("Edited version");
  });

  it("stacks one accepted group, top first, and drops it from the list", async () => {
    const onapplied = vi.fn();
    render(StackSuggestionsDialog, { open: true, onapplied });
    await screen.findAllByTestId("stack-suggestion");

    await fireEvent.click(screen.getAllByTestId("stack-suggestion-accept")[0]);

    await waitFor(() => expect(stackPhotos).toHaveBeenCalledWith([1, 2]));
    await waitFor(() => expect(screen.getAllByTestId("stack-suggestion")).toHaveLength(1));
    expect(onapplied).toHaveBeenCalled();
  });

  it("stacks every group on request", async () => {
    render(StackSuggestionsDialog, { open: true, onapplied: vi.fn() });
    await screen.findAllByTestId("stack-suggestion");

    await fireEvent.click(screen.getByTestId("stack-suggestions-accept-all"));

    await waitFor(() => expect(stackPhotos).toHaveBeenCalledTimes(2));
    expect(stackPhotos).toHaveBeenCalledWith([1, 2]);
    expect(stackPhotos).toHaveBeenCalledWith([3, 4]);
    await waitFor(() => expect(screen.getByTestId("stack-suggestions-empty")).toBeTruthy());
  });

  it("says so when there is nothing to stack", async () => {
    vi.mocked(stackSuggestions).mockResolvedValue([]);
    render(StackSuggestionsDialog, { open: true, onapplied: vi.fn() });
    expect(await screen.findByTestId("stack-suggestions-empty")).toBeTruthy();
  });

  it("keeps a group listed and reports the error when stacking fails", async () => {
    vi.mocked(stackPhotos).mockRejectedValue("no write access");
    render(StackSuggestionsDialog, { open: true, onapplied: vi.fn() });
    await screen.findAllByTestId("stack-suggestion");

    await fireEvent.click(screen.getAllByTestId("stack-suggestion-accept")[0]);

    expect((await screen.findByRole("alert")).textContent).toContain("no write access");
    expect(screen.getAllByTestId("stack-suggestion")).toHaveLength(2);
  });
});
