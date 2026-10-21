// Pure grid math for the virtualized thumbnail grid (TASK-01-013, PLAN §4).
// Kept free of Svelte/DOM so it is directly unit-testable.

/// One photo as the grid consumes it. `hash` is the render crate's cache key
/// (first 16 bytes of SHA-256, lowercase hex) backing the `thumb://` URI;
/// `null` while metadata extraction has not assigned it yet.
export interface GridItem {
  id: number;
  filename: string;
  hash: string | null;
}

/// Grid gap in px, both between cells and between rows.
export const GRID_GAP = 8;

/// Full columns that fit `containerWidth` px at `thumbSize` px cells with
/// `gap` px gutters: at least one, only whole columns (no clipped cells).
export function columnCount(containerWidth: number, thumbSize: number, gap: number = GRID_GAP): number {
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
export function thumbUri(hash: string | null): string | null {
  if (hash === null || !isThumbHash(hash)) return null;
  return `thumb://${hash}`;
}

/// The items of one grid row.
export function rowItems<T>(items: T[], columns: number, row: number): T[] {
  return items.slice(row * columns, row * columns + columns);
}

/// Unique content hashes currently on screen, in row order. These feed the
/// job queue's visible-first priority (PLAN §3 rule 4) via the backend.
export function visibleHashes(items: GridItem[], columns: number, rowIndexes: number[]): string[] {
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
