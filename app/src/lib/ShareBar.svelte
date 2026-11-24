<!--
  Touch multi-select + share (TASK-09-005, PLAN §6 Phase 5). Touch devices
  have no modifier keys, so "Select" switches the grid into a mode where taps
  toggle selection; "Share" hands the selection to the native share sheet in
  one call. Hidden without a touch pointer (desktop uses the photo menu).
-->
<script lang="ts">
  import { hasCoarsePointer, type GridItem } from "./grid";
  import { gridState } from "./grid-state.svelte.js";
  import { t } from "./i18n.svelte.js";
  import { shareIds, sharePhotos } from "./share";

  interface Props {
    /// The photos the grid shows; the share order follows it.
    photos?: GridItem[];
  }

  let { photos = [] }: Props = $props();

  const touch = hasCoarsePointer();
  let error = $state<string | null>(null);
  let sharing = $state(false);

  function leaveSelectMode(): void {
    gridState.selectMode = false;
    gridState.selection = new Set();
    error = null;
  }

  async function share(): Promise<void> {
    const ids = shareIds(gridState.selection, photos);
    if (ids.length === 0 || sharing) return;
    error = null;
    sharing = true;
    try {
      const reply = await sharePhotos(ids);
      if (reply.failed > 0) error = t("share.skipped", { n: reply.failed });
    } catch (e) {
      error = t("share.failed", { error: String(e) });
    } finally {
      sharing = false;
    }
  }
</script>

{#if touch}
  <div class="share-bar" data-testid="share-bar">
    {#if gridState.selectMode}
      <span data-testid="share-count">{t("share.count", { n: gridState.selection.size })}</span>
      <button
        type="button"
        data-testid="share-action"
        disabled={gridState.selection.size === 0 || sharing}
        onclick={() => void share()}
      >
        {t("share.action")}
      </button>
      <button type="button" data-testid="share-cancel" onclick={leaveSelectMode}>
        {t("share.cancel")}
      </button>
    {:else}
      <button
        type="button"
        data-testid="share-select-toggle"
        onclick={() => (gridState.selectMode = true)}
      >
        {t("share.select")}
      </button>
    {/if}
    {#if error}
      <span class="share-error" data-testid="share-error" role="alert">{error}</span>
    {/if}
  </div>
{/if}
