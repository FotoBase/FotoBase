// Folder Manager IPC wrappers (TASK-01-016, PLAN §2A). The backend owns
// persistence and watcher registration; the frontend only sends decisions.

/** Per-folder scan mode, mirroring the catalog's `folders.scan_mode`. */
export type FolderMode = "once" | "always";

/** Apply a folder's mode: "always" arms the watcher, "once" scans now. */
export async function setFolderMode(
  path: string,
  mode: FolderMode,
): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("set_folder_mode", { path, mode });
}

/** Per-folder face-detection toggle (TASK-05-010, PLAN §2 Tools menu). The
 * backend persists the decision and, on "on", triggers a background face
 * pass for the enabled folders (TASK-05-002). */
export async function setFolderFace(
  path: string,
  enabled: boolean,
): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("set_folder_face", { path, enabled });
}

/** Remove a folder from the catalog; files on disk are never touched. */
export async function removeFolder(path: string): Promise<number> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<number>("remove_folder", { path });
}
