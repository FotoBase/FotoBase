// Pure geometry + op emission for the Basic Fixes crop/straighten controls
// (TASK-04-005). All geometry is normalized to [0, 1] per the edit-recipe
// spec §2, so the same values feed the recipe ops (Rust engine, shared
// preview/export path) and the CSS-transform live preview.

/// A normalized crop rectangle, fractions of image width/height.
export interface CropRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/// The crop/straighten state the panel edits and the edit view persists.
export interface CropStraightenState {
  crop: CropRect;
  /// Straighten angle in degrees, CW positive; small angles only (±10).
  angle: number;
  /// Straighten auto-fill: scale the content so no empty corner shows.
  autoFill: boolean;
}

/// The smallest crop the UI will allow, in normalized units — keeps handles
/// grabbable and prevents a zero-area crop.
const MIN_SIZE = 0.05;

const clamp01 = (v: number): number => Math.min(1, Math.max(0, v));

/// Fit a rect back inside the image, never smaller than MIN_SIZE.
export function clampCrop(r: CropRect): CropRect {
  const w = Math.min(Math.max(r.w, MIN_SIZE), 1);
  const h = Math.min(Math.max(r.h, MIN_SIZE), 1);
  return {
    x: clamp01(Math.min(r.x, 1 - w)),
    y: clamp01(Math.min(r.y, 1 - h)),
    w,
    h,
  };
}

/// The largest rect with the given aspect ratio (w/h) that fits an image of
/// aspect `imgAspect` (w/h), centered — returned in normalized coordinates
/// (fractions of image width/height).
export function maxRectWithRatio(imgAspect: number, ratio: number): CropRect {
  const wPx = Math.min(imgAspect, ratio);
  const hPx = wPx / ratio;
  return {
    x: (imgAspect - wPx) / 2 / imgAspect,
    y: (1 - hPx) / 2,
    w: wPx / imgAspect,
    h: hPx,
  };
}

/// Preset aspect ratios offered by the Basic Fixes tab (spec: free + 1:1,
/// 4:3 / 3:4, 6:4 / 4:6, 16:9). `null` ratio = free.
export interface AspectPreset {
  id: string;
  ratio: number | null;
}

export const ASPECT_PRESETS: AspectPreset[] = [
  { id: "free", ratio: null },
  { id: "1:1", ratio: 1 },
  { id: "4:3", ratio: 4 / 3 },
  { id: "3:4", ratio: 3 / 4 },
  { id: "6:4", ratio: 6 / 4 },
  { id: "4:6", ratio: 4 / 6 },
  { id: "16:9", ratio: 16 / 9 },
];

/// Apply a preset to the current crop: free resets to the full image; a
/// ratio snaps to the largest matching rect for an image with the given
/// aspect (w/h, normalized coordinates are relative to the image box) —
/// a fresh preset should not keep a tiny crop.
export function applyAspectPreset(
  current: CropRect,
  ratio: number | null,
  imgAspect = 1,
): CropRect {
  if (ratio === null) return { x: 0, y: 0, w: 1, h: 1 };
  return maxRectWithRatio(imgAspect, 1, ratio);
}

/// Which crop handle is being dragged.
export type Handle = "tl" | "tr" | "bl" | "br";

/// Resize the crop by dragging `handle` by `(dx, dy)` normalized units.
/// When `ratio` is non-null the rect keeps that aspect (w/h), anchored at
/// the corner opposite the dragged handle. Result is clamped.
export function resizeCrop(
  start: CropRect,
  handle: Handle,
  dx: number,
  dy: number,
  ratio: number | null,
): CropRect {
  let r: CropRect;
  if (ratio === null) {
    const left = handle === "tl" || handle === "bl";
    const top = handle === "tl" || handle === "tr";
    r = {
      x: left ? start.x + dx : start.x,
      y: top ? start.y + dy : start.y,
      w: start.w + (left ? -dx : dx),
      h: start.h + (top ? -dy : dy),
    };
  } else {
    // Anchor the opposite corner; project the drag onto the ratio diagonal
    // (dirX, dirY·ratio) so one gesture stays on the ratio line.
    const anchorX = handle === "tl" || handle === "bl" ? start.x + start.w : start.x;
    const anchorY = handle === "tl" || handle === "tr" ? start.y + start.h : start.y;
    const dirX = handle === "tl" || handle === "bl" ? -1 : 1;
    const dirY = handle === "tl" || handle === "tr" ? -1 : 1;
    const s = (dx * dirX + dy * dirY * ratio) / (1 + ratio * ratio);
    let w = Math.max(start.w + s * dirX, MIN_SIZE);
    let h = w / ratio;
    r = {
      x: dirX > 0 ? anchorX : anchorX - w,
      y: dirY > 0 ? anchorY : anchorY - h,
      w,
      h,
    };
  }
  return clampCrop(r);
}

/// Move the crop by `(dx, dy)` normalized units, clamped inside the image.
export function moveCrop(r: CropRect, dx: number, dy: number): CropRect {
  return clampCrop({ ...r, x: r.x + dx, y: r.y + dy });
}

/// Scale factor so a rotated frame of `w × h` (normalized) rotated by
/// `angleDeg` still covers the frame — the auto-crop rule (no empty
/// corners). `enabled=false` or an angle that already fits returns 1.
export function autoFillScale(angleDeg: number, w: number, h: number, enabled = true): number {
  if (!enabled) return 1;
  const t = (Math.abs(angleDeg) * Math.PI) / 180;
  const needW = w * Math.cos(t) + h * Math.sin(t);
  const needH = w * Math.sin(t) + h * Math.cos(t);
  const scale = Math.min(w / needW, h / needH);
  return scale >= 1 ? 1 : scale;
}

/// CSS transform for the live preview: straighten rotation plus the
/// auto-fill shrink, and the crop applied via the element box (the caller
/// positions/sizes the image element to the crop rect).
export function previewTransform(state: CropStraightenState): string {
  const scale = autoFillScale(state.angle, state.crop.w, state.crop.h, state.autoFill);
  return `rotate(${state.angle}deg) scale(${scale})`;
}

/// Recipe ops for the current state, in spec §3 order (crop before
/// downstream ops). Default state emits no ops at all.
export function opsFromState(state: {
  crop: CropRect;
  angle: number;
  autoFill?: boolean;
}): Array<{ type: "crop" | "straighten"; params: Record<string, number | boolean> }> {
  const ops: Array<{ type: "crop" | "straighten"; params: Record<string, number | boolean> }> = [];
  const full = state.crop.x <= 0 && state.crop.y <= 0 && state.crop.w >= 1 && state.crop.h >= 1;
  if (!full) {
    ops.push({
      type: "crop",
      params: { x: state.crop.x, y: state.crop.y, w: state.crop.w, h: state.crop.h },
    });
  }
  if (state.angle !== 0) {
    ops.push({
      type: "straighten",
      params: { angle: state.angle, auto_fill: state.autoFill ?? true },
    });
  }
  return ops;
}
