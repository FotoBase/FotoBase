// Pure state + recipe assembly for the edit view (TASK-04-010, PLAN §2D
// "Edit view — single photo with edit tabs and filmstrip"). The view keeps
// one state object per tab; this module folds them into a single v1 recipe
// (docs/edit-recipe-spec-v1.md §1–3), preserving saved op order, drives the
// session undo/redo history, and rebuilds the tab states from a stored
// recipe so reopening an edited photo restores the sliders. The pixel work
// is the shared Rust engine at preview/export resolution; this module never
// touches files and never sees image bytes (PLAN §3 rule 3).

import { opsFromState as autoFixOps, type AutoFixState } from "./auto-fix";
import {
  opsFromState as cropOps,
  type CropStraightenState,
} from "./crop-straighten";
import {
  effectFromOp,
  opsFromEffects as effectOps,
  type EffectsState,
} from "./effects";
import { opsFromState as redEyeOps, type RedEyeState } from "./red-eye";
import {
  DEFAULT_TEXT,
  isTextOpOwned,
  opsFromText,
  textFromOps,
  type TextState,
} from "./text-overlay";
import {
  DEFAULT_RETOUCH,
  opsFromRetouch,
  retouchFromOps,
  type RetouchState,
} from "./retouch";
import {
  DEFAULT_TUNING,
  opsFromState as tuningOps,
  type TuningState,
} from "./tuning";

/// The edit view's tabs (PLAN §2D).
export type EditTab = "basic" | "tuning" | "pro" | "effects";

export const EDIT_TABS: readonly EditTab[] = ["basic", "tuning", "pro", "effects"];

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

export const DEFAULT_EFFECTS: EffectsState = {};/// The untouched edit state: every tab at its neutral position.

export interface ProState {
  curves: { enabled: boolean; points: number[][] };
  levels: { black: number; white: number; gamma: number };
  hsl: { hue: number; saturation: number; luminance: number };
  exposure: number;
  vibrance: number;
  keystone: { top: number; bottom: number; left: number; right: number };
  /// Resolved `lens_correction` params (Lensfun XML profile + shot data),
  /// from the `lens_profile` backend command; `null` = no correction.
  lensCorrection: Record<string, unknown> | null;
  noiseReduction: { amount: number; radius: number };
}

export const DEFAULT_PRO: ProState = {
  curves: { enabled: false, points: [[0, 0], [0.5, 0.5], [1, 1]] },
  levels: { black: 0, white: 1, gamma: 1 },
  hsl: { hue: 0, saturation: 0, luminance: 0 },
  exposure: 0,
  vibrance: 0,
  keystone: { top: 0, bottom: 0, left: 0, right: 0 },
  lensCorrection: null,
  noiseReduction: { amount: 0, radius: 0.01 },
};/// The untouched edit state: every tab at its neutral position.
export const EMPTY_EDIT_STATE: EditState = {
  crop: DEFAULT_CROP,
  redEye: DEFAULT_RED_EYE,
  retouch: DEFAULT_RETOUCH,
  text: DEFAULT_TEXT,
  autoFix: DEFAULT_AUTO_FIX,
  tuning: DEFAULT_TUNING,
  pro: DEFAULT_PRO,
  effects: DEFAULT_EFFECTS,
  autoRotate: null,
  extra: [],
};

export interface EditState {
  crop: CropStraightenState;
  redEye: RedEyeState;
  /// Heal-brush strokes (TASK-08-002).
  retouch: RetouchState;
  /// Text overlays (TASK-08-003).
  text: TextState;
  autoFix: AutoFixState;
  tuning: TuningState;
  /// F2 advanced-editor operations (curves, levels, HSL, exposure,
  /// vibrance, keystone, lens correction, and noise reduction).
  pro: ProState;
  effects: EffectsState;
  /// AI-estimated leveling angle in degrees (TASK-07-007), `null` when no
  /// estimate was applied. Emitted as the v1 `auto_rotate` op.
  autoRotate: number | null;
  /// Ops no tab owns (legacy text, rotate, film grain — written by the
  /// batch flow or future tooling). Passed through verbatim so saving an
  /// edit never destroys edits the tabs cannot represent (spec §5: a
  /// reader must not lose foreign data).
  extra: RecipeOp[];
  sourceRecipe?: { ops: RecipeOp[]; baseline: RecipeOp[] };
}

export type RecipeOp = { type: string; params: Record<string, unknown> };

