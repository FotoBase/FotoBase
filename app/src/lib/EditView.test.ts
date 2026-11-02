// TASK-04-010 tests: the edit view — tabs, filmstrip, live preview, undo.
// IPC is mocked; the component's job is composing the panel states into a
// recipe (edit-recipe.ts), driving the debounced render, and never losing
// the original (checksums are the backend integration tests' job).

import { flushSync } from "svelte";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import EditView from "./EditView.svelte";
import type { GridItem } from "./grid";

vi.mock("@tauri-apps/api/core", () => {
  const invoke = vi.fn();
  return { invoke, __invoke: invoke };
});
vi.mock("@tauri-apps/plugin-dialog", () => {
  const save = vi.fn();
  return { save, __save: save };
});
import * as pluginDialog from "@tauri-apps/plugin-dialog";
const saveDialogMock = (
  pluginDialog as unknown as { __save: ReturnType<typeof vi.fn> }
).__save;
import * as tauriCore from "@tauri-apps/api/core";
const invokeMock = (
  tauriCore as unknown as { __invoke: ReturnType<typeof vi.fn> }
).__invoke;

const HASH = (n: number) => n.toString(16).padStart(32, "0");

function items(count: number): GridItem[] {
  return Array.from({ length: count }, (_, i) => ({
    id: i + 1,
    filename: `p${i}.jpg`,
    hash: HASH(i),
  }));
}

const settle = async (): Promise<void> => {
  await new Promise((r) => setTimeout(r, 0));
  flushSync();
};

// Fake-timer-safe settle: microtasks only, never a macrotask the fake
// clock would never fire.
const settleMicro = async (): Promise<void> => {
  for (let i = 0; i < 6; i++) await Promise.resolve();
  flushSync();
};

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(null);
  saveDialogMock.mockReset();
  saveDialogMock.mockResolvedValue(null);
});

afterEach(cleanup);

function props() {
  return { items: items(3), startIndex: 0, onclose: vi.fn() };
}

