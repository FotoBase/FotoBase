// Options-dialog settings (TASK-01-022, PLAN §2 General tab). Mirrors the
// Rust `settings` module: the backend owns persistence; the frontend only
// reads/writes through IPC commands. Defaults match the Rust side exactly.

export type UpdateMode = "auto" | "check-but-ask" | "off";

export interface Settings {
  language: string;
  update_mode: UpdateMode;
  show_tooltips: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  language: "en",
  update_mode: "check-but-ask",
  show_tooltips: true,
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
