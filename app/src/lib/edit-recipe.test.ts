// TASK-04-010 tests: edit-view recipe state. The edit view keeps one state
// object per tab, folds them into a single v1 recipe (edit-recipe spec §1–3)
// in canonical op order, drives the undo/redo history, and rebuilds panel
// states from a stored recipe so reopening an edited photo restores the
// sliders. Pure module — no Svelte, no DOM.

import { describe, expect, it } from "vitest";

import { DEFAULT_TUNING, type TuningState } from "./tuning";
import { addOverlay, defaultOverlay, removeOverlay, updateOverlay } from "./text-overlay";
import {
  EMPTY_EDIT_STATE,
  duplicateRecipeOp,
  editHistory,
  isEmptyState,
  moveRecipeOp,
  opsFromEditState,
  pushEditState,
  recipeFromState,
  removeRecipeOp,
  redo,
  stateFromRecipe,
  undo,
  type EditState,
} from "./edit-recipe";

const untouched: EditState = {
  ...EMPTY_EDIT_STATE,
  tuning: { ...DEFAULT_TUNING },
};

describe("opsFromEditState", () => {
  it("stacks a copy of an operation and keeps it after reopening", () => {
    const state = { ...untouched, pro: { ...untouched.pro, exposure: 1 } };
    const saved = recipeFromState(duplicateRecipeOp(state, 0));
    expect(saved.ops).toEqual([
      { type: "exposure", params: { ev: 1 } },
      { type: "exposure", params: { ev: 1 } },
    ]);
    expect(recipeFromState(stateFromRecipe(saved))).toEqual(saved);
  });

  it("removes a selected stacked operation without removing its neighbors", () => {
    const state = stateFromRecipe({ ops: [
      { type: "exposure", params: { ev: 1 } },
      { type: "bw", params: { amount: 0.5 } },
      { type: "exposure", params: { ev: -0.5 } },
    ] });
    expect(recipeFromState(removeRecipeOp(state, 0)).ops).toEqual([
      { type: "bw", params: { amount: 0.5 } },
      { type: "exposure", params: { ev: -0.5 } },
    ]);
  });

  it("reorders stacked operations and keeps their new order when sliders change", () => {
    const state = stateFromRecipe({ ops: [
      { type: "exposure", params: { ev: 1 } },
      { type: "bw", params: { amount: 0.5 } },
      { type: "exposure", params: { ev: -0.5 } },
    ] });
    const moved = moveRecipeOp(state, 2, 0);
    expect(recipeFromState({ ...moved, pro: { ...moved.pro, exposure: 0.25 } }).ops).toEqual([
      { type: "exposure", params: { ev: -0.5 } },
      { type: "exposure", params: { ev: 0.25 } },
      { type: "bw", params: { amount: 0.5 } },
    ]);
  });

  it("preserves reordered non-Pro operations after reopening", () => {
    const state = { ...untouched, effects: { bw: 1, sepia: 0.5 } };
    const moved = moveRecipeOp(state, 1, 0);
    const saved = recipeFromState(moved);
    expect(recipeFromState(stateFromRecipe(saved))).toEqual(saved);
  });

  it("edits one text overlay without duplicating other stacked overlays", () => {
    const state = {
      ...untouched,
      pro: { ...untouched.pro, exposure: 1 },
      text: { overlays: [defaultOverlay("first"), defaultOverlay("second")] },
    };
    const restored = stateFromRecipe(recipeFromState(moveRecipeOp(state, 0, 1)));
    const changed = { ...restored, text: updateOverlay(restored.text, 1, { text: "changed" }) };
    expect(recipeFromState(changed).ops.filter((op) => op.type === "text").map((op) => op.params.text))
      .toEqual(["first", "changed"]);
  });

  it("removes one stacked text overlay without deleting or duplicating the others", () => {
    const state = {
      ...untouched,
      pro: { ...untouched.pro, exposure: 1 },
      text: { overlays: [defaultOverlay("first"), defaultOverlay("second")] },
    };
    const restored = stateFromRecipe(recipeFromState(moveRecipeOp(state, 0, 1)));
    const changed = { ...restored, text: removeOverlay(restored.text, 0) };
    expect(recipeFromState(changed).ops.filter((op) => op.type === "text").map((op) => op.params.text))
      .toEqual(["second"]);
  });

  it("adds a text overlay without moving existing operations", () => {
    const state = {
      ...untouched,
      pro: { ...untouched.pro, exposure: 1 },
      text: { overlays: [defaultOverlay("first"), defaultOverlay("second")] },
    };
    const restored = stateFromRecipe(recipeFromState(moveRecipeOp(state, 0, 1)));
    const changed = { ...restored, text: addOverlay(restored.text, "third") };
    const ops = recipeFromState(changed).ops;
    expect(ops.map((op) => op.type)).toEqual(["text", "exposure", "text", "text"]);
    expect(ops.filter((op) => op.type === "text").map((op) => op.params.text))
      .toEqual(["first", "second", "third"]);
  });

  it("an untouched state emits no ops", () => {
    expect(opsFromEditState(untouched)).toEqual([]);
  });

  it("folds every F2 Pro tab op into the same recipe engine", () => {
    const state: EditState = {
      ...untouched,
      pro: {
        curves: { enabled: true, points: [[0, 0], [0.5, 0.7], [1, 1]] },
        levels: { black: 0.1, white: 0.9, gamma: 1.1 },
        hsl: { hue: 0.2, saturation: 0.3, luminance: -0.1 },
        exposure: 0.5,
        vibrance: 0.4,
        keystone: { top: 0.1, bottom: 0.0, left: 0.0, right: 0.0 },
        lensCorrection: { profile: "<lensdatabase/>", lens: "Testlens", crop: 1, focal: 50 },
        noiseReduction: { amount: 0.6, radius: 0.01 },
      },
    };
    expect(opsFromEditState(state).map((op) => op.type)).toEqual([
      "curves",
      "levels",
      "hsl",
      "exposure",
      "vibrance",
      "keystone",
      "lens_correction",
      "noise_reduction",
    ]);
  });

  it("folds every tab's ops in canonical order (crop first, effects last)", () => {
    const state: EditState = {
      ...untouched,
      crop: {
        crop: { x: 0.1, y: 0.1, w: 0.8, h: 0.8 },
        angle: 0,
        autoFill: true,
      },
      autoFix: { autoColor: true, autoContrast: true },
      tuning: { ...DEFAULT_TUNING, fillLight: 0.4, temperature: -0.2 },
      effects: { bw: 1 },
    };
    const types = opsFromEditState(state).map((op) => op.type);
    expect(types).toEqual([
      "crop",
      "auto_color",
      "auto_contrast",
      "fill_light",
      "temperature",
      "bw",
    ]);
  });

  it("omits tabs at their neutral positions", () => {
    const state: EditState = { ...untouched, effects: { sharpen: 0.5 } };
    expect(opsFromEditState(state)).toEqual([
      { type: "sharpen", params: { amount: 0.5 } },
    ]);
  });
});

