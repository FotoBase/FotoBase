<!--
  Geotag dialog (TASK-06-006, PLAN §2 Tools "Geotag"): one location applied
  to the selection, a whole folder, or a whole album in one pass. The
  backend queues the run through the job queue at background priority,
  writes each photo's sidecar GPS through the merge-on-write path, streams
  a progress counter, and can be cancelled between photos. The prior GPS
  values are snapshotted, so Undo restores them.
-->
<script lang="ts">
  import {
    GeotagTargetScope,
    cancelGeotag,
    onGeotagProgress,
    parseLatitude,
    parseLongitude,
    runGeotagTool,
    undoGeotag,
    type GeotagProgress,
    type GeotagToolReport,
  } from "./geotag-tool";
  import { t } from "./i18n.svelte.js";

  let {
    open = $bindable(false),
    ids,
    folderId,
    albumId,
    onapplied,
  }: {
    open?: boolean;
    ids: number[];
    folderId: number | null;
    albumId: number | null;
    onapplied: (report: GeotagToolReport) => void;
  } = $props();

  let scope = $state(GeotagTargetScope.Selection);
  let latitude = $state("");
  let longitude = $state("");
  let running = $state(false);
  let undoing = $state(false);
  let progress = $state<GeotagProgress | null>(null);
  let report = $state<GeotagToolReport | null>(null);
  let undone = $state<number | null>(null);
  let error = $state<string | null>(null);

  const latValid = $derived(parseLatitude(latitude) !== null);
  const lonValid = $derived(parseLongitude(longitude) !== null);
  const canApply = $derived(latValid && lonValid && !running && !undoing);
  const canUndo = $derived(report !== null && undone === null && !running && !undoing);

  // Re-seed the dialog every time it opens.
  $effect(() => {
    if (!open) return;
    scope = GeotagTargetScope.Selection;
    latitude = "";
    longitude = "";
    running = false;
    undoing = false;
    progress = null;
    report = null;
    undone = null;
    error = null;
  });

  // Escape closes the open dialog unless a run is writing sidecars.
  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent): void => {
      e.stopPropagation();
      if (e.key === "Escape" && !running && !undoing) open = false;
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  async function apply(): Promise<void> {
    if (!canApply) return;
    const lat = parseLatitude(latitude);
    const lon = parseLongitude(longitude);
    if (lat === null || lon === null) return;
    running = true;
    error = null;
    report = null;
    undone = null;
    progress = null;
    let unlisten: () => void = () => {};
    try {
      // Progress is a nicety: a failed subscription never blocks the job.
      unlisten = await onGeotagProgress((p) => (progress = p)).catch(() => () => {});
      const target =
        scope === GeotagTargetScope.Folder && folderId !== null
          ? { kind: "folder", folder_id: folderId }
          : scope === GeotagTargetScope.Album && albumId !== null
            ? { kind: "album", album_id: albumId }
            : { kind: "ids", ids: [...ids] };
      const result = await runGeotagTool(target, lat, lon);
      report = result;
      onapplied(result);
    } catch (e) {
      error = t("geotag.error", { error: String(e) });
    } finally {
      unlisten();
      running = false;
      progress = null;
    }
  }

  async function undo(): Promise<void> {
    if (!canUndo) return;
    undoing = true;
    error = null;
    progress = null;
    let unlisten: () => void = () => {};
    try {
      unlisten = await onGeotagProgress((p) => (progress = p)).catch(() => () => {});
      undone = await undoGeotag();
    } catch (e) {
      error = t("geotag.error", { error: String(e) });
    } finally {
      unlisten();
      undoing = false;
      progress = null;
    }
  }

  async function cancel(): Promise<void> {
    try {
      await cancelGeotag();
    } catch {
      // Best-effort: a finished run cannot be cancelled.
    }
  }
</script>

{#if open}
  <div class="gt-backdrop" data-testid="geotag-backdrop">
    <div class="gt-dialog" data-testid="geotag-dialog" role="dialog" aria-label={t("geotag.title")}>
      <h2>{t("geotag.title")}</h2>
      <p class="gt-count">{t("tray.count", { count: ids.length, held: ids.length })}</p>

      <div class="gt-coords">
        <label>
          <span>{t("geotag.latitude")}</span>
          <input
            type="text"
            data-testid="geotag-lat"
            bind:value={latitude}
            disabled={running || undoing}
            placeholder="52.5200"
          />
        </label>
        <label>
          <span>{t("geotag.longitude")}</span>
          <input
            type="text"
            data-testid="geotag-lon"
            bind:value={longitude}
            disabled={running || undoing}
            placeholder="13.4050"
          />
        </label>
      </div>

      <label class="gt-scope">
        <span>{t("geotag.scope")}</span>
        <select data-testid="geotag-scope" bind:value={scope} disabled={running || undoing}>
          <option value={GeotagTargetScope.Selection}>{t("geotag.scopeSelection")}</option>
          {#if folderId !== null}
            <option value={GeotagTargetScope.Folder}>{t("geotag.scopeFolder")}</option>
          {/if}
          {#if albumId !== null}
            <option value={GeotagTargetScope.Album}>{t("geotag.scopeAlbum")}</option>
          {/if}
        </select>
      </label>

      {#if progress}
        <p data-testid="geotag-progress">
          {t("geotag.progress", { done: progress.done, total: progress.total })}
        </p>
      {/if}
      {#if report}
        <p data-testid="geotag-done">
          {report.failed.length === 0
            ? t("geotag.done", { applied: report.applied })
            : t("geotag.doneFailed", { applied: report.applied, failed: report.failed.length })}
        </p>
      {/if}
      {#if report?.cancelled}
        <p data-testid="geotag-cancelled">{t("geotag.cancelledNote")}</p>
      {/if}
      {#if report && report.failed.length > 0}
        <ul class="gt-failures" data-testid="geotag-failures">
          {#each report.failed as failure (failure.id)}
            <li>{t("geotag.failure", { error: failure.error })}</li>
          {/each}
        </ul>
      {/if}
      {#if undone !== null}
        <p data-testid="geotag-undone">{t("geotag.undone", { restored: undone })}</p>
      {/if}
      {#if error}
        <p class="gt-error" data-testid="geotag-error" role="alert">{error}</p>
      {/if}

      <footer>
        {#if running || undoing}
          <button type="button" data-testid="geotag-cancel" onclick={() => void cancel()}>
            {t("batch.cancel")}
          </button>
        {:else}
          {#if canUndo}
            <button type="button" data-testid="geotag-undo" onclick={() => void undo()}>
              {t("geotag.undo")}
            </button>
          {/if}
          <button
            type="button"
            data-testid="geotag-apply"
            disabled={!canApply}
            onclick={() => void apply()}
          >
            {t("geotag.apply")}
          </button>
          <button type="button" data-testid="geotag-close" onclick={() => (open = false)}>
            {t("batchEdit.close")}
          </button>
        {/if}
      </footer>
    </div>
  </div>
{/if}

<style>
  .gt-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: rgb(0 0 0 / 40%);
    display: grid;
    place-items: center;
  }
  .gt-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 24rem;
    max-width: 90vw;
    max-height: 90vh;
    overflow-y: auto;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px rgb(0 0 0 / 30%);
    display: grid;
    gap: 0.6rem;
  }
  h2,
  p {
    margin: 0;
  }
  .gt-count {
    font-size: 0.85rem;
    color: var(--fb-muted, #666);
  }
  .gt-coords {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.6rem;
  }
  .gt-coords label,
  .gt-scope {
    display: grid;
    gap: 0.25rem;
    font-size: 0.85rem;
  }
  .gt-coords input,
  .gt-scope select {
    font: inherit;
    padding: 0.35rem 0.5rem;
  }
  .gt-failures {
    margin: 0;
    padding-left: 1.2rem;
    font-size: 0.85rem;
    color: var(--fb-danger, #b00);
  }
  .gt-error {
    color: var(--fb-danger, #b00);
    font-size: 0.85rem;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
  footer button {
    font: inherit;
    padding: 0.35rem 0.9rem;
  }
</style>
