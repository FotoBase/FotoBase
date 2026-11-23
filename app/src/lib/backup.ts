// Backup Manager (TASK-08-014, PLAN §2L "Backup Pictures", §6 P4):
// incremental backup sets of the catalog, sidecars and settings to an
// external location. The backend owns the set format and runs the job; this
// module is the IPC wrapper. Restore (TASK-08-015) is the reverse: the
// backend validates the whole set before it writes anything.

export type BackupSchedule = "off" | "daily" | "weekly";

export const SCHEDULES: readonly BackupSchedule[] = ["off", "daily", "weekly"];

/// Mirrors the backend `BackupConfig` (backup.json).
export interface BackupConfig {
  destination: string | null;
  schedule: BackupSchedule;
}

/// Progress event payload (counters only).
export interface BackupProgress {
  done: number;
  total: number;
  current: string;
  bytes_copied: number;
}

/// One committed set (mirrors the backend `BackupReport`).
export interface BackupReport {
  set_id: string;
  set_path: string;
  parent: string | null;
  files_total: number;
  files_copied: number;
  files_reused: number;
  bytes_copied: number;
}

/// One set as listed at a destination (mirrors `SetSummary`).
export interface BackupSet {
  id: string;
  created_at: string;
  created_unix: number;
  parent: string | null;
  files: number;
  new_files: number;
  new_bytes: number;
}

/// Outcome of a restore (mirrors the backend `RestoreReport`).
export interface RestoreReport {
  set_id: string;
  files_restored: number;
  files_unchanged: number;
  /// Sidecars whose folder no longer exists.
  skipped: string[];
}

/// Restore progress event payload (counters only).
export interface RestoreProgress {
  done: number;
  total: number;
  current: string;
}

export const BACKUP_EVENT = "backup-progress";
export const RESTORE_EVENT = "restore-progress";

async function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

export function getBackupConfig(): Promise<BackupConfig> {
  return call<BackupConfig>("get_backup_config");
}

export function setBackupConfig(config: BackupConfig): Promise<void> {
  return call<void>("set_backup_config", { config });
}

/// Write one set to `dest`: full the first time, the delta afterwards.
export function runBackup(dest: string): Promise<BackupReport> {
  return call<BackupReport>("run_backup", { dest });
}

/// Stop the running backup between files; no set is recorded.
export function cancelBackup(): Promise<void> {
  return call<void>("cancel_backup");
}

export function listBackupSets(dest: string): Promise<BackupSet[]> {
  return call<BackupSet[]>("list_backup_sets", { dest });
}

/// Replace the active catalog, sidecars and settings from a set. The caller
/// must have the user's explicit consent for `confirmOverwrite`.
export function restoreBackup(
  dest: string,
  setId: string,
  confirmOverwrite: boolean,
): Promise<RestoreReport> {
  return call<RestoreReport>("restore_backup", { dest, setId, confirmOverwrite });
}

export async function onRestoreProgress(cb: (p: RestoreProgress) => void): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<RestoreProgress>(RESTORE_EVENT, (e) => cb(e.payload));
}

export async function onBackupProgress(cb: (p: BackupProgress) => void): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<BackupProgress>(BACKUP_EVENT, (e) => cb(e.payload));
}

/// Human-readable byte count (binary units, one decimal past KiB).
export function formatBytes(n: number): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return i === 0 ? `${v} ${units[i]}` : `${v.toFixed(1)} ${units[i]}`;
}