describe("recipeFromState", () => {
  it("emits the v1 envelope with the folded ops", () => {
    const state: EditState = { ...untouched, effects: { sepia: 1 } };
    expect(recipeFromState(state)).toEqual({
      spec: "fotobase.recipe",
      version: 1,
      ops: [{ type: "sepia", params: { amount: 1 } }],
    });
  });
});

describe("stateFromRecipe", () => {
  it("restores a stored Lensfun profile and re-emits it unchanged", () => {
    const params = {
      profile: "<lensdatabase><lens/></lensdatabase>",
      lens: "Canon EF 50mm f/1.8 II",
      crop: 1,
      focal: 50,
      aperture: 2.8,
      distortion: true,
      vignetting: false,
      tca: true,
    };
    const state = stateFromRecipe({ ops: [{ type: "lens_correction", params }] });
    expect(state.pro.lensCorrection).toEqual(params);
    expect(state.extra).toEqual([]);
    expect(recipeFromState(state).ops).toEqual([{ type: "lens_correction", params }]);
    expect(recipeFromState({ ...state, pro: { ...state.pro, lensCorrection: null } }).ops)
      .toEqual([]);
  });

  it("preserves stacked Pro operations and their order when another edit changes", () => {
    const ops = [
      { type: "exposure", params: { ev: 1, future: "keep" } },
      { type: "bw", params: { amount: 0.5 } },
      { type: "exposure", params: { ev: -0.5 } },
    ];
    const state = stateFromRecipe({ ops });
    expect(recipeFromState(state).ops).toEqual(ops);
    expect(recipeFromState({
      ...state,
      tuning: { ...state.tuning, fillLight: 0.4 },
    }).ops).toEqual([
      { type: "fill_light", params: { amount: 0.4 } },
      ...ops,
    ]);
  });

  it("edits the last stacked Pro operation without losing earlier operations or unknown parameters", () => {
    const state = stateFromRecipe({ ops: [
      { type: "exposure", params: { ev: 1 } },
      { type: "bw", params: { amount: 0.5 } },
      { type: "exposure", params: { ev: -0.5, future: "keep" } },
    ] });
    expect(recipeFromState({ ...state, pro: { ...state.pro, exposure: 0.25 } }).ops).toEqual([
      { type: "exposure", params: { ev: 1 } },
      { type: "bw", params: { amount: 0.5 } },
      { type: "exposure", params: { ev: 0.25, future: "keep" } },
    ]);
  });

  it("preserves unknown parameters when a Pro adjustment returns to neutral", () => {
    const state = stateFromRecipe({ ops: [
      { type: "exposure", params: { ev: 1, future: "keep" } },
    ] });
    expect(recipeFromState({ ...state, pro: { ...state.pro, exposure: 0 } }).ops).toEqual([
      { type: "exposure", params: { ev: 0, future: "keep" } },
    ]);
  });

  it("round-trips through recipeFromState", () => {
    const state: EditState = {
      ...untouched,
      crop: {
        crop: { x: 0, y: 0, w: 0.5, h: 0.5 },
        angle: 2.5,
        autoFill: false,
      },
      tuning: { ...DEFAULT_TUNING, highlights: 0.7, tint: 0.1 },
      effects: { saturation: -0.5 },
    };
    const restored = stateFromRecipe(recipeFromState(state));
    expect(restored).toEqual(state);
  });

  it("an empty recipe restores the untouched state", () => {
    expect(
      stateFromRecipe({ spec: "fotobase.recipe", version: 1, ops: [] }),
    ).toEqual(untouched);
  });

  it("ignores ops the tabs do not own (text) instead of failing", () => {
    const restored = stateFromRecipe({
      spec: "fotobase.recipe",
      version: 1,
      ops: [
        { type: "text", params: {} },
        { type: "bw", params: { amount: 0.8 } },
      ],
    });
    expect(restored.effects).toEqual({ bw: 0.8 });
  });

  it("owns retouch: strokes are restored and re-emitted in a single op (TASK-08-002)", () => {
    const strokes = [
      { points: [[0.2, 0.3]], radius: 0.03 },
      { points: [[0.5, 0.5], [0.6, 0.5]], radius: 0.05 },
    ];
    const restored = stateFromRecipe({
      ops: [{ type: "retouch", params: { strokes } }],
    });
    expect(restored.retouch.strokes).toEqual(strokes);
    expect(restored.extra).toEqual([]);
    expect(isEmptyState(restored)).toBe(false);
    expect(opsFromEditState(restored)).toEqual([
      { type: "retouch", params: { strokes } },
    ]);
  });

  it("retouch heals the source pixels: it comes before geometry ops", () => {
    // Strokes are drawn on the uncropped preview, so they must apply
    // before crop/straighten for the coordinates to mean the same pixels.
    const state: EditState = {
      ...untouched,
      crop: {
        crop: { x: 0.1, y: 0.1, w: 0.8, h: 0.8 },
        angle: 0,
        autoFill: true,
      },
      retouch: { strokes: [{ points: [[0.5, 0.5]], radius: 0.03 }] },
      effects: { bw: 1 },
    };
    expect(opsFromEditState(state).map((op) => op.type)).toEqual([
      "retouch",
      "crop",
      "bw",
    ]);
  });

  it("owns glyph-renderer text without changing the saved operation order", () => {
    const text = {
      algo: 2,
      text: "Hi",
      x: 0.3,
      y: 0.4,
      size: 0.1,
      color: "#ffffff",
      font: "sans",
      anchor: "center",
      transparency: 1,
    };
    const restored = stateFromRecipe({
      ops: [
        { type: "text", params: text },
        { type: "border", params: { amount: 0.5 } },
        { type: "bw", params: { amount: 1 } },
      ],
    });
    expect(restored.text.overlays).toHaveLength(1);
    expect(restored.extra).toEqual([]);
    expect(opsFromEditState(restored).map((op) => op.type)).toEqual([
      "text",
      "border",
      "bw",
    ]);
  });

  it("legacy text ops (no algo) stay untouched in extra", () => {
    const legacy = { type: "text", params: { text: "Old", x: 0.1, y: 0.1 } };
    const restored = stateFromRecipe({ ops: [legacy] });
    expect(restored.text.overlays).toEqual([]);
    expect(restored.extra).toEqual([legacy]);
  });

  it("keeps ops no tab owns so a save re-emits them (batch edits survive)", () => {
    // A recipe the batch flow (TASK-04-011) wrote: rotate + a text overlay.
    const batched = {
      spec: "fotobase.recipe",
      version: 1,
      ops: [
        { type: "rotate", params: { angle: 90 } },
        { type: "text", params: { text: "Caption", x: 0.05, y: 0.05 } },
        { type: "film_grain", params: { amount: 0.5, seed: 3 } },
      ],
    };
    const restored = stateFromRecipe(batched);
    expect(restored.extra).toEqual(batched.ops);
    expect(isEmptyState(restored)).toBe(false);

    // Saving from the edit view must not silently drop those edits.
    expect(recipeFromState(restored).ops).toEqual(batched.ops);
  });

  it("extra ops ride along with tab edits in canonical order (tabs first)", () => {
    const state: EditState = {
      ...stateFromRecipe({
        ops: [{ type: "rotate", params: { angle: 90 } }],
      }),
      effects: { bw: 1 },
    };
    expect(opsFromEditState(state)).toEqual([
      { type: "bw", params: { amount: 1 } },
      { type: "rotate", params: { angle: 90 } },
    ]);
  });
});

