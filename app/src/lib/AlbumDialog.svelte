<!--
  Album dialog (TASK-03-008, PLAN §2 B Albums): create an album with name +
  date and an optional place and description, or edit an existing one. The
  backend validates again and owns the write; its error is shown and the
  dialog stays open so nothing typed is lost.
-->
<script lang="ts">
  import {
    createAlbum,
    isValidAlbumFields,
    todayIso,
    updateAlbum,
    type Album,
    type AlbumFields,
  } from "./albums";
  import { t } from "./i18n.svelte.js";

  let {
    open = $bindable(false),
    album = null,
    onsaved,
  }: {
    open?: boolean;
    /// The album being edited; `null` creates a new one.
    album?: Album | null;
    onsaved: (album: Album) => void;
  } = $props();

  let fields = $state<AlbumFields>({ name: "", date: todayIso(), place: "", description: "" });
  let error = $state<string | null>(null);
  let busy = $state(false);

  // Re-seed the form every time the dialog opens.
  $effect(() => {
    if (!open) return;
    fields = {
      name: album?.name ?? "",
      date: album?.date ?? todayIso(),
      place: album?.place ?? "",
      description: album?.description ?? "",
    };
    error = null;
  });

  // Escape closes the open dialog, like the other overlays.
  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") open = false;
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  async function save(): Promise<void> {
    if (busy || !isValidAlbumFields(fields)) return;
    busy = true;
    error = null;
    try {
      const saved = album ? await updateAlbum(album.id, fields) : await createAlbum(fields);
      open = false;
      onsaved(saved);
    } catch (e) {
      error = t("albums.failed", { error: String(e) });
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <div class="album-backdrop" data-testid="album-backdrop">
    <div
      class="album-dialog"
      data-testid="album-dialog"
      role="dialog"
      aria-label={album ? t("albums.dialog.editTitle") : t("albums.dialog.createTitle")}
    >
      <form
        onsubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <h2>{album ? t("albums.dialog.editTitle") : t("albums.dialog.createTitle")}</h2>
        <label>
          {t("albums.name")}
          <input data-testid="album-name" type="text" bind:value={fields.name} />
        </label>
        <label>
          {t("albums.date")}
          <input data-testid="album-date" type="date" bind:value={fields.date} />
        </label>
        <label>
          {t("albums.place")}
          <input data-testid="album-place" type="text" bind:value={fields.place} />
        </label>
        <label>
          {t("albums.description")}
          <textarea data-testid="album-description" rows="3" bind:value={fields.description}
          ></textarea>
        </label>
        {#if error}
          <p class="album-error" data-testid="album-error" role="alert">{error}</p>
        {/if}
        <footer>
          <button
            type="button"
            data-testid="album-cancel"
            onclick={() => {
              open = false;
            }}
          >
            {t("albums.cancel")}
          </button>
          <button
            type="submit"
            data-testid="album-save"
            disabled={busy || !isValidAlbumFields(fields)}
          >
            {t("albums.save")}
          </button>
        </footer>
      </form>
    </div>
  </div>
{/if}

<style>
  .album-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: rgb(0 0 0 / 40%);
    display: grid;
    place-items: center;
  }
  .album-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 24rem;
    max-width: 90vw;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px rgb(0 0 0 / 30%);
  }
  form {
    display: grid;
    gap: 0.6rem;
  }
  h2 {
    margin: 0;
  }
  label {
    display: grid;
    gap: 0.2rem;
    font-size: 0.85rem;
  }
  input,
  textarea {
    font: inherit;
    padding: 0.3rem 0.5rem;
    box-sizing: border-box;
    width: 100%;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
  .album-error {
    color: #b00020;
    margin: 0;
  }
</style>
