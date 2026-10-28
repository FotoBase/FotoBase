// Thumbnail caption modes (TASK-03-015, PLAN §6 P2a "Thumbnail captions").
// Pure and DOM-free so both grid renderers share one resolver and one strip
// geometry. Caption text comes from data already on the grid row — switching
// modes never queries the backend or rescans.

import type { GridItem } from "./grid";
import type { Settings } from "./settings";

export type CaptionMode = "none" | "filename" | "caption" | "tags" | "resolution";

export const DEFAULT_CAPTION_MODE: CaptionMode = "none";

export const CAPTION_MODES: { value: CaptionMode; labelKey: string }[] = [
  { value: "none", labelKey: "grid.captions.none" },
  { value: "filename", labelKey: "grid.captions.filename" },
  { value: "caption", labelKey: "grid.captions.caption" },
  { value: "tags", labelKey: "grid.captions.tags" },
  { value: "resolution", labelKey: "grid.captions.resolution" },
];

export function isCaptionMode(value: unknown): value is CaptionMode {
  return CAPTION_MODES.some((m) => m.value === value);
}

/// Text printed under `item`'s thumbnail in `mode`; empty when the photo has
/// nothing for that mode.
export function captionText(item: GridItem, mode: CaptionMode): string {
  switch (mode) {
    case "none":
      return "";
    case "filename":
      return item.filename;
    case "caption":
      // A caption can be multi-line; the strip is one line tall.
      return (item.caption ?? "").trim().split("\n")[0].trim();
    case "tags":
      // Compact: hierarchical tags (`Vacation|Italy`) show their leaf only.
      return (item.tags ?? []).map((tag) => tag.split("|").pop() ?? tag).join(", ");
    case "resolution":
      return item.width != null && item.height != null
        ? `${item.width} × ${item.height}`
        : "";
  }
}

/// Height in px of the caption strip under each cell. Depends only on whether
/// captions are on and the zoom level — never on the mode or the text — so
/// switching modes cannot shift the layout.
export function captionStripHeight(mode: CaptionMode, thumbSize: number): number {
  if (mode === "none") return 0;
  if (thumbSize < 96) return 14;
  if (thumbSize < 256) return 18;
  return 22;
}

/// Canvas truncation: the longest prefix of `text` that, with a trailing
/// ellipsis, fits `maxWidth` under `measure`. Empty when nothing fits.
export function ellipsize(
  text: string,
  maxWidth: number,
  measure: (s: string) => number,
): string {
  if (text === "" || measure(text) <= maxWidth) return text;
  let lo = 0;
  let hi = text.length;
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    if (measure(`${text.slice(0, mid)}…`) <= maxWidth) lo = mid;
    else hi = mid - 1;
  }
  const out = `${text.slice(0, lo)}…`;
  return measure(out) <= maxWidth ? out : "";
}

/// The mode to show for a loaded `settings` value: an unknown persisted
/// string (hand-edited file, newer build) degrades to the default, never
/// crashes the grid.
export function captionModeFromSettings(settings: Pick<Settings, "caption_mode">): CaptionMode {
  return isCaptionMode(settings.caption_mode) ? settings.caption_mode : DEFAULT_CAPTION_MODE;
}

/// `settings` carrying `mode`, or `null` when it already does — the shell
/// saves only on a real change.
export function settingsWithCaptionMode(settings: Settings, mode: CaptionMode): Settings | null {
  return settings.caption_mode === mode ? null : { ...settings, caption_mode: mode };
}
