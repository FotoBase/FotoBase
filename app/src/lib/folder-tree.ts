// Folders-view data model (TASK-01-015, PLAN §2B): real on-disk folders
// grouped under year nodes. Pure module — no Svelte — so the grouping rules
// are unit-testable.

import { sortFolders, type FolderSortOptions } from "./folder-sort";

/** One catalog folder as served by the `list_folders` command. */
export interface FolderInfo {
  id: number;
  path: string;
  /** Year of the folder's newest file, or null when nothing is dated. */
  year: number | null;
  /** Folder Manager mode (TASK-01-016, PLAN §2A): watched or not. */
  scan_mode: "once" | "always";
  /// Hidden folder (or inside one), listed only while "show hidden" is on
  /// (TASK-03-009): rendered grayed.
  hidden?: boolean;
  /// Folder-sort aggregates (TASK-03-016): files directly in the folder,
  /// their summed bytes, and the newest mtime in unix seconds (null when
  /// empty). Absent values sort as empty.
  file_count?: number;
  total_size?: number;
  newest_mtime?: number | null;
}

/** A year node in the tree; `year: null` is the trailing "unknown" group. */
export interface YearNode {
  year: number | null;
  folders: FolderInfo[];
}

/** One context-menu entry; `disabled: true` marks a P2 placeholder. */
export interface MenuItem {
  id: string;
  label: string;
  disabled?: boolean;
}

/**
 * Group folders under year nodes: years newest-first, the unknown-year
 * group last, folders sorted inside each group (by name unless `sort` says
 * otherwise, TASK-03-016) so the tree is stable across re-fetches.
 */
export function groupFoldersByYear(
  folders: FolderInfo[],
  sort: FolderSortOptions = { sort: "name", reverse: false },
): YearNode[] {
  const byYear = new Map<number | null, FolderInfo[]>();
  for (const folder of folders) {
    const list = byYear.get(folder.year);
    if (list) {
      list.push(folder);
    } else {
      byYear.set(folder.year, [folder]);
    }
  }
  return [...byYear.entries()]
    .sort(([a], [b]) => {
      if (a === null) return 1; // unknown year always last
      if (b === null) return -1;
      return b - a; // newest year first
    })
    .map(([year, fs]) => ({ year, folders: sortFolders(fs, sort.sort, sort.reverse) }));
}

/** Display name of a folder: its last path segment. */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? "";
}
