// TASK-03-007 tests: captions. The caption editor lives in the info pane;
// edits are written through `set_caption` (sidecar first, catalog second) and
// typing must never trigger the viewer's single-key shortcuts.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import CaptionEditor from "./CaptionEditor.svelte";
import Viewer from "./Viewer.svelte";
import type { GridItem } from "./grid";
import { isEditableTarget } from "./viewer";

vi.mock("@tauri-apps/api/core", () => {
  const invoke = vi.fn();
  return { invoke, convertFileSrc: (p: string) => `asset://test${p}`, __invoke: invoke };
});
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ setFullscreen: vi.fn() }),
}));

import * as tauriCore from "@tauri-apps/api/core";
const invokeMock = (tauriCore as unknown as { __invoke: ReturnType<typeof vi.fn> }).__invoke;

const settle = async (): Promise<void> => {
  await new Promise((r) => setTimeout(r, 0));
  flushSync();
};

/// Backend double: one stored caption per photo id, echoing what landed.
function backend(initial: Record<number, string | null> = {}): Record<number, string | null> {
  const store = { ...initial };
  invokeMock.mockImplementation(
    (cmd: string, args: { fileId?: number; caption?: string }) => {
      const id = args?.fileId ?? 0;
      if (cmd === "get_caption") return Promise.resolve(store[id] ?? null);
      if (cmd === "set_caption") {
        const text = (args.caption ?? "").trim() === "" ? null : (args.caption ?? "");
        store[id] = text;
        return Promise.resolve(text);
      }
      if (cmd === "get_tags") return Promise.resolve([]);
      return Promise.resolve(null);
    },
  );
  return store;
}

beforeEach(() => {
  invokeMock.mockReset();
});

afterEach(cleanup);

const box = (): HTMLTextAreaElement => screen.getByTestId("caption-input") as HTMLTextAreaElement;

describe("Caption editor (TASK-03-007)", () => {
  it("shows the photo's existing caption", async () => {
    backend({ 1: "sunset over the bay" });
    render(CaptionEditor, { props: { photoId: 1 } });
    await settle();

    expect(box().value).toBe("sunset over the bay");
    expect(invokeMock).toHaveBeenCalledWith("get_caption", { fileId: 1 });
  });

  it("saves an edited caption on blur via set_caption", async () => {
    backend({ 1: null });
    const onchanged = vi.fn();
    render(CaptionEditor, { props: { photoId: 1, onchanged } });
    await settle();

    await fireEvent.input(box(), { target: { value: "grandma's birthday" } });
    await fireEvent.blur(box());
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("set_caption", {
      fileId: 1,
      caption: "grandma's birthday",
    });
    expect(onchanged).toHaveBeenCalled();
  });

  it("does not write when nothing changed", async () => {
    backend({ 1: "same" });
    render(CaptionEditor, { props: { photoId: 1 } });
    await settle();

    await fireEvent.blur(box());
    await settle();

    expect(invokeMock).not.toHaveBeenCalledWith("set_caption", expect.anything());
  });

  it("supports multi-line captions: Enter adds a line, Ctrl+Enter saves", async () => {
    backend({ 1: null });
    render(CaptionEditor, { props: { photoId: 1 } });
    await settle();

    await fireEvent.input(box(), { target: { value: "line one\nline two" } });
    await fireEvent.keyDown(box(), { key: "Enter" });
    await settle();
    expect(invokeMock).not.toHaveBeenCalledWith("set_caption", expect.anything());

    await fireEvent.keyDown(box(), { key: "Enter", ctrlKey: true });
    await settle();
    expect(invokeMock).toHaveBeenCalledWith("set_caption", {
      fileId: 1,
      caption: "line one\nline two",
    });
  });

  it("emptying the caption clears it", async () => {
    const store = backend({ 1: "to remove" });
    render(CaptionEditor, { props: { photoId: 1 } });
    await settle();

    await fireEvent.input(box(), { target: { value: "" } });
    await fireEvent.blur(box());
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("set_caption", { fileId: 1, caption: "" });
    expect(store[1]).toBeNull();
    expect(box().value).toBe("");
  });

  it("shows what the backend stored (blank input comes back empty)", async () => {
    backend({ 1: null });
    render(CaptionEditor, { props: { photoId: 1 } });
    await settle();

    await fireEvent.input(box(), { target: { value: "  \n " } });
    await fireEvent.blur(box());
    await settle();

    expect(box().value).toBe("");
  });

  it("reloads when the viewer moves to another photo", async () => {
    backend({ 1: "first", 2: "second" });
    const { rerender } = render(CaptionEditor, { props: { photoId: 1 } });
    await settle();
    expect(box().value).toBe("first");

    await rerender({ photoId: 2 });
    await settle();

    expect(box().value).toBe("second");
  });

  it("reports a failed write and keeps the draft", async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "set_caption" ? Promise.reject("sidecar write failed") : Promise.resolve(null),
    );
    render(CaptionEditor, { props: { photoId: 1 } });
    await settle();

    await fireEvent.input(box(), { target: { value: "draft" } });
    await fireEvent.blur(box());
    await settle();

    expect(screen.getByTestId("caption-error").textContent).toContain("sidecar write failed");
    expect(box().value).toBe("draft");
  });

  it("renders nothing editable without a photo", async () => {
    backend();
    render(CaptionEditor, { props: { photoId: null } });
    await settle();

    expect(box().disabled).toBe(true);
  });
});

describe("typing in the viewer's info pane", () => {
  const items: GridItem[] = [
    { id: 1, filename: "a.jpg", hash: "a".repeat(32), rating: 0 },
    { id: 2, filename: "b.jpg", hash: "b".repeat(32), rating: 0 },
  ];

  it("isEditableTarget recognizes text fields only", () => {
    expect(isEditableTarget(document.createElement("textarea"))).toBe(true);
    expect(isEditableTarget(document.createElement("input"))).toBe(true);
    expect(isEditableTarget(document.createElement("button"))).toBe(false);
    expect(isEditableTarget(window)).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
  });

  it("single-key shortcuts are ignored while a caption is being typed", async () => {
    backend({ 1: null });
    render(Viewer, { items, startIndex: 0 });
    await fireEvent.keyDown(window, { key: "i" }); // open the info pane
    await settle();
    const input = box();

    for (const key of ["s", "i", "f", " ", "ArrowRight"]) {
      await fireEvent.keyDown(input, { key });
    }
    await settle();

    expect(invokeMock).not.toHaveBeenCalledWith("set_rating", expect.anything());
    expect(invokeMock).not.toHaveBeenCalledWith("get_caption", { fileId: 2 }); // no navigation
    expect(screen.queryByTestId("caption-input")).not.toBeNull(); // pane still open
    expect(screen.getByTestId("viewer")).toBeTruthy();
  });
});
