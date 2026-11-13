// Transport wrapper for the library's one query (TASK-01-018, TASK-03-014,
// PLAN §2C). Folder selection, album view, live search text and the filter
// buttons / date slider all go to the backend's `query_files` command, which
// compiles them into a single SQL query — so every combination ANDs and a
// 50k-photo library never ships rows over IPC just to hide most of them.
// Search text is trimmed: blank means "no search" (the backend treats a
// present-but-unsearchable text as "match nothing"). No image bytes cross IPC.

import type { GridItem } from "./grid";

/// Grid order: path (default; best match / album order where those apply) or
/// chronological by capture time, adjusted times included (TASK-03-019).
export type GridSort = "path" | "capture";

export interface LibraryView {
  /// Folders-view selection; empty = the whole library.
  folders: number[];
  /// Album view; takes the place of the folder filter.
  album: number | null;
  query: string;
  starred: boolean;
  videosOnly: boolean;
  /// Only photos with a detected or named face (TASK-05-007).
  hasFaces: boolean;
  /// Only photos with a complete GPS mirror (TASK-06-007, PLAN §6 P3).
  geotagged: boolean;
  /// `YYYY-MM-DD` cutoff from the date slider, `null` = all time.
  takenSince: string | null;
  sort: GridSort;
}

export async function queryLibraryItems(
  view: LibraryView,
): Promise<GridItem[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const text = view.query.trim();
  return invoke<GridItem[]>("query_files", {
    text: text === "" ? null : text,
    folders: view.folders.length ? view.folders : null,
    album: view.album,
    starred: view.starred,
    videosOnly: view.videosOnly,
    hasFaces: view.hasFaces,
    geotagged: view.geotagged,
    takenSince: view.takenSince,
    sort: view.sort,
  });
}

/// TASK-07-003 (PLAN §6 P3): the same view through the semantic blend
/// command — FTS matches and CLIP ANN hits fused catalog-side. Resolves to
/// `null` when the semantic path is unavailable (the opt-in model is not
/// downloaded, or any backend failure): the caller must fall back to the
/// plain FTS result and never block the grid on the model.
export async function queryLibraryItemsSemantic(
  view: LibraryView,
): Promise<{ items: GridItem[]; modelMissing: boolean } | null> {
  const { invoke } = await import("@tauri-apps/api/core");
  const text = view.query.trim();
  try {
    const items = await invoke<GridItem[]>("query_files_semantic", {
      text: text === "" ? null : text,
      folders: view.folders.length ? view.folders : null,
      album: view.album,
      starred: view.starred,
      videosOnly: view.videosOnly,
      hasFaces: view.hasFaces,
      geotagged: view.geotagged,
      takenSince: view.takenSince,
      sort: view.sort,
    });
    return { items, modelMissing: false };
  } catch (error) {
    if (modelMissingError(error)) {
      // The opt-in model is not installed: FTS-only plus the enable hint.
      return { items: [], modelMissing: true };
    }
    // Any other backend failure: FTS-only, no hint, the grid never blocks.
    return null;
  }
}

/// `true` when the backend refused the semantic query because the opt-in
/// model is not installed — the "enable semantic search" hint's trigger.
export function modelMissingError(error: unknown): boolean {
  return String(error).includes("clip-text-model-missing");
}

/// `true` when the blended result contains ids the FTS-only result did not
/// have — the search bar's small "semantic" indicator's trigger.
export function semanticContributed(
  fts: GridItem[],
  blended: GridItem[],
): boolean {
  const known = new Set(fts.map((item) => item.id));
  return blended.some((item) => !known.has(item.id));
}
