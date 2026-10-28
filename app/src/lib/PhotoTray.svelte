<!--
  Photo Tray (TASK-03-010, PLAN §2 B "Photo Tray"): a temporary cross-folder
  selection pinned at the bottom of the window. It survives browsing (the
  store is module-global), "Hold" pins items against "Clear", and a click on
  a thumbnail jumps to that photo in the grid. Thumbnails are `thumb://` URIs
  only — no image bytes over IPC (PLAN §3 rule 3). Pure UI state: nothing
  here touches the catalog or a sidecar.
-->
<script lang="ts">
  import { thumbUri } from "./grid";
  import { gridState } from "./grid-state.svelte.js";
  import { t } from "./i18n.svelte.js";
  import { tray, type TrayPhoto } from "./tray.svelte.js";

  interface Props {
    /// Grid selection that "Add selection" moves into the tray.
    selectedIds?: number[];
    /// Resolves ids to the photos the tray draws (hash for the thumbnail).
    resolve?: (ids: number[]) => TrayPhoto[];
    /// A tray thumbnail was clicked: the shell scrolls the grid to it.
    onjump?: (photo: TrayPhoto) => void;
    /// "Add to album" was pressed: the shell opens its album picker at the
    /// anchor for these ids.
    onaddtoalbum?: (e: { ids: number[]; x: number; y: number }) => void;
  }

  let {
    selectedIds = [],
    resolve = () => [],
    onjump = () => {},
    onaddtoalbum = () => {},
  }: Props = $props();

  function addSelection(): void {
    tray.add(resolve(selectedIds));
  }

  function addToAlbum(e: MouseEvent): void {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    // The picker opens above the tray: the panel sits at the window bottom.
    onaddtoalbum({ ids: tray.ids, x: rect.left, y: rect.top });
  }
</script>

<section class="photo-tray" data-testid="photo-tray" aria-label={t("tray.title")}>
  <header>
    <h3>{t("tray.title")}</h3>
    <span data-testid="tray-count"
      >{t("tray.count", { count: tray.count, held: tray.heldCount })}</span
    >
    <button
      type="button"
      data-testid="tray-add-selection"
      disabled={selectedIds.length === 0}
      onclick={addSelection}>{t("tray.add")}</button
    >
    <button
      type="button"
      data-testid="tray-add-to-album"
      disabled={tray.count === 0}
      onclick={addToAlbum}>{t("tray.addToAlbum")}</button
    >
    <button
      type="button"
      data-testid="tray-clear"
      disabled={tray.count === tray.heldCount}
      title={t("tray.clearTitle")}
      onclick={() => tray.clearUnheld()}>{t("tray.clear")}</button
    >
  </header>
  {#if tray.count === 0}
    <p data-testid="tray-empty">{t("tray.empty")}</p>
  {:else}
    <ul>
      {#each tray.items as item (item.id)}
        <li data-testid="tray-item" data-held={item.held}>
          <button
            type="button"
            class="tray-thumb"
            data-testid="tray-jump"
            title={item.filename}
            onclick={() => onjump(item)}
          >
            {#if thumbUri(item.hash)}
              <img
                src={thumbUri(item.hash, gridState.thumbVersions[item.hash ?? ""] ?? 0) ?? ""}
                alt={item.filename}
                draggable="false"
              />
            {:else}
              <span class="tray-placeholder"></span>
            {/if}
          </button>
          <button
            type="button"
            class="tray-hold"
            data-testid="tray-hold"
            aria-pressed={item.held}
            onclick={() => tray.toggleHold(item.id)}
            >{item.held ? t("tray.release") : t("tray.hold")}</button
          >
          <button
            type="button"
            class="tray-remove"
            data-testid="tray-remove"
            aria-label={t("tray.remove")}
            title={t("tray.remove")}
            onclick={() => tray.remove(item.id)}>×</button
          >
        </li>
      {/each}
    </ul>
  {/if}
</section>
