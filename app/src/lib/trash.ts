// Catalog trash and hide (TASK-03-012, TASK-03-009; PLAN §2 A, §6 P2a). The
// backend owns every mutation: remove-from-album, hide and unlink are
// soft-deleted into a trash table with a restore payload, and undo replays
// the reverse operation. Nothing here touches disk. This module is the IPC
// wrapper plus the pure bits the trash view needs.

export type TrashAction = "album_remove" | "hide" | "unlink";

/// One pending, undoable action as served by `list_trash`.
export interface TrashEntry {
  id: number;
  action: TrashAction;
  /// Photo path, or folder path for a folder hide / unlink.
  path: string;
  /// File name, or the folder's last path component.
  name: string;
  /// Album name for `album_remove`; `"folder"` for a folder hide.
  detail: string | null;
  /// Unix seconds.
  created_at: number;
}

export interface UndoReport {
  restored: number;
  /// Entries that could not be reversed any more (album deleted, photo gone).
  skipped: number;
  /// Folder roots the backend rescans after an undone unlink.
  rescan: string[];
}

/// Display order of the action groups in the trash view.
export const TRASH_ACTIONS: TrashAction[] = ["album_remove", "hide", "unlink"];

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

export async function listTrash(): Promise<TrashEntry[]> {
  return (await call<TrashEntry[] | null>("list_trash")) ?? [];
}

export function undoTrash(id: number): Promise<UndoReport> {
  return call<UndoReport>("undo_trash", { id });
}

/// Undo every pending entry of one action type.
export function undoTrashAll(action: TrashAction): Promise<UndoReport> {
  return call<UndoReport>("undo_trash_all", { action });
}

/// Hide photos (`fb:Hidden`, library-only); returns how many were hidden.
export function hidePhotos(fileIds: number[]): Promise<number> {
  return call<number>("hide_photos", { fileIds });
}

export function unhidePhotos(fileIds: number[]): Promise<number> {
  return call<number>("unhide_photos", { fileIds });
}

/// Hide a folder (its `.fotobase` flag); photos and subfolders inherit.
export function hideFolder(path: string): Promise<boolean> {
  return call<boolean>("hide_folder", { path });
}

export function unhideFolder(path: string): Promise<boolean> {
  return call<boolean>("unhide_folder", { path });
}

/// Entries grouped by action in display order; empty groups are left out.
export function groupTrash(
  entries: readonly TrashEntry[],
): { action: TrashAction; entries: TrashEntry[] }[] {
  return TRASH_ACTIONS.map((action) => ({
    action,
    entries: entries.filter((e) => e.action === action),
  })).filter((g) => g.entries.length > 0);
}

/// i18n key naming what the entry did; a folder hide reads differently from
/// a photo hide.
export function trashActionKey(entry: Pick<TrashEntry, "action" | "detail">): string {
  return entry.action === "hide" && entry.detail === "folder"
    ? "trash.action.hideFolder"
    : `trash.action.${entry.action}`;
}

/// Local, human-readable time of an entry.
export function formatTrashTime(createdAt: number): string {
  return new Date(createdAt * 1000).toLocaleString();
}
