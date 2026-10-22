// File-menu / folder-context wrappers (TASK-01-021, PLAN §2). The backend
// owns the OS work — reveal, trash, queue re-render, clean exit — and the
// confirmation dialogs live in the shell; this module only forwards the
// decision, like folder-manager.ts.

/** "Locate on disk": reveal a file or folder in the OS file manager. */
export async function revealInDisk(path: string): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("reveal_path", { path });
}

/** "Delete from disk": move the photo to the OS trash and drop its catalog
 * row. The caller confirms first — this never permanent-deletes. */
export async function deleteFromDisk(fileId: number): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("delete_to_trash", { fileId });
}

/** "Refresh thumbnails": invalidate the folder's cached thumbnails and
 * re-queue their renders in the bounded job queue (background priority). */
export async function refreshFolderThumbnails(path: string): Promise<number> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<number>("refresh_folder_thumbnails", { path });
}

/** "Exit": quit cleanly — the backend drains the job queues first. */
export async function exitApp(): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("exit_app");
}
