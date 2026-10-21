// Options-dialog settings (TASK-01-022, PLAN §2 General tab). Mirrors the
// Rust `settings` module: the backend owns persistence; the frontend only
// reads/writes through IPC commands. Defaults match the Rust side exactly.

export type UpdateMode = "auto" | "check-but-ask" | "off";

/// Library grid renderer (TASK-01-014). Default DOM; canvas is an explicit
/// user override — no accepted ADR has chosen canvas as an OS default.
export type GridRendererSetting = "dom" | "canvas";

export interface Settings {
  language: string;
  update_mode: UpdateMode;
  show_tooltips: boolean;
  grid_renderer: GridRendererSetting;
}

export const DEFAULT_SETTINGS: Settings = {
  language: "en",
  update_mode: "check-but-ask",
  show_tooltips: true,
  grid_renderer: "dom",
};

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
