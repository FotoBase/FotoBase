<!--
  Tag editor (TASK-03-006, PLAN §2B): add/remove/rename tags for the photo
  the viewer shows. Nested tags are typed with the `|` path separator
  (`Vacation|2024`); every change is written through to the sidecar by the
  backend before the UI reflects it.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import { addTag, getTags, leafName, normalizeTag, removeTag, renameTag } from "./tags";

  interface Props {
    /// The photo being edited; `null` renders nothing.
    photoId?: number | null;
    /// Fired when tags changed, so the shell can refresh the grid.
    onchanged?: () => void;
  }

  let { photoId = null, onchanged = () => {} }: Props = $props();

  let tags = $state<string[]>([]);
  let draft = $state("");
  let renameOld = $state("");
  let renameNew = $state("");
  let error = $state<string | null>(null);

  let requestSeq = 0;
  $effect(() => {
    const id = photoId;
    const seq = ++requestSeq;
    void (async () => {
      try {
        const loaded = id === null ? [] : await getTags(id);
        if (seq === requestSeq) tags = loaded;
      } catch {
        if (seq === requestSeq) tags = [];
      }
    })();
  });

  async function add(): Promise<void> {
    const id = photoId;
    const tag = normalizeTag(draft);
    if (id === null || tag === "") return;
    error = null;
    try {
      tags = await addTag(id, tag);
      draft = "";
      onchanged();
    } catch (e) {
      error = t("tags.failed", { error: String(e) });
    }
  }

  async function remove(tag: string): Promise<void> {
    const id = photoId;
    if (id === null) return;
    error = null;
    try {
      tags = await removeTag(id, tag);
      onchanged();
    } catch (e) {
      error = t("tags.failed", { error: String(e) });
    }
  }

  async function rename(): Promise<void> {
    const old = normalizeTag(renameOld);
    const next = normalizeTag(renameNew);
    if (old === "" || next === "") return;
    error = null;
    try {
      await renameTag(old, next);
      renameOld = "";
      renameNew = "";
      // The current photo may carry the renamed tag: re-read its list.
      if (photoId !== null) tags = await getTags(photoId);
      onchanged();
    } catch (e) {
      error = t("tags.failed", { error: String(e) });
    }
  }
</script>

<div class="tag-editor" data-testid="tag-editor">
  <div class="tag-chips">
    {#each tags as tag (tag)}
      <span class="tag-chip" data-testid={`tag-chip-${tag}`}>
        {leafName(tag)}
        <button
          type="button"
          data-testid="tag-remove"
          title={t("tags.remove", { tag })}
          aria-label={t("tags.remove", { tag })}
          onclick={() => void remove(tag)}
        >
          ×
        </button>
      </span>
    {/each}
  </div>
  <input
    type="text"
    class="tag-input"
    data-testid="tag-input"
    placeholder={t("tags.addPlaceholder")}
    bind:value={draft}
    onkeydown={(e) => {
      if (e.key === "Enter") void add();
    }}
  />
  <div class="tag-rename">
    <input
      type="text"
      class="rename-old"
      data-testid="rename-old"
      placeholder={t("tags.renameOld")}
      bind:value={renameOld}
    />
    <input
      type="text"
      class="rename-new"
      data-testid="rename-new"
      placeholder={t("tags.renameNew")}
      bind:value={renameNew}
    />
    <button type="button" data-testid="rename-submit" onclick={() => void rename()}>
      {t("tags.rename")}
    </button>
  </div>
  {#if error}
    <span class="tag-error" data-testid="tag-error" role="alert">{error}</span>
  {/if}
</div>

<style>
  .tag-editor {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .tag-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .tag-chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 12px;
    background: var(--chip, rgba(128, 128, 128, 0.2));
    border-radius: 10px;
    padding: 2px 8px;
  }
  .tag-chip button {
    border: none;
    background: none;
    cursor: pointer;
    padding: 0 2px;
  }
  input,
  button {
    font-size: 12px;
  }
  .tag-rename {
    display: flex;
    gap: 4px;
  }
  .tag-error {
    color: var(--danger, #b00);
    font-size: 12px;
  }
</style>
