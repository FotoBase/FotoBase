<!--
  Stack suggestions (TASK-03-021): the backend's RAW+JPEG pairs and edited
  versions, offered to the user. Nothing is stacked until a group is accepted
  (one by one, or all). Bursts have no auto-detection — they are stacked by
  hand from the photo menu.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import { stackPhotos, stackSuggestions, type StackSuggestion } from "./stacks";

  let {
    open = $bindable(false),
    onapplied,
  }: {
    open?: boolean;
    /// At least one stack was made: the shell re-reads the grid.
    onapplied: () => void;
  } = $props();

  let groups = $state<StackSuggestion[]>([]);
  let loaded = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    if (!open) return;
    loaded = false;
    error = null;
    groups = [];
    void (async () => {
      try {
        groups = await stackSuggestions();
      } catch (e) {
        error = t("stack.error", { error: String(e) });
      } finally {
        loaded = true;
      }
    })();
  });

  // Stack the groups in order; a failure stops the run and leaves that group
  // (and the rest) listed, so nothing is silently skipped.
  async function accept(chosen: StackSuggestion[]): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    let stacked = 0;
    try {
      for (const group of chosen) {
        await stackPhotos(group.file_ids);
        groups = groups.filter((g) => g !== group);
        stacked++;
      }
    } catch (e) {
      error = t("stack.error", { error: String(e) });
    } finally {
      busy = false;
      if (stacked > 0) onapplied();
    }
  }
</script>

{#if open}
  <div class="ss-backdrop" data-testid="stack-suggestions-backdrop">
    <div class="ss-dialog" role="dialog" aria-label={t("stacks.title")}>
      <h2>{t("stacks.title")}</h2>
      {#if loaded && groups.length === 0 && !error}
        <p data-testid="stack-suggestions-empty">{t("stacks.empty")}</p>
      {/if}
      <ul>
        {#each groups as group, i (group.file_ids.join(","))}
          <li data-testid="stack-suggestion">
            <span class="ss-kind">{t(`stacks.kind.${group.kind}`)}</span>
            <span class="ss-files">{group.filenames.join(", ")}</span>
            <button
              type="button"
              data-testid="stack-suggestion-accept"
              disabled={busy}
              onclick={() => accept([groups[i]])}
            >
              {t("stacks.stack")}
            </button>
          </li>
        {/each}
      </ul>
      {#if error}
        <p class="ss-error" role="alert">{error}</p>
      {/if}
      <div class="ss-actions">
        {#if groups.length > 1}
          <button
            type="button"
            data-testid="stack-suggestions-accept-all"
            disabled={busy}
            onclick={() => accept([...groups])}
          >
            {t("stacks.stackAll")}
          </button>
        {/if}
        <button
          type="button"
          data-testid="stack-suggestions-close"
          onclick={() => (open = false)}
        >
          {t("stacks.close")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .ss-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: var(--fb-backdrop, rgb(0 0 0 / 40%));
    display: grid;
    place-items: center;
  }
  .ss-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    /* A definite width: the 1fr filename column has overflow hidden, so in a
       shrink-to-fit dialog it would collapse to nothing. */
    width: 36rem;
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
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.4rem;
  }
  li {
    display: grid;
    grid-template-columns: 8rem 1fr auto;
    align-items: center;
    gap: 0.5rem;
  }
  .ss-kind {
    font-size: 0.8rem;
    color: var(--fb-muted, #6b6b6b);
  }
  .ss-files {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .ss-error {
    color: var(--fb-error, #b00020);
    font-size: 0.85rem;
  }
  .ss-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.8rem;
  }
</style>
