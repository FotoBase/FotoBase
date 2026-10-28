// TASK-03-006 tests: the tag editor. Add/remove/rename tags on the photo
// the viewer shows; nested tags are typed with a `|` path separator.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import TagEditor from "./TagEditor.svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import * as tauriCore from "@tauri-apps/api/core";

const invokeMock = vi.mocked(tauriCore.invoke);

const settle = async (): Promise<void> => {
  await new Promise((r) => setTimeout(r, 0));
  flushSync();
};

beforeEach(() => {
  invokeMock.mockReset();
});

afterEach(cleanup);

describe("Tag editor (TASK-03-006)", () => {
  it("lists the photo's tags with a remove button each", async () => {
    invokeMock.mockResolvedValue(["sea", "Vacation|2024"]);
    render(TagEditor, { props: { photoId: 1 } });
    await settle();

    expect(screen.getByTestId("tag-chip-sea")).toBeTruthy();
    expect(screen.getByTestId("tag-chip-Vacation|2024").textContent).toContain(
      "2024",
    );
    expect(screen.getAllByTestId("tag-remove")).toHaveLength(2);
  });

  it("typing a tag and pressing enter adds it via add_tag", async () => {
    invokeMock.mockResolvedValue(["sea"]);
    render(TagEditor, { props: { photoId: 1 } });
    await settle();

    const input = screen.getByTestId("tag-input") as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "sea" } });
    await fireEvent.keyDown(input, { key: "Enter" });
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("add_tag", {
      fileId: 1,
      tag: "sea",
    });
    expect(screen.getByTestId("tag-chip-sea")).toBeTruthy();
  });

  it("a nested tag is normalized to an A|B path before sending", async () => {
    invokeMock.mockResolvedValue(["Vacation|2024"]);
    render(TagEditor, { props: { photoId: 1 } });
    await settle();

    const input = screen.getByTestId("tag-input") as HTMLInputElement;
    await fireEvent.input(input, { target: { value: " Vacation | 2024 " } });
    await fireEvent.keyDown(input, { key: "Enter" });
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("add_tag", {
      fileId: 1,
      tag: "Vacation|2024",
    });
  });

  it("the remove button calls remove_tag", async () => {
    invokeMock.mockResolvedValue(["sea"]);
    render(TagEditor, { props: { photoId: 1 } });
    await settle();

    fireEvent.click(screen.getAllByTestId("tag-remove")[0]);
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("remove_tag", {
      fileId: 1,
      tag: "sea",
    });
  });

  it("rename calls rename_tag with old and new name", async () => {
    invokeMock.mockResolvedValue(1);
    render(TagEditor, { props: { photoId: 1 } });
    await settle();

    await fireEvent.input(screen.getByTestId("rename-old"), {
      target: { value: "sea" },
    });
    await fireEvent.input(screen.getByTestId("rename-new"), {
      target: { value: "ocean" },
    });
    fireEvent.click(screen.getByTestId("rename-submit"));
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("rename_tag", {
      old: "sea",
      new: "ocean",
    });
  });
});