describe("EditView", () => {
  it("renders the three edit tabs and the filmstrip of siblings", () => {
    render(EditView, props());
    expect(screen.getByTestId("edit-tab-basic")).toBeTruthy();
    expect(screen.getByTestId("edit-tab-tuning")).toBeTruthy();
    expect(screen.getByTestId("edit-tab-effects")).toBeTruthy();
    const thumbs = screen.getAllByTestId("edit-filmstrip-thumb");
    expect(thumbs.length).toBe(3);
  });

  it("the preview img points at the photo's preview:// URI", () => {
    render(EditView, props());
    const img = screen.getByTestId("edit-preview") as HTMLImageElement;
    expect(img.src).toContain(`preview://${HASH(0)}`);
  });

  it("loads an existing recipe for the photo on open", async () => {
    invokeMock.mockImplementation((_cmd: string, args: { fileId: number }) =>
      _cmd === "get_recipe"
        ? Promise.resolve(
            JSON.stringify({
              spec: "fotobase.recipe",
              version: 1,
              ops: [{ type: "bw", params: { amount: 1 } }],
            }),
          )
        : Promise.resolve(null),
    );
    render(EditView, props());
    await settle();
    expect(invokeMock).toHaveBeenCalledWith("get_recipe", { fileId: 1 });
    // The Effects tab shows the restored effect as active.
    fireEvent.click(screen.getByTestId("edit-tab-effects"));
    await settle();
    expect(
      (screen.getByTestId("effect-bw") as HTMLButtonElement).className,
    ).toContain("selected");
  });

  it("an edit in a tab triggers a debounced recipe render and persists the recipe", async () => {
    vi.useFakeTimers();
    render(EditView, props());
    await settleMicro();
    invokeMock.mockClear();
    fireEvent.click(screen.getByTestId("edit-tab-effects"));
    await settleMicro();
    fireEvent.click(screen.getByRole("button", { name: "B&W" }));
    await settleMicro();

    // Debounced: nothing sent yet.
    expect(invokeMock).not.toHaveBeenCalledWith(
      "render_edit_preview",
      expect.anything(),
    );
    vi.advanceTimersByTime(400);
    await vi.runAllTimersAsync();

    const renderCall = invokeMock.mock.calls.find(
      (c) => c[0] === "render_edit_preview",
    );
    expect(renderCall).toBeTruthy();
    const recipe = JSON.parse(renderCall![1].recipe);
    expect(recipe.ops).toEqual([{ type: "bw", params: { amount: 1 } }]);
    const saveCall = invokeMock.mock.calls.find((c) => c[0] === "set_recipe");
    expect(saveCall).toBeTruthy();
    expect(JSON.parse(saveCall![1].recipe).ops).toEqual([
      { type: "bw", params: { amount: 1 } },
    ]);
    vi.useRealTimers();
  });

  it("a completed render swaps the preview to the edited URI", async () => {
    vi.useFakeTimers();
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "render_edit_preview"
        ? Promise.resolve(`preview://${HASH(0)}?r=abc`)
        : Promise.resolve(null),
    );
    render(EditView, props());
    await settleMicro();
    invokeMock.mockClear();
    fireEvent.click(screen.getByTestId("edit-tab-tuning"));
    await settleMicro();
    const slider = screen.getByTestId("fill-light");
    fireEvent.input(slider, { target: { value: "0.5" } });
    await settleMicro();
    vi.advanceTimersByTime(400);
    await vi.runAllTimersAsync();
    const img = screen.getByTestId("edit-preview") as HTMLImageElement;
    expect(img.src).toContain("?r=");
    vi.useRealTimers();
  });

  it("undo reverts the recipe and re-renders", async () => {
    vi.useFakeTimers();
    render(EditView, props());
    await settleMicro();
    fireEvent.click(screen.getByTestId("edit-tab-effects"));
    await settleMicro();
    fireEvent.click(screen.getByRole("button", { name: "B&W" }));
    await settleMicro();
    vi.advanceTimersByTime(400);
    await vi.runAllTimersAsync();

    invokeMock.mockClear();
    fireEvent.click(screen.getByTestId("edit-undo"));
    await settleMicro();
    vi.advanceTimersByTime(400);
    await vi.runAllTimersAsync();

    const saveCall = invokeMock.mock.calls.find((c) => c[0] === "set_recipe");
    expect(saveCall).toBeTruthy();
    expect(JSON.parse(saveCall![1].recipe).ops).toEqual([]);
    vi.useRealTimers();
  });

  it("arrow keys navigate the filmstrip and load the next photo's recipe", async () => {
    render(EditView, props());
    await settle();
    invokeMock.mockClear();
    await fireEvent.keyDown(window, { key: "ArrowRight" });
    await settle();
    expect(invokeMock).toHaveBeenCalledWith("get_recipe", { fileId: 2 });
    expect(screen.getByTestId("edit-count").textContent).toBe("2/3");
  });

  it("clicking a filmstrip thumb navigates to that photo", async () => {
    render(EditView, props());
    await settle();
    const thumbs = screen.getAllByTestId("edit-filmstrip-thumb");
    await fireEvent.click(thumbs[2]);
    await settle();
    expect(screen.getByTestId("edit-count").textContent).toBe("3/3");
  });

  it("Escape closes the edit view", async () => {
    const onclose = vi.fn();
    render(EditView, { ...props(), onclose });
    await settle();
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(onclose).toHaveBeenCalled();
  });

  it("a render failure surfaces as an inline error, never a dialog", async () => {
    vi.useFakeTimers();
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "render_edit_preview"
        ? Promise.reject("decode failed")
        : Promise.resolve(null),
    );
    render(EditView, props());
    await settleMicro();
    fireEvent.click(screen.getByTestId("edit-tab-effects"));
    await settleMicro();
    fireEvent.click(screen.getByRole("button", { name: "Sepia" }));
    await settleMicro();
    vi.advanceTimersByTime(400);
    await vi.runAllTimersAsync();
    expect(screen.getByTestId("edit-error").textContent).toContain(
      "decode failed",
    );
    vi.useRealTimers();
  });
});

