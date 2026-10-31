// TASK-04-010 tests: edit-view recipe state. The edit view keeps one state
// object per tab, folds them into a single v1 recipe (edit-recipe spec §1–3)
// in canonical op order, drives the undo/redo history, and rebuilds panel
// states from a stored recipe so reopening an edited photo restores the
// sliders. Pure module — no Svelte, no DOM.

import { describe, expect, it } from "vitest";

import { DEFAULT_TUNING, type TuningState } from "./tuning";
import {
  EMPTY_EDIT_STATE,
  editHistory,
  isEmptyState,
  opsFromEditState,
  pushEditState,
  recipeFromState,
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
  it("an untouched state emits no ops", () => {
    expect(opsFromEditState(untouched)).toEqual([]);
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

  it("ignores ops the tabs do not own (retouch, text) instead of failing", () => {
    const restored = stateFromRecipe({
      spec: "fotobase.recipe",
      version: 1,
      ops: [
        { type: "retouch", params: {} },
        { type: "bw", params: { amount: 0.8 } },
      ],
    });
    expect(restored.effects).toEqual({ bw: 0.8 });
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
