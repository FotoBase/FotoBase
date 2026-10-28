<!--
  Quick Tags panel (TASK-03-006, PLAN §2B): a fixed 10-button strip of
  user-configurable one-click tags. One click applies the tag to every
  photo in the current selection (remove for photos that already carry it,
  add for the rest). Unconfigured slots render disabled placeholders.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import { getTags, addTag, removeTag } from "./tags";

  interface Props {
    /// The 10 configured tags from the app options; shorter lists leave the
    /// remaining slots disabled.
    tags?: string[];
    /// Selected photo ids; empty selection disables the whole strip.
    selectedIds?: number[];
    /// Fired after a tag was applied, so the shell can refresh the grid.
    onapplied?: (tag: string) => void;
  }

  let { tags = [], selectedIds = [], onapplied = () => {} }: Props = $props();

  const SLOTS = 10;

  let error = $state<string | null>(null);
  let busy = $state(false);

  async function apply(tag: string): Promise<void> {
    if (busy || selectedIds.length === 0) return;
    busy = true;
    error = null;
    let failed: string | null = null;
    try {
      for (const id of selectedIds) {
        try {
          const current = await getTags(id);
          if (current.includes(tag)) {
            await removeTag(id, tag);
          } else {
            await addTag(id, tag);
          }
        } catch (e) {
          // One failed photo must not abort the rest of the selection.
          failed ??= String(e);
        }
      }
    } finally {
      busy = false;
    }
    if (failed !== null) {
      error = t("quickTags.failed", { error: failed });
    }
    onapplied(tag);
  }
</script>

<div class="quick-tags" data-testid="quick-tags" role="group" aria-label={t("quickTags.title")}>
  <span class="quick-tags-title">{t("quickTags.title")}</span>
  {#each Array.from({ length: SLOTS }, (_, i) => i) as slot (slot)}
    {@const label = tags[slot]}
    {#if label}
      <button
        type="button"
        class="quick-tag"
        data-testid="quick-tag"
        disabled={busy || selectedIds.length === 0}
        title={t("quickTags.apply", { tag: label })}
        onclick={() => void apply(label)}
      >
        {label}
      </button>
    {:else}
      <button
        type="button"
        class="quick-tag empty"
        data-testid="quick-tag"
        disabled
        title={t("quickTags.slot", { n: slot + 1 })}
      >
        {t("quickTags.slot", { n: slot + 1 })}
      </button>
    {/if}
  {/each}
  {#if error}
    <span class="quick-tags-error" data-testid="quick-tags-error" role="alert">{error}</span>
  {/if}
</div>

<style>
  .quick-tags {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    padding: 4px 8px;
  }
  .quick-tags-title {
    font-size: 12px;
    opacity: 0.7;
  }
  .quick-tag {
    font-size: 12px;
    padding: 2px 8px;
    border-radius: 10px;
    cursor: pointer;
  }
  .quick-tag.empty {
    opacity: 0.4;
    cursor: default;
  }
  .quick-tags-error {
    color: var(--danger, #b00);
    font-size: 12px;
  }
</style>
