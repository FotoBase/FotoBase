// TASK-08-003: pure state + op emission for the text tool.

import { describe, expect, it } from "vitest";

import {
  addOverlay,
  defaultOverlay,
  isTextOpOwned,
  moveOverlay,
  opsFromText,
  removeOverlay,
  textFromOps,
  updateOverlay,
  type TextState,
} from "./text-overlay";

const empty: TextState = { overlays: [] };

describe("text overlay state", () => {
  it("emits no op without overlays", () => {
    expect(opsFromText(empty)).toEqual([]);
  });

  it("emits one algo-2 text op per overlay with normalized params", () => {
    const state = addOverlay(empty, "Hello");
    const [op] = opsFromText(state);
    expect(op.type).toBe("text");
    expect(op.params).toMatchObject({
      algo: 2,
      text: "Hello",
      x: 0.5,
      y: 0.5,
      anchor: "center",
      font: "sans",
      transparency: 1,
    });
  });

  it("omits the outline when its width is zero and rotation when zero", () => {
    const flat = updateOverlay(addOverlay(empty, "A"), 0, { outlineWidth: 0 });
    const [op] = opsFromText(flat);
    expect(op.params).not.toHaveProperty("outline");
    expect(op.params).not.toHaveProperty("rotation");
    const styled = updateOverlay(flat, 0, {
      outlineWidth: 0.01,
      outlineColor: "#112233",
      rotation: 15,
    });
    expect(opsFromText(styled)[0].params).toMatchObject({
      outline: { width: 0.01, color: "#112233" },
      rotation: 15,
    });
  });

  it("dragging moves the overlay and clamps to the image", () => {
    const state = addOverlay(empty, "A");
    expect(moveOverlay(state, 0, 0.123456, 0.9).overlays[0]).toMatchObject({
      x: 0.1235,
      y: 0.9,
    });
    expect(moveOverlay(state, 0, -3, 7).overlays[0]).toMatchObject({ x: 0, y: 1 });
  });

  it("clamps edited params to what the engine accepts", () => {
    const state = updateOverlay(addOverlay(empty, "A"), 0, {
      size: 5,
      transparency: -1,
      outlineWidth: 1,
    });
    expect(state.overlays[0]).toMatchObject({
      size: 0.5,
      transparency: 0,
      outlineWidth: 0.05,
    });
  });

  it("an empty text is not emitted (the engine rejects it)", () => {
    const state = updateOverlay(addOverlay(empty, "A"), 0, { text: "" });
    expect(opsFromText(state)).toEqual([]);
  });

  it("round-trips through the recipe params", () => {
    let state = addOverlay(empty, "One");
    state = updateOverlay(state, 0, {
      font: "serif",
      color: "#ffcc00",
      outlineWidth: 0.004,
      outlineColor: "#000000",
      transparency: 0.5,
      size: 0.12,
      rotation: -10,
    });
    state = moveOverlay(addOverlay(state, "Two"), 1, 0.2, 0.8);
    const ops = opsFromText(state);
    expect(textFromOps(ops.map((o) => o.params))).toEqual(state);
  });

  it("owns only algo-2 text ops", () => {
    expect(isTextOpOwned("text", { algo: 2, text: "x" })).toBe(true);
    expect(isTextOpOwned("text", { text: "legacy" })).toBe(false);
    expect(isTextOpOwned("sepia", { algo: 2 })).toBe(false);
  });

  it("removes an overlay by index", () => {
    const state = addOverlay(addOverlay(empty, "A"), "B");
    expect(removeOverlay(state, 0).overlays.map((o) => o.text)).toEqual(["B"]);
    expect(removeOverlay(state, 9)).toBe(state);
  });

  it("new overlays use the default look", () => {
    expect(defaultOverlay("Hi")).toMatchObject({
      text: "Hi",
      color: "#ffffff",
      outlineColor: "#000000",
    });
  });
});
