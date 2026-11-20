// Print dialog (TASK-08-004, PLAN §2J Print): option model, IPC wrappers and
// the preview geometry. The page plan comes from the backend layout engine
// (cells, fit rectangles, text bands, all in inches); this module only turns
// it into CSS, so the preview cannot disagree with the printed page. Edits
// show because the preview images are the edit-view renders. TASK-08-005
// adds the sinks: an installed printer or a PDF, both fed by the same plan.

export type Paper = "letter" | "a4";
export type PrintLayout =
  | "wallet"
  | "3.5x5"
  | "4x6"
  | "5x7"
  | "8x10"
  | "full_page"
  | "contact_sheet";
export type FitMode = "shrink" | "crop";
export type TextSource = "none" | "caption" | "filename" | "exif";
export type TextPosition = "above" | "below";

export const LAYOUTS: PrintLayout[] = [
  "wallet",
  "3.5x5",
  "4x6",
  "5x7",
  "8x10",
  "full_page",
  "contact_sheet",
];

export interface PrintOptions {
  paper: Paper;
  layout: PrintLayout;
  images_per_page: number;
  fit: FitMode;
  border: { width_in: number; color: string };
  text: {
    source: TextSource;
    font: string;
    color: string;
    size_pt: number;
    position: TextPosition;
    wrap: boolean;
  };
  auto_rotate: boolean;
}

export const DEFAULT_OPTIONS: PrintOptions = {
  paper: "letter",
  layout: "4x6",
  images_per_page: 20,
  fit: "shrink",
  border: { width_in: 0, color: "#000000" },
  text: {
    source: "none",
    font: "sans",
    color: "#000000",
    size_pt: 10,
    position: "below",
    wrap: true,
  },
  auto_rotate: true,
};

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface CellPlan {
  file_id: number;
  cell: Rect;
  image_box: Rect;
  fit: { src: Rect; dst: Rect };
  rotated: boolean;
  border: Rect | null;
  text: { rect: Rect; text: string } | null;
}

export interface PagePlan {
  width: number;
  height: number;
  cells: CellPlan[];
}

export interface PlanPhoto {
  file_id: number;
  filename: string;
  recipe: string;
}

export interface PrintPlan {
  pages: PagePlan[];
  photos: PlanPhoto[];
}

export interface PrintBatch {
  /** Page PNGs, or the one PDF; empty for a physical print. */
  files: string[];
  pages: number;
  /** The OS spooler's job id for a physical print, when it reports one. */
  job_id: string | null;
  failed: { file_id: number; error: string }[];
}

export interface Printer {
  name: string;
  is_default: boolean;
  papers: Paper[];
  default_paper: Paper | null;
  /** Dialog paper sizes this printer does not match; printing warns first. */
  mismatch: Paper[];
}

export const MAX_IMAGES_PER_PAGE = 100;
export const MAX_COPIES = 99;

export function clampInt(v: number, min: number, max: number): number {
  if (!Number.isFinite(v)) return min;
  return Math.min(max, Math.max(min, Math.round(v)));
}

export async function printPlan(
  ids: number[],
  copies: number,
  options: PrintOptions,
): Promise<PrintPlan> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<PrintPlan>("print_plan", {
    ids,
    request: { copies, options },
  });
}

export async function printPages(
  ids: number[],
  dest: string,
  copies: number,
  options: PrintOptions,
): Promise<PrintBatch> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<PrintBatch>("print_pages", {
    ids,
    dest,
    request: { copies, options },
  });
}

/// Installed printers. An unavailable spooler is an empty list: the dialog
/// still offers the PDF and PNG sinks.
export async function listPrinters(): Promise<Printer[]> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return (await invoke<Printer[]>("list_printers", {})) ?? [];
  } catch {
    return [];
  }
}

/// Print on `printer` (a name from [`listPrinters`]).
export async function printToPrinter(
  ids: number[],
  printer: string,
  copies: number,
  options: PrintOptions,
): Promise<PrintBatch> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<PrintBatch>("print_to_printer", {
    ids,
    printer,
    request: { copies, options },
  });
}

/// Export the same layout as a PDF at `dest`.
export async function exportPrintPdf(
  ids: number[],
  dest: string,
  copies: number,
  options: PrintOptions,
): Promise<PrintBatch> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<PrintBatch>("export_print_pdf", {
    ids,
    dest,
    request: { copies, options },
  });
}

/// Stop the print or PDF job in flight (before it reaches the printer/file).
export async function cancelPrint(): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("cancel_print", {});
}

/// Subscribe to per-sheet progress while a job is in flight.
export async function onPrintProgress(cb: (done: number) => void): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<{ done: number }>("print-progress", (e) => cb(e.payload.done));
}

/// The `preview://` URI of the photo's edited render (the edit view's
/// pipeline: bounded queue, cached); no image bytes cross IPC.
export async function photoPreviewUri(photo: PlanPhoto): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<string>("render_edit_preview", {
    fileId: photo.file_id,
    recipe: photo.recipe,
  });
}

const pct = (v: number): string => `${+(v * 100).toFixed(4)}%`;

/// Absolute position of `r` as percentages of the page.
export function pageRectStyle(r: Rect, page: { width: number; height: number }): string {
  return (
    `left:${pct(r.x / page.width)};top:${pct(r.y / page.height)};` +
    `width:${pct(r.w / page.width)};height:${pct(r.h / page.height)}`
  );
}

/// Style of the `<img>` inside its clip box (the fit's `dst` rect): scales
/// the photo so that exactly the fit's visible source rectangle fills the
/// box. A rotated cell shows the photo turned 90° clockwise, so the image
/// element is laid out in its unrotated size and rotated around its center.
export function photoStyle(cell: CellPlan): string {
  const { src, dst } = cell.fit;
  // Full (rotated-frame) photo size and offset, in inches relative to dst.
  const fw = dst.w / src.w;
  const fh = dst.h / src.h;
  const fx = (-src.x * dst.w) / src.w;
  const fy = (-src.y * dst.h) / src.h;
  if (!cell.rotated) {
    return (
      `left:${pct(fx / dst.w)};top:${pct(fy / dst.h)};` +
      `width:${pct(fw / dst.w)};height:${pct(fh / dst.h)}`
    );
  }
  const cx = fx + fw / 2;
  const cy = fy + fh / 2;
  // Unrotated element: fh wide, fw tall, centered on (cx, cy).
  return (
    `left:${pct((cx - fh / 2) / dst.w)};top:${pct((cy - fw / 2) / dst.h)};` +
    `width:${pct(fh / dst.w)};height:${pct(fw / dst.h)};` +
    "transform:rotate(90deg)"
  );
}

/// CSS font stack for a print font option (the bundled families map to the
/// generic stacks; installed families pass through).
export function fontStack(font: string): string {
  switch (font.toLowerCase()) {
    case "serif":
      return "serif";
    case "mono":
    case "monospace":
      return "monospace";
    case "sans":
    case "sans-serif":
    case "default":
    case "":
      return "sans-serif";
    default:
      return `"${font.replace(/"/g, "")}", sans-serif`;
  }
}
