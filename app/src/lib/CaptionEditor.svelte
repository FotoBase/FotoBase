<!--
  Caption editor (TASK-03-007, PLAN §2B): edit the caption of the photo the
  viewer shows. Multi-line: Enter adds a line, Ctrl/Cmd+Enter or leaving the
  field saves. The backend writes `dc:description` through the sidecar first;
  an emptied field clears it. The field shows what the backend stored.
-->
<script lang="ts">
  import { getCaption, setCaption } from "./caption";
  import { t } from "./i18n.svelte.js";

  interface Props {
    /// The photo being edited; `null` disables the field.
    photoId?: number | null;
    /// Fired after a caption was written, so the shell can refresh the grid.
    onchanged?: () => void;
  }

  let { photoId = null, onchanged = () => {} }: Props = $props();

  let saved = $state("");
  let draft = $state("");
  let error = $state<string | null>(null);

  let requestSeq = 0;
  $effect(() => {
    const id = photoId;
    const seq = ++requestSeq;
    error = null;
    void (async () => {
      let loaded = "";
      try {
        loaded = id === null ? "" : await getCaption(id);
      } catch {
        loaded = "";
      }
      if (seq === requestSeq) {
        saved = loaded;
        draft = loaded;
      }
    })();
  });

  async function save(): Promise<void> {
    const id = photoId;
    if (id === null || draft === saved) return;
    error = null;
    try {
      const stored = await setCaption(id, draft);
      if (id !== photoId) return;
      saved = stored;
      draft = stored;
      onchanged();
    } catch (e) {
      error = t("caption.failed", { error: String(e) });
    }
  }
</script>

<div class="caption-editor" data-testid="caption-editor">
  <textarea
    class="caption-input"
    data-testid="caption-input"
    rows="3"
    placeholder={t("caption.placeholder")}
    aria-label={t("caption.title")}
    disabled={photoId === null}
    bind:value={draft}
    onblur={() => void save()}
    onkeydown={(e) => {
      if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        void save();
      } else if (e.key === "Escape") {
        e.stopPropagation();
        (e.currentTarget as HTMLTextAreaElement).blur();
      }
    }}
  ></textarea>
  {#if error}
    <span class="caption-error" data-testid="caption-error" role="alert">{error}</span>
  {/if}
</div>

<style>
  .caption-editor {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .caption-input {
    width: 100%;
    box-sizing: border-box;
    font: inherit;
    font-size: 12px;
    resize: vertical;
  }
  .caption-error {
    color: var(--danger, #b00);
    font-size: 12px;
  }
</style>