/// Fold tab ops into a recipe, preserving the order of loaded operations.
/// New recipes use the canonical order from edit-recipe spec §3.
export function opsFromEditState(state: EditState): RecipeOp[] {
  const canonical = canonicalOpsFromEditState(state);
  if (!state.sourceRecipe) return canonical;

  const { ops, baseline } = state.sourceRecipe;
  const types = new Set([...baseline, ...canonical].map(recipeOpGroup));
  let result = [...ops];
  for (const type of types) {
    const before = baseline.filter((op) => recipeOpGroup(op) === type);
    const after = canonical.filter((op) => recipeOpGroup(op) === type);
    if (JSON.stringify(before) === JSON.stringify(after)) continue;
    if (before.length > 1) {
      const indices = result.flatMap((op, index) => recipeOpGroup(op) === type ? [index] : [])
        .slice(-before.length);
      if (indices.length === before.length) {
        reconcileRecipeGroup(result, indices, before, after);
        continue;
      }
    }
    let index = result.length - 1;
    while (index >= 0 && recipeOpGroup(result[index]) !== type) index--;
    if (index < 0) {
      const canonicalIndex = canonical.findIndex((op) => recipeOpGroup(op) === type);
      const later = new Set(canonical.slice(canonicalIndex + 1).map(recipeOpGroup));
      const next = result.findIndex((op) => later.has(recipeOpGroup(op)));
      result.splice(next < 0 ? result.length : next, 0, ...after);
    } else {
      const current = result[index];
      let replacement = after.length === 1
        ? [mergeRecipeOp(current, before[0], after[0])]
        : after;
      if (after.length === 0 && before.length === 1) {
        const known = new Set(Object.keys(before[0].params));
        const neutral = neutralProParams(current.type);
        if (neutral && Object.keys(current.params).some((key) => !known.has(key))) {
          replacement = [{ ...current, params: { ...current.params, ...neutral } }];
        }
      }
      result.splice(index, 1, ...replacement);
    }
  }
  return result;
}

function mergeRecipeOp(current: RecipeOp, before: RecipeOp | undefined, after: RecipeOp): RecipeOp {
  const params = { ...current.params };
  for (const key of Object.keys(before?.params ?? {})) {
    if (!(key in after.params)) delete params[key];
  }
  return { ...current, ...after, params: { ...params, ...after.params } };
}

function reconcileRecipeGroup(
  result: RecipeOp[],
  indices: number[],
  before: RecipeOp[],
  after: RecipeOp[],
): void {
  const same = (i: number, j: number) => JSON.stringify(before[i]) === JSON.stringify(after[j]);
  const costs = Array.from({ length: before.length + 1 }, () => Array<number>(after.length + 1).fill(0));
  for (let i = before.length; i >= 0; i--) {
    for (let j = after.length; j >= 0; j--) {
      if (i === before.length) costs[i][j] = after.length - j;
      else if (j === after.length) costs[i][j] = before.length - i;
      else costs[i][j] = Math.min(
        costs[i + 1][j + 1] + (same(i, j) ? 0 : 1),
        costs[i + 1][j] + 1,
        costs[i][j + 1] + 1,
      );
    }
  }
  let i = 0;
  let j = 0;
  let offset = 0;
  while (i < before.length || j < after.length) {
    const position = i < before.length
      ? indices[i] + offset
      : indices[indices.length - 1] + offset + 1;
    if (i < before.length && j < after.length &&
        costs[i][j] === costs[i + 1][j + 1] + (same(i, j) ? 0 : 1)) {
      if (!same(i, j)) result[position] = mergeRecipeOp(result[position], before[i], after[j]);
      i++;
      j++;
    } else if (i < before.length && costs[i][j] === costs[i + 1][j] + 1) {
      result.splice(position, 1);
      i++;
      offset--;
    } else {
      result.splice(position, 0, after[j]);
      j++;
      offset++;
    }
  }
}

function recipeOpGroup(op: RecipeOp): string {
  if (op.type === "text") return isTextOpOwned(op.type, op.params) ? "text:modern" : "text:legacy";
  if (op.type === "tint") return "color" in op.params ? "tint:color" : "tint:balance";
  return op.type;
}

function neutralProParams(type: string): Record<string, unknown> | null {
  switch (type) {
    case "curves": return { points: [[0, 0], [1, 1]] };
    case "levels": return { black: 0, white: 1, gamma: 1 };
    case "hsl": return { hue: 0, saturation: 0, luminance: 0 };
    case "exposure": return { ev: 0 };
    case "vibrance": return { amount: 0 };
    case "keystone": return { top: 0, bottom: 0, left: 0, right: 0 };
    case "noise_reduction": return { amount: 0, radius: 0.01 };
    default: return null;
  }
}

