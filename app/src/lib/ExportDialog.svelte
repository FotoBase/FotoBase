<!--
  Export dialog (TASK-04-015, PLAN §2J File menu "Export image"): copy the
  selected photos to a folder — original size or custom resize with
  horizontal/vertical pixel limits (slider + input), quality presets
  (Auto/Normal/Maximum/Minimum/Custom %), optional numbered file names to
  preserve order, optional text watermark. Edits are baked in by the shared
  full-res export engine; the batch reports per-photo progress.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    PRESET_QUALITY,
    exportPhotos,
    watermarkLine,
    type QualityPreset,
  } from "./export-photos";

  let {
    open = $bindable(false),
    ids = $bindable<number[]>([]),
  }: { open?: boolean; ids?: number[] } = $props();

  const PRESETS = ["auto", "normal", "maximum", "minimum", "custom"] as const;

  let folder = $state("");
  let customSize = $state(false);
  let maxWidth = $state(2048);
  let maxHeight = $state(2048);
  let preset = $state<QualityPreset>("auto");
  let customQuality = $state(85);
  let numbered = $state(false);
  let watermarkOn = $state(false);
  let wmName = $state("");
  let wmDomain = $state("");
  let wmCopyright = $state("");
  let busy = $state(false);
  let done = $state(0);
  let total = $state(0);
  let error = $state<string | null>(null);

  let quality = $derived(preset === "custom" ? customQuality : PRESET_QUALITY[preset]);
  let canRun = $derived(!busy && folder.trim() !== "" && ids.length > 0);

  async function run(): Promise<void> {
    if (!canRun) return;
    busy = true;
    error = null;
    done = 0;
    total = ids.length;
    try {
      const batch = await exportPhotos(ids, folder.trim(), {
        resizeMaxWidth: customSize ? maxWidth : null,
        resizeMaxHeight: customSize ? maxHeight : null,
        quality,
        numbered,
        watermark: watermarkOn ? watermarkLine(wmName, wmDomain, wmCopyright) || null : null,
      });
      if (batch.failed.length > 0) {
        error = t("imgexport.partial", { n: batch.failed.length });
      }
    } catch (e) {
      error = t("imgexport.error", { error: String(e) });
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <div class="ex-backdrop" data-testid="imgexport-backdrop">
    <div class="ex-dialog" role="dialog" aria-label={t("imgexport.title")}>
      <h2>{t("imgexport.title")}</h2>

      <label class="ex-folder">
        {t("imgexport.folder")}
        <input
          type="text"
          data-testid="imgexport-folder"
          placeholder={t("imgexport.folder.placeholder")}
          bind:value={folder}
          disabled={busy}
        />
      </label>

      <fieldset>
        <legend>{t("imgexport.size")}</legend>
        <label>
          <input type="radio" name="imgexport-size" checked onchange={() => (customSize = false)} disabled={busy} />
          {t("imgexport.size.original")}
        </label>
        <label>
          <input
            type="radio"
            name="imgexport-size"
            checked={customSize}
            onchange={() => (customSize = true)}
            disabled={busy}
          />
          {t("imgexport.size.custom")}
        </label>
        {#if customSize}
          <label class="ex-limit">
            {t("imgexport.limit.h")}
            <input
              type="range"
              min="64"
              max="8192"
              step="64"
              data-testid="imgexport-limit-h-slider"
              bind:value={maxWidth}
              disabled={busy}
            />
            <input
              type="number"
              min="16"
              max="100000"
              data-testid="imgexport-limit-h"
              bind:value={maxWidth}
              disabled={busy}
            />
          </label>
          <label class="ex-limit">
            {t("imgexport.limit.v")}
            <input
              type="range"
              min="64"
              max="8192"
              step="64"
              data-testid="imgexport-limit-v-slider"
              bind:value={maxHeight}
              disabled={busy}
            />
            <input
              type="number"
              min="16"
              max="100000"
              data-testid="imgexport-limit-v"
              bind:value={maxHeight}
              disabled={busy}
            />
          </label>
        {/if}
      </fieldset>

      <fieldset>
        <legend>{t("imgexport.quality")}</legend>
        {#each PRESETS as p (p)}
          <label>
            <input
              type="radio"
              name="imgexport-quality"
              value={p}
              data-testid={`imgexport-quality-${p}`}
              bind:group={preset}
              disabled={busy}
            />
            {t(`imgexport.quality.${p}`)}
          </label>
        {/each}
        {#if preset === "custom"}
          <label class="ex-custom-quality">
            {t("imgexport.quality.percent", { n: customQuality })}
            <input
              type="range"
              min="1"
              max="100"
              data-testid="imgexport-quality-slider"
              bind:value={customQuality}
              disabled={busy}
            />
          </label>
        {/if}
      </fieldset>

      <label>
        <input type="checkbox" data-testid="imgexport-numbered" bind:checked={numbered} disabled={busy} />
        {t("imgexport.numbered")}
      </label>

      <fieldset>
        <legend>{t("imgexport.watermark")}</legend>
        <label>
          <input type="checkbox" data-testid="imgexport-watermark" bind:checked={watermarkOn} disabled={busy} />
          {t("imgexport.watermark.enable")}
        </label>
        {#if watermarkOn}
          <input
            type="text"
            data-testid="imgexport-watermark-name"
            placeholder={t("imgexport.watermark.name")}
            bind:value={wmName}
            disabled={busy}
          />
          <input
            type="text"
            data-testid="imgexport-watermark-domain"
            placeholder={t("imgexport.watermark.domain")}
            bind:value={wmDomain}
            disabled={busy}
          />
          <input
            type="text"
            data-testid="imgexport-watermark-copyright"
            placeholder={t("imgexport.watermark.copyright")}
            bind:value={wmCopyright}
            disabled={busy}
          />
        {/if}
      </fieldset>

      {#if busy}
        <p data-testid="imgexport-progress">{t("imgexport.progress", { done, total })}</p>
      {/if}
      {#if error}
        <p class="ex-error" role="alert" data-testid="imgexport-error">{error}</p>
      {/if}

      <div class="ex-actions">
        <button type="button" data-testid="imgexport-run" data-dbg={String(ids.length)} disabled={!canRun} onclick={run}>
          {busy ? t("imgexport.running") : t("imgexport.run")}
        </button>
        <button type="button" data-testid="imgexport-close" onclick={() => (open = false)}>
          {t("imgexport.close")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .ex-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: var(--fb-backdrop, rgb(0 0 0 / 40%));
    display: grid;
    place-items: center;
  }
  .ex-dialog {
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
    display: grid;
    gap: 0.5rem;
  }
  h2,
  p {
    margin: 0;
  }
  fieldset {
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem 1rem;
  }
  .ex-folder {
    display: grid;
    gap: 0.25rem;
  }
  .ex-limit {
    display: grid;
    grid-template-columns: auto 1fr 5rem;
    gap: 0.4rem;
    align-items: center;
  }
  .ex-custom-quality {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.4rem;
    align-items: center;
  }
  .ex-dialog fieldset input[type='text'] {
    width: 100%;
  }
  .ex-error {
    color: var(--fb-error, #b00020);
    font-size: 0.85rem;
  }
  .ex-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.4rem;
  }
</style>
