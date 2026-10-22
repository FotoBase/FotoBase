// Transport wrapper for the library's item queries (TASK-01-018, PLAN §2C).
// The grid's items come from `list_files` (plain browse) or `search_files`
// (live FTS search over filenames + folders); which command runs is decided
// by the query here, not by the caller. Search terms are trimmed — the
// backend treats whitespace-only queries as "match nothing", the plain list
// is what an empty search box means. No image bytes ever cross IPC.

import type { GridItem } from "./grid";

export async function listLibraryItems(
  folders: number[],
  query: string,
): Promise<GridItem[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const q = query.trim();
  if (q !== "") {
    return invoke<GridItem[]>("search_files", {
      query: q,
      folders: folders.length ? folders : null,
    });
  }
  return folders.length
    ? invoke<GridItem[]>("list_files", { folders })
    : invoke<GridItem[]>("list_files");
}
