// Theme resolution (TASK-01-024, PLAN §2 additions "Dark mode / HiDPI:
// native theming"). The pure decision logic lives here; `app.css` keys all
// colors off a `data-theme` attribute on the document root, so resolving the
// setting once per change swaps every palette variable at once. Plain CSS
// variables only — no framework (PLAN §4).

/// What the user picked in Options → General. `system` follows the OS.
export type ThemeSetting = "system" | "light" | "dark";

/// The resolved theme the stylesheet consumes.
export type Theme = "light" | "dark";

/// Resolve the setting against the OS preference. Explicit choices win;
/// unknown persisted values degrade to system (a corrupt settings.json must
/// never crash the app — same policy as the backend's settings loader).
export function resolveTheme(setting: ThemeSetting, systemPrefersDark: boolean): Theme {
  if (setting === "light") return "light";
  if (setting === "dark") return "dark";
  return systemPrefersDark ? "dark" : "light";
}

/// Publish the resolved theme to the stylesheet: `:root[data-theme=…]`.
export function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
}

/// Does the OS currently prefer dark? Best-effort: an environment without
/// `matchMedia` (old jsdom, odd WebViews) reads as light, never throws.
export function systemPrefersDark(): boolean {
  try {
    return window.matchMedia("(prefers-color-scheme: dark)").matches;
  } catch {
    return false;
  }
}

/// Call back whenever the OS preference flips (only meaningful while the
/// setting is `system`). Returns the unsubscribe function.
export function watchSystemTheme(cb: (prefersDark: boolean) => void): () => void {
  try {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const handler = (e: MediaQueryListEvent): void => cb(e.matches);
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  } catch {
    return () => {};
  }
}