function canonicalOpsFromEditState(state: EditState): RecipeOp[] {
  const effects = effectOps(state.effects);
  const photoEffects = effects.filter((op) => !FRAME_OPS.has(op.type));
  const frameEffects = effects.filter((op) => FRAME_OPS.has(op.type));
  const pro = state.pro ?? DEFAULT_PRO;
  const proOps: RecipeOp[] = [];
  if (pro.curves.enabled) {
    proOps.push({ type: "curves", params: { points: pro.curves.points } });
  }
  if (pro.levels.black !== 0 || pro.levels.white !== 1 || pro.levels.gamma !== 1) {
    proOps.push({ type: "levels", params: { ...pro.levels } });
  }
  if (pro.hsl.hue !== 0 || pro.hsl.saturation !== 0 || pro.hsl.luminance !== 0) {
    proOps.push({ type: "hsl", params: { ...pro.hsl } });
  }
  if (pro.exposure !== 0) {
    proOps.push({ type: "exposure", params: { ev: pro.exposure } });
  }
  if (pro.vibrance !== 0) {
    proOps.push({ type: "vibrance", params: { amount: pro.vibrance } });
  }
  if (Object.values(pro.keystone).some((v) => v !== 0)) {
    proOps.push({ type: "keystone", params: { ...pro.keystone } });
  }
  if (pro.lensCorrection) {
    proOps.push({ type: "lens_correction", params: { ...pro.lensCorrection } });
  }
  if (pro.noiseReduction.amount !== 0) {
    proOps.push({ type: "noise_reduction", params: { ...pro.noiseReduction } });
  }
  return [
    // Strokes are placed on the uncropped preview: heal the source pixels
    // first, then let geometry ops move them.
    ...opsFromRetouch(state.retouch),
    ...cropOps(state.crop),
    ...(state.autoRotate !== null
      ? [{ type: "auto_rotate", params: { angle_deg: state.autoRotate } } satisfies RecipeOp]
      : []),
    ...autoFixOps(state.autoFix),
    ...tuningOps(state.tuning),
    ...proOps,
    ...redEyeOps(state.redEye),
    ...photoEffects,
    // Text sits on the finished photo; frames grow the canvas, so they
    // wrap the text too (spec §3: frames after every photo-content op).
    ...opsFromText(state.text ?? DEFAULT_TEXT),
    ...frameEffects,
    ...(state.extra ?? []),
  ];
}

const FRAME_OPS: ReadonlySet<string> = new Set(["border", "drop_shadow"]);

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

export function moveRecipeOp(state: EditState, from: number, to: number): EditState {
  const ops = opsFromEditState(state);
  if (!Number.isInteger(from) || !Number.isInteger(to) ||
      from < 0 || from >= ops.length || to < 0 || to >= ops.length) {
    throw new RangeError("Recipe operation index out of range");
  }
  if (from === to) return state;
  const [op] = ops.splice(from, 1);
  ops.splice(to, 0, op);
  return stateWithRecipeOps(ops);
}

export function duplicateRecipeOp(state: EditState, index: number): EditState {
  const ops = opsFromEditState(state);
  if (!Number.isInteger(index) || index < 0 || index >= ops.length) {
    throw new RangeError("Recipe operation index out of range");
  }
  ops.splice(index + 1, 0, { ...ops[index], params: { ...ops[index].params } });
  return stateWithRecipeOps(ops);
}

export function removeRecipeOp(state: EditState, index: number): EditState {
  const ops = opsFromEditState(state);
  if (!Number.isInteger(index) || index < 0 || index >= ops.length) {
    throw new RangeError("Recipe operation index out of range");
  }
  ops.splice(index, 1);
  return stateWithRecipeOps(ops);
}

function stateWithRecipeOps(ops: RecipeOp[]): EditState {
  const restored = stateFromRecipe({ ops });
  restored.sourceRecipe = { ops, baseline: canonicalOpsFromEditState(restored) };
  return restored;
}

