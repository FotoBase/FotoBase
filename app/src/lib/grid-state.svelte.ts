// Shared grid state (TASK-01-014, PLAN §4/§6). The DOM grid (01-013) and the
// canvas grid draw through different renderers but must stay in sync
// feature-wise: one module-level store carries zoom, selection, and scroll
// position, so switching renderers (config-driven, user-overridable) loses
// none of it. Svelte 5 runes in a `.svelte.ts` module keep this reactive.

export type GridRenderer = "dom" | "canvas";

/// Zoom range: 64 = wall of small tiles, 512 = near-preview. Shared by both
/// sliders so the two renderers offer identical zoom behavior.
export const DEFAULT_THUMB_SIZE = 160;
export const ZOOM_MIN = 64;
export const ZOOM_MAX = 512;
export const ZOOM_STEP = 8;

class GridState {
  /// Thumbnail edge in px; the zoom sliders drive it live.
  thumbSize = $state(DEFAULT_THUMB_SIZE);
  /// Selected photo ids. Both renderers read the same set.
  selection = $state(new Set<number>());
  /// Last scroll offset, so a renderer switch resumes where the user was.
  scrollTop = $state(0);

  toggleSelection(id: number): void {
    const next = new Set(this.selection);
    if (!next.delete(id)) next.add(id);
    this.selection = next;
  }
}

export const gridState = new GridState();

/// Test hook: restore the store's defaults (the store itself is global and
/// must survive renderer switches — only tests reset it).
export function resetGridState(): void {
  gridState.thumbSize = DEFAULT_THUMB_SIZE;
  gridState.selection = new Set();
  gridState.scrollTop = 0;
}

/// Which renderer to mount for a persisted setting. Canvas only when
/// explicitly configured: no accepted ADR has chosen canvas as an OS default
/// yet (the task's "Linux default per the 01-004 ADR" clause has no such ADR
/// to lean on), so DOM stays the default everywhere and canvas is available
/// as an override on every OS. Unknown values degrade to DOM, never crash.
export function pickRenderer(setting: GridRenderer): GridRenderer {
  return setting === "canvas" ? "canvas" : "dom";
}
