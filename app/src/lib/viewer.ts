// Pure viewer logic for the single-photo viewer (TASK-01-019, PLAN §2D).
// Kept free of Svelte/DOM so navigation, preloading, and URI rules are
// directly unit-testable — same pattern as grid.ts.

import { isThumbHash } from "./grid";

/// Advance one photo, wrapping from the last to the first. An empty list
/// degenerates to index 0 — navigation on nothing must never panic.
export function nextIndex(index: number, count: number): number {
  return count === 0 ? 0 : (index + 1) % count;
}

/// Go back one photo, wrapping from the first to the last.
export function prevIndex(index: number, count: number): number {
  return count === 0 ? 0 : (index - 1 + count) % count;
}

/// `preview://` URI for the full-size image. Same content-hash key and the
/// same validation as `thumb://` — the shape check doubles as the
/// path-traversal guard, and images never travel over IPC (PLAN §3 rule 3):
/// the WebView fetches the original bytes from disk through the scheme
/// handler. Returns `null` for files without a hash yet.
export function previewUri(hash: string | null): string | null {
  if (hash === null || !isThumbHash(hash)) return null;
  return `preview://${hash}`;
}

/// URIs to preload around the current photo so navigation feels instant
/// (TASK-01-019 functional spec): the previous and next neighbors, wrapped
/// at the ends, deduped, and only for files that actually have a hash.
export function preloadUris(uris: (string | null)[], index: number): string[] {
  const count = uris.length;
  if (count < 2) return [];
  const out: string[] = [];
  for (const i of [prevIndex(index, count), nextIndex(index, count)]) {
    const uri = uris[i];
    if (uri !== null && !out.includes(uri)) out.push(uri);
  }
  return out;
}
