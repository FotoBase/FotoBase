// Options-dialog settings (TASK-01-022, PLAN §2 General tab). Mirrors the
// Rust `settings` module: the backend owns persistence; the frontend only
// reads/writes through IPC commands. Defaults match the Rust side exactly.

export type UpdateMode = "auto" | "check-but-ask" | "off";

/// Library grid renderer (TASK-01-014). Default DOM; canvas is an explicit
/// user override — no accepted ADR has chosen canvas as an OS default.
export type GridRendererSetting = "dom" | "canvas";

/// UI theme (TASK-01-024): follow the OS, or force light/dark. Mirrors the
/// Rust `Theme` enum (kebab-case serialization).
export type ThemeSetting = "system" | "light" | "dark";

export interface Settings {
  language: string;
  update_mode: UpdateMode;
  show_tooltips: boolean;
  grid_renderer: GridRendererSetting;
  theme: ThemeSetting;
  file_associations: string[];
}

export const DEFAULT_SETTINGS: Settings = {
  language: "en",
  update_mode: "check-but-ask",
  show_tooltips: true,
  grid_renderer: "dom",
  theme: "system",
  file_associations: [],
};

// Image types the file-association option (TASK-01-025, PLAN §2M) can
// register Fotobase as the default viewer for. Mirrors the Rust
// `associations::IMAGE_EXTS` (the scanner's cataloged image types).
export const IMAGE_EXTENSIONS: string[] = [
  "jpg", "jpeg", "png", "gif", "tif", "tiff", "webp", "heic", "heif",
  "avif", "bmp", "dng", "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2",
];

export const THEME_CHOICES: { value: ThemeSetting; labelKey: string }[] = [
  { value: "system", labelKey: "options.theme.system" },
  { value: "light", labelKey: "options.theme.light" },
  { value: "dark", labelKey: "options.theme.dark" },
];

export const UPDATE_MODES: { value: UpdateMode; labelKey: string }[] = [
  { value: "auto", labelKey: "options.updates.auto" },
  { value: "check-but-ask", labelKey: "options.updates.checkButAsk" },
  { value: "off", labelKey: "options.updates.off" },
];

export async function loadSettings(): Promise<Settings> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<Settings>("get_settings");
  } catch {
    // Browser dev (no backend): defaults, nothing persists.
    return { ...DEFAULT_SETTINGS };
  }
}

export async function saveSettings(settings: Settings): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("set_settings", { new: settings });
}

export async function clearCache(): Promise<number> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<number>("clear_cache");
}

// Rebuild catalog (TASK-01-023, PLAN §5): explicit full re-scan of every
// watched folder. Progress streams on the `rebuild-progress` event while
// the command runs in the background job queue; the invoke promise resolves
// with the final report.
export interface RebuildProgress {
  folders_done: number;
  folders_total: number;
  files_done: number;
  current_folder: string;
}

export interface RebuildReport {
  added: number;
  updated: number;
  skipped: number;
  removed: number;
  cancelled: boolean;
}

export async function rebuildCatalog(): Promise<RebuildReport> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<RebuildReport>("rebuild_catalog");
}

/** Subscribe to rebuild progress; returns the unlisten function. */
export async function onRebuildProgress(
  cb: (p: RebuildProgress) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<RebuildProgress>("rebuild-progress", (e) => cb(e.payload));
}

// File associations (TASK-01-025, PLAN §2M): register/unregister Fotobase
// as the OS default viewer for image types. The OS writes happen in the
// backend; the persisted settings list is the dialog's source of truth.
export async function listAssociations(): Promise<{ ext: string; registered: boolean }[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<{ ext: string; registered: boolean }[]>("list_associations");
}

export async function registerAssociations(exts: string[]): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("register_associations", { exts });
}

export async function unregisterAssociations(exts: string[]): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("unregister_associations", { exts });
}

// OS-opened files (TASK-01-025): the backend parks the paths the OS passed
// at launch; the shell drains them once and routes them into the viewer.
export async function takeOpenFiles(): Promise<string[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<string[]>("take_open_files");
}

export interface FilePathMatch {
  id: number;
  filename: string;
  hash: string | null;
}

export async function findFileByPath(path: string): Promise<FilePathMatch | null> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<FilePathMatch | null>("find_file_by_path", { path });
}
