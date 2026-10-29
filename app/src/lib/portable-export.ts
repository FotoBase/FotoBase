// Portable export of user data (TASK-03-023, PLAN §5 anti-lock-in): albums,
// tags and edit recipes to JSON or CSV at any time. The backend reads and
// writes everything; this module is the IPC wrapper.

export interface ExportSections {
  albums: boolean;
  tags: boolean;
  recipes: boolean;
}

export type ExportFormat = "json" | "csv";

/// What one export wrote (mirrors the backend `ExportReport`).
export interface ExportReport {
  files: string[];
  albums: number;
  tags: number;
  recipes: number;
  /// Recipes that are not conforming v1 packets: kept raw in the JSON.
  unreadable_recipes: number;
}

export function hasSection(s: ExportSections): boolean {
  return s.albums || s.tags || s.recipes;
}

export async function exportUserData(
  dir: string,
  sections: ExportSections,
  format: ExportFormat,
): Promise<ExportReport> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<ExportReport>("export_user_data", {
    dir,
    sections,
    format,
  });
}
