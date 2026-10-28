// Captions (TASK-03-007, PLAN §2B). The backend owns the write (sidecar
// `dc:description` first via the sync engine, catalog mirror + FTS second);
// this module is only the IPC wrapper.

/// Read a photo's caption from the catalog mirror (`""` when none).
export async function getCaption(fileId: number): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  return (await invoke<string | null>("get_caption", { fileId })) ?? "";
}

/// Write a photo's caption; a blank caption clears the field. Returns the
/// caption that actually landed (`""` after a clear).
export async function setCaption(
  fileId: number,
  caption: string,
): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  return (
    (await invoke<string | null>("set_caption", { fileId, caption })) ?? ""
  );
}
