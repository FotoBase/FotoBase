<script lang="ts">
  // Options dialog — General tab (TASK-01-022, PLAN §2). Every change is
  // persisted immediately through `set_settings`; the language change also
  // flips the i18n locale live.
  import { t, setLocale, locales } from "./i18n.svelte.js";
  import {
    clearCache,
    onRebuildProgress,
    rebuildCatalog,
    registerAssociations,
    saveSettings,
    unregisterAssociations,
    IMAGE_EXTENSIONS,
    THEME_CHOICES,
    UPDATE_MODES,
    type Settings,
  } from "./settings";

  let {
    open = $bindable(false),
    settings,
    onchange,
    onrevealchanged,
  }: {
    open?: boolean;
    settings: Settings;
    onchange: (s: Settings) => void;
    /// "Show hidden" was toggled and the backend has applied it: re-list.
    onrevealchanged?: () => void;
  } = $props();

  let clearResult = $state<string | null>(null);

  function apply(next: Settings): void {
    const revealChanged = next.show_hidden !== settings.show_hidden;
    onchange(next);
    void saveSettings(next)
      .then(() => {
        if (revealChanged) onrevealchanged?.();
      })
      .catch(() => {
        // Browser dev has no backend; the in-memory change still applies.
      });
  }

  function setLanguage(lang: string): void {
    setLocale(lang);
    apply({ ...settings, language: lang });
  }

  function setUpdateMode(mode: Settings["update_mode"]): void {
    apply({ ...settings, update_mode: mode });
  }

  function setShowTooltips(show: boolean): void {
    apply({ ...settings, show_tooltips: show });
  }

  function setGridRenderer(renderer: Settings["grid_renderer"]): void {
    apply({ ...settings, grid_renderer: renderer });
  }

  function setTheme(theme: Settings["theme"]): void {
    apply({ ...settings, theme });
  }

  function setRatingsEnabled(enabled: boolean): void {
    apply({ ...settings, ratings_enabled: enabled });
  }

  function setShowHidden(show: boolean): void {
    apply({ ...settings, show_hidden: show });
  }

  // Whole days, at least one; anything unparsable falls back to the default.
  function setTrashWindow(raw: string): void {
    const days = Math.trunc(Number(raw));
    apply({
      ...settings,
      trash_window_days: Number.isFinite(days) && raw.trim() !== "" ? Math.max(1, days) : 30,
    });
  }

  function setQuickTags(tags: string[]): void {
    apply({ ...settings, quick_tags: tags });
  }

  // File associations (TASK-01-025, PLAN §2M): a checked type registers
  // Fotobase as the OS default viewer for it (unchecking unregisters and
  // restores the previous handler). The persisted list is the checked
  // state's source of truth; an OS-side failure is reported, never silent.
  let assocResult = $state<string | null>(null);

  function setAssociation(ext: string, want: boolean): void {
    const current = settings.file_associations;
    const next = want
      ? current.includes(ext)
        ? current
        : [...current, ext]
      : current.filter((e) => e !== ext);
    apply({ ...settings, file_associations: next });
    assocResult = null;
    const call = want ? registerAssociations([ext]) : unregisterAssociations([ext]);
    void call.catch((e) => {
      assocResult = t("options.associations.failed", { error: String(e) });
    });
  }

  async function onClearCache(): Promise<void> {
    clearResult = null;
    try {
      const removed = await clearCache();
      clearResult = t("options.clearCache.done", { n: removed });
    } catch (e) {
      clearResult = t("options.clearCache.failed", { error: String(e) });
    }
  }

  // Rebuild catalog (TASK-01-023): the command runs in the background job
  // queue; progress arrives as `rebuild-progress` events. Subscribe before
  // invoking so no early event is missed, and unsubscribe when done.
  let rebuilding = $state(false);
  let rebuildProgress = $state<string | null>(null);
  let rebuildResult = $state<string | null>(null);

  async function onRebuildCatalog(): Promise<void> {
    rebuildResult = null;
    rebuildProgress = null;
    rebuilding = true;
    let unlisten: (() => void) | null = null;
    try {
      unlisten = await onRebuildProgress((p) => {
        rebuildProgress = t("options.rebuildCatalog.running", {
          done: p.folders_done,
          total: p.folders_total,
          files: p.files_done,
        });
      });
      const report = await rebuildCatalog();
      rebuildResult = t("options.rebuildCatalog.done", {
        added: report.added,
        updated: report.updated,
        removed: report.removed,
      });
    } catch (e) {
      rebuildResult = t("options.rebuildCatalog.failed", { error: String(e) });
    } finally {
      unlisten?.();
      rebuilding = false;
      rebuildProgress = null;
    }
  }
