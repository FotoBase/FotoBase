<!--
  Album membership (TASK-03-008, PLAN §2 B Albums): in the viewer's info
  pane, tick the albums the shown photo belongs to. A photo can be in many
  albums; ticking adds it (the id lands in the sidecar's `fb:Albums` bag),
  unticking removes it. The box reverts and the error shows if the write
  fails.
-->
<script lang="ts">
  import { addToAlbum, getFileAlbums, listAlbums, removeFromAlbum, type Album } from "./albums";
  import { t } from "./i18n.svelte.js";

  interface Props {
    /// The photo being edited; `null` disables the boxes.
    photoId?: number | null;
    /// Fired after a membership change, so the shell can refresh counts.
    onchanged?: () => void;
  }

  let { photoId = null, onchanged = () => {} }: Props = $props();

  let albums = $state<Album[]>([]);
  let memberOf = $state<number[]>([]);
  let error = $state<string | null>(null);

  let requestSeq = 0;
  $effect(() => {
    const id = photoId;
    const seq = ++requestSeq;
    error = null;
    void (async () => {
      let all: Album[] = [];
      let mine: number[] = [];
      try {
        all = await listAlbums();
        mine = id === null ? [] : await getFileAlbums(id);
      } catch {
        // Browser dev / backend down: nothing to show.
      }
      if (seq === requestSeq) {
        albums = all;
        memberOf = mine;
      }
    })();
  });

  async function toggle(album: Album, checked: boolean): Promise<void> {
    const id = photoId;
    if (id === null) return;
    error = null;
    const before = memberOf;
    memberOf = checked ? [...before, album.id] : before.filter((a) => a !== album.id);
    try {
      if (checked) {
        await addToAlbum(album.id, [id]);
      } else {
        await removeFromAlbum(album.id, [id]);
      }
      onchanged();
    } catch (e) {
      memberOf = before;
      error = t("albums.failed", { error: String(e) });
    }
  }
</script>

<div class="album-membership" data-testid="album-membership">
  {#if albums.length === 0}
    <p data-testid="album-membership-empty">{t("albums.membershipEmpty")}</p>
  {:else}
    <ul>
      {#each albums as album (album.id)}
        <li>
          <label>
            <input
              type="checkbox"
              data-testid={`album-member-${album.id}`}
              disabled={photoId === null}
              checked={memberOf.includes(album.id)}
              onchange={(e) => void toggle(album, e.currentTarget.checked)}
            />
            {album.name}
          </label>
        </li>
      {/each}
    </ul>
  {/if}
  {#if error}
    <span class="album-membership-error" data-testid="album-membership-error" role="alert"
      >{error}</span
    >
  {/if}
</div>

<style>
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: 12px;
  }
  label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  p {
    font-size: 12px;
    margin: 0;
  }
  .album-membership-error {
    color: var(--danger, #b00);
    font-size: 12px;
  }
</style>