describe("isEmptyState", () => {
  it("true for the untouched state, false once any tab moves", () => {
    expect(isEmptyState(untouched)).toBe(true);
    expect(isEmptyState({ ...untouched, effects: { bw: 1 } })).toBe(false);
  });
});

describe("editHistory", () => {
  it("starts on the untouched state with nothing to undo or redo", () => {
    const h = editHistory(untouched);
    expect(h.present).toBe(untouched);
    expect(undo(h)).toBeNull();
    expect(redo(h)).toBeNull();
  });

  it("undo steps back through pushed states, redo steps forward", () => {
    let h = editHistory(untouched);
    const second: EditState = { ...untouched, effects: { bw: 1 } };
    const third: EditState = {
      ...untouched,
      tuning: { ...DEFAULT_TUNING, fillLight: 0.5 },
    };
    h = pushEditState(h, second);
    h = pushEditState(h, third);
    expect(h.present).toBe(third);
    h = undo(h)!;
    expect(h.present).toBe(second);
    h = undo(h)!;
    expect(h.present).toBe(untouched);
    expect(undo(h)).toBeNull();
    h = redo(h)!;
    expect(h.present).toBe(second);
  });

  it("a new edit after undo discards the redo branch", () => {
    let h = editHistory(untouched);
    h = pushEditState(h, { ...untouched, effects: { bw: 1 } });
    h = undo(h)!;
    h = pushEditState(h, { ...untouched, effects: { sepia: 1 } });
    expect(redo(h)).toBeNull();
    expect(h.present.effects).toEqual({ sepia: 1 });
  });

  it("pushing an identical state is a no-op", () => {
    let h = editHistory(untouched);
    h = pushEditState(h, untouched);
    expect(undo(h)).toBeNull();
  });
});

