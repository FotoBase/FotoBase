// TASK-03-018 tests: view-time sepia / B&W display modes (PLAN §6 P2a).
// Two layers: the pure mode state machine, and the viewer wiring (filter on
// the stage, nothing written, nothing persisted).

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Viewer from "./Viewer.svelte";
import { DISPLAY_MODES, displayFilter, toggleDisplayMode } from "./display-mode";
import type { GridItem } from "./grid";

describe("display mode state machine", () => {
  it("toggling a mode on from normal selects it", () => {
    expect(toggleDisplayMode("normal", "sepia")).toBe("sepia");
    expect(toggleDisplayMode("normal", "bw")).toBe("bw");
  });

  it("toggling the active mode off restores normal", () => {
    expect(toggleDisplayMode("sepia", "sepia")).toBe("normal");
    expect(toggleDisplayMode("bw", "bw")).toBe("normal");
  });

  it("switching straight from one mode to the other", () => {
    expect(toggleDisplayMode("sepia", "bw")).toBe("bw");
    expect(toggleDisplayMode("bw", "sepia")).toBe("sepia");
  });

  it("normal maps to no filter at all", () => {
    expect(displayFilter("normal")).toBe("none");
  });

  it("bw is a full grayscale, sepia a sepia() filter", () => {
    expect(displayFilter("bw")).toBe("grayscale(1)");
    expect(displayFilter("sepia")).toMatch(/^sepia\(0?\.\d+\)$/);
  });

  it("only ever yields plain mode strings — never a recipe op", () => {
    for (const from of DISPLAY_MODES) {
      for (const picked of ["sepia", "bw"] as const) {
        const next = toggleDisplayMode(from, picked);
        expect(typeof next).toBe("string");
        expect(DISPLAY_MODES).toContain(next);
      }
    }
  });
});

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ setFullscreen: vi.fn() }),
}));

vi.mock("@tauri-apps/api/core", () => {
  const invoke = vi.fn();
  return {
    invoke,
    convertFileSrc: (path: string) => `asset://test${path}`,
    __invoke: invoke,
  };
});

import * as tauriCore from "@tauri-apps/api/core";
const invokeMock = (tauriCore as unknown as { __invoke: ReturnType<typeof vi.fn> }).__invoke;

const HASH = (n: number) => n.toString(16).padStart(32, "0");
const items: GridItem[] = [0, 1].map((i) => ({
  id: i,
  filename: `photo-${i}.jpg`,
  hash: HASH(i),
}));

function renderViewer() {
  const result = render(Viewer, { items });
  flushSync();
  return result;
}

function stageFilter(): string {
  return (screen.getByTestId("viewer-stage") as HTMLElement).style.filter;
}

function click(testId: string): void {
  fireEvent.click(screen.getByTestId(testId));
  flushSync();
}

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(null);
});

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe("Viewer display modes", () => {
  it("starts in normal colors", () => {
    renderViewer();
    expect(stageFilter()).toBe("");
    expect(screen.getByTestId("display-sepia").getAttribute("aria-pressed")).toBe("false");
    expect(screen.getByTestId("display-bw").getAttribute("aria-pressed")).toBe("false");
  });

  it("sepia applies immediately and toggles back off", () => {
    renderViewer();
    click("display-sepia");
    expect(stageFilter()).toMatch(/sepia/);
    expect(screen.getByTestId("display-sepia").getAttribute("aria-pressed")).toBe("true");

    click("display-sepia");
    expect(stageFilter()).toBe("");
    expect(screen.getByTestId("display-sepia").getAttribute("aria-pressed")).toBe("false");
  });

  it("B&W applies immediately, swaps with sepia, and toggles off", () => {
    renderViewer();
    click("display-bw");
    expect(stageFilter()).toMatch(/grayscale\(1\)/);

    click("display-sepia");
    expect(stageFilter()).toMatch(/sepia/);
    expect(screen.getByTestId("display-bw").getAttribute("aria-pressed")).toBe("false");

    click("display-bw");
    click("display-bw");
    expect(stageFilter()).toBe("");
  });

  it("the main image src never changes — no re-decode on toggle", () => {
    renderViewer();
    const img = screen.getByTestId("viewer-main") as HTMLImageElement;
    const before = img.src;
    click("display-bw");
    expect(screen.getByTestId("viewer-main")).toBe(img);
    expect(img.src).toBe(before);
    click("display-bw");
    expect(img.src).toBe(before);
  });

  it("the mode stays while navigating between photos", () => {
    renderViewer();
    click("display-sepia");
    fireEvent.keyDown(window, { key: "ArrowRight" });
    flushSync();
    expect(stageFilter()).toMatch(/sepia/);
  });

  it("toggling modes writes nothing: no backend command, no persisted state", () => {
    renderViewer();
    invokeMock.mockClear();
    const setItem = vi.spyOn(Storage.prototype, "setItem");
    click("display-sepia");
    click("display-bw");
    click("display-bw");
    expect(invokeMock).not.toHaveBeenCalled();
    expect(setItem).not.toHaveBeenCalled();
    setItem.mockRestore();
  });

  it("a fresh viewer starts normal again (transient state)", () => {
    const first = renderViewer();
    click("display-bw");
    first.unmount();
    cleanup();
    renderViewer();
    expect(stageFilter()).toBe("");
  });
});
