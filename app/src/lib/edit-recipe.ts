// Pure state + recipe assembly for the edit view (TASK-04-010, PLAN §2D
// "Edit view — single photo with edit tabs and filmstrip"). The view keeps
// one state object per tab; this module folds them into a single v1 recipe
// (docs/edit-recipe-spec-v1.md §1–3) in canonical op order, drives the
// session undo/redo history, and rebuilds the tab states from a stored
// recipe so reopening an edited photo restores the sliders. The pixel work
// is the shared Rust engine at preview/export resolution; this module never
// touches files and never sees image bytes (PLAN §3 rule 3).

import { opsFromState as autoFixOps, type AutoFixState } from "./auto-fix";
import {
  opsFromState as cropOps,
  type CropStraightenState,
} from "./crop-straighten";
import { opsFromEffects as effectOps, type EffectsState } from "./effects";
import { opsFromState as redEyeOps, type RedEyeState } from "./red-eye";
import {
  DEFAULT_TUNING,
  opsFromState as tuningOps,
  type TuningState,
} from "./tuning";

/// The edit view's tabs (PLAN §2D).
export type EditTab = "basic" | "tuning" | "effects";

export const EDIT_TABS: readonly EditTab[] = ["basic", "tuning", "effects"];

export const DEFAULT_CROP: CropStraightenState = {
  crop: { x: 0, y: 0, w: 1, h: 1 },
  angle: 0,
  autoFill: true,
};

export const DEFAULT_AUTO_FIX: AutoFixState = {
  autoColor: false,
  autoContrast: false,
};

export const DEFAULT_RED_EYE: RedEyeState = { auto: false, regions: [] };

export const DEFAULT_EFFECTS: EffectsState = {};

/// The untouched edit state: every tab at its neutral position.
export const EMPTY_EDIT_STATE: EditState = {
  crop: DEFAULT_CROP,
  redEye: DEFAULT_RED_EYE,
  autoFix: DEFAULT_AUTO_FIX,
  tuning: DEFAULT_TUNING,
  effects: DEFAULT_EFFECTS,
};

export interface EditState {
  crop: CropStraightenState;
  redEye: RedEyeState;
  autoFix: AutoFixState;
  tuning: TuningState;
  effects: EffectsState;
}

export type RecipeOp = { type: string; params: Record<string, unknown> };

/// Fold every tab's ops into one recipe, in canonical order (edit-recipe
/// spec §3: geometry first, then tone, then color, then local/one-click
/// effects). A tab at its neutral position contributes nothing.
export function opsFromEditState(state: EditState): RecipeOp[] {
  return [
    ...cropOps(state.crop),
    ...autoFixOps(state.autoFix),
    ...tuningOps(state.tuning),
    ...redEyeOps(state.redEye),
    ...effectOps(state.effects),
  ];
}

/// The v1 recipe packet the view persists to `fb:Recipe` (edit-recipe
/// spec §1). An empty `ops` array means "no edits" and is stored as no
/// recipe at all by the backend.
export function recipeFromState(state: EditState): {
  spec: "fotobase.recipe";
  version: 1;
  ops: RecipeOp[];
} {
  return { spec: "fotobase.recipe", version: 1, ops: opsFromEditState(state) };
}

/// Rebuild the tab states from a stored recipe. Ops the tabs do not own
/// (retouch, text — written by future tooling) are skipped, never fail the
/// load, matching the reader conformance rule (spec §5).
export function stateFromRecipe(recipe: {
  ops?: Array<{ type: string; params?: Record<string, unknown> }>;
}): EditState {
  const state: EditState = {
    crop: { ...DEFAULT_CROP, crop: { ...DEFAULT_CROP.crop } },
    redEye: { ...DEFAULT_RED_EYE, regions: [] },
    autoFix: { ...DEFAULT_AUTO_FIX },
    tuning: { ...DEFAULT_TUNING },
    effects: { ...DEFAULT_EFFECTS },
  };
  for (const op of recipe.ops ?? []) {
    const params = op.params ?? {};
    switch (op.type) {
      case "crop":
        state.crop = {
          ...state.crop,
          crop: {
            x: num(params.x, 0),
            y: num(params.y, 0),
            w: num(params.w, 1),
            h: num(params.h, 1),
          },
        };
        break;
      case "straighten":
        state.crop = {
          ...state.crop,
          angle: num(params.angle, 0),
          autoFill: params.auto_fill !== false,
        };
        break;
      case "auto_color":
        state.autoFix = { ...state.autoFix, autoColor: true };
        break;
      case "auto_contrast":
        state.autoFix = { ...state.autoFix, autoContrast: true };
        break;
      case "fill_light":
        state.tuning = { ...state.tuning, fillLight: num(params.amount, 0) };
        break;
      case "highlight_recovery":
        state.tuning = { ...state.tuning, highlights: num(params.amount, 0) };
        break;
      case "shadow_recovery":
        state.tuning = { ...state.tuning, shadows: num(params.amount, 0) };
        break;
      case "temperature":
        state.tuning = {
          ...state.tuning,
          temperature: num(params.kelvin_shift, 0),
        };
        break;
      case "tint":
        state.tuning = { ...state.tuning, tint: num(params.green_magenta, 0) };
        break;
      case "red_eye":
        if (params.auto === true) {
          state.redEye = { ...state.redEye, auto: true };
        } else if (Array.isArray(params.regions)) {
          state.redEye = {
            ...state.redEye,
            regions: (params.regions as Array<Record<string, unknown>>).map(
              (r) => ({
                x: num(r.x, 0),
                y: num(r.y, 0),
                r: num(r.r, 0),
              }),
            ),
          };
        }
        break;
      case "sharpen":
      case "sepia":
      case "bw":
      case "saturation":
        state.effects = { ...state.effects, [op.type]: num(params.amount, 0) };
        break;
      default:
        // Unknown op: skip with no failure (spec §2), the engine still
        // applies it at render time.
        break;
    }
  }
  return state;
}

function num(v: unknown, fallback: number): number {
  return typeof v === "number" && Number.isFinite(v) ? v : fallback;
}

/// True when every tab sits at its neutral position — nothing to persist,
/// nothing to render differently from the original.
export function isEmptyState(state: EditState): boolean {
  return opsFromEditState(state).length === 0;
}

// --- Session undo/redo (PLAN §2D; per-photo history is 04-003) --------------

/// Undo/redo history: linear past/present/future, values shared by
/// reference (states are replaced wholesale, never mutated).
export interface EditHistory {
  past: EditState[];
  present: EditState;
  future: EditState[];
}

export function editHistory(initial: EditState): EditHistory {
  return { past: [], present: initial, future: [] };
}

/// Commit a new state. Pushing the current state back is a no-op so a
/// slider dragged to its old position does not pollute the history.
export function pushEditState(
  history: EditHistory,
  next: EditState,
): EditHistory {
  if (next === history.present) return history;
  return {
    past: [...history.past, history.present],
    present: next,
    future: [],
  };
}

export function undo(history: EditHistory): EditHistory | null {
  if (history.past.length === 0) return null;
  const past = history.past.slice(0, -1);
  const previous = history.past[history.past.length - 1];
  return {
    past,
    present: previous,
    future: [history.present, ...history.future],
  };
}

export function redo(history: EditHistory): EditHistory | null {
  if (history.future.length === 0) return null;
  const [next, ...future] = history.future;
  return { past: [...history.past, history.present], present: next, future };
}