// TASK-07-007: auto-rotate (AI-estimated leveling) is a first-class state
// field, emitted as the v1 `auto_rotate` op and restored on reopen.
describe("auto_rotate", () => {
  it("an autoRotate angle emits an auto_rotate op in the geometry group", () => {
    const state: EditState = { ...untouched, autoRotate: -5 };
    expect(opsFromEditState(state)).toEqual([
      { type: "auto_rotate", params: { angle_deg: -5 } },
    ]);
  });

  it("no estimate means no op", () => {
    expect(opsFromEditState({ ...untouched, autoRotate: null })).toEqual([]);
    expect(isEmptyState({ ...untouched, autoRotate: null })).toBe(true);
  });

  it("round-trips through recipeFromState / stateFromRecipe", () => {
    const state: EditState = { ...untouched, autoRotate: 3.5 };
    const restored = stateFromRecipe(recipeFromState(state));
    expect(restored.autoRotate).toBeCloseTo(3.5);
  });

  it("a stored auto_rotate op restores the angle", () => {
    const restored = stateFromRecipe({
      spec: "fotobase.recipe",
      version: 1,
      ops: [{ type: "auto_rotate", params: { angle_deg: -2.5 } }],
    });
    expect(restored.autoRotate).toBeCloseTo(-2.5);
  });

  it("composes with the manual straighten slider", () => {
    const state: EditState = {
      ...untouched,
      autoRotate: -5,
      crop: { crop: { x: 0, y: 0, w: 1, h: 1 }, angle: 2, autoFill: true },
    };
    const types = opsFromEditState(state).map((op) => op.type);
    expect(types).toEqual(["straighten", "auto_rotate"]);
  });
});

