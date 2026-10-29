<!--
  Metadata editor (TASK-03-020, PLAN §6 P2): capture date, GPS, artist and
  copyright of the photo the viewer shows. Read-only until "Edit"; Save sends
  only what changed and the backend writes the sidecar first (the catalog
  mirror follows), refusing invalid input field by field — those errors show
  beside their field and nothing is written. Single photo only.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import type { PhotoInfo } from "./info-pane";
  import {
    buildEdit,
    draftFromInfo,
    editPhotoMetadata,
    formatCoordinate,
    type MetadataDraft,
    type MetadataField,
  } from "./metadata-edit";

  interface Props {
    /// The photo being edited; `null` disables editing.
    info?: PhotoInfo | null;
    /// Fired with the photo as the backend stored it after a successful save.
    onsaved?: (info: PhotoInfo) => void;
  }

  let { info = null, onsaved = () => {} }: Props = $props();

  const MISSING = "—";

  let editing = $state(false);
  let saving = $state(false);
  let draft = $state<MetadataDraft>({
    takenAt: "",
    latitude: "",
    longitude: "",
    artist: "",
    copyright: "",
  });
  let errors = $state<Partial<Record<MetadataField, string>>>({});
  let failure = $state<string | null>(null);
  // What the backend stored on our last save, until the parent hands down
  // its own refreshed `info`.
  let saved = $state<PhotoInfo | null>(null);

  const shown = $derived(saved && saved.id === info?.id ? saved : info);

  $effect(() => {
    void info;
    saved = null;
  });

  // Moving to another photo drops an edit in progress.
  let lastId: number | null = null;
  $effect(() => {
    const id = info?.id ?? null;
    if (id !== lastId) {
      lastId = id;
      editing = false;
      errors = {};
      failure = null;
    }
  });

  function startEditing(): void {
    if (!shown) return;
    draft = draftFromInfo(shown);
    errors = {};
    failure = null;
    editing = true;
  }

  function cancel(): void {
    editing = false;
    errors = {};
    failure = null;
  }

  function touched(field: MetadataField | null): void {
    failure = null;
    if (field) errors = { ...errors, [field]: undefined };
  }

  async function save(): Promise<void> {
    const current = shown;
    if (!current || saving) return;
    const edit = buildEdit(current, draft);
    if (Object.keys(edit).length === 0) {
      cancel();
      return;
    }
    saving = true;
    failure = null;
    try {
      const result = await editPhotoMetadata(current.id, edit);
      if (current.id !== info?.id) return;
      if (result.errors.length > 0) {
        errors = Object.fromEntries(result.errors.map((e) => [e.field, e.message]));
        return;
      }
      if (result.info) {
        saved = result.info;
        onsaved(result.info);
      }
      cancel();
    } catch (e) {
      failure = t("meta.failed", { error: String(e) });
    } finally {
      saving = false;
    }
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === "Enter") {
      e.preventDefault();
      void save();
    } else if (e.key === "Escape") {
      e.stopPropagation();
      cancel();
    }
  }

  const gpsText = $derived(
    shown && shown.latitude !== null && shown.longitude !== null
      ? `${formatCoordinate(shown.latitude)}, ${formatCoordinate(shown.longitude)}`
      : MISSING,
  );
</script>

<div class="meta-editor" data-testid="meta-editor">
  {#if !editing}
    <dl>
      <dt>{t("meta.gps")}</dt>
      <dd data-testid="meta-gps">{gpsText}</dd>
      <dt>{t("meta.artist")}</dt>
      <dd data-testid="meta-artist">{shown?.artist ?? MISSING}</dd>
      <dt>{t("meta.copyright")}</dt>
      <dd data-testid="meta-copyright">{shown?.copyright ?? MISSING}</dd>
    </dl>
    <button type="button" data-testid="meta-edit" disabled={!shown} onclick={startEditing}>
      {t("meta.edit")}
    </button>
  {:else}
    <form
      class="meta-form"
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <label>
        <span>{t("meta.date")}</span>
        <input
          type="text"
          data-testid="meta-input-date"
          placeholder={t("meta.dateHint")}
          aria-invalid={errors.taken_at ? "true" : undefined}
          bind:value={draft.takenAt}
          oninput={() => touched("taken_at")}
          onkeydown={onKey}
        />
        {#if errors.taken_at}
          <span class="meta-error" data-testid="meta-error-date" role="alert">{errors.taken_at}</span>
        {/if}
      </label>
      <label>
        <span>{t("meta.latitude")}</span>
        <input
          type="text"
          data-testid="meta-input-lat"
          placeholder={t("meta.gpsHint")}
          aria-invalid={errors.latitude ? "true" : undefined}
          bind:value={draft.latitude}
          oninput={() => touched("latitude")}
          onkeydown={onKey}
        />
        {#if errors.latitude}
          <span class="meta-error" data-testid="meta-error-lat" role="alert">{errors.latitude}</span>
        {/if}
      </label>
      <label>
        <span>{t("meta.longitude")}</span>
        <input
          type="text"
          data-testid="meta-input-lon"
          placeholder={t("meta.gpsHint")}
          aria-invalid={errors.longitude ? "true" : undefined}
          bind:value={draft.longitude}
          oninput={() => touched("longitude")}
          onkeydown={onKey}
        />
        {#if errors.longitude}
          <span class="meta-error" data-testid="meta-error-lon" role="alert">{errors.longitude}</span>
        {/if}
      </label>
      <label>
        <span>{t("meta.artist")}</span>
        <input
          type="text"
          data-testid="meta-input-artist"
          bind:value={draft.artist}
          oninput={() => touched(null)}
          onkeydown={onKey}
        />
      </label>
      <label>
        <span>{t("meta.copyright")}</span>
        <input
          type="text"
          data-testid="meta-input-copyright"
          bind:value={draft.copyright}
          oninput={() => touched(null)}
          onkeydown={onKey}
        />
      </label>
      {#if failure}
        <span class="meta-error" data-testid="meta-failure" role="alert">{failure}</span>
      {/if}
      <div class="meta-actions">
        <button type="submit" data-testid="meta-save" disabled={saving}>{t("meta.save")}</button>
        <button type="button" data-testid="meta-cancel" onclick={cancel}>{t("meta.cancel")}</button>
      </div>
    </form>
  {/if}
</div>

<style>
  .meta-editor {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .meta-form {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .meta-form label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
  }
  .meta-form input {
    width: 100%;
    box-sizing: border-box;
    font: inherit;
    font-size: 12px;
  }
  .meta-form input[aria-invalid="true"] {
    border-color: var(--danger, #b00);
  }
  .meta-error {
    color: var(--danger, #b00);
    font-size: 12px;
  }
  .meta-actions {
    display: flex;
    gap: 6px;
  }
</style>
