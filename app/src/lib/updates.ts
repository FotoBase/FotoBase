// Update-mode behavior (TASK-01-022): auto installs, check-but-ask prompts,
// off never phones home. The backend command enforces the "off" gate too
// (defense in depth); this module only exists so the UI can drive the flow
// and show the outcome.

import type { UpdateMode } from "./settings";

export interface UpdateCheck {
  checked: boolean;
  available: boolean;
  version: string | null;
  notes: string | null;
}

async function invokeChecked<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

/**
 * Run the update flow for `mode`. Returns `null` when nothing was checked
 * (mode off or no backend), otherwise the check result; auto mode installs
 * without asking, check-but-ask prompts first.
 */
export async function runUpdateFlow(mode: UpdateMode): Promise<UpdateCheck | null> {
  if (mode === "off") return null; // never phones home (PLAN §1)
  try {
    const check = await invokeChecked<UpdateCheck>("check_for_updates");
    if (check.available && mode === "auto") {
      await invokeChecked("install_pending_update");
      return check;
    }
    if (check.available && mode === "check-but-ask") {
      const version = check.version ?? "unknown";
      if (confirm(`Fotobase ${version} is available. Install it now?`)) {
        await invokeChecked("install_pending_update");
      }
    }
    return check;
  } catch {
    // No backend (browser dev) or feed unreachable: not an error dialog.
    return null;
  }
}
