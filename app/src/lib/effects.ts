// Pure state + op emission for the Basic Fixes Effects tab (TASK-04-009,
// PLAN §2E Effects, §6 P2b starter effects). Each effect is a one-click
// recipe op with a strength param (`amount`): Sharpen, Sepia, B&W and
// Saturation. The pixel work is done by the shared Rust engine
// (golden-tested) at preview/export resolution; this module only records
// which effects are on and at what strength. The B&W *recipe op* is
// distinct from the EPIC-03 view-only B&W display mode. Slider positions
// persist in the recipe and round-trip through XMP via the recipe sidecar
// field (edit-recipe spec §2). Never touches the original file.

/// The four starter effect ops.
export type EffectName = "sharpen" | "sepia" | "bw" | "saturation";

export const EFFECTS: readonly EffectName[] = [
  "sharpen",
  "sepia",
  "bw",
  "saturation",
];

/// Strength the one-click button applies (the op's `amount` param).
export const DEFAULT_STRENGTH: Record<EffectName, number> = {
  sharpen: 0.5,
  sepia: 1,
  bw: 1,
  saturation: 0.3,
};

/// Param range per effect (edit-recipe spec §3).
const RANGES: Record<EffectName, [number, number]> = {
  sharpen: [0, 1],
  sepia: [0, 1],
  bw: [0, 1],
  saturation: [-1, 1],
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

/// Recipe ops for the current state, in canonical effect order. A strength
/// of 0 emits no op at all — the buffer must stay bit-identical.
export function opsFromEffects(state: EffectsState): EffectOp[] {
  return EFFECTS.filter((e) => (state[e] ?? 0) !== 0).map((e) => ({
    type: e,
    params: { amount: state[e] },
  }));
}
