// Folder sort + flat/tree view mode (TASK-03-016, PLAN §6 P2a). Pure and
// DOM-free: sorting runs over the folder list already loaded from
// `list_folders` — no extra query, no catalog change.

import type { FolderInfo } from "./folder-tree";
import type { Settings } from "./settings";

export type FolderSortKey = "name" | "date" | "size";
export type FolderViewMode = "tree" | "flat";

export interface FolderSortOptions {
  sort: FolderSortKey;
  reverse: boolean;
}

export interface FolderViewOptions extends FolderSortOptions {
  view: FolderViewMode;
}

export const DEFAULT_FOLDER_VIEW: FolderViewOptions = {
  view: "tree",
  sort: "name",
  reverse: false,
};

export const FOLDER_SORTS: { value: FolderSortKey; labelKey: string }[] = [
  { value: "name", labelKey: "tree.sort.name" },
  { value: "date", labelKey: "tree.sort.date" },
  { value: "size", labelKey: "tree.sort.size" },
];

export const FOLDER_VIEWS: { value: FolderViewMode; labelKey: string }[] = [
  { value: "tree", labelKey: "tree.view.tree" },
  { value: "flat", labelKey: "tree.view.flat" },
];

export function isFolderSortKey(value: unknown): value is FolderSortKey {
  return FOLDER_SORTS.some((s) => s.value === value);
}

export function isFolderViewMode(value: unknown): value is FolderViewMode {
  return FOLDER_VIEWS.some((v) => v.value === value);
}

/// Last path segment, lowercased: the sort key for "name" and the same
/// display name the tree shows.
function nameKey(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return (parts.at(-1) ?? "").toLowerCase();
}

function cmp<T extends number | string>(a: T, b: T): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/// Natural order per key: name A→Z, date newest first (empty folders last),
/// size largest first. Ties fall back to path so the order is deterministic
/// across re-fetches.
function compare(key: FolderSortKey): (a: FolderInfo, b: FolderInfo) => number {
  const byPath = (a: FolderInfo, b: FolderInfo): number => cmp(a.path, b.path);
  switch (key) {
    case "name":
      return (a, b) => cmp(nameKey(a.path), nameKey(b.path)) || byPath(a, b);
    case "date":
      return (a, b) =>
        cmp(b.newest_mtime ?? Number.NEGATIVE_INFINITY, a.newest_mtime ?? Number.NEGATIVE_INFINITY) ||
        byPath(a, b);
    case "size":
      return (a, b) => cmp(b.total_size ?? 0, a.total_size ?? 0) || byPath(a, b);
  }
}

/// A new array of `folders` sorted by `key`; `reverse` flips the whole order.
export function sortFolders(
  folders: readonly FolderInfo[],
  key: FolderSortKey,
  reverse: boolean,
): FolderInfo[] {
  const sorted = [...folders].sort(compare(key));
  return reverse ? sorted.reverse() : sorted;
}

/// The view options to show for a loaded `settings` value: unknown persisted
/// strings (hand-edited file, newer build) degrade to the P1 defaults.
export function folderViewFromSettings(
  settings: Pick<Settings, "folder_view" | "folder_sort" | "folder_sort_reverse">,
): FolderViewOptions {
  return {
    view: isFolderViewMode(settings.folder_view) ? settings.folder_view : DEFAULT_FOLDER_VIEW.view,
    sort: isFolderSortKey(settings.folder_sort) ? settings.folder_sort : DEFAULT_FOLDER_VIEW.sort,
    reverse: settings.folder_sort_reverse === true,
  };
}

/// `settings` carrying `options`, or `null` when it already does — the shell
/// saves only on a real change.
export function settingsWithFolderView(
  settings: Settings,
  options: FolderViewOptions,
): Settings | null {
  if (
    settings.folder_view === options.view &&
    settings.folder_sort === options.sort &&
    settings.folder_sort_reverse === options.reverse
  ) {
    return null;
  }
  return {
    ...settings,
    folder_view: options.view,
    folder_sort: options.sort,
    folder_sort_reverse: options.reverse,
  };
}
