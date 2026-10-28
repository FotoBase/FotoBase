// TASK-03-006 tests: the Quick Tags strip. One click toggles a tag on the
// whole multi-selection; the strip is a fixed 10-button panel fed from the
// app options.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import QuickTags from "./QuickTags.svelte";

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

describe("Quick Tags panel (TASK-03-006)", () => {
  it("renders exactly 10 buttons from the configured tags", () => {
    render(QuickTags, {
      props: { tags: ["sea", "family"], selectedIds: [1] },
    });
    const buttons = screen.getAllByTestId("quick-tag");
    expect(buttons).toHaveLength(10);
    expect(buttons[0].textContent).toContain("sea");
    expect(buttons[1].textContent).toContain("family");
    // Unconfigured slots stay as disabled placeholders.
    expect((buttons[2] as HTMLButtonElement).disabled).toBe(true);
  });

  it("one click applies the tag to every selected photo", async () => {
    // get_tags: photo 1 has the tag, photo 2 does not.
    invokeMock.mockImplementation((_cmd: string, args?: unknown) =>
      Promise.resolve(
        (args as { fileId?: number } | undefined)?.fileId === 1 ? ["sea"] : [],
      ),
    );
    render(QuickTags, {
      props: { tags: ["sea"], selectedIds: [1, 2] },
    });
    await settle();

    fireEvent.click(screen.getAllByTestId("quick-tag")[0]);
    await settle();

    // Photo 1 already had `sea` → removed; photo 2 lacked it → added.
    expect(invokeMock).toHaveBeenCalledWith("remove_tag", {
      fileId: 1,
      tag: "sea",
    });
    expect(invokeMock).toHaveBeenCalledWith("add_tag", {
      fileId: 2,
      tag: "sea",
    });
  });

  it("reports an error without aborting the remaining photos", async () => {
    invokeMock.mockImplementation((cmd: string, args?: unknown) => {
      const fileId = (args as { fileId?: number } | undefined)?.fileId;
      if (cmd === "get_tags") return Promise.resolve([]);
      if (fileId === 1) return Promise.reject(new Error("disk full"));
      return Promise.resolve(["sea"]);
    });
    render(QuickTags, {
      props: { tags: ["sea"], selectedIds: [1, 2] },
    });
    await settle();

    fireEvent.click(screen.getAllByTestId("quick-tag")[0]);
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("add_tag", {
      fileId: 2,
      tag: "sea",
    });
    expect(screen.getByTestId("quick-tags-error")).toBeTruthy();
  });

  it("no selection means nothing to click", () => {
    render(QuickTags, { props: { tags: ["sea"], selectedIds: [] } });
    const buttons = screen.getAllByTestId("quick-tag");
    expect(buttons.every((b) => (b as HTMLButtonElement).disabled)).toBe(true);
  });
});