</script>

{#if open}
  <div class="options-backdrop" data-testid="options-backdrop">
    <div class="options-dialog" role="dialog" aria-label={t("options.title")}>
      <h2>{t("options.title")}</h2>
      <nav class="options-tabs">
        <button type="button" class="active">{t("options.tab.general")}</button>
      </nav>

      <div class="options-body">
        <label class="options-row">
          <span>{t("options.language")}</span>
          <select
            value={settings.language}
            onchange={(e) => setLanguage(e.currentTarget.value)}
            title={settings.show_tooltips ? t("options.language") : undefined}
          >
            {#each locales as lang (lang)}
              <option value={lang}>{lang}</option>
            {/each}
          </select>
        </label>

        <label class="options-row">
          <span>{t("options.gridRenderer")}</span>
          <select
            value={settings.grid_renderer}
            onchange={(e) => setGridRenderer(e.currentTarget.value as Settings["grid_renderer"])}
            title={settings.show_tooltips ? t("options.gridRenderer") : undefined}
          >
            <option value="dom">{t("options.gridRenderer.dom")}</option>
            <option value="canvas">{t("options.gridRenderer.canvas")}</option>
          </select>
        </label>

        <!-- Theme (TASK-01-024): follow the OS by default, or force a
             palette. App.svelte resolves the setting to a data-theme
             attribute the stylesheet keys off. -->
        <label class="options-row">
          <span>{t("options.theme")}</span>
          <select
            data-testid="theme-select"
            value={settings.theme}
            onchange={(e) => setTheme(e.currentTarget.value as Settings["theme"])}
            title={settings.show_tooltips ? t("options.theme") : undefined}
          >
            {#each THEME_CHOICES as choice (choice.value)}
              <option value={choice.value}>{t(choice.labelKey)}</option>
            {/each}
          </select>
        </label>

        <fieldset class="options-row options-updates">
          <legend>{t("options.updates")}</legend>
          {#each UPDATE_MODES as mode (mode.value)}
            <label>
              <input
                type="radio"
                name="update-mode"
                value={mode.value}
                checked={settings.update_mode === mode.value}
                onchange={() => setUpdateMode(mode.value)}
              />
              {t(mode.labelKey)}
            </label>
          {/each}
        </fieldset>

        <div class="options-row">
          <button type="button" onclick={onClearCache} data-testid="clear-cache">
            {t("options.clearCache")}
          </button>
          {#if clearResult}
            <span class="clear-result" data-testid="clear-cache-result">{clearResult}</span>
          {/if}
        </div>

        <div class="options-row">
          <button
            type="button"
            onclick={onRebuildCatalog}
            disabled={rebuilding}
            data-testid="rebuild-catalog"
          >
            {t("options.rebuildCatalog")}
          </button>
          {#if rebuildProgress}
            <span class="clear-result" data-testid="rebuild-progress">{rebuildProgress}</span>
          {/if}
          {#if rebuildResult}
            <span class="clear-result" data-testid="rebuild-result">{rebuildResult}</span>
          {/if}
        </div>

        <!-- File associations (TASK-01-025, PLAN §2M): which image types
             the OS opens with Fotobase. -->
        <fieldset class="options-row options-associations">
          <legend>{t("options.associations")}</legend>
          <p class="assoc-hint">{t("options.associations.hint")}</p>
          <div class="assoc-list">
            {#each IMAGE_EXTENSIONS as ext (ext)}
              <label>
                <input
                  type="checkbox"
                  data-testid={`assoc-${ext}`}
                  checked={settings.file_associations.includes(ext)}
                  onchange={(e) => setAssociation(ext, e.currentTarget.checked)}
                />
                {ext}
              </label>
            {/each}
          </div>
          {#if assocResult}
            <span class="clear-result" data-testid="assoc-result">{assocResult}</span>
          {/if}
        </fieldset>

        <label class="options-row options-check">
          <input
            type="checkbox"
            data-testid="ratings-enabled"
            checked={settings.ratings_enabled}
            onchange={(e) => setRatingsEnabled(e.currentTarget.checked)}
          />
          {t("options.ratings")}
        </label>

        <!-- Hidden items (TASK-03-009) and the catalog-trash undo window
             (TASK-03-012). -->
        <label class="options-row options-check">
          <input
            type="checkbox"
            data-testid="show-hidden"
            checked={settings.show_hidden}
            onchange={(e) => setShowHidden(e.currentTarget.checked)}
          />
          {t("options.showHidden")}
        </label>
        <label class="options-row">
          {t("options.trashWindow")}
          <input
            type="number"
            min="1"
            step="1"
            data-testid="trash-window"
            value={settings.trash_window_days}
            onchange={(e) => setTrashWindow(e.currentTarget.value)}
          />
        </label>

        <!-- Quick Tags (TASK-03-006, PLAN §2B): up to 10 one-click tags. -->
        <fieldset class="options-fieldset">
          <legend>{t("options.quickTags")}</legend>
          <textarea
            class="quick-tags-input"
            data-testid="quick-tags-input"
            rows="4"
            value={settings.quick_tags.join("\n")}
            onchange={(e) =>
              setQuickTags(
                e.currentTarget.value
                  .split("\n")
                  .map((line) => line.trim())
                  .filter((line) => line.length > 0)
                  .slice(0, 10),
              )}
          ></textarea>
        </fieldset>

        <label class="options-row options-check">
          <input
            type="checkbox"
            checked={settings.show_tooltips}
            onchange={(e) => setShowTooltips(e.currentTarget.checked)}
          />
          {t("options.showTooltips")}
        </label>
      </div>

      <footer class="options-footer">
        <button type="button" onclick={() => (open = false)}>{t("options.close")}</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .options-backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 40%);
    display: grid;
    place-items: center;
  }
  .options-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 22rem;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px rgb(0 0 0 / 30%);
  }
  .options-tabs {
    display: flex;
    gap: 0.5rem;
    margin: 0.5rem 0 1rem;
  }
  .options-tabs button {
    border: none;
    background: none;
    padding: 0.25rem 0.75rem;
    cursor: pointer;
    border-bottom: 2px solid transparent;
  }
  .options-tabs button.active {
    border-bottom-color: currentcolor;
    font-weight: 600;
  }
  .options-body {
    display: grid;
    gap: 0.9rem;
  }
  .options-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  .options-updates {
    display: grid;
    gap: 0.3rem;
    border: 1px solid rgb(128 128 128 / 40%);
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
  }
  .options-associations {
    display: grid;
    gap: 0.3rem;
    border: 1px solid rgb(128 128 128 / 40%);
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
  }
  .assoc-hint {
    margin: 0;
    color: var(--fb-muted, #555);
    font-size: 0.85rem;
  }
  .assoc-list {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 0.9rem;
  }
  .assoc-list label {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
  }
  .options-check {
    justify-content: start;
  }
  .options-footer {
    display: flex;
    justify-content: flex-end;
    margin-top: 1rem;
  }
  .clear-result {
    color: var(--fb-muted, #555);
  }
</style>
