// Export dialog (TASK-04-015, PLAN §2J): copy photos to a folder with
// resize limits, a quality preset, optional numbered file names and an
// optional text watermark — edits baked in by the shared export engine.
// This module is the IPC wrapper + the preset table.

export type QualityPreset =
  "auto" | "normal" | "maximum" | "minimum" | "custom";

/// §2J preset table: Auto = 100 %, Maximum = 100 %, Normal = balanced,
/// Minimum = smallest with some quality loss; Custom is the slider value.
export const PRESET_QUALITY: Record<
  Exclude<QualityPreset, "custom">,
  number
> = {
  auto: 100,
  normal: 85,
  maximum: 100,
  minimum: 50,
};

/// The watermark line the backend stamps onto the output pixels: name,
/// web domain, copyright notice — only the filled parts join the line.
export function watermarkLine(
  name: string,
  domain: string,
  copyright: string,
): string {
  return [name.trim(), domain.trim(), copyright.trim()]
    .filter((p) => p !== "")
    .join(" · ");
}

/// Dialog options as the backend `ExportRequest` expects them.
export interface ExportOptions {
  resizeMaxWidth: number | null;
  resizeMaxHeight: number | null;
  quality: number;
  numbered: boolean;
  watermark: string | null;
}

export interface ExportBatch {
  exported: { file_id: number; path: string }[];
  failed: { file_id: number; error: string }[];
}

export async function exportPhotos(
  ids: number[],
  dest: string,
  options: ExportOptions,
): Promise<ExportBatch> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<ExportBatch>("export_photos", {
    ids,
    dest,
    request: {
      resize_max_width: options.resizeMaxWidth,
      resize_max_height: options.resizeMaxHeight,
      quality: options.quality,
      numbered: options.numbered,
      watermark: options.watermark,
    },
  });
}
