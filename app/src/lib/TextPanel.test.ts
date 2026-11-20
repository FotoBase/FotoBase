// TASK-08-003 tests: the text tool panel — add an overlay, edit its text,
// font, size, color, outline and transparency, drag it over the preview,
// delete it, and the state emissions the edit view persists.

import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import TextPanel from "./TextPanel.svelte";
import { addOverlay, type TextState } from "./text-overlay";

// jsdom has no layout engine: give every element a fixed 200×100 box so the
// pointer math is deterministic.
Element.prototype.getBoundingClientRect = function () {
  return { left: 0, top: 0, width: 200, height: 100, right: 200, bottom: 100, x: 0, y: 0, toJSON() {} } as DOMRect;
};

afterEach(cleanup);

function renderPanel(initial?: TextState) {
  const onchange = vi.fn();
  render(TextPanel, { previewSrc: "preview://abc", onchange, initial });
  return onchange;
}

const last = (fn: ReturnType<typeof vi.fn>): TextState => fn.mock.calls.at(-1)![0];

describe("TextPanel", () => {
  it("starts empty and adds a centered overlay", async () => {
    const onchange = renderPanel();
    expect(screen.queryByTestId("text-input")).toBeNull();
    await fireEvent.click(screen.getByTestId("text-add"));
    expect(last(onchange).overlays).toHaveLength(1);
    expect(last(onchange).overlays[0]).toMatchObject({ x: 0.5, y: 0.5 });
    expect(screen.getByTestId("text-input")).toBeTruthy();
  });

  it("edits text, font, size, color, outline and transparency", async () => {
    const onchange = renderPanel();
    await fireEvent.click(screen.getByTestId("text-add"));
    await fireEvent.input(screen.getByTestId("text-input"), { target: { value: "Hello" } });
    await fireEvent.change(screen.getByTestId("text-font"), { target: { value: "serif" } });
    await fireEvent.input(screen.getByTestId("text-size"), { target: { value: "0.2" } });
    await fireEvent.input(screen.getByTestId("text-color"), { target: { value: "#ff0000" } });
    await fireEvent.input(screen.getByTestId("text-outline-width"), { target: { value: "0.01" } });
    await fireEvent.input(screen.getByTestId("text-outline-color"), { target: { value: "#00ff00" } });
    await fireEvent.input(screen.getByTestId("text-transparency"), { target: { value: "0.5" } });
    expect(last(onchange).overlays[0]).toMatchObject({
      text: "Hello",
      font: "serif",
      size: 0.2,
      color: "#ff0000",
      outlineWidth: 0.01,
      outlineColor: "#00ff00",
      transparency: 0.5,
    });
  });

  it("dragging the handle moves the overlay in normalized coordinates", async () => {
    const onchange = renderPanel(addOverlay({ overlays: [] }, "Drag me"));
    const stage = screen.getByTestId("text-stage");
    const handle = screen.getByTestId("text-handle-0");
    await fireEvent.pointerDown(handle, { clientX: 100, clientY: 50, pointerId: 1 });
    await fireEvent.pointerMove(stage, { clientX: 50, clientY: 80, pointerId: 1 });
    await fireEvent.pointerUp(stage, { clientX: 50, clientY: 80, pointerId: 1 });
    expect(last(onchange).overlays[0]).toMatchObject({ x: 0.25, y: 0.8 });
  });

  it("restores overlays from the initial state and edits the selected one", async () => {
    const initial = addOverlay(addOverlay({ overlays: [] }, "A"), "B");
    const onchange = renderPanel(initial);
    await fireEvent.click(screen.getByTestId("text-item-1"));
    await fireEvent.input(screen.getByTestId("text-input"), { target: { value: "B2" } });
    expect(last(onchange).overlays.map((o) => o.text)).toEqual(["A", "B2"]);
  });

  it("deleting removes the overlay and hides the controls", async () => {
    const onchange = renderPanel(addOverlay({ overlays: [] }, "A"));
    await fireEvent.click(screen.getByTestId("text-delete"));
    expect(last(onchange).overlays).toEqual([]);
    expect(screen.queryByTestId("text-input")).toBeNull();
  });
});
