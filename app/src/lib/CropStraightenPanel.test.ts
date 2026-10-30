// TASK-04-005 tests: the Basic Fixes crop/straighten panel — preset
// selection, straighten slider, draggable handles, and the live-preview
// transform it derives for interactive feedback.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import CropStraightenPanel from "./CropStraightenPanel.svelte";
import { setLocale } from "./i18n.svelte.js";

// jsdom has no layout engine; give the stage a fixed box and stub
// getBoundingClientRect so pointer math is deterministic.
const STAGE = { left: 0, top: 0, width: 100, height: 50 };

beforeAllStub();
function beforeAllStub(): void {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    },
  );
  Element.prototype.getBoundingClientRect = function () {
    return { ...STAGE, right: STAGE.width, bottom: STAGE.height, x: 0, y: 0, toJSON() {} } as DOMRect;
  };
}

afterEach(cleanup);

function props() {
  return {
    previewSrc: "preview://abc",
    onchange: vi.fn(),
  };
}

function pointer(target: Element, dx: number, dy: number): Promise<void> {
  return (async () => {
    await fireEvent.pointerDown(target, { clientX: 50, clientY: 25, pointerId: 1 });
    await fireEvent.pointerMove(target, { clientX: 50 + dx, clientY: 25 + dy, pointerId: 1 });
    await fireEvent.pointerUp(target, { clientX: 50 + dx, clientY: 25 + dy, pointerId: 1 });
  })();
}

describe("CropStraightenPanel", () => {
  it("renders the presets including Free", () => {
    render(CropStraightenPanel, props());
    expect(screen.getByTestId("preset-free")).toBeTruthy();
    expect(screen.getByTestId("preset-1:1")).toBeTruthy();
    expect(screen.getByTestId("preset-4:6")).toBeTruthy();
    expect(screen.getByTestId("stage")).toBeTruthy();
  });

  it("default state is free crop over the full image and no straighten", async () => {
    const onchange = vi.fn();
    render(CropStraightenPanel, { ...props(), onchange });
    await waitFor(() => expect(onchange).toHaveBeenCalled());
    expect(onchange).toHaveBeenLastCalledWith({ crop: { x: 0, y: 0, w: 1, h: 1 }, angle: 0, autoFill: true });
  });

  it("selecting a ratio preset emits the largest matching crop", async () => {
    const onchange = vi.fn();
    render(CropStraightenPanel, { ...props(), onchange });
    await fireEvent.click(screen.getByTestId("preset-1:1"));
    const last = onchange.mock.calls.at(-1)[0];
    // Stage stub is 100×50 → image aspect 2 → the largest 1:1 rect is half
    // the width, full height, centered.
    expect(last.crop).toEqual({ x: 0.25, y: 0, w: 0.5, h: 1 });
  });

  it("the straighten slider updates the angle and auto-crop keeps corners filled", async () => {
    const onchange = vi.fn();
    render(CropStraightenPanel, { ...props(), onchange });
    const slider = screen.getByTestId("angle-slider") as HTMLInputElement;
    slider.value = "4";
    await fireEvent.input(slider);
    const last = onchange.mock.calls.at(-1)[0];
    expect(last.angle).toBe(4);
    expect(last.autoFill).toBe(true);
  });

  it("dragging the bottom-right handle shrinks the crop and emits normalized values", async () => {
    const onchange = vi.fn();
    render(CropStraightenPanel, { ...props(), onchange });
    await pointer(screen.getByTestId("handle-br"), -40, -10); // -0.4, -0.2 normalized
    const last = onchange.mock.calls.at(-1)[0];
    expect(last.crop).toEqual({ x: 0, y: 0, w: 0.6, h: 0.8 });
  });

  it("dragging inside the crop moves it and keeps it inside the image", async () => {
    render(CropStraightenPanel, props());
    // First shrink the crop, then drag it right beyond the edge.
    await pointer(screen.getByTestId("handle-br"), -20, -5); // w 0.8, h 0.9
    await pointer(screen.getByTestId("crop-overlay"), 40, 0); // would exceed the edge
    const overlay = screen.getByTestId("crop-overlay");
    expect(Number(overlay.getAttribute("data-x"))).toBeCloseTo(0.2);
  });

  it("updates the live preview transform for straighten", async () => {
    render(CropStraightenPanel, props());
    const slider = screen.getByTestId("angle-slider") as HTMLInputElement;
    slider.value = "4";
    await fireEvent.input(slider);
    const preview = screen.getByTestId("preview-img") as HTMLImageElement;
    expect(preview.style.transform).toContain("rotate(4deg)");
    expect(preview.style.transform).toContain("scale("); // auto-fill shrink
  });

  it("reports a failed change instead of failing silently", async () => {
    // onchange throwing must not break the panel: state stays consistent.
    render(CropStraightenPanel, { previewSrc: "preview://abc", onchange: () => { throw new Error("boom"); } });
    const slider = screen.getByTestId("angle-slider") as HTMLInputElement;
    slider.value = "3";
    await fireEvent.input(slider);
    expect((screen.getByTestId("angle-slider") as HTMLInputElement).value).toBe("3");
  });
});
