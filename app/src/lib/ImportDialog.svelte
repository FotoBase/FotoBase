<script lang="ts">
  // Import dialog (TASK-01-017, PLAN §2A/§6 P1): pick a source folder and a
  // destination, choose date-based or custom destination naming, and run the
  // import with "Exclude duplicates" on by default (PLAN §2: "Detect
  // duplicates while importing"). Progress streams from the backend; a
  // cancelled import leaves no half-copied files (the backend guarantees
  // per-file cleanup). The parent refreshes the library through `onchanged`.
  import { t } from "./i18n.svelte.js";
  import {
    cancelImport,
    importPhotos,
    onImportProgress,
    type ImportProgress,
    type ImportReport,
    type ImportNaming,
  } from "./import-photos";

  let { open = $bindable(false), onchanged }: { open?: boolean; onchanged: () => void } =
    $props();

  let source = $state("");
  let destination = $state("");
  let naming = $state<ImportNaming>("date");
  let customPattern = $state("");
  let excludeDuplicates = $state(true);
  let running = $state(false);
  let progress = $state<ImportProgress | null>(null);
  let report = $state<ImportReport | null>(null);
  let error = $state<string | null>(null);

  // Subscribe once while the dialog can be open; the listener is inert when
  // no import is running.
  $effect(() => {
    if (!open) return;
    let unlisten: (() => void) | undefined;
    void onImportProgress((p) => {
      if (running) progress = p;
    }).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  });

  async function start(): Promise<void> {
    error = null;
    report = null;
    progress = null;
    running = true;
    try {
      const result = await importPhotos({
        source,
        destination,
        naming,
        customPattern: naming === "custom" ? customPattern : undefined,
        excludeDuplicates,
      });
      report = result;
      onchanged();
    } catch (e) {
      error = t("import.error", { error: String(e) });
    } finally {
      running = false;
      progress = null;
    }
  }

  async function cancel(): Promise<void> {
    try {
      await cancelImport();
    } catch {
      // Best-effort: a finished import cannot be cancelled.
    }
  }
</script>

{#if open}
  <div class="import-backdrop" data-testid="import-backdrop">
    <div class="import-dialog" role="dialog" aria-label={t("import.title")}>
      <h2>{t("import.title")}</h2>

      <label class="import-field">
        <span>{t("import.source")}</span>
        <input
          type="text"
          bind:value={source}
          disabled={running}
          data-testid="import-source"
        />
      </label>
      <label class="import-field">
        <span>{t("import.destination")}</span>
        <input
          type="text"
          bind:value={destination}
          disabled={running}
          data-testid="import-destination"
        />
      </label>

      <fieldset class="import-naming" disabled={running}>
        <legend>{t("import.naming")}</legend>
        <label>
          <input
            type="radio"
            name="import-naming"
            value="date"
            bind:group={naming}
            data-testid="import-naming-date"
          />
          {t("import.naming.date")}
        </label>
        <label>
          <input
            type="radio"
            name="import-naming"
            value="custom"
            bind:group={naming}
            data-testid="import-naming-custom"
          />
          {t("import.naming.custom")}
        </label>
        {#if naming === "custom"}
          <input
            type="text"
            bind:value={customPattern}
            placeholder={t("import.customPattern.placeholder")}
            data-testid="import-custom-pattern"
          />
        {/if}
      </fieldset>

      <label class="import-exclude">
        <input
          type="checkbox"
          bind:checked={excludeDuplicates}
          disabled={running}
          data-testid="import-exclude-duplicates"
        />
        {t("import.excludeDuplicates")}
      </label>

      {#if progress}
        <p class="import-progress" data-testid="import-progress">
          {t("import.running", {
            done: progress.files_done,
            total: progress.files_total,
            file: progress.current_file,
          })}
        </p>
      {/if}
      {#if report}
        <p class="import-report" data-testid="import-report">
          {report.cancelled
            ? t("import.report.cancelled", {
                copied: report.copied,
                duplicates: report.duplicates,
              })
            : t("import.report", {
                copied: report.copied,
                duplicates: report.duplicates,
                failed: report.failed,
              })}
        </p>
      {/if}
      {#if error}
        <p class="import-error" data-testid="import-error">{error}</p>
      {/if}

      <footer class="import-footer">
        {#if running}
          <button type="button" onclick={cancel} data-testid="import-cancel">
            {t("import.cancel")}
          </button>
        {:else}
          <button
            type="button"
            onclick={start}
            disabled={!source || !destination || (naming === "custom" && !customPattern)}
            data-testid="import-start"
          >
            {t("import.start")}
          </button>
        {/if}
        <button type="button" onclick={() => (open = false)} data-testid="import-close">
          {t("options.close")}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .import-backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 40%);
    display: grid;
    place-items: center;
  }
  .import-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 30rem;
    max-width: 90vw;
    max-height: 80vh;
    overflow: auto;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px rgb(0 0 0 / 30%);
    display: grid;
    gap: 0.75rem;
  }
  .import-field {
    display: grid;
    gap: 0.25rem;
  }
  .import-field input {
    width: 100%;
    box-sizing: border-box;
  }
  .import-naming {
    border: 1px solid rgb(128 128 128 / 30%);
    border-radius: 6px;
    display: grid;
    gap: 0.25rem;
    padding: 0.5rem 0.75rem;
  }
  .import-exclude {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .import-progress {
    color: var(--fb-muted, #555);
  }
  .import-report {
    color: var(--fb-text, #111);
  }
  .import-error {
    color: #b00020;
  }
  .import-footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.25rem;
  }
</style>
