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
  /// Live-search query (TASK-01-018, PLAN §2C). Lives in the shared store so
  /// the DOM and canvas grids filter for free: the shell fetches the
  /// matching items once and hands them to whichever renderer is mounted.
  searchQuery = $state("");
  /// Per-hash cache-bust versions (PLAN §3 rule 3). A cold-start miss serves
  /// the placeholder; when the queued render lands, the backend emits
  /// `thumb-rendered` and the shell bumps the hash's version here — every
  /// consumer re-requests `thumb://<hash>?v=N`, so the placeholder is
  /// replaced without a refetch storm (only hashes that rendered bump).
  thumbVersions = $state<Record<string, number>>({});
  /// Pending "show me this photo" request (TASK-03-010: a tray click). The
  /// mounted grid scrolls to the photo, selects it and clears the request —
  /// it stays pending until the photo is among the shown items, so the shell
  /// can widen the filters first and let the list reload.
  revealRequest = $state<{ id: number } | null>(null);

  toggleSelection(id: number): void {
    const next = new Set(this.selection);
    if (!next.delete(id)) next.add(id);
    this.selection = next;
  }

  /// Record that a thumbnail's render landed; the next URI for this hash
  /// carries the new version. Idempotent per event, monotonic per hash.
  bumpThumbVersion(hash: string): void {
    if (!hash) return;
    this.thumbVersions = {
      ...this.thumbVersions,
      [hash]: (this.thumbVersions[hash] ?? 0) + 1,
    };
  }

  /// Display profile changed (TASK-02-001): the backend now renders into the
  /// new profile's cache entries, so every hash already on screen must
  /// re-request with a bumped version — otherwise stale pixels for the old
  /// profile would stay until reload. Hashes that never rendered keep no
  /// entry and simply trigger the (new-profile) render on first request.
  bumpAllThumbVersions(): void {
    const next: Record<string, number> = {};
    for (const [hash, v] of Object.entries(this.thumbVersions)) {
      next[hash] = v + 1;
    }
    this.thumbVersions = next;
  }
}

export const gridState = new GridState();

/// Test hook: restore the store's defaults (the store itself is global and
/// must survive renderer switches — only tests reset it).
export function resetGridState(): void {
  gridState.thumbSize = DEFAULT_THUMB_SIZE;
  gridState.selection = new Set();
  gridState.scrollTop = 0;
  gridState.searchQuery = "";
  gridState.thumbVersions = {};
  gridState.revealRequest = null;
}

/// Which renderer to mount for a persisted setting. Canvas only when
/// explicitly configured: no accepted ADR has chosen canvas as an OS default
/// yet (the task's "Linux default per the 01-004 ADR" clause has no such ADR
/// to lean on), so DOM stays the default everywhere and canvas is available
/// as an override on every OS. Unknown values degrade to DOM, never crash.
export function pickRenderer(setting: GridRenderer): GridRenderer {
  return setting === "canvas" ? "canvas" : "dom";
}
