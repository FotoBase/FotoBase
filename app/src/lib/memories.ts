// "On this day" memories (TASK-07-008, PLAN §2 2026 additions): the IPC
// wrapper plus the pure helpers the panel shares. Read-only — the backend
// answers from a live catalog query, so nothing here writes or caches. No
// image bytes cross IPC; the panel shows `thumb://` URIs like the grid.

import type { GridItem } from "./grid";
import type { LibraryView } from "./search";

/// One year's photos, as the backend groups them (newest year first).
export interface MemoryYear {
  year: number;
  files: GridItem[];
}

/// Local calendar date as `YYYY-MM-DD` — "today" is the user's day, not UTC.
export function localToday(now: Date = new Date()): string {
  const pad = (n: number, width = 2) => String(n).padStart(width, "0");
  return `${pad(now.getFullYear(), 4)}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/// The memories for `today`, narrowed by the grid's current view so existing
/// filters apply (hidden photos are excluded backend-side).
export async function loadMemories(
  today: string,
  view?: LibraryView,
): Promise<MemoryYear[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const text = view?.query.trim() ?? "";
  return await invoke<MemoryYear[]>("memories", {
    today,
    text: text === "" ? null : text,
    folders: view?.folders.length ? view.folders : null,
    album: view?.album ?? null,
    starred: view?.starred ?? null,
    videosOnly: view?.videosOnly ?? null,
    hasFaces: view?.hasFaces ?? null,
    geotagged: view?.geotagged ?? null,
    takenSince: view?.takenSince ?? null,
  });
}

/// Every memory photo in display order (year groups concatenated) — the
/// list the viewer pages through when a memory is opened.
export function flattenMemories(years: MemoryYear[]): GridItem[] {
  return years.flatMap((y) => y.files);
}