/// Rebuild the tab states from a stored recipe. Ops the tabs do not own
/// (legacy text — written by older versions) are skipped, never fail the
/// load, matching the reader conformance rule (spec §5).
export function stateFromRecipe(recipe: {
  spec?: string;
  version?: number;
  ops?: Array<{ type: string; params?: Record<string, unknown> }>;
}): EditState {
  const state: EditState = {
    crop: { ...DEFAULT_CROP, crop: { ...DEFAULT_CROP.crop } },
    redEye: { ...DEFAULT_RED_EYE, regions: [] },
    retouch: { strokes: [] },
    text: { overlays: [] },
    autoFix: { ...DEFAULT_AUTO_FIX },
    tuning: { ...DEFAULT_TUNING },
    pro: {
      curves: { ...DEFAULT_PRO.curves, points: DEFAULT_PRO.curves.points.map((p) => [...p]) },
      levels: { ...DEFAULT_PRO.levels },
      hsl: { ...DEFAULT_PRO.hsl },
      exposure: 0,
      vibrance: 0,
      keystone: { ...DEFAULT_PRO.keystone },
      lensCorrection: null,
      noiseReduction: { ...DEFAULT_PRO.noiseReduction },
    },
    effects: { ...DEFAULT_EFFECTS },
    autoRotate: null,
    extra: [],
  };
  const textParams: Array<Record<string, unknown>> = [];
  for (const op of recipe.ops ?? []) {
    const params = op.params ?? {};
    if (isTextOpOwned(op.type, params)) {
      textParams.push(params);
      continue;
    }
    const owned = effectFromOp(op.type, params);
    if (owned !== null) {
      state.effects = { ...state.effects, [owned[0]]: owned[1] };
      continue;
    }
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
      case "auto_rotate":
        state.autoRotate = num(params.angle_deg, 0);
        break;
      case "retouch":
        state.retouch = {
          strokes: [...state.retouch.strokes, ...retouchFromOps([params]).strokes],
        };
        break;
      case "auto_color":
        state.autoFix = { ...state.autoFix, autoColor: true };
        break;
      case "auto_contrast":
        state.autoFix = { ...state.autoFix, autoContrast: true };
        break;
      case "curves":
        if (Array.isArray(params.points)) {
          state.pro = {
            ...state.pro,
            curves: {
              enabled: true,
              points: (params.points as unknown[]).filter(Array.isArray).map((p) => [
                num((p as unknown[])[0], 0),
                num((p as unknown[])[1], 0),
              ]),
            },
          };
        }
        break;
      case "levels":
        state.pro = {
          ...state.pro,
          levels: {
            black: num(params.black, 0),
            white: num(params.white, 1),
            gamma: num(params.gamma, 1),
          },
        };
        break;
      case "hsl":
        state.pro = {
          ...state.pro,
          hsl: {
            hue: num(params.hue, 0),
            saturation: num(params.saturation, 0),
            luminance: num(params.luminance, 0),
          },
        };
        break;
      case "exposure":
        state.pro = { ...state.pro, exposure: num(params.ev, 0) };
        break;
      case "vibrance":
        state.pro = { ...state.pro, vibrance: num(params.amount, 0) };
        break;
      case "keystone":
        state.pro = {
          ...state.pro,
          keystone: {
            top: num(params.top, 0),
            bottom: num(params.bottom, 0),
            left: num(params.left, 0),
            right: num(params.right, 0),
          },
        };
        break;
      case "lens_correction":
        state.pro = { ...state.pro, lensCorrection: { ...params } };
        break;
      case "noise_reduction":
        state.pro = {
          ...state.pro,
          noiseReduction: {
            amount: num(params.amount, 0),
            radius: num(params.radius, 0.01),
          },
        };
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
        // A `tint` with `color` is the Effects-tab wash (TASK-08-001); one
        // the tab does not own verbatim (custom color) is kept, never
        // squeezed into the white-balance slider.
        if ("color" in params) {
          state.extra = [...state.extra, { type: op.type, params }];
        } else {
          state.tuning = {
            ...state.tuning,
            tint: num(params.green_magenta, 0),
          };
        }
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
      default:
        // An op no tab owns (text, rotate, film grain — the batch
        // flow or future tooling): keep it so a later save re-emits it
        // verbatim instead of silently dropping the user's edits.
        state.extra = [...state.extra, { type: op.type, params }];
        break;
    }
  }
  state.text = textFromOps(textParams);
  const originalOps = (recipe.ops ?? []).map((op) => ({ ...op, params: op.params ?? {} }));
  const baseline = canonicalOpsFromEditState(state);
  if (JSON.stringify(originalOps) !== JSON.stringify(baseline)) {
    state.sourceRecipe = {
      ops: originalOps,
      baseline,
    };
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