// TASK-04-016: Save writes the edited pixels to the original path (the
// backend archives the untouched original into `.originals/`); Revert
// restores it. The component's job is wiring the two commands and keeping
// its session state honest.
describe("EditView save & revert", () => {
  it("save is disabled while there are no edits", () => {
    render(EditView, props());
    expect(
      (screen.getByTestId("edit-save") as HTMLButtonElement).disabled,
    ).toBe(true);
  });

  it("an edit enables Save, which invokes save_photo for the photo", async () => {
    vi.useFakeTimers();
    render(EditView, props());
    await settleMicro();
    fireEvent.click(screen.getByTestId("edit-tab-effects"));
    await settleMicro();
    fireEvent.click(screen.getByRole("button", { name: "B&W" }));
    await settleMicro();
    invokeMock.mockClear();
    vi.useRealTimers();
    await settleMicro();

    const save = screen.getByTestId("edit-save") as HTMLButtonElement;
    expect(save.disabled).toBe(false);
    fireEvent.click(save);
    await settle();
    expect(invokeMock).toHaveBeenCalledWith("save_photo", { fileId: 1 });
    expect(screen.getByTestId("edit-save").textContent).toBe("Saved");
  });

  it("a save failure is surfaced, not swallowed", async () => {
    vi.useFakeTimers();
    render(EditView, props());
    await settleMicro();
    fireEvent.click(screen.getByTestId("edit-tab-effects"));
    await settleMicro();
    fireEvent.click(screen.getByRole("button", { name: "B&W" }));
    await settleMicro();
    invokeMock.mockClear();
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "save_photo" ? Promise.reject("denied") : Promise.resolve(null),
    );
    vi.useRealTimers();
    await settleMicro();

    fireEvent.click(screen.getByTestId("edit-save"));
    await settle();
    expect(
      (screen.getByTestId("edit-save-error") as HTMLElement).textContent,
    ).toContain("denied");
  });

  it("revert invokes revert_photo and resets the session to no edits", async () => {
    invokeMock.mockImplementation((_cmd: string, args: { fileId: number }) =>
      _cmd === "get_recipe"
        ? Promise.resolve(
            JSON.stringify({
              spec: "fotobase.recipe",
              version: 1,
              ops: [{ type: "bw", params: { amount: 1 } }],
            }),
          )
        : Promise.resolve(null),
    );
    render(EditView, props());
    await settle();
    invokeMock.mockClear();

    fireEvent.click(screen.getByTestId("edit-revert"));
    await settle();
    expect(invokeMock).toHaveBeenCalledWith("revert_photo", { fileId: 1 });
    // The session is clean again: Save is disabled (no edits) and the
    // stored recipe was cleared.
    expect(
      (screen.getByTestId("edit-save") as HTMLButtonElement).disabled,
    ).toBe(true);
  });
});

// TASK-04-017: "Save as…" renders the edited pixels into a new file at a
// chosen location; "Save a copy" duplicates the current picture next to the
// original. The component's job is wiring the destination dialog and the
// two commands — the never-touch-the-original guarantees are the backend
// integration tests' job.
describe("EditView save as… & save a copy", () => {
  async function makeAnEdit(): Promise<void> {
    vi.useFakeTimers();
    render(EditView, props());
    await settleMicro();
    fireEvent.click(screen.getByTestId("edit-tab-effects"));
    await settleMicro();
    fireEvent.click(screen.getByRole("button", { name: "B&W" }));
    await settleMicro();
    invokeMock.mockClear();
    vi.useRealTimers();
    await settleMicro();
  }

  it("save a copy invokes save_copy_photo without a destination", async () => {
    await makeAnEdit();
    fireEvent.click(screen.getByTestId("edit-save-copy"));
    await settle();
    expect(invokeMock).toHaveBeenCalledWith("save_copy_photo", {
      fileId: 1,
      destination: null,
    });
  });

  it("save as… asks for a location and invokes save_as_photo with it", async () => {
    await makeAnEdit();
    saveDialogMock.mockResolvedValue("/chosen/dir/edited.jpg");
    fireEvent.click(screen.getByTestId("edit-save-as"));
    await settle();
    expect(saveDialogMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("save_as_photo", {
      fileId: 1,
      destination: "/chosen/dir/edited.jpg",
    });
  });

  it("cancelling the save-as dialog invokes nothing", async () => {
    await makeAnEdit();
    saveDialogMock.mockResolvedValue(null);
    fireEvent.click(screen.getByTestId("edit-save-as"));
    await settle();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("a save-as failure is surfaced, not swallowed", async () => {
    await makeAnEdit();
    saveDialogMock.mockResolvedValue("/chosen/dir/edited.jpg");
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "save_as_photo" ? Promise.reject("denied") : Promise.resolve(null),
    );
    fireEvent.click(screen.getByTestId("edit-save-as"));
    await settle();
    expect(
      (screen.getByTestId("edit-save-error") as HTMLElement).textContent,
    ).toContain("denied");
  });
});
