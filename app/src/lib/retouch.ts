// Pure state + op emission for the heal brush (TASK-08-002, PLAN §2E
// Retouch, edit-recipe spec §3 `retouch`). The brush records strokes, never
// pixels: points are fractions of the image width/height and the radius is
// a fraction of the shorter side, so the shared Rust engine re-runs the heal
// identically at preview and export resolution (PLAN §3 rule 5).

/// A point as `[x, y]` fractions of the image width/height.
export type StrokePoint = [number, number];

export interface RetouchStroke {
  points: StrokePoint[];
  /// Brush radius, fraction of the image's shorter side.
  radius: number;
}

export interface RetouchState {
  strokes: RetouchStroke[];
}

export const DEFAULT_RETOUCH: RetouchState = { strokes: [] };

export const MIN_RADIUS = 0.005;
export const MAX_RADIUS = 0.15;
export const DEFAULT_RADIUS = 0.03;

const clamp01 = (v: number): number => Math.min(1, Math.max(0, v));

/// Round to 4 decimals: a screen-resolution drag needs no more, and it keeps
/// the stored recipe small.
const round4 = (v: number): number => Math.round(v * 10000) / 10000;

export function clampRadius(radius: number): number {
  return Math.min(MAX_RADIUS, Math.max(MIN_RADIUS, radius));
}

/// Build a stroke from the points of one drag. Points are clamped to the
/// image and consecutive duplicates (after rounding) dropped. No points, no
/// stroke.
export function strokeFromPoints(
  raw: ReadonlyArray<{ x: number; y: number }>,
  radius: number,
): RetouchStroke | null {
  const points: StrokePoint[] = [];
  for (const p of raw) {
    const next: StrokePoint = [round4(clamp01(p.x)), round4(clamp01(p.y))];
    const last = points[points.length - 1];
    if (last === undefined || last[0] !== next[0] || last[1] !== next[1]) {
      points.push(next);
    }
  }
  if (points.length === 0) return null;
  return { points, radius: clampRadius(radius) };
}

export function removeStroke(state: RetouchState, index: number): RetouchState {
  if (index < 0 || index >= state.strokes.length) return state;
  return { strokes: state.strokes.filter((_, i) => i !== index) };
}

export function undoLastStroke(state: RetouchState): RetouchState {
  return removeStroke(state, state.strokes.length - 1);
}

/// The recipe op for the current state. No strokes → no op.
export function opsFromRetouch(
  state: RetouchState,
): Array<{ type: "retouch"; params: Record<string, unknown> }> {
  if (state.strokes.length === 0) return [];
  return [
    {
      type: "retouch",
      params: {
        strokes: state.strokes.map((s) => ({
          points: s.points.map((p): StrokePoint => [p[0], p[1]]),
          radius: s.radius,
        })),
      },
    },
  ];
}

function strokeFromParams(raw: unknown): RetouchStroke | null {
  if (typeof raw !== "object" || raw === null) return null;
  const { points, radius } = raw as { points?: unknown; radius?: unknown };
  if (!Array.isArray(points) || points.length === 0) return null;
  if (typeof radius !== "number" || !(radius >= 0 && radius <= 1)) return null;
  const out: StrokePoint[] = [];
  for (const p of points) {
    if (!Array.isArray(p) || p.length !== 2) return null;
    const [x, y] = p;
    if (typeof x !== "number" || typeof y !== "number") return null;
    if (!(x >= 0 && x <= 1 && y >= 0 && y <= 1)) return null;
    out.push([x, y]);
  }
  return { points: out, radius };
}

/// Rebuild the state from the `params` of every stored `retouch` op, in
/// order. Strokes the engine would reject (malformed, out of range) are
/// dropped so a damaged recipe still opens.
export function retouchFromOps(
  paramsList: ReadonlyArray<Record<string, unknown>>,
): RetouchState {
  const strokes: RetouchStroke[] = [];
  for (const params of paramsList) {
    if (!Array.isArray(params.strokes)) continue;
    for (const raw of params.strokes) {
      const stroke = strokeFromParams(raw);
      if (stroke !== null) strokes.push(stroke);
    }
  }
  return { strokes };
}
