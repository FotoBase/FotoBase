// Pure state + op emission for the text tool (TASK-08-003, PLAN §2E Text,
// edit-recipe spec §3 `text`, `algo: 2`). An overlay is recipe params, never
// pixels: position is a fraction of the image width/height, size and outline
// width are fractions of the image height, so the shared Rust engine draws
// the same text at preview and export resolution (PLAN §3 rule 5) and every
// overlay stays editable and draggable.

export interface TextOverlay {
  text: string;
  /// Anchor point as fractions of the image width/height.
  x: number;
  y: number;
  /// Which point of the text block sits at (x, y); the UI places and drags
  /// by the center.
  anchor: string;
  /// `sans` / `serif` / `mono` (bundled) or an installed family name.
  font: string;
  /// Font size, fraction of the image height.
  size: number;
  color: string;
  /// Outline width, fraction of the image height; 0 = no outline.
  outlineWidth: number;
  outlineColor: string;
  /// Overall opacity of the overlay: 1 = opaque, 0 = invisible (the
  /// recipe's `transparency` param).
  transparency: number;
  /// Degrees, clockwise.
  rotation: number;
}

export interface TextState {
  overlays: TextOverlay[];
}

export const DEFAULT_TEXT: TextState = { overlays: [] };

export const MIN_SIZE = 0.01;
export const MAX_SIZE = 0.5;
export const MAX_OUTLINE = 0.05;
export const FONT_FAMILIES = ["sans", "serif", "mono"] as const;

export interface TextOp {
  type: "text";
  params: Record<string, unknown>;
}

const clamp = (v: number, lo: number, hi: number): number =>
  Math.min(hi, Math.max(lo, Number.isFinite(v) ? v : lo));

/// Round to 4 decimals: a screen-resolution drag needs no more, and it keeps
/// the stored recipe small.
const round4 = (v: number): number => Math.round(v * 10000) / 10000;

const HEX = /^#[0-9a-fA-F]{6}$/;

export function defaultOverlay(text = "Text"): TextOverlay {
  return {
    text,
    x: 0.5,
    y: 0.5,
    anchor: "center",
    font: "sans",
    size: 0.08,
    color: "#ffffff",
    outlineWidth: 0.004,
    outlineColor: "#000000",
    transparency: 1,
    rotation: 0,
  };
}

function normalized(o: TextOverlay): TextOverlay {
  return {
    ...o,
    x: round4(clamp(o.x, 0, 1)),
    y: round4(clamp(o.y, 0, 1)),
    size: clamp(o.size, MIN_SIZE, MAX_SIZE),
    outlineWidth: clamp(o.outlineWidth, 0, MAX_OUTLINE),
    transparency: clamp(o.transparency, 0, 1),
    rotation: clamp(o.rotation, -180, 180),
  };
}

export function addOverlay(state: TextState, text = "Text"): TextState {
  return { overlays: [...state.overlays, defaultOverlay(text)] };
}

export function updateOverlay(
  state: TextState,
  index: number,
  patch: Partial<TextOverlay>,
): TextState {
  if (index < 0 || index >= state.overlays.length) return state;
  return {
    overlays: state.overlays.map((o, i) =>
      i === index ? normalized({ ...o, ...patch }) : o,
    ),
  };
}

export function moveOverlay(
  state: TextState,
  index: number,
  x: number,
  y: number,
): TextState {
  return updateOverlay(state, index, { x, y });
}

export function removeOverlay(state: TextState, index: number): TextState {
  if (index < 0 || index >= state.overlays.length) return state;
  return { overlays: state.overlays.filter((_, i) => i !== index) };
}

/// One `text` op per overlay. An overlay without text is skipped: the engine
/// rejects empty text, and an empty label draws nothing anyway.
export function opsFromText(state: TextState): TextOp[] {
  return state.overlays
    .filter((o) => o.text !== "")
    .map((o) => {
      const params: Record<string, unknown> = {
        algo: 2,
        text: o.text,
        x: o.x,
        y: o.y,
        anchor: o.anchor,
        font: o.font,
        size: o.size,
        color: o.color,
        transparency: o.transparency,
      };
      if (o.outlineWidth > 0) {
        params.outline = { width: o.outlineWidth, color: o.outlineColor };
      }
      if (o.rotation !== 0) params.rotation = o.rotation;
      return { type: "text", params };
    });
}

/// True for the ops this tool writes: `text` with `algo: 2`. Older text ops
/// (the bitmap look) are not owned and ride along verbatim.
export function isTextOpOwned(
  type: string,
  params: Record<string, unknown>,
): boolean {
  return type === "text" && params.algo === 2;
}

function overlayFromParams(p: Record<string, unknown>): TextOverlay | null {
  if (typeof p.text !== "string" || p.text === "") return null;
  const base = defaultOverlay(p.text);
  const num = (v: unknown, d: number): number =>
    typeof v === "number" && Number.isFinite(v) ? v : d;
  const hex = (v: unknown, d: string): string =>
    typeof v === "string" && HEX.test(v) ? v : d;
  const outline =
    typeof p.outline === "object" && p.outline !== null
      ? (p.outline as Record<string, unknown>)
      : null;
  return normalized({
    text: p.text,
    x: num(p.x, base.x),
    y: num(p.y, base.y),
    anchor: typeof p.anchor === "string" ? p.anchor : "top-left",
    font: typeof p.font === "string" && p.font !== "" ? p.font : base.font,
    size: num(p.size, 0.05),
    color: hex(p.color, base.color),
    outlineWidth: outline === null ? 0 : num(outline.width, 0),
    outlineColor: outline === null ? base.outlineColor : hex(outline.color, base.outlineColor),
    transparency: num(p.transparency, 1),
    rotation: num(p.rotation, 0),
  });
}

/// Rebuild the state from the `params` of every stored algo-2 `text` op, in
/// order. Overlays the engine would reject are dropped so a damaged recipe
/// still opens.
export function textFromOps(
  paramsList: ReadonlyArray<Record<string, unknown>>,
): TextState {
  const overlays: TextOverlay[] = [];
  for (const params of paramsList) {
    const overlay = overlayFromParams(params);
    if (overlay !== null) overlays.push(overlay);
  }
  return { overlays };
}
