<!--
  Catalog trash view (TASK-03-012, PLAN §6 P2a): the soft-deleted
  remove-from-album, hide and unlink actions, newest first within each action
  type. Each can be undone on its own, or all of one type at once. Nothing
  here touches disk; the backend replays the reverse operation and says how
  many entries could no longer be restored.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    formatTrashTime,
    groupTrash,
    listTrash,
    trashActionKey,
    undoTrash,
    undoTrashAll,
    type TrashAction,
    type TrashEntry,
    type UndoReport,
  } from "./trash";

  let {
    open = $bindable(false),
    onchanged,
  }: {
    open?: boolean;
    /// Called after an undo: the library, folder tree and albums changed.
    onchanged: () => void;
  } = $props();

  let entries = $state<TrashEntry[]>([]);
  let loaded = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let skipped = $state(0);

  const groups = $derived(groupTrash(entries));

  async function reload(): Promise<void> {
    try {
      entries = await listTrash();
    } catch {
      // Browser dev / backend down: nothing pending.
      entries = [];
    } finally {
      loaded = true;
    }
  }

  // Re-read the trash every time the dialog opens.
  $effect(() => {
    if (!open) return;
    loaded = false;
    error = null;
    skipped = 0;
    void reload();
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

  async function run(undo: () => Promise<UndoReport>): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    skipped = 0;
    try {
      const report = await undo();
      skipped = report.skipped;
      onchanged();
    } catch (e) {
      error = t("trash.failed", { error: String(e instanceof Error ? e.message : e) });
    } finally {
      busy = false;
    }
    await reload();
  }

  const undoOne = (entry: TrashEntry): Promise<void> => run(() => undoTrash(entry.id));
  const undoAll = (action: TrashAction): Promise<void> => run(() => undoTrashAll(action));
</script>

{#if open}
  <div class="trash-backdrop" data-testid="trash-backdrop">
    <div
      class="trash-dialog"
      data-testid="trash-dialog"
      role="dialog"
      aria-label={t("trash.title")}
    >
      <h2>{t("trash.title")}</h2>
      {#if loaded && entries.length === 0}
        <p data-testid="trash-empty">{t("trash.empty")}</p>
      {/if}
      <div class="trash-groups">
        {#each groups as group (group.action)}
          <section>
            <header>
              <h3>{t(`trash.action.${group.action}`)}</h3>
              <button
                type="button"
                data-testid={`trash-undo-all-${group.action}`}
                disabled={busy}
                onclick={() => void undoAll(group.action)}>{t("trash.undoAll")}</button
              >
            </header>
            <ul>
              {#each group.entries as entry (entry.id)}
                <li data-testid="trash-row" title={entry.path}>
                  <span class="trash-what">
                    <strong>{entry.name}</strong>
                    <small>
                      {t(trashActionKey(entry))}{#if entry.action === "album_remove" && entry.detail}
                        · {entry.detail}{/if}
                    </small>
                  </span>
                  <time data-testid="trash-time">{formatTrashTime(entry.created_at)}</time>
                  <button
                    type="button"
                    data-testid="trash-undo"
                    disabled={busy}
                    onclick={() => void undoOne(entry)}>{t("trash.undo")}</button
                  >
                </li>
              {/each}
            </ul>
          </section>
        {/each}
      </div>
      {#if skipped > 0}
        <p class="trash-note" data-testid="trash-skipped">{t("trash.skipped", { n: skipped })}</p>
      {/if}
      {#if error}
        <p class="trash-error" data-testid="trash-error" role="alert">{error}</p>
      {/if}
      <footer>
        <button type="button" data-testid="trash-close" onclick={() => (open = false)}
          >{t("trash.close")}</button
        >
      </footer>
    </div>
  </div>
{/if}

<style>
  .trash-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: rgb(0 0 0 / 40%);
    display: grid;
    place-items: center;
  }
  .trash-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 30rem;
    max-width: 90vw;
    max-height: 80vh;
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr) auto;
    gap: 0.6rem;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px rgb(0 0 0 / 30%);
  }
  h2,
  h3,
  p {
    margin: 0;
  }
  h3 {
    font-size: 0.95rem;
  }
  .trash-groups {
    overflow-y: auto;
    display: grid;
    gap: 0.8rem;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  ul {
    margin: 0.3rem 0 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--fb-border, #ccc);
    border-radius: 6px;
  }
  li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: 0.75rem;
    align-items: center;
    padding: 0.3rem 0.5rem;
    font-size: 0.85rem;
  }
  li + li {
    border-top: 1px solid var(--fb-border, #ccc);
  }
  .trash-what {
    display: grid;
    min-width: 0;
  }
  .trash-what strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  small,
  time {
    color: var(--fb-muted, #666);
    font-size: 0.75rem;
  }
  .trash-error {
    color: #b00020;
  }
  footer {
    display: flex;
    justify-content: flex-end;
  }
</style>
