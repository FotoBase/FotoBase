// Pure state + op emission for the Basic Fixes Tuning tab (TASK-04-008,
// PLAN §2E). Four sliders map to recipe ops — Fill Light (`fill_light`),
// Highlights (`highlight_recovery`), Shadows (`shadow_recovery`), Color
// Temperature (`temperature`) — and the neutral-color picker's computed
// balance lands on the temperature/tint params. The pixel work (including
// the picker's sample → params math) is done by the shared Rust engine
// (golden-tested) at preview/export resolution; this module only records
// slider positions. Slider positions persist in the recipe and round-trip
// through XMP via the recipe sidecar field (edit-recipe spec §2). Never
// touches the original file.

/// The tuning state the panel edits and the edit view persists.
export interface TuningState {
  /// `fill_light` amount [0, 1] — shadow lift.
  fillLight: number;
  /// `highlight_recovery` amount [0, 1].
  highlights: number;
  /// `shadow_recovery` amount [0, 1].
  shadows: number;
  /// `temperature` `kelvin_shift` [-1, 1] (negative = cooler).
  temperature: number;
  /// `tint` `green_magenta` [-1, 1] — set by the neutral picker.
  tint: number;
}

/// Slider bounds per key (edit-recipe spec §3 param ranges).
const RANGES: Record<keyof TuningState, [number, number]> = {
  fillLight: [0, 1],
  highlights: [0, 1],
  shadows: [0, 1],
  temperature: [-1, 1],
  tint: [-1, 1],
};

/// The untouched state: every slider at neutral, no ops emitted.
export const DEFAULT_TUNING: TuningState = {
  fillLight: 0,
  highlights: 0,
  shadows: 0,
  temperature: 0,
  tint: 0,
};

/// Clamp a slider value to its op's documented param range.
export function clampedSlider(key: keyof TuningState, value: number): number {
  const [min, max] = RANGES[key];
  return Math.min(max, Math.max(min, value));
}

export type TuningOp = {
  type:
    | "fill_light"
    | "highlight_recovery"
    | "shadow_recovery"
    | "temperature"
    | "tint";
  params: Record<string, unknown>;
};

/// Recipe ops for the current state. A slider at its neutral position emits
/// no op at all, so an untouched Tuning tab adds nothing to the recipe.
export function opsFromState(state: TuningState): TuningOp[] {
  const ops: TuningOp[] = [];
  if (state.fillLight !== 0) {
    ops.push({ type: "fill_light", params: { amount: state.fillLight } });
  }
  if (state.highlights !== 0) {
    ops.push({
      type: "highlight_recovery",
      params: { amount: state.highlights },
    });
  }
  if (state.shadows !== 0) {
    ops.push({ type: "shadow_recovery", params: { amount: state.shadows } });
  }
  if (state.temperature !== 0) {
    ops.push({
      type: "temperature",
      params: { kelvin_shift: state.temperature },
    });
  }
  if (state.tint !== 0) {
    ops.push({ type: "tint", params: { green_magenta: state.tint } });
  }
  return ops;
}

/// The engine's answer to a picker sample (`neutral_balance` command):
/// the temperature/tint params that neutralize the sampled pixel.
export interface WhiteBalance {
  kelvinShift: number;
  greenMagenta: number;
}

/// Land a picker balance on the current state: the computed temperature and
/// tint *replace* any previous balance (the picker is the temperature/tint
/// authority; the sliders show what it chose).
export function applyBalance(
  state: TuningState,
  balance: WhiteBalance,
): TuningState {
  return {
    ...state,
    temperature: clampedSlider("temperature", balance.kelvinShift),
    tint: clampedSlider("tint", balance.greenMagenta),
  };
}

/// Ask the engine to turn a sampled pixel (normalized [0, 1] RGB, as read
/// from the preview canvas) into the temperature/tint params that
/// neutralize it. The math lives in the Rust engine next to the ops it
/// inverts (`fotobase_core::apply::neutral_balance`).
export async function balanceFromSample(
  sample: [number, number, number],
): Promise<WhiteBalance> {
  const { invoke } = await import("@tauri-apps/api/core");
  const [r, g, b] = sample;
  return await invoke<WhiteBalance>("neutral_balance", { r, g, b });
}

/// Average a small window of pixels around a click point from raw RGBA
/// canvas data, returning normalized [0, 1] RGB. The window is clamped at
/// the image edges; alpha is ignored (the preview is opaque).
export function sampleNeutral(
  data: Uint8ClampedArray,
  width: number,
  height: number,
  cx: number,
  cy: number,
): [number, number, number] {
  let r = 0;
  let g = 0;
  let b = 0;
  let n = 0;
  for (let y = Math.max(0, cy - 1); y <= Math.min(height - 1, cy + 1); y++) {
    for (let x = Math.max(0, cx - 1); x <= Math.min(width - 1, cx + 1); x++) {
      const i = (y * width + x) * 4;
      r += data[i];
      g += data[i + 1];
      b += data[i + 2];
      n++;
    }
  }
  return [r / (n * 255), g / (n * 255), b / (n * 255)];
}
