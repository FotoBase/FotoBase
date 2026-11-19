// Pure state + op emission for the Basic Fixes Effects tab (TASK-04-009,
// PLAN §2E Effects, §6 P2b starter effects). Each effect is a one-click
// recipe op with a strength param (`amount`): Sharpen, Sepia, B&W and
// Saturation. The pixel work is done by the shared Rust engine
// (golden-tested) at preview/export resolution; this module only records
// which effects are on and at what strength. The B&W *recipe op* is
// distinct from the EPIC-03 view-only B&W display mode. Slider positions
// persist in the recipe and round-trip through XMP via the recipe sidecar
// field (edit-recipe spec §2). Never touches the original file.
//
// TASK-08-001 adds Warmify, Tint, Film Grain, Soft Focus and Vignette plus
// the Border / Drop Shadow frames. Frames grow the canvas, so they come last
// in the canonical order and photo effects never act on the frame.

/// Effect and frame ops owned by the Effects tab; each name is its op type.
export type EffectName =
  | "sharpen"
  | "sepia"
  | "bw"
  | "saturation"
  | "warmify"
  | "tint"
  | "film_grain"
  | "soft_focus"
  | "vignette"
  | "border"
  | "drop_shadow";

/// Canonical op order: photo effects first, frames last.
export const EFFECTS: readonly EffectName[] = [
  "sharpen",
  "sepia",
  "bw",
  "saturation",
  "warmify",
  "tint",
  "film_grain",
  "soft_focus",
  "vignette",
  "border",
  "drop_shadow",
];

/// Color the one-click Tint effect washes the photo toward. It is also what
/// tells the engine this `tint` op is the effect, not the Tuning-tab
/// green/magenta slider.
export const DEFAULT_TINT_COLOR = "#3a7bd5";

/// Strength the one-click button applies (the op's `amount` param).
export const DEFAULT_STRENGTH: Record<EffectName, number> = {
  sharpen: 0.5,
  sepia: 1,
  bw: 1,
  saturation: 0.3,
  warmify: 0.5,
  tint: 0.5,
  film_grain: 0.5,
  soft_focus: 0.5,
  vignette: 0.5,
  border: 0.5,
  drop_shadow: 0.5,
};

/// Param range per effect (edit-recipe spec §3).
const RANGES: Record<EffectName, [number, number]> = {
  sharpen: [0, 1],
  sepia: [0, 1],
  bw: [0, 1],
  saturation: [-1, 1],
  warmify: [0, 1],
  tint: [0, 1],
  film_grain: [0, 1],
  soft_focus: [0, 1],
  vignette: [0, 1],
  border: [0, 1],
  drop_shadow: [0, 1],
};

/// The effects state: effect name → strength. Absent = off.
export type EffectsState = Partial<Record<EffectName, number>>;

/// Clamp a strength to its op's documented param range.
export function clampedStrength(effect: EffectName, value: number): number {
  const [min, max] = RANGES[effect];
  return Math.min(max, Math.max(min, value));
}

/// One-click toggle: activate at the default strength, or remove.
export function toggleEffect(
  state: EffectsState,
  effect: EffectName,
): EffectsState {
  if (effect in state) {
    const { [effect]: _removed, ...rest } = state;
    return rest;
  }
  return { ...state, [effect]: DEFAULT_STRENGTH[effect] };
}

export type EffectOp = {
  type: EffectName;
  params: Record<string, unknown>;
};

/// Params the tab writes for an effect at a strength. Anything beyond
/// `amount` is a fixed companion param the engine needs to read the op as
/// this effect.
function paramsFor(
  effect: EffectName,
  strength: number,
): Record<string, unknown> {
  switch (effect) {
    case "tint":
      return { amount: strength, color: DEFAULT_TINT_COLOR };
    case "film_grain":
      return { amount: strength, seed: 0 };
    default:
      return { amount: strength };
  }
}

/// Recipe ops for the current state, in canonical effect order. A strength
/// of 0 emits no op at all — the buffer must stay bit-identical.
export function opsFromEffects(state: EffectsState): EffectOp[] {
  return EFFECTS.filter((e) => (state[e] ?? 0) !== 0).map((e) => ({
    type: e,
    params: paramsFor(e, state[e] as number),
  }));
}

/// The effect and strength a stored op represents, or null when the tab
/// does not own it (white-balance `tint`, a custom border color, a grain
/// seed, an `algo` marker…). Ownership is exact: the op must equal what the
/// tab itself would emit, so loading and saving never rewrites foreign
/// params.
export function effectFromOp(
  type: string,
  params: Record<string, unknown>,
): [EffectName, number] | null {
  const effect = EFFECTS.find((e) => e === type);
  const amount = params.amount;
  if (effect === undefined || typeof amount !== "number") return null;
  if (!Number.isFinite(amount)) return null;
  const expected = paramsFor(effect, amount);
  const keys = Object.keys(params);
  const exact =
    keys.length === Object.keys(expected).length &&
    keys.every((k) => params[k] === expected[k]);
  return exact ? [effect, amount] : null;
}
