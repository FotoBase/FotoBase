// Transport wrapper for the library's one query (TASK-01-018, TASK-03-014,
// PLAN §2C). Folder selection, album view, live search text and the filter
// buttons / date slider all go to the backend's `query_files` command, which
// compiles them into a single SQL query — so every combination ANDs and a
// 50k-photo library never ships rows over IPC just to hide most of them.
// Search text is trimmed: blank means "no search" (the backend treats a
// present-but-unsearchable text as "match nothing"). No image bytes cross IPC.

import type { GridItem } from "./grid";

export interface LibraryView {
  /// Folders-view selection; empty = the whole library.
  folders: number[];
  /// Album view; takes the place of the folder filter.
  album: number | null;
  query: string;
  starred: boolean;
  videosOnly: boolean;
  /// `YYYY-MM-DD` cutoff from the date slider, `null` = all time.
  takenSince: string | null;
}

export async function queryLibraryItems(view: LibraryView): Promise<GridItem[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const text = view.query.trim();
  return invoke<GridItem[]>("query_files", {
    text: text === "" ? null : text,
    folders: view.folders.length ? view.folders : null,
    album: view.album,
    starred: view.starred,
    videosOnly: view.videosOnly,
    takenSince: view.takenSince,
  });
}
