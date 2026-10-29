// Pure info-pane model (TASK-01-020, PLAN §2D). Formatting and histogram
// binning live here, free of Svelte/DOM, so the pane's content is directly
// unit-testable — same pattern as grid.ts / viewer.ts.

/** One photo's info payload as the backend `photo_info` command returns it:
 * file properties from the `files` row plus the EXIF fields cached in the
 * `metadata` table at scan time (null until extraction lands). */
export interface PhotoInfo {
  id: number;
  path: string;
  filename: string;
  size: number;
  mtime: number;
  width: number | null;
  height: number | null;
  media_type: string;
  camera: string | null;
  lens: string | null;
  exposure: string | null;
  f_number: number | null;
  iso: number | null;
  focal_length: number | null;
  taken_at: string | null;
  /// Sidecar-mirrored user data edited in the info pane (TASK-03-020).
  latitude: number | null;
  longitude: number | null;
  artist: string | null;
  copyright: string | null;
}

/** Fetch one photo's info via IPC. Properties + cached EXIF only — no image
 * bytes ever travel over IPC (PLAN §3 rule 3). Returns `null` when the
 * backend is missing (browser dev) or the id is unknown; the pane shows
 * placeholders instead of erroring. */
export async function loadPhotoInfo(fileId: number): Promise<PhotoInfo | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<PhotoInfo | null>("photo_info", { fileId });
  } catch {
    return null;
  }
}

/** "f/2.8 · 1/250 s · ISO 100" from the cached EXIF fields. Missing parts
 * are skipped, an all-empty payload yields `null` (the pane renders "—"). */
export function formatExposure(ex: {
  f_number?: number | null;
  iso?: number | null;
  exposure?: string | null;
}): string | null {
  const parts: string[] = [];
  if (ex.f_number != null) parts.push(`f/${ex.f_number}`);
  if (ex.exposure) parts.push(`${ex.exposure} s`);
  if (ex.iso != null) parts.push(`ISO ${ex.iso}`);
  return parts.length ? parts.join(" · ") : null;
}

/// File size in binary units (KB = 1024 B), one decimal past bytes.
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = -1;
  do {
    value /= 1024;
    unit++;
  } while (value >= 1024 && unit < units.length - 1);
  return `${value.toFixed(1)} ${units[unit]}`;
}

/// "6000 × 4000"; `null` until scan-time dimension extraction filled both.
export function formatDimensions(
  width: number | null,
  height: number | null,
): string | null {
  return width !== null && height !== null ? `${width} × ${height}` : null;
}

/// Deterministic UTC timestamp ("2025-10-09 08:13") from `files.mtime`
/// (unix seconds). UTC keeps tests and displays identical on all three OSes.
export function formatDate(unixSeconds: number): string {
  const d = new Date(unixSeconds * 1000);
  const p = (n: number): string => String(n).padStart(2, "0");
  return (
    `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())}` +
    ` ${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`
  );
}

/// RGB histogram over RGBA pixel bytes (as `getImageData` returns them):
/// `bins` buckets per channel over the 0–255 range, bin 0 = darkest. Every
/// pixel counts once — the caller downsamples by drawing the displayed
/// preview into a small canvas first (technical spec: "downsampled bins are
/// fine", computed in the frontend; no full-res trips). Alpha is never
/// counted, so transparent padding cannot skew the bins.
export function histogram(
  data: Uint8ClampedArray | Uint8Array,
  bins = 64,
): { r: number[]; g: number[]; b: number[] } {
  const r = new Array<number>(bins).fill(0);
  const g = new Array<number>(bins).fill(0);
  const b = new Array<number>(bins).fill(0);
  const scale = bins / 256;
  for (let i = 0; i + 3 < data.length; i += 4) {
    if (data[i + 3] === 0) continue; // fully transparent padding never counts
    r[Math.min(bins - 1, Math.floor(data[i] * scale))]++;
    g[Math.min(bins - 1, Math.floor(data[i + 1] * scale))]++;
    b[Math.min(bins - 1, Math.floor(data[i + 2] * scale))]++;
  }
  return { r, g, b };
}

/// File format label from the filename extension ("beach.jpg" → "JPEG").
/// Falls back to the catalog's media type when there is no extension.
export function formatFormat(filename: string, mediaType: string): string {
  const ext = filename.includes(".") ? filename.split(".").pop() : null;
  const known: Record<string, string> = {
    jpg: "JPEG",
    jpeg: "JPEG",
    png: "PNG",
    gif: "GIF",
    bmp: "BMP",
    tif: "TIFF",
    tiff: "TIFF",
    webp: "WebP",
    heic: "HEIC",
    avif: "AVIF",
    psd: "PSD",
  };
  if (ext) {
    const label = known[ext.toLowerCase()];
    if (label) return label;
  }
  return mediaType;
}
