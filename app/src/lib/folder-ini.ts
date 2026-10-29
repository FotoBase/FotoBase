// Folder ini migration (TASK-03-022, PLAN §1 onboarding): the scan records a
// folder's ini as an *offer*; nothing is converted until the
// user accepts. The backend owns every write (stars, albums and faces land in
// the XMP sidecars; the ini is never touched). This module is the IPC wrapper.

/// What one migration run did (mirrors the backend `FolderIniReport`).
export interface FolderIniReport {
  ini_files: number;
  matched: number;
  written: number;
  unchanged: number;
  unmatched: number;
  stars: number;
  faces: number;
  faces_skipped: number;
  albums_created: number;
  /// Photos that could not be migrated; the offer stays open for a retry.
  failed: string[];
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

/// Folders with an ini whose offer is still unanswered.
export async function folderIniPendingFolders(): Promise<string[]> {
  return (await call<string[] | null>("folder_ini_pending_folders")) ?? [];
}

export function ingestFolderIni(path: string): Promise<FolderIniReport> {
  return call<FolderIniReport>("ingest_folder_ini", { path });
}

/// "Don't ask again" for this folder.
export async function declineFolderIni(path: string): Promise<void> {
  await call<void>("decline_folder_ini", { path });
}

/// Subscribe to `folder-ini-offered` (folders a scan just offered); returns the
/// unsubscribe function.
export async function onFolderIniOffered(cb: (folders: string[]) => void): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<string[]>("folder-ini-offered", (e) => cb(e.payload));
}
