// TASK-04-006 tests: the Basic Fixes red-eye panel — auto toggle, manual
// drag-to-mark regions with the overlay preview, region removal, and the
// change emissions the edit view persists.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import RedEyePanel from "./RedEyePanel.svelte";
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

function drag(target: Element, dx: number, dy: number): Promise<void> {
  return (async () => {
    await fireEvent.pointerDown(target, { clientX: 20, clientY: 25, pointerId: 1 });
    await fireEvent.pointerMove(target, { clientX: 20 + dx, clientY: 25 + dy, pointerId: 1 });
    await fireEvent.pointerUp(target, { clientX: 20 + dx, clientY: 25 + dy, pointerId: 1 });
  })();
}

describe("RedEyePanel", () => {
  it("default state emits no ops (untouched photo)", async () => {
    const onchange = vi.fn();
    render(RedEyePanel, { ...props(), onchange });
    await waitFor(() => expect(onchange).toHaveBeenCalled());
    expect(onchange).toHaveBeenLastCalledWith({ auto: false, regions: [] });
  });

  it("auto mode toggles and emits the auto op state", async () => {
    const onchange = vi.fn();
    render(RedEyePanel, { ...props(), onchange });
    await fireEvent.click(screen.getByTestId("auto-button"));
    expect(onchange).toHaveBeenLastCalledWith({ auto: true, regions: [] });
    await fireEvent.click(screen.getByTestId("auto-button"));
    expect(onchange).toHaveBeenLastCalledWith({ auto: false, regions: [] });
  });

  it("manual drag marks a normalized region and emits it", async () => {
    const onchange = vi.fn();
    render(RedEyePanel, { ...props(), onchange });
    await fireEvent.click(screen.getByTestId("manual-button"));
    await drag(screen.getByTestId("stage"), 10, 0); // 20→30px of 100 → x 0.2→0.3
    const last = onchange.mock.calls.at(-1)[0];
    expect(last.auto).toBe(false);
    expect(last.regions).toHaveLength(1);
    expect(last.regions[0].x).toBeCloseTo(0.25);
    expect(last.regions[0].y).toBeCloseTo(0.5);
    expect(last.regions[0].r).toBeCloseTo(0.05);
    // The overlay preview shows the region.
    expect(screen.getByTestId("region-0")).toBeTruthy();
  });

  it("dragging outside manual mode does nothing", async () => {
    const onchange = vi.fn();
    render(RedEyePanel, { ...props(), onchange });
    await waitFor(() => expect(onchange).toHaveBeenCalled());
    const calls = onchange.mock.calls.length;
    await drag(screen.getByTestId("stage"), 10, 0);
    expect(onchange.mock.calls.length).toBe(calls);
  });

  it("clicking a marked region in manual mode removes it", async () => {
    const onchange = vi.fn();
    render(RedEyePanel, { ...props(), onchange });
    await fireEvent.click(screen.getByTestId("manual-button"));
    await drag(screen.getByTestId("stage"), 10, 0);
    expect(screen.getByTestId("region-0")).toBeTruthy();
    await fireEvent.pointerDown(screen.getByTestId("region-0"), { clientX: 25, clientY: 25, pointerId: 1 });
    await waitFor(() => expect(screen.queryByTestId("region-0")).toBeNull());
    expect(onchange).toHaveBeenLastCalledWith({ auto: false, regions: [] });
  });

  it("clear resets to the untouched state", async () => {
    const onchange = vi.fn();
    render(RedEyePanel, { ...props(), onchange });
    await fireEvent.click(screen.getByTestId("auto-button"));
    const clear = screen.getByTestId("clear") as HTMLButtonElement;
    await fireEvent.click(clear);
    expect(onchange).toHaveBeenLastCalledWith({ auto: false, regions: [] });
  });

  it("clear is disabled while nothing is marked", () => {
    render(RedEyePanel, props());
    expect((screen.getByTestId("clear") as HTMLButtonElement).disabled).toBe(true);
  });
});
