// Import IPC wrappers (TASK-01-017, PLAN §2A/§6 P1). The backend owns the
// copy, the catalog writes and the job queue; the frontend only sends the
// decision and renders progress/report.

export type ImportNaming = "date" | "custom";

export interface ImportReport {
  copied: number;
  duplicates: number;
  failed: number;
  cancelled: boolean;
}

export interface ImportProgress {
  files_done: number;
  files_total: number;
  current_file: string;
}

export interface ImportRequest {
  source: string;
  destination: string;
  naming: ImportNaming;
  customPattern?: string;
  /** "Exclude duplicates" — default on (PLAN §2). */
  excludeDuplicates?: boolean;
}

/** Start an import; resolves with the report when the run finishes. */
export async function importPhotos(req: ImportRequest): Promise<ImportReport> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<ImportReport>("start_import", {
    source: req.source,
    destination: req.destination,
    naming: req.naming,
    customPattern: req.customPattern ?? null,
    excludeDuplicates: req.excludeDuplicates ?? true,
  });
}

/** Cancel the running import; a no-op when none is in flight. */
export async function cancelImport(): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("cancel_import");
}

/** Subscribe to `import-progress`; returns the unsubscribe function. */
export async function onImportProgress(
  cb: (p: ImportProgress) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<ImportProgress>("import-progress", (e) => cb(e.payload));
}
