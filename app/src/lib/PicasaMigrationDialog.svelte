<!--
  Picasa.ini migration offer (TASK-03-022): folders whose legacy Picasa.ini a
  scan found. "Migrate" converts that folder's stars, albums and faces into
  XMP sidecars (the ini is never changed); "Don't ask again" declines for
  good; closing the dialog just asks again later.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import { declinePicasaFolder, ingestPicasaFolder, type PicasaReport } from "./picasa";

  let {
    open = $bindable(false),
    folders = $bindable<string[]>([]),
    onmigrated,
  }: {
    open?: boolean;
    /// Folders still waiting for an answer; answered ones leave the list.
    folders?: string[];
    /// A folder was migrated: the shell re-reads the library.
    onmigrated: () => void;
  } = $props();

  let busy = $state(false);
  let error = $state<string | null>(null);
  let reports = $state<Record<string, PicasaReport>>({});

  async function migrate(path: string): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    try {
      const report = await ingestPicasaFolder(path);
      reports[path] = report;
      if (report.failed.length === 0) folders = folders.filter((f) => f !== path);
      else error = t("picasa.failed", { n: report.failed.length });
      onmigrated();
    } catch (e) {
      error = t("picasa.error", { error: String(e) });
    } finally {
      busy = false;
    }
  }

  async function decline(path: string): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await declinePicasaFolder(path);
      folders = folders.filter((f) => f !== path);
    } catch (e) {
      error = t("picasa.error", { error: String(e) });
    } finally {
      busy = false;
    }
  }

  // Nothing left to answer: close the dialog.
  $effect(() => {
    if (open && folders.length === 0 && Object.keys(reports).length === 0) open = false;
  });
</script>

{#if open}
  <div class="pm-backdrop" data-testid="picasa-backdrop">
    <div class="pm-dialog" role="dialog" aria-label={t("picasa.title")}>
      <h2>{t("picasa.title")}</h2>
      <p>{t("picasa.intro")}</p>
      <ul>
        {#each folders as folder (folder)}
          <li data-testid="picasa-folder">
            <span class="pm-path" title={folder}>{folder}</span>
            <button
              type="button"
              data-testid="picasa-migrate"
              disabled={busy}
              onclick={() => migrate(folder)}
            >
              {t("picasa.migrate")}
            </button>
            <button
              type="button"
              data-testid="picasa-decline"
              disabled={busy}
              onclick={() => decline(folder)}
            >
              {t("picasa.decline")}
            </button>
          </li>
        {/each}
      </ul>
      {#each Object.entries(reports) as [path, r] (path)}
        <p class="pm-done" data-testid="picasa-report">
          {t("picasa.done", {
            folder: path,
            written: r.written,
            stars: r.stars,
            albums: r.albums_created,
            faces: r.faces,
          })}
        </p>
      {/each}
      {#if error}
        <p class="pm-error" role="alert">{error}</p>
      {/if}
      <div class="pm-actions">
        <button type="button" data-testid="picasa-close" onclick={() => (open = false)}>
          {t("picasa.later")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .pm-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: var(--fb-backdrop, rgb(0 0 0 / 40%));
    display: grid;
    place-items: center;
  }
  .pm-dialog {
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
  ul {
    list-style: none;
    margin: 0 0 0.6rem;
    padding: 0;
    display: grid;
    gap: 0.4rem;
  }
  li {
    display: grid;
    grid-template-columns: 1fr auto auto;
    align-items: center;
    gap: 0.5rem;
  }
  .pm-path {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .pm-done {
    font-size: 0.85rem;
    color: var(--fb-muted, #6b6b6b);
  }
  .pm-error {
    color: var(--fb-error, #b00020);
    font-size: 0.85rem;
  }
  .pm-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 0.8rem;
  }
</style>
