// Folders-view data model (TASK-01-015, PLAN §2B): real on-disk folders
// grouped under year nodes. Pure module — no Svelte — so the grouping rules
// are unit-testable.

/** One catalog folder as served by the `list_folders` command. */
export interface FolderInfo {
  id: number;
  path: string;
  /** Year of the folder's newest file, or null when nothing is dated. */
  year: number | null;
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
 * group last, folders sorted by path inside each group so the tree is
 * stable across re-fetches.
 */
export function groupFoldersByYear(folders: FolderInfo[]): YearNode[] {
  const byYear = new Map<number | null, FolderInfo[]>();
  for (const folder of folders) {
    const list = byYear.get(folder.year);
    if (list) {
      list.push(folder);
    } else {
      byYear.set(folder.year, [folder]);
    }
  }
  const byPath = (a: FolderInfo, b: FolderInfo): number =>
    a.path < b.path ? -1 : a.path > b.path ? 1 : 0;
  return [...byYear.entries()]
    .sort(([a], [b]) => {
      if (a === null) return 1; // unknown year always last
      if (b === null) return -1;
      return b - a; // newest year first
    })
    .map(([year, fs]) => ({ year, folders: fs.sort(byPath) }));
}

/** Display name of a folder: its last path segment. */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? "";
}
