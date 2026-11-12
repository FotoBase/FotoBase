// Smart search model management (TASK-07-001, PLAN §1 "models are opt-in
// downloads", §6 P3 Semantic search): the IPC wrapper plus the pure
// consent rules the Options dialog shares. Nothing downloads without the
// user's explicit consent — the wrapper refuses to call the backend
// download command unless the consent flag is set, so a stray call can
// never bypass the prompt.

/// The backend's model status (TASK-07-001): installed state, pinned size
/// and the weights' license, shown next to the download button.
export interface ClipModelStatus {
  installed: boolean;
  size_bytes: number;
  license: string;
}

async function call<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

/// Current model status; a safe default (not installed, 0 bytes) when the
/// backend is absent (browser dev).
export async function clipModelStatus(): Promise<ClipModelStatus> {
  try {
    return await call<ClipModelStatus>("clip_model_status");
  } catch {
    return { installed: false, size_bytes: 0, license: "" };
  }
}

/// The human-readable download size ("352 MB" fits one line).
export function modelSizeLabel(sizeBytes: number): string {
  if (sizeBytes <= 0) return "";
  const mib = sizeBytes / (1024 * 1024);
  return mib >= 1024 ? `${(mib / 1024).toFixed(1)} GB` : `${Math.round(mib)} MB`;
}

/// Download may only fire after explicit consent (PLAN §1): `consented`
/// is the user's answer to the opt-in prompt in this session.
export function mayDownload(status: ClipModelStatus, consented: boolean): boolean {
  return consented && !status.installed;
}

/// The consented opt-in download. Throws when called without consent —
/// the boundary that keeps the download opt-in.
export async function downloadClipModel(consented: boolean): Promise<void> {
  if (!consented) {
    throw new Error("model download requires explicit consent");
  }
  await call("download_clip_model");
}
