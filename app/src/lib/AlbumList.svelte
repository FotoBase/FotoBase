<!--
  Album list (TASK-03-008, PLAN §2 B Albums): the sidebar's virtual
  collections with photo counts. Clicking an album shows its photos in the
  grid (click again to leave); right-click opens the album menu. Covers are
  `thumb://` URIs only — no image bytes over IPC (PLAN §3 rule 3).
-->
<script lang="ts">
  import { albumSubtitle, type Album } from "./albums";
  import { thumbUri } from "./grid";
  import { t } from "./i18n.svelte.js";

  let {
    albums,
    selected = null,
    onselect,
    oncreate,
    oncontextmenu,
  }: {
    albums: Album[];
    selected?: number | null;
    onselect: (id: number | null) => void;
    oncreate: () => void;
    oncontextmenu?: (e: { album: Album; x: number; y: number }) => void;
  } = $props();
</script>

<section class="album-list" data-testid="album-list" aria-label={t("albums.title")}>
  <header>
    <h3>{t("albums.title")}</h3>
    <button type="button" class="album-new" data-testid="album-new" onclick={oncreate}>
      {t("albums.new")}
    </button>
  </header>
  {#if albums.length === 0}
    <p data-testid="albums-empty">{t("albums.empty")}</p>
  {:else}
    <ul>
      {#each albums as album (album.id)}
        <li>
          <button
            type="button"
            data-testid={`album-${album.id}`}
            aria-pressed={selected === album.id}
            title={albumSubtitle(album)}
            onclick={() => onselect(selected === album.id ? null : album.id)}
            oncontextmenu={(e) => {
              e.preventDefault();
              oncontextmenu?.({ album, x: e.clientX, y: e.clientY });
            }}
          >
            {#if thumbUri(album.cover_hash)}
              <img
                class="album-cover"
                data-testid={`album-cover-${album.id}`}
                src={thumbUri(album.cover_hash) ?? ""}
                alt=""
              />
            {:else}
              <span class="album-cover album-cover-empty" aria-hidden="true"></span>
            {/if}
            <span class="album-name">{album.name}</span>
            <span class="album-count" data-testid={`album-count-${album.id}`}>{album.count}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .album-list {
    margin-top: 1rem;
    border-top: 1px solid var(--fb-border-soft);
    padding-top: 0.25rem;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  h3 {
    margin: 0.75rem 0 0.25rem;
    font-size: 0.85rem;
    color: var(--fb-muted);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  button {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
    padding: 0.2rem 0.4rem;
    border: 0;
    border-radius: 4px;
    background: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }
  button:hover {
    background: var(--fb-surface);
  }
  button[aria-pressed="true"] {
    background: var(--fb-accent);
    color: var(--fb-on-accent);
  }
  .album-new {
    width: auto;
    margin-top: 0.5rem;
    border: 1px solid var(--fb-border);
    font-size: 0.75rem;
  }
  .album-cover {
    flex: 0 0 24px;
    width: 24px;
    height: 24px;
    border-radius: 3px;
    object-fit: cover;
  }
  .album-cover-empty {
    background: var(--fb-surface-alt);
  }
  .album-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .album-count {
    font-size: 0.75rem;
    opacity: 0.75;
  }
</style>
