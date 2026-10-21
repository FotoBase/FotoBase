// Minimal i18n scaffold (TASK-01-022: "language selection changes UI
// language; i18n scaffold at minimum; full translations can follow").
// A `.svelte.ts` module so the current locale is a rune-backed reactive
// value; components re-render when it changes. `t()` falls back to English,
// then to the key itself — a missing translation must never render empty.

type Dict = Record<string, string>;

const messages: Record<string, Dict> = {
  en: {
    "app.title": "Fotobase",
    "app.tagline": "Local-first photo organizer — skeleton.",
    "app.ping": "Ping backend",
    "app.ping.tooltip": "Sends a trivial IPC round-trip to the Rust core",
    "app.backendSays": "Backend says:",
    "app.backendMissing": "no backend (browser dev)",
    "app.options": "Options",
    "options.title": "Options",
    "options.tab.general": "General",
    "options.language": "Language",
    "options.updates": "Automatic updates",
    "options.updates.auto": "Install updates automatically",
    "options.updates.checkButAsk": "Check for updates, ask before installing",
    "options.updates.off": "Never check for updates",
    "options.clearCache": "Clear cache files",
    "options.clearCache.done": "Removed {n} cached files",
    "options.clearCache.failed": "Clearing the cache failed: {error}",
    "options.rebuildCatalog": "Rebuild catalog",
    "options.rebuildCatalog.running":
      "Rebuilding… folder {done}/{total} — {files} files",
    "options.rebuildCatalog.done":
      "Rebuilt: {added} added, {updated} updated, {removed} removed",
    "options.rebuildCatalog.failed": "Rebuilding the catalog failed: {error}",
    "options.showTooltips": "Show tooltips",
    "options.gridRenderer": "Thumbnail grid renderer",
    "options.gridRenderer.dom": "DOM (default)",
    "options.gridRenderer.canvas": "Canvas",
    "options.close": "Close",
    "grid.zoom": "Thumbnail size",
    "tree.label": "Folders",
    "tree.empty": "No folders scanned yet.",
    "tree.unknownYear": "Unknown year",
    "menu.viewSlideshow": "View slideshow",
    "menu.refreshThumbnails": "Refresh thumbnails",
    "menu.locateOnDisk": "Locate on disk",
    "menu.removeFromFotobase": "Remove from Fotobase",
    "menu.addToAlbum": "Add to album",
  },
  de: {
    "app.title": "Fotobase",
    "app.tagline": "Lokaler Foto-Organizer — Grundgerüst.",
    "app.ping": "Backend anpingen",
    "app.ping.tooltip": "Sendet einen einfachen IPC-Rundtrip an den Rust-Kern",
    "app.backendSays": "Backend antwortet:",
    "app.backendMissing": "kein Backend (Browser-Entwicklung)",
    "app.options": "Optionen",
    "options.title": "Optionen",
    "options.tab.general": "Allgemein",
    "options.language": "Sprache",
    "options.updates": "Automatische Updates",
    "options.updates.auto": "Updates automatisch installieren",
    "options.updates.checkButAsk": "Nach Updates suchen, vor Installation fragen",
    "options.updates.off": "Nie nach Updates suchen",
    "options.clearCache": "Cache-Dateien löschen",
    "options.clearCache.done": "{n} Cache-Dateien entfernt",
    "options.clearCache.failed": "Leeren des Caches fehlgeschlagen: {error}",
    "options.rebuildCatalog": "Katalog neu aufbauen",
    "options.rebuildCatalog.running":
      "Neuaufbau… Ordner {done}/{total} — {files} Dateien",
    "options.rebuildCatalog.done":
      "Neu aufgebaut: {added} neu, {updated} aktualisiert, {removed} entfernt",
    "options.rebuildCatalog.failed":
      "Neuaufbau des Katalogs fehlgeschlagen: {error}",
    "options.showTooltips": "Tooltips anzeigen",
    "options.gridRenderer": "Vorschaugitter-Renderer",
    "options.gridRenderer.dom": "DOM (Standard)",
    "options.gridRenderer.canvas": "Canvas",
    "options.close": "Schließen",
    "grid.zoom": "Vorschaugröße",
    "tree.label": "Ordner",
    "tree.empty": "Noch keine Ordner eingelesen.",
    "tree.unknownYear": "Jahr unbekannt",
    "menu.viewSlideshow": "Diashow starten",
    "menu.refreshThumbnails": "Vorschaubilder aktualisieren",
    "menu.locateOnDisk": "Auf Festplatte zeigen",
    "menu.removeFromFotobase": "Aus Fotobase entfernen",
    "menu.addToAlbum": "Zum Album hinzufügen",
  },
};

let current = $state("en");

export const locales: string[] = Object.keys(messages);

export function locale(): string {
  return current;
}

export function setLocale(tag: string): void {
  if (messages[tag] !== undefined) current = tag;
}

/** Translate `key` in the current locale (English fallback, then the key). */
export function t(key: string, vars?: Record<string, string | number>): string {
  const raw = messages[current]?.[key] ?? messages.en[key] ?? key;
  if (vars === undefined) return raw;
  return raw.replace(/\{(\w+)\}/g, (_, name) => String(vars[name] ?? `{${name}}`));
}
