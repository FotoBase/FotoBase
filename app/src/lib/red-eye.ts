// Pure geometry + op emission for the Basic Fixes red-eye controls
// (TASK-04-006). Regions are normalized circles ({x, y, r} fractions of
// image width/height, edit-recipe spec §2), so the same values feed the
// recipe op (Rust engine, shared preview/export path) and the overlay
// preview. Auto mode is the engine's detection heuristic; the panel just
// emits `{ auto: true }`.

/// A normalized circular region.
export interface RedEyeRegion {
  x: number;
  y: number;
  r: number;
}

/// The red-eye state the panel edits and the edit view persists.
export interface RedEyeState {
  /// One-click auto mode (engine-side detection).
  auto: boolean;
  /// Manually marked regions (ignored when `auto` is true).
  regions: RedEyeRegion[];
}

const clamp01 = (v: number): number => Math.min(1, Math.max(0, v));

const MIN_R = 0.01;
const MAX_R = 0.2;

/// Keep the center inside the image and the radius sane.
export function clampRegion(region: RedEyeRegion): RedEyeRegion {
  return {
    x: clamp01(region.x),
    y: clamp01(region.y),
    r: Math.min(Math.max(region.r, MIN_R), MAX_R),
  };
}

/// Build a region from a press point and the current drag point: the circle
/// is centered on the midpoint with half the drag distance as radius.
export function regionFromDrag(from: { x: number; y: number }, to: { x: number; y: number }): RedEyeRegion {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const r = Math.max(Math.hypot(dx, dy) / 2, MIN_R);
  return clampRegion({ x: (from.x + to.x) / 2, y: (from.y + to.y) / 2, r });
}

/// Is the normalized point inside the region (for hit-testing/removal)?
export function regionContains(region: RedEyeRegion, x: number, y: number): boolean {
  // Radii scale with width and height separately (matches the op's ellipse
  // geometry: rx = r·width, ry = r·height), so "contains" is per-axis too.
  const dx = (x - region.x) / region.r;
  const dy = (y - region.y) / region.r;
  return dx * dx + dy * dy <= 1;
}

/// Recipe ops for the current state. Default state emits no ops at all.
export function opsFromState(
  state: RedEyeState,
): Array<{ type: "red_eye"; params: Record<string, unknown> }> {
  if (state.auto) {
    return [{ type: "red_eye", params: { auto: true } }];
  }
  if (state.regions.length === 0) {
    return [];
  }
  return [
    {
      type: "red_eye",
      params: { regions: state.regions.map((g) => ({ x: g.x, y: g.y, r: g.r })) },
    },
  ];
}
