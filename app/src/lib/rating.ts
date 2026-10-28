// Star / 1-5 rating (TASK-03-005, ADR-0002, PLAN §5). Pure decisions live
// here; the backend owns the write (sidecar first, catalog second), so the
// UI only reflects a rating after `set_rating` resolved.

/// One star = `xmp:Rating` 5.
export const STAR = 5;

/// Star toggle: any rating (a 3 written by another tool included) clears,
/// unrated becomes a star.
export function toggleStar(rating: number): number {
  return rating > 0 ? 0 : STAR;
}

/// Digit-key rating, only while ratings are enabled; `null` = not a rating key.
export function ratingForKey(key: string, ratingsEnabled: boolean): number | null {
  if (!ratingsEnabled || !/^[0-5]$/.test(key)) return null;
  return Number(key);
}

/// Write one photo's rating (0 unstars). Rejects with the backend's message
/// when the sidecar write failed — nothing changed in that case.
export async function setRating(fileId: number, rating: number): Promise<number> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<number>("set_rating", { fileId, rating });
}
