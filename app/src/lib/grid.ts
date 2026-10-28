// Pure grid math for the virtualized thumbnail grid (TASK-01-013, PLAN §4).
// Kept free of Svelte/DOM so it is directly unit-testable.

/// One photo as the grid consumes it. `hash` is the render crate's cache key
/// (first 16 bytes of SHA-256, lowercase hex) backing the `thumb://` URI;
/// `null` while metadata extraction has not assigned it yet.
export interface GridItem {
  id: number;
  filename: string;
  hash: string | null;
  /// `image` or `video` (TASK-02-007, from the catalog's `files.media_type`).
  /// Absent in older payloads and browser-dev stubs — treated as image.
  media_type?: "image" | "video";
  /// `xmp:Rating` mirror from the catalog, 0 = unrated (TASK-03-005).
  rating?: number;
}

/// Grid gap in px, both between cells and between rows.
export const GRID_GAP = 8;

/// Full columns that fit `containerWidth` px at `thumbSize` px cells with
/// `gap` px gutters: at least one, only whole columns (no clipped cells).
export function columnCount(
  containerWidth: number,
  thumbSize: number,
  gap: number = GRID_GAP,
): number {
  return Math.max(1, Math.floor((containerWidth + gap) / (thumbSize + gap)));
}

/// A well-formed render cache key: exactly 32 lowercase hex chars. The shape
/// check doubles as the path-traversal guard — nothing else can name a cache
/// entry (mirrors the `thumb://` handler's validation, TASK-01-011).
export function isThumbHash(s: string): boolean {
  return s.length === 32 && /^[0-9a-f]+$/.test(s);
}

/// `thumb://` URI for a cached thumbnail. Images never travel over IPC
/// (PLAN §3 rule 3): the WebView fetches bytes from the disk cache through
/// the custom scheme handler. Returns `null` for files without a hash yet —
/// the grid shows the placeholder for those instead.
///
/// `version` (from the shared grid store, bumped on each `thumb-rendered`
/// event) appends `?v=N`: after a cache miss served the placeholder and the
/// queued render landed, the changed URI forces the WebView to re-request
/// — the miss placeholder would otherwise never be replaced.
export function thumbUri(hash: string | null, version = 0): string | null {
  if (hash === null || !isThumbHash(hash)) return null;
  return version > 0 ? `thumb://${hash}?v=${version}` : `thumb://${hash}`;
}

/// HiDPI srcset (TASK-01-024): on a display with devicePixelRatio > 1 the
/// browser is offered a 2x variant (`?s=2` → the backend's 1024px cache
/// entry) so cells up to ZOOM_MAX stay sharp on Retina/4K. `null` on 1x
/// displays — the single `src` is already sharp there, and no second render
/// should ever be queued for it.
export function thumbSrcset(
  hash: string | null,
  devicePixelRatio: number,
  version = 0,
): string | null {
  if (hash === null || !isThumbHash(hash)) return null;
  if (devicePixelRatio <= 1) return null;
  const v = version > 0 ? `?v=${version}&` : "?";
  return `thumb://${hash}${v}s=2 2x`;
}

/// Keyboard grid navigation (TASK-01-024): the neighbor of cell `index` in
/// a `columns`-wide grid of `count` items for an arrow key, or `null` when
/// the move would leave the grid (no wrapping — the edge is the edge).
export function nextCell(
  index: number,
  columns: number,
  count: number,
  key: "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown",
): number | null {
  const target =
    key === "ArrowLeft"
      ? index - 1
      : key === "ArrowRight"
        ? index + 1
        : key === "ArrowUp"
          ? index - columns
          : index + columns;
  if (target < 0 || target >= count) return null;
  return target;
}

/// The items of one grid row.
export function rowItems<T>(items: T[], columns: number, row: number): T[] {
  return items.slice(row * columns, row * columns + columns);
}

/// Unique content hashes currently on screen, in row order. These feed the
/// job queue's visible-first priority (PLAN §3 rule 4) via the backend.
export function visibleHashes(
  items: GridItem[],
  columns: number,
  rowIndexes: number[],
): string[] {
  const hashes: string[] = [];
  const seen = new Set<string>();
  for (const row of rowIndexes) {
    for (const item of rowItems(items, columns, row)) {
      if (item.hash !== null && !seen.has(item.hash)) {
        seen.add(item.hash);
        hashes.push(item.hash);
      }
    }
  }
  return hashes;
}

/// Trailing-edge debounce: collapses a burst into the last call. Used for
/// visible-range reporting so fast scrolling does not spam IPC per frame.
export function debounce<A extends unknown[]>(
  fn: (...args: A) => void,
  ms: number,
): (...args: A) => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return (...args: A) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), ms);
  };
}

/// Leading-edge throttle with a trailing catch-up: fires immediately, then at
/// most once per `ms` window — and if calls kept arriving during the window,
/// one last call with the latest args fires when it closes. Unlike
/// `debounce`, a slow drip of events is never starved: a burst that renews
/// itself faster than the window still delivers its FIRST call, not only the
/// last one. Used for `library-changed` re-lists (TASK-01-004 perf run: the
/// hash backfill emits per batch, faster than any debounce window — the
/// pure-trailing debounce postponed the grid refresh until the entire 50k
/// backfill ended, 33 s to first thumbnail).
export function throttle<A extends unknown[]>(
  fn: (...args: A) => void,
  ms: number,
): (...args: A) => void {
  let last = -Infinity;
  let timer: ReturnType<typeof setTimeout> | undefined;
  return (...args: A) => {
    const now = Date.now();
    const remaining = ms - (now - last);
    clearTimeout(timer);
    if (remaining <= 0) {
      last = now;
      fn(...args);
      return;
    }
    // Window still open: remember the latest args for the trailing catch-up.
    timer = setTimeout(() => {
      last = Date.now();
      fn(...args);
    }, remaining);
  };
}

/// Canvas hit-testing (TASK-01-014): which item lives under a click at
/// viewport-relative `offsetX`/`offsetY` when the grid is scrolled to
/// `scrollTop`. Pure math so the canvas grid keeps its pointer handling
/// unit-testable. Returns `null` for gutter/edge clicks and past-the-end
/// positions — never a negative or out-of-range index.
export function cellAt(
  offsetX: number,
  offsetY: number,
  scrollTop: number,
  thumbSize: number,
  columns: number,
  itemCount: number,
): number | null {
  const step = thumbSize + GRID_GAP;
  const col = Math.floor(offsetX / step);
  if (col < 0 || col >= columns) return null;
  const row = Math.floor((offsetY + scrollTop) / step);
  if (row < 0) return null;
  const index = row * columns + col;
  if (index >= itemCount) return null;
  return index;
}
