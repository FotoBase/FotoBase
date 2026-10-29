// Picasa.ini migration (TASK-03-022, PLAN §1 onboarding): the scan records a
// folder's legacy Picasa.ini as an *offer*; nothing is converted until the
// user accepts. The backend owns every write (stars, albums and faces land in
// the XMP sidecars; the ini is never touched). This module is the IPC wrapper.

/// What one migration run did (mirrors the backend `PicasaReport`).
export interface PicasaReport {
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

/// Folders with a Picasa.ini whose offer is still unanswered.
export async function picasaPendingFolders(): Promise<string[]> {
  return (await call<string[] | null>("picasa_pending_folders")) ?? [];
}

export function ingestPicasaFolder(path: string): Promise<PicasaReport> {
  return call<PicasaReport>("picasa_ingest_folder", { path });
}

/// "Don't ask again" for this folder.
export async function declinePicasaFolder(path: string): Promise<void> {
  await call<void>("picasa_decline_folder", { path });
}

/// Subscribe to `picasa-offered` (folders a scan just offered); returns the
/// unsubscribe function.
export async function onPicasaOffered(cb: (folders: string[]) => void): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<string[]>("picasa-offered", (e) => cb(e.payload));
}
