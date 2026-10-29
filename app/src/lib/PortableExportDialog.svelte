<!--
  Portable export (TASK-03-023, PLAN §5 anti-lock-in): pick what to export
  (albums / tags / recipes) and the format (JSON = full fidelity, CSV = one
  table per section), name an existing folder, export. Nothing in the
  library changes; earlier exports are never overwritten.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    exportUserData,
    hasSection,
    type ExportFormat,
    type ExportReport,
    type ExportSections,
  } from "./portable-export";

  let { open = $bindable(false) }: { open?: boolean } = $props();

  const SECTIONS = ["albums", "tags", "recipes"] as const;

  let sections = $state<ExportSections>({ albums: true, tags: true, recipes: true });
  let format = $state<ExportFormat>("json");
  let folder = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let report = $state<ExportReport | null>(null);

  let canRun = $derived(!busy && folder.trim() !== "" && hasSection(sections));

  async function run(): Promise<void> {
    if (!canRun) return;
    busy = true;
    error = null;
    report = null;
    try {
      report = await exportUserData(folder.trim(), { ...sections }, format);
    } catch (e) {
      error = t("export.error", { error: String(e) });
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <div class="pe-backdrop" data-testid="export-backdrop">
    <div class="pe-dialog" role="dialog" aria-label={t("export.title")}>
      <h2>{t("export.title")}</h2>
      <p>{t("export.intro")}</p>

      <fieldset>
        <legend>{t("export.what")}</legend>
        {#each SECTIONS as s (s)}
          <label>
            <input
              type="checkbox"
              data-testid={`export-section-${s}`}
              bind:checked={sections[s]}
              disabled={busy}
            />
            {t(`export.section.${s}`)}
          </label>
        {/each}
      </fieldset>

      <fieldset>
        <legend>{t("export.format")}</legend>
        <label>
          <input
            type="radio"
            name="export-format"
            value="json"
            data-testid="export-format-json"
            bind:group={format}
            disabled={busy}
          />
          {t("export.format.json")}
        </label>
        <label>
          <input
            type="radio"
            name="export-format"
            value="csv"
            data-testid="export-format-csv"
            bind:group={format}
            disabled={busy}
          />
          {t("export.format.csv")}
        </label>
      </fieldset>

      <label class="pe-folder">
        {t("export.folder")}
        <input
          type="text"
          data-testid="export-folder"
          placeholder={t("export.folder.placeholder")}
          bind:value={folder}
          disabled={busy}
        />
      </label>

      {#if report}
        <div class="pe-done" data-testid="export-result">
          <p>
            {t("export.done", {
              albums: report.albums,
              tags: report.tags,
              recipes: report.recipes,
            })}
          </p>
          <ul>
            {#each report.files as file (file)}
              <li data-testid="export-file">{file}</li>
            {/each}
          </ul>
        </div>
        {#if report.unreadable_recipes > 0}
          <p class="pe-warn" data-testid="export-unreadable">
            {t("export.unreadable", { n: report.unreadable_recipes })}
          </p>
        {/if}
      {/if}
      {#if error}
        <p class="pe-error" role="alert">{error}</p>
      {/if}

      <div class="pe-actions">
        <button type="button" data-testid="export-run" disabled={!canRun} onclick={run}>
          {busy ? t("export.running") : t("export.run")}
        </button>
        <button type="button" data-testid="export-close" onclick={() => (open = false)}>
          {t("export.close")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .pe-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: var(--fb-backdrop, rgb(0 0 0 / 40%));
    display: grid;
    place-items: center;
  }
  .pe-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 28rem;
    max-width: 90vw;
    max-height: 90vh;
    overflow-y: auto;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px var(--fb-shadow, rgb(0 0 0 / 30%));
    text-align: left;
  }
  h2,
  p {
    margin: 0 0 0.6rem;
  }
  fieldset {
    margin: 0 0 0.6rem;
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem 1rem;
  }
  .pe-folder {
    display: grid;
    gap: 0.25rem;
  }
  .pe-done,
  .pe-warn {
    margin-top: 0.6rem;
    font-size: 0.85rem;
  }
  .pe-done ul {
    margin: 0;
    padding-left: 1.2rem;
    color: var(--fb-muted, #6b6b6b);
    word-break: break-all;
  }
  .pe-error {
    color: var(--fb-error, #b00020);
    font-size: 0.85rem;
  }
  .pe-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.8rem;
  }
</style>