// TASK-08-001: the new effects and frames round-trip through the recipe and
// keep working with undo/redo.
describe("remaining effects and frames", () => {
  const effects = {
    warmify: 0.5,
    tint: 0.4,
    film_grain: 0.3,
    soft_focus: 0.6,
    vignette: 0.7,
    border: 0.5,
    drop_shadow: 0.8,
  };

  it("emit in canonical order after tuning, frames last", () => {
    const state: EditState = {
      ...untouched,
      tuning: { ...DEFAULT_TUNING, tint: 0.2 },
      effects,
    };
    expect(opsFromEditState(state).map((o) => o.type)).toEqual([
      "tint", // Tuning-tab white-balance tint
      "warmify",
      "tint", // Effects-tab color tint
      "film_grain",
      "soft_focus",
      "vignette",
      "border",
      "drop_shadow",
    ]);
  });

  it("round-trip through recipeFromState / stateFromRecipe", () => {
    const state: EditState = {
      ...untouched,
      tuning: { ...DEFAULT_TUNING, tint: 0.2 },
      effects,
    };
    expect(stateFromRecipe(recipeFromState(state))).toEqual(state);
  });

  it("a colored tint never leaks into the Tuning tint slider", () => {
    const restored = stateFromRecipe({
      spec: "fotobase.recipe",
      version: 1,
      ops: [{ type: "tint", params: { color: "#ff0000", amount: 0.5 } }],
    });
    expect(restored.tuning.tint).toBe(DEFAULT_TUNING.tint);
    // Custom color: not the tab's own op, so it is preserved verbatim.
    expect(restored.extra).toEqual([
      { type: "tint", params: { color: "#ff0000", amount: 0.5 } },
    ]);
  });

  it("undo/redo step through effect changes", () => {
    let h = editHistory(untouched);
    h = pushEditState(h, { ...untouched, effects: { vignette: 0.5 } });
    h = pushEditState(h, {
      ...untouched,
      effects: { vignette: 0.5, border: 0.5 },
    });
    h = undo(h)!;
    expect(h.present.effects).toEqual({ vignette: 0.5 });
    h = redo(h)!;
    expect(h.present.effects).toEqual({ vignette: 0.5, border: 0.5 });
  });
});
